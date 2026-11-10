//! The History tab of an entity page (0010 §5; 19 §3.3–3.5): `?action=history` on the
//! entity's title, as MediaWiki addresses a history.
//!
//! The rows are `GET /entity/{id}/history`'s, drawn as `Special:RecentChanges` draws
//! its rows ([`crate::special::changes`]): local edits with the Local chip, mirror syncs
//! with the provider's chip and "Synced from …", log events, erased records as gap
//! rows; grouped by day, newest first, paged by the feed's cursor.
//!
//! - **The source switch** above the list offers *All sources*, *Made here* and one
//!   entry per mirror that contributes to the entity, from the provenance's graphs; it
//!   is hidden when only one source contributes, so a purely local item's history looks
//!   like MediaWiki's (19 §3.5).
//! - **Only the latest state is kept here.** When a contributing mirror's history policy
//!   is `latest`, a note says so beside the list (19 §3.5). **Keep full history here…**
//!   and the link to the provider's history wait for the retention write and the
//!   provider registry's history URL.
//!
//! - **Compare** (19 §3.7): rows with a local revision carry radio buttons for the older
//!   and the newer side, in a form that opens `?oldid=…&diff=…` ([`CompareForm`]); a
//!   mirror's syncs have no revision ID to compare yet, and a discussion's history has no
//!   form.
//!
//! The upstream-edit fold, cluster members' rows, "History of this value"
//! and the log-only view come with the later Phase 4 pull requests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use triplespace_client::activity::ActivityQuery;
use triplespace_client::{ActivityPage, ActivityRow, Provenance, SiteInfo};

use serde::Serialize;

use crate::frame::title_url;
use crate::html::esc;
use crate::i18n::Messages;
use crate::pages::Context;
use crate::special::changes::{self, ListModel, Pick, Radio};

/// Rows per page, and the most allowed.
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 500;

/// What the query asks of the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// One graph, or every graph.
    pub source: Option<String>,
    /// Rows per page.
    pub limit: u32,
    /// The feed's cursor.
    pub cursor: Option<String>,
    /// The filters of the panel (19 §3.3).
    pub filter: Filter,
}

/// A history's filters, the ones recent changes and the watchlist have (19 §3.3): one
/// actor's rows, without bot or minor edits, with a tag, between two days; and for an
/// entity, the edits that touched a part of it, optionally by language, site or
/// property (16 §4.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only this user's rows.
    pub user: Option<String>,
    /// Without bot edits.
    pub hide_bots: bool,
    /// Without minor edits.
    pub hide_minor: bool,
    /// Only rows with this tag.
    pub tag: Option<String>,
    /// From this day, `YYYY-MM-DD`.
    pub from: Option<String>,
    /// To the end of this day.
    pub to: Option<String>,
    /// What changed: one of [`CHANGED`]'s names.
    pub changed: Option<String>,
    /// The language, site or property of what changed.
    pub key: Option<String>,
}

/// What an entity's history can be narrowed to, by the name the form sends: the parts
/// of a change set's summary each stands for (16 §4.1), and its message.
const CHANGED: [(&str, &[&str], &str); 5] = [
    ("labels", &["labels"], "ts-history-changed-labels"),
    (
        "descriptions",
        &["descriptions"],
        "ts-history-changed-descriptions",
    ),
    ("aliases", &["aliases"], "ts-history-changed-aliases"),
    ("sitelinks", &["sitelinks"], "ts-history-changed-sitelinks"),
    (
        "statements",
        &["claims", "statements", "references", "qualifiers"],
        "ts-history-changed-statements",
    ),
];

/// A day as the date field sends it.
fn day(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        }))
    .then(|| s.to_string())
}

impl Filter {
    fn of(q: &BTreeMap<String, String>) -> Self {
        let text = |k: &str| {
            q.get(k)
                .map(|v| v.replace('_', " ").trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let code = |k: &str| {
            q.get(k).map(|v| v.trim().to_string()).filter(|v| {
                !v.is_empty()
                    && v.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
        };
        Self {
            user: text("user"),
            hide_bots: q.get("hidebots").is_some_and(|v| v == "1"),
            hide_minor: q.get("hideminor").is_some_and(|v| v == "1"),
            tag: q
                .get("tagfilter")
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            from: q.get("from").and_then(|d| day(d.trim())),
            to: q.get("to").and_then(|d| day(d.trim())),
            changed: q
                .get("changed")
                .filter(|c| CHANGED.iter().any(|(n, _, _)| n == c))
                .cloned(),
            key: code("changedkey"),
        }
    }

    /// Whether anything is narrowed.
    fn any(&self) -> bool {
        *self != Self::default()
    }

    /// The filters as the address carries them.
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut v = Vec::new();
        let mut push = |k: &'static str, o: &Option<String>| {
            if let Some(x) = o {
                v.push((k, x.clone()));
            }
        };
        push("user", &self.user);
        push("tagfilter", &self.tag);
        push("from", &self.from);
        push("to", &self.to);
        push("changed", &self.changed);
        push("changedkey", &self.key);
        if self.hide_bots {
            v.push(("hidebots", "1".into()));
        }
        if self.hide_minor {
            v.push(("hideminor", "1".into()));
        }
        v
    }

    /// The `changed` values the feed takes: each part the choice stands for, with the key.
    fn parts(&self) -> Vec<String> {
        let Some((_, parts, _)) = self
            .changed
            .as_ref()
            .and_then(|c| CHANGED.iter().find(|(n, _, _)| n == c))
        else {
            return Vec::new();
        };
        parts
            .iter()
            // A removed statement is recorded by its ID, not its property.
            .filter(|p| !(self.key.is_some() && **p == "statements"))
            .map(|p| match &self.key {
                Some(k) => format!("{p}:{k}"),
                None => (*p).to_string(),
            })
            .collect()
    }
}

impl Ask {
    /// From the page's query.
    #[must_use]
    pub fn of(query: &BTreeMap<String, String>) -> Self {
        Self {
            source: query
                .get("source")
                .filter(|s| {
                    !s.is_empty()
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_')
                })
                .cloned(),
            limit: query
                .get("limit")
                .and_then(|l| l.parse::<u32>().ok())
                .map_or(DEFAULT_LIMIT, |l| l.clamp(1, MAX_LIMIT)),
            cursor: query.get("continue").cloned().filter(|c| !c.is_empty()),
            filter: Filter::of(query),
        }
    }

    /// The feed to fetch.
    #[must_use]
    pub fn query(&self, lang: &str) -> ActivityQuery {
        ActivityQuery {
            sources: self.source.iter().cloned().collect(),
            limit: self.limit,
            cursor: self.cursor.clone(),
            lang: lang.to_string(),
            actor: self.filter.user.clone(),
            show: [
                self.filter.hide_bots.then_some("!bot"),
                self.filter.hide_minor.then_some("!minor"),
            ]
            .into_iter()
            .flatten()
            .map(str::to_string)
            .collect(),
            tag: self.filter.tag.clone(),
            changed: self.filter.parts(),
            since: self.filter.from.as_ref().map(|d| format!("{d}T00:00:00Z")),
            until: self.filter.to.as_ref().map(|d| format!("{d}T23:59:59Z")),
            ..ActivityQuery::default()
        }
    }

    /// The tab's URL with these parameters and a cursor.
    fn href(&self, title: &str, cursor: Option<&str>) -> String {
        let mut s = format!("/wiki/{}?action=history", title_url(title));
        if let Some(src) = &self.source {
            let _ = write!(s, "&source={}", esc(src));
        }
        if self.limit != DEFAULT_LIMIT {
            let _ = write!(s, "&limit={}", self.limit);
        }
        for (k, v) in self.filter.pairs() {
            let _ = write!(
                s,
                "&{k}={}",
                percent_encoding::utf8_percent_encode(&v, percent_encoding::NON_ALPHANUMERIC)
            );
        }
        if let Some(c) = cursor {
            let _ = write!(
                s,
                "&continue={}",
                percent_encoding::utf8_percent_encode(c, percent_encoding::NON_ALPHANUMERIC)
            );
        }
        s
    }
}

/// The graphs that contribute to the entity, local first.
fn contributing(provenance: Option<&Provenance>) -> Vec<String> {
    let mut graphs: Vec<String> = provenance
        .map(|p| p.graphs.iter().map(|g| g.graph.clone()).collect())
        .unwrap_or_default();
    graphs.sort_by_key(|g| (g != "local", g.clone()));
    graphs.dedup();
    graphs
}

/// A mirror graph's display name: the provider's name, or the slug.
fn graph_name(site: &SiteInfo, graph: &str) -> String {
    graph
        .strip_prefix("mirror/")
        .and_then(|slug| site.providers.iter().find(|p| p.slug == slug))
        .map_or_else(|| graph.to_string(), |p| p.name.clone())
}

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `entity/history.html` draws.
// ---------------------------------------------------------------------------------------

/// A link in a switch or a pager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// The page.
    pub href: String,
    /// The text.
    pub label: String,
    /// Whether it is the current one.
    pub current: bool,
}

/// The source switch (19 §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Switch {
    /// The accessible name.
    pub label: String,
    /// *All sources*, *Made here*, then each mirror.
    pub items: Vec<Choice>,
}

/// The pager: **Newest** when on a later page, **Older** when there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pager {
    /// The accessible name.
    pub label: String,
    /// The first page.
    pub newest: Option<Choice>,
    /// The next page.
    pub older: Option<Choice>,
}

/// The History tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryModel {
    /// The source switch, when more than one source contributes.
    pub switch: Option<Switch>,
    /// The note that a mirror keeps only its latest state.
    pub note: Option<String>,
    /// What the tab says when it has no rows.
    pub empty: Option<String>,
    /// The rows as `Special:RecentChanges` draws them.
    pub list: Option<crate::special::changes::ListModel>,
    /// The pager, when there is another page.
    pub pager: Option<Pager>,
    /// The compare form, when two of the rows can be compared (19 §3.7).
    pub compare: Option<CompareForm>,
    /// The filter panel (19 §3.3).
    pub filter: FilterForm,
}

/// The filter panel of a history: a disclosure, open while a filter is on, holding a form
/// that works by `GET`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilterForm {
    /// The disclosure's summary.
    pub label: String,
    /// Whether a filter is on.
    pub open: bool,
    /// The page's title, for `title=`.
    pub title: String,
    /// The source in force, kept across the form.
    pub source: Option<String>,
    /// The fields.
    pub items: Vec<changes::FormItem>,
    /// The button.
    pub apply: String,
    /// The way back to the whole history, while a filter is on.
    pub clear: Option<Choice>,
}

/// A date field.
fn date_item(label: String, name: &str, value: Option<&str>) -> changes::FormItem {
    changes::FormItem::Field {
        label,
        control: crate::special::controls::ControlModel::Input {
            id: None,
            name: name.to_string(),
            input_type: "date".into(),
            value: value.unwrap_or_default().to_string(),
            required: false,
            inputmode: None,
            described_by: None,
        },
    }
}

/// The panel. `entity` adds what an entity's change sets record: what changed, and its
/// language, site or property.
fn filter_model(m: &Messages, title: &str, ask: &Ask, entity: bool) -> FilterForm {
    use changes::FormItem;
    let f = &ask.filter;
    let mut items = vec![
        FormItem::text(
            m.get("ts-history-filter-user"),
            "user",
            f.user.as_deref().unwrap_or(""),
        ),
        FormItem::text(
            m.get("ts-history-filter-tag"),
            "tagfilter",
            f.tag.as_deref().unwrap_or(""),
        ),
        date_item(m.get("ts-history-filter-from"), "from", f.from.as_deref()),
        date_item(m.get("ts-history-filter-to"), "to", f.to.as_deref()),
    ];
    if entity {
        let mut options = vec![(
            String::new(),
            m.get("ts-history-changed-any"),
            f.changed.is_none(),
        )];
        options.extend(CHANGED.iter().map(|(name, _, msg)| {
            (
                (*name).to_string(),
                m.get(msg),
                f.changed.as_deref() == Some(*name),
            )
        }));
        items.push(FormItem::select(
            m.get("ts-history-filter-changed"),
            "changed",
            options,
        ));
        items.push(FormItem::text(
            m.get("ts-history-filter-key"),
            "changedkey",
            f.key.as_deref().unwrap_or(""),
        ));
    }
    items.push(FormItem::group(
        m.get("ts-history-filter-hide"),
        vec![
            FormItem::check(
                "hidebots",
                "1",
                f.hide_bots,
                m.get("ts-history-filter-bots"),
            ),
            FormItem::check(
                "hideminor",
                "1",
                f.hide_minor,
                m.get("ts-history-filter-minor"),
            ),
        ],
    ));
    FilterForm {
        label: m.get("ts-history-filter"),
        open: f.any(),
        title: title.to_string(),
        source: ask.source.clone(),
        items,
        apply: m.get("ts-history-filter-apply"),
        clear: f.any().then(|| Choice {
            href: Ask {
                source: ask.source.clone(),
                limit: ask.limit,
                cursor: None,
                filter: Filter::default(),
            }
            .href(title, None),
            label: m.get("ts-history-filter-clear"),
            current: false,
        }),
    }
}

/// The form around a history's rows that compares the two revisions chosen with its
/// radio buttons, as MediaWiki's history does: a GET to `/w/index.php` with `title`,
/// `oldid` and `diff`, so it works without JavaScript (0010 §5.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompareForm {
    /// The page's title, for `title=`.
    pub title: String,
    /// The button.
    pub button: String,
}

/// Adds the compare column to a history's rows: each row that has a local revision gets
/// radio buttons for the older side (`oldid`) and the newer (`diff`), the newest two
/// chosen to begin with, and the newest of the history no older-side button, since nothing
/// is newer. `None` when fewer than two rows can be compared.
fn compare_model(
    m: &Messages,
    title: &str,
    ask: &Ask,
    rows: &[ActivityRow],
    list: &mut ListModel,
) -> Option<CompareForm> {
    let revids: Vec<Option<u64>> = rows.iter().map(changes::comparable).collect();
    if revids.iter().flatten().count() < 2 {
        return None;
    }
    let first_page = ask.cursor.is_none();
    let mut seen = 0usize;
    let models = list.days.iter_mut().flat_map(|d| d.rows.iter_mut());
    for ((row, revid), model) in rows.iter().zip(&revids).zip(models) {
        let Some(revid) = *revid else {
            model.pick = Some(Pick::default());
            continue;
        };
        let at = format!(
            "{}, {}",
            changes::clock(&row.time),
            changes::date(m, &row.time)
        );
        let radio = |key: &str, checked: bool| Radio {
            revid,
            label: m.with(key, &[&at]),
            checked,
        };
        model.pick = Some(Pick {
            old: (!(first_page && seen == 0)).then(|| radio("ts-history-compare-from", seen == 1)),
            new: Some(radio("ts-history-compare-to", seen == 0)),
        });
        seen += 1;
    }
    list.columns
        .insert(0, ("compare", m.get("ts-history-compare-col")));
    Some(CompareForm {
        title: title.to_string(),
        button: m.get("ts-history-compare"),
    })
}

fn switch_model(
    m: &Messages,
    site: &SiteInfo,
    title: &str,
    ask: &Ask,
    graphs: &[String],
) -> Option<Switch> {
    if graphs.len() < 2 {
        return None;
    }
    let item = |label: String, source: Option<&str>| Choice {
        href: Ask {
            source: source.map(str::to_string),
            limit: ask.limit,
            cursor: None,
            filter: ask.filter.clone(),
        }
        .href(title, None),
        label,
        current: ask.source.as_deref() == source,
    };
    let mut items = vec![item(m.get("ts-history-all-sources"), None)];
    for g in graphs {
        let label = if g == "local" {
            m.get("ts-history-made-here")
        } else {
            graph_name(site, g)
        };
        items.push(item(label, Some(g)));
    }
    Some(Switch {
        label: m.get("ts-history-switch-label"),
        items,
    })
}

fn latest_only_model(
    m: &Messages,
    site: &SiteInfo,
    provenance: Option<&Provenance>,
) -> Option<String> {
    let latest: Vec<String> = provenance?
        .graphs
        .iter()
        .filter(|g| g.graph.starts_with("mirror/") && g.history.as_deref() == Some("latest"))
        .map(|g| graph_name(site, &g.graph))
        .collect();
    if latest.is_empty() {
        return None;
    }
    Some(m.with("ts-history-latest-only", &[&latest.join(", ")]))
}

fn pager_model(m: &Messages, title: &str, ask: &Ask, next: Option<&str>) -> Option<Pager> {
    if ask.cursor.is_none() && next.is_none() {
        return None;
    }
    Some(Pager {
        label: m.get("ts-search-pages-label"),
        newest: ask.cursor.as_ref().map(|_| Choice {
            href: ask.href(title, None),
            label: m.get("ts-changes-newest"),
            current: false,
        }),
        older: next.map(|n| Choice {
            href: ask.href(title, Some(n)),
            label: m.get("ts-changes-older"),
            current: false,
        }),
    })
}

/// The tab's model: the rows read relative to the talk page `here` when there is one.
fn model(
    cx: &Context,
    title: &str,
    ask: &Ask,
    provenance: Option<&Provenance>,
    here: Option<u64>,
    page: &ActivityPage,
    switch: bool,
) -> HistoryModel {
    let (m, site) = (&cx.m, &cx.site);
    let graphs = contributing(provenance);
    let mut list = crate::special::changes::list_model_here(
        m,
        site,
        &page.rows,
        here,
        crate::special::changes::RowActions::of(cx),
    );
    // A discussion's history has no diffs to compare (16 §6.1).
    let compare = if switch {
        list.as_mut()
            .and_then(|l| compare_model(m, title, ask, &page.rows, l))
    } else {
        None
    };
    HistoryModel {
        switch: switch
            .then(|| switch_model(m, site, title, ask, &graphs))
            .flatten(),
        note: switch
            .then(|| latest_only_model(m, site, provenance))
            .flatten(),
        empty: page.rows.is_empty().then(|| {
            m.get(if ask.filter.any() {
                "ts-history-none-filtered"
            } else {
                "ts-history-none"
            })
        }),
        filter: filter_model(m, title, ask, provenance.is_some()),
        list,
        pager: if page.rows.is_empty() {
            None
        } else {
            pager_model(m, title, ask, page.next.as_deref())
        },
        compare,
    }
}

/// The History tab of a talk page or thread (16 §6.1): the rows as [`body`] draws them, read
/// relative to the talk page `here`, with the pager; no source switch, since every row of a
/// discussion is local.
#[must_use]
pub fn body_here(
    cx: &Context,
    title: &str,
    ask: &Ask,
    here: Option<u64>,
    page: &ActivityPage,
) -> String {
    cx.draw(
        "entity/history.html",
        model(cx, title, ask, None, here, page, false),
    )
}

/// The tab's content.
#[must_use]
pub fn body(
    cx: &Context,
    title: &str,
    ask: &Ask,
    provenance: Option<&Provenance>,
    page: &ActivityPage,
) -> String {
    cx.draw(
        "entity/history.html",
        model(cx, title, ask, provenance, None, page, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::entity::GraphStanding;

    #[test]
    fn the_ask_reads_the_query() {
        let mut q = BTreeMap::new();
        let a = Ask::of(&q);
        assert_eq!(a.source, None);
        assert_eq!(a.limit, DEFAULT_LIMIT);
        assert_eq!(a.href("Item:Q6", None), "/wiki/Item:Q6?action=history");
        q.insert("source".into(), "mirror/wikidata".into());
        q.insert("limit".into(), "9999".into());
        q.insert("continue".into(), "1|2|3".into());
        let a = Ask::of(&q);
        assert_eq!(a.source.as_deref(), Some("mirror/wikidata"));
        assert_eq!(a.limit, MAX_LIMIT);
        assert_eq!(
            a.href("Item:Q6", Some("4|5|6")),
            "/wiki/Item:Q6?action=history&source=mirror/wikidata&limit=500&continue=4%7C5%7C6"
        );
        q.insert("source".into(), "<x>".into());
        assert_eq!(Ask::of(&q).source, None, "a source is a graph name");
    }

    // 19 §3.3; 0010 A56: a history's filters are read from the address, kept in its
    // links, and sent to the feed.
    #[test]
    fn the_filters_reach_the_feed_and_stay_in_the_address() {
        let q: BTreeMap<String, String> = [
            ("user", "Alice_Bot"),
            ("tagfilter", "job:3"),
            ("from", "2026-11-01"),
            ("to", "not a day"),
            ("hidebots", "1"),
            ("changed", "statements"),
            ("changedkey", "P31"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let a = Ask::of(&q);
        assert_eq!(a.filter.user.as_deref(), Some("Alice Bot"));
        assert_eq!(a.filter.to, None, "a day is YYYY-MM-DD");
        let feed = a.query("en");
        assert_eq!(feed.actor.as_deref(), Some("Alice Bot"));
        assert_eq!(feed.show, ["!bot"]);
        assert_eq!(feed.tag.as_deref(), Some("job:3"));
        assert_eq!(feed.since.as_deref(), Some("2026-11-01T00:00:00Z"));
        assert_eq!(feed.until, None);
        // A removed statement is recorded by its ID, so a property leaves that part out.
        assert_eq!(
            feed.changed,
            ["claims:P31", "references:P31", "qualifiers:P31"]
        );
        assert_eq!(
            a.href("Item:Q6", None),
            "/wiki/Item:Q6?action=history&user=Alice%20Bot&tagfilter=job%3A3&from=2026%2D11%2D01&changed=statements&changedkey=P31&hidebots=1"
        );
        // An unknown part and a key that is not a code are dropped.
        let mut q = BTreeMap::new();
        q.insert("changed".to_string(), "everything".to_string());
        q.insert("changedkey".to_string(), "a b|c".to_string());
        let a = Ask::of(&q);
        assert!(!a.filter.any());
        assert!(a.query("en").changed.is_empty());
        // The panel: open while a filter is on, with the way back; an entity's has what
        // changed.
        let m = Messages::for_language("en");
        let on = Ask::of(
            &[("hideminor".to_string(), "1".to_string())]
                .into_iter()
                .collect(),
        );
        let f = filter_model(&m, "Item:Q6", &on, true);
        assert!(f.open);
        assert_eq!(
            f.clear.as_ref().map(|c| c.href.as_str()),
            Some("/wiki/Item:Q6?action=history")
        );
        let names = |f: &FilterForm| -> Vec<String> {
            f.items
                .iter()
                .filter_map(|i| i.name().map(str::to_string))
                .collect()
        };
        assert!(names(&f).contains(&"changed".to_string()));
        let page = filter_model(&m, "Project:Home", &Ask::of(&BTreeMap::new()), false);
        assert!(!page.open && page.clear.is_none());
        assert!(!names(&page).contains(&"changed".to_string()));
    }

    #[test]
    fn contributing_graphs_are_local_first() {
        let p = Provenance {
            graphs: vec![
                GraphStanding {
                    graph: "mirror/wikidata".into(),
                    ..GraphStanding::default()
                },
                GraphStanding {
                    graph: "local".into(),
                    ..GraphStanding::default()
                },
            ],
            ..Provenance::default()
        };
        assert_eq!(contributing(Some(&p)), vec!["local", "mirror/wikidata"]);
        assert!(contributing(None).is_empty());
    }
}
