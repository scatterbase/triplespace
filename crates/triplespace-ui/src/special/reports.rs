//! The report pages (0047 §4; 21 §2): `Special:WantedPages`, `Special:ShortPages`,
//! `Special:ListProperties` and the rest of [`REPORTS`], one page for all of them, from
//! `list=querypage`.
//!
//! Each is served under its name, its MediaWiki name and its aliases (`Special:Wantedpages`,
//! `Special:BrokenLinks`), in any case, as MediaWiki resolves them. MediaWiki's `limit`
//! (50 by default, at most 500) and `offset` page through the rows; a row shows its value
//! as the report measures it (links, members, categories, revisions, bytes, a time), a
//! redirect its target, a property its label. A title that does not exist (a wanted page,
//! a broken redirect's target) is a red link.
//!
//! Above the list, the report says how fresh it is: a live query says nothing more; a
//! report kept as pages change says so, and when changes are still being applied, how
//! many. `Special:LonelyPages` says that only links count, since transclusions are not
//! counted yet.

use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::{ReportPage, ReportQuery, ReportRow};

use crate::Site;
use crate::features::{Feature, Features};
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};
use crate::special::changes::{clock, date};

/// Rows to a page unless asked, as MediaWiki shows.
const LIMIT: u32 = 50;

/// The most rows to a page.
const MAX_LIMIT: u32 = 500;

/// The page sizes offered.
const SIZES: [u32; 5] = [20, 50, 100, 250, 500];

/// What a row's value counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// Nothing shown.
    None,
    /// Links to the title.
    Links,
    /// Pages in the category.
    Members,
    /// Categories the page is in.
    Categories,
    /// The page's revisions.
    Revisions,
    /// The page's size.
    Bytes,
    /// When the page was last edited.
    Time,
}

/// A report the site offers a page for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The registry's name: the page's title and what `qppage` is given.
    pub name: &'static str,
    /// MediaWiki's name, where it differs.
    pub mediawiki_name: Option<&'static str>,
    /// Other names the page answers to.
    pub aliases: &'static [&'static str],
    /// Whether it lists document pages, and so needs them.
    pub pages: bool,
    unit: Unit,
    /// Whether its titles do not exist (red links).
    missing: bool,
    /// Whether its redirect targets do not exist.
    missing_target: bool,
}

const fn report(name: &'static str, mediawiki_name: &'static str, unit: Unit) -> Report {
    Report {
        name,
        mediawiki_name: Some(mediawiki_name),
        aliases: &[],
        pages: true,
        unit,
        missing: false,
        missing_target: false,
    }
}

/// The reports, in `Special:SpecialPages` order.
pub const REPORTS: &[Report] = &[
    Report {
        aliases: &["BrokenLinks"],
        missing: true,
        ..report("WantedPages", "Wantedpages", Unit::Links)
    },
    Report {
        missing: true,
        ..report("WantedCategories", "Wantedcategories", Unit::Members)
    },
    Report {
        aliases: &["MostLinked"],
        ..report("MostLinkedPages", "Mostlinked", Unit::Links)
    },
    Report {
        aliases: &["MostUsedCategories"],
        ..report(
            "MostLinkedCategories",
            "Mostlinkedcategories",
            Unit::Members,
        )
    },
    report("MostCategories", "Mostcategories", Unit::Categories),
    report("MostRevisions", "Mostrevisions", Unit::Revisions),
    report("FewestRevisions", "Fewestrevisions", Unit::Revisions),
    report("DeadendPages", "Deadendpages", Unit::None),
    Report {
        aliases: &["OrphanedPages"],
        ..report("LonelyPages", "Lonelypages", Unit::None)
    },
    report("UncategorizedPages", "Uncategorizedpages", Unit::None),
    report(
        "UncategorizedCategories",
        "Uncategorizedcategories",
        Unit::None,
    ),
    report(
        "UncategorizedTemplates",
        "Uncategorizedtemplates",
        Unit::None,
    ),
    report("UnusedCategories", "Unusedcategories", Unit::None),
    report("ShortPages", "Shortpages", Unit::Bytes),
    report("LongPages", "Longpages", Unit::Bytes),
    report("AncientPages", "Ancientpages", Unit::Time),
    report("ListRedirects", "Listredirects", Unit::None),
    Report {
        mediawiki_name: None,
        missing_target: true,
        ..report("BrokenRedirects", "", Unit::None)
    },
    Report {
        mediawiki_name: None,
        ..report("DoubleRedirects", "", Unit::None)
    },
    Report {
        mediawiki_name: None,
        pages: false,
        ..report("ListProperties", "", Unit::None)
    },
];

impl Report {
    /// The report a special page's name names: its name, MediaWiki name or an alias, in
    /// any case.
    #[must_use]
    pub fn of(name: &str) -> Option<&'static Self> {
        REPORTS.iter().find(|r| {
            r.name.eq_ignore_ascii_case(name)
                || r.mediawiki_name
                    .is_some_and(|m| m.eq_ignore_ascii_case(name))
                || r.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
        })
    }

    /// The page's title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Special:{}", self.name)
    }

    /// The message stem: `ts-report-wantedpages`.
    fn key(&self) -> String {
        format!("ts-report-{}", self.name.to_ascii_lowercase())
    }

    /// Its label on `Special:SpecialPages` and its page's title.
    #[must_use]
    pub fn label_key(&self) -> String {
        format!("{}-title", self.key())
    }

    /// Its description on `Special:SpecialPages`.
    #[must_use]
    pub fn description_key(&self) -> String {
        format!("ts-specialpages-{}", self.name.to_ascii_lowercase())
    }

    /// Whether the site offers it here.
    #[must_use]
    pub fn offered(&self, features: &Features) -> bool {
        features.has(Feature::Reports) && (!self.pages || features.has(Feature::Pages))
    }
}

/// What the page was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    offset: u32,
    limit: u32,
}

impl Window {
    fn of(params: &Params) -> Self {
        let number = |k: &str| params.map.get(k).and_then(|v| v.trim().parse::<u32>().ok());
        Self {
            offset: number("offset").unwrap_or(0),
            limit: number("limit").unwrap_or(LIMIT).clamp(1, MAX_LIMIT),
        }
    }

    fn href(self, report: &Report) -> String {
        let mut s = format!("/w/index.php?title={}", report.title());
        if self.limit != LIMIT {
            let _ = write!(s, "&limit={}", self.limit);
        }
        if self.offset > 0 {
            let _ = write!(s, "&offset={}", self.offset);
        }
        s
    }
}

/// `GET Special:{report}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    report: &'static Report,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !report.offered(&cx.features) {
        return pages::no_page(&cx, headers, report.title());
    }
    let window = Window::of(params);
    let found = match site
        .client()
        .report(
            &incoming,
            &ReportQuery {
                name: report.name.to_string(),
                offset: window.offset,
                limit: window.limit,
            },
        )
        .await
    {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(found.cache);
    let body = body(&cx.m, report, window, &found.value);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get(&report.label_key()),
        returnto: Some(report.title()),
        body,
        ..Page::default()
    })
}

fn body(m: &Messages, report: &Report, window: Window, page: &ReportPage) -> String {
    let mut s = String::new();
    let note = format!("{}-note", report.key());
    if m.has(&note) {
        let _ = write!(s, "<p class=\"ts-report__note\">{}</p>", esc(&m.get(&note)));
    }
    let state = &page.state;
    let freshness = if state.backing != "projection" {
        None
    } else if state.waiting > 0 {
        Some(m.with("ts-report-catching-up", &[&state.waiting.to_string()]))
    } else if let Some(at) = &state.computed_at {
        Some(m.with(
            "ts-report-kept-since",
            &[&format!("{}, {}", clock(at), date(m, at))],
        ))
    } else {
        Some(m.get("ts-report-kept"))
    };
    if let Some(f) = freshness {
        let _ = write!(s, "<p class=\"ts-report__state\">{}</p>", esc(&f));
    }
    if page.rows.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-report-none"))
        );
        if window.offset > 0 {
            s.push_str(&pager(m, report, window, None));
        }
        return s;
    }
    let first = window.offset + 1;
    let count = u32::try_from(page.rows.len()).unwrap_or(u32::MAX);
    let _ = write!(
        s,
        "<p class=\"ts-report__range\">{}</p>",
        esc(&m.with(
            "ts-report-showing",
            &[
                &count.to_string(),
                &first.to_string(),
                &(window.offset + count).to_string()
            ]
        ))
    );
    let nav = pager(m, report, window, page.next);
    s.push_str(&nav);
    let _ = write!(s, "<ol class=\"ts-report\" start=\"{first}\">");
    for row in &page.rows {
        let _ = write!(s, "<li>{}</li>", row_html(m, report, row));
    }
    s.push_str("</ol>");
    s.push_str(&nav);
    s
}

/// A link to a title, red where it does not exist.
fn title_link(title: &str, fragment: Option<&str>, missing: bool) -> String {
    let mut href = format!("/wiki/{}", title_url(title));
    let mut text = title.to_string();
    if let Some(f) = fragment.filter(|f| !f.is_empty()) {
        let _ = write!(href, "#{}", title_url(f));
        let _ = write!(text, "#{f}");
    }
    link(
        &href,
        &esc(&text),
        if missing { " class=\"new\"" } else { "" },
    )
}

fn row_html(m: &Messages, report: &Report, row: &ReportRow) -> String {
    let arrow = if m.dir() == "rtl" {
        " \u{2190} "
    } else {
        " \u{2192} "
    };
    if report.name == "ListProperties" {
        let id = row
            .title
            .rsplit_once(':')
            .map_or(row.title.as_str(), |(_, id)| id);
        return match row.label.as_deref().filter(|l| !l.is_empty()) {
            Some(label) => format!(
                "{} ({})",
                link(&format!("/wiki/{}", title_url(&row.title)), &esc(label), ""),
                esc(id)
            ),
            None => link(&format!("/wiki/{}", title_url(&row.title)), &esc(id), ""),
        };
    }
    let mut s = title_link(&row.title, None, report.missing);
    if let Some(target) = &row.target {
        s.push_str(arrow);
        s.push_str(&title_link(
            target,
            row.fragment.as_deref(),
            report.missing_target,
        ));
    }
    if let Some(last) = &row.last {
        s.push_str(arrow);
        s.push_str(&title_link(last, None, false));
    }
    let unit = match report.unit {
        Unit::None => None,
        Unit::Links => Some("ts-report-links"),
        Unit::Members => Some("ts-report-members"),
        Unit::Categories => Some("ts-report-categories"),
        Unit::Revisions => Some("ts-report-revisions"),
        Unit::Bytes => Some("ts-report-bytes"),
        Unit::Time => {
            if !row.value.is_empty() {
                let _ = write!(
                    s,
                    " ({}, {})",
                    esc(clock(&row.value)),
                    esc(&date(m, &row.value))
                );
            }
            None
        }
    };
    if let Some(key) = unit
        && !row.value.is_empty()
    {
        let _ = write!(s, " ({})", esc(&m.with(key, &[&row.value])));
    }
    s
}

/// **Previous**, **Next** and the page sizes, as MediaWiki's `viewprevnext` offers.
fn pager(m: &Messages, report: &Report, window: Window, next: Option<u32>) -> String {
    let size = window.limit.to_string();
    let previous = if window.offset > 0 {
        link(
            &Window {
                offset: window.offset.saturating_sub(window.limit),
                ..window
            }
            .href(report),
            &esc(&m.with("ts-report-previous", &[&size])),
            " rel=\"prev\"",
        )
    } else {
        esc(&m.with("ts-report-previous", &[&size]))
    };
    let following = match next {
        Some(offset) => link(
            &Window { offset, ..window }.href(report),
            &esc(&m.with("ts-report-next", &[&size])),
            " rel=\"next\"",
        ),
        None => esc(&m.with("ts-report-next", &[&size])),
    };
    let sizes: Vec<String> = SIZES
        .iter()
        .map(|&limit| {
            if limit == window.limit {
                limit.to_string()
            } else {
                link(
                    &Window { limit, ..window }.href(report),
                    &limit.to_string(),
                    "",
                )
            }
        })
        .collect();
    format!(
        "<nav class=\"ts-pager ts-report__pager\" aria-label=\"{}\">({previous} | {following}) ({})</nav>",
        esc(&m.get("ts-report-pages-label")),
        sizes.join(" | ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_aliases_and_case() {
        assert_eq!(Report::of("WantedPages").unwrap().name, "WantedPages");
        assert_eq!(Report::of("Wantedpages").unwrap().name, "WantedPages");
        assert_eq!(Report::of("BrokenLinks").unwrap().name, "WantedPages");
        assert_eq!(Report::of("Mostlinked").unwrap().name, "MostLinkedPages");
        assert_eq!(Report::of("OrphanedPages").unwrap().name, "LonelyPages");
        assert_eq!(
            Report::of("ListProperties").unwrap().title(),
            "Special:ListProperties"
        );
        assert!(Report::of("AllPages").is_none());
    }

    #[derive(serde::Deserialize)]
    struct Registry {
        page: Vec<Entry>,
    }

    #[derive(serde::Deserialize)]
    struct Entry {
        name: String,
        mediawiki_name: Option<String>,
        #[serde(default)]
        aliases: Vec<String>,
        status: String,
        report: Option<toml::Value>,
    }

    /// Every report page is a served report of the registry, under its names.
    #[test]
    fn the_reports_follow_the_registry() {
        let r: Registry =
            toml::from_str(include_str!("../../../../docs/registry/special-pages.toml"))
                .expect("special-pages.toml parses");
        for report in REPORTS {
            let e = r
                .page
                .iter()
                .find(|p| p.name == report.name)
                .unwrap_or_else(|| panic!("{} is not in the registry", report.name));
            assert!(e.report.is_some(), "{} is not a report", report.name);
            assert_eq!(e.status, "served", "{}", report.name);
            assert_eq!(
                e.mediawiki_name.as_deref(),
                report.mediawiki_name,
                "{}",
                report.name
            );
            let aliases: Vec<&str> = e.aliases.iter().map(String::as_str).collect();
            assert_eq!(aliases, report.aliases, "{}", report.name);
        }
    }

    #[test]
    fn windows_link_back_to_the_page() {
        let w = Window {
            offset: 100,
            limit: 20,
        };
        let r = Report::of("ShortPages").unwrap();
        assert_eq!(
            w.href(r),
            "/w/index.php?title=Special:ShortPages&limit=20&offset=100"
        );
        let w = Window {
            offset: 0,
            limit: LIMIT,
        };
        assert_eq!(w.href(r), "/w/index.php?title=Special:ShortPages");
    }
}
