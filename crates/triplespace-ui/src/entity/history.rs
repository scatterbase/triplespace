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
//! Diff links, the upstream-edit fold, cluster members' rows, "History of this value"
//! and the log-only view come with the later Phase 4 pull requests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use triplespace_client::activity::ActivityQuery;
use triplespace_client::{ActivityPage, Provenance, SiteInfo};

use crate::frame::title_url;
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::special::changes::list_html;

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

/// The source switch (19 §3.5), or nothing when one source contributes.
fn switch_html(m: &Messages, site: &SiteInfo, title: &str, ask: &Ask, graphs: &[String]) -> String {
    if graphs.len() < 2 {
        return String::new();
    }
    let mut s = format!(
        "<nav class=\"ts-search-filter ts-history__switch\" aria-label=\"{}\"><ul>",
        esc(&m.get("ts-history-switch-label"))
    );
    let mut item = |label: String, source: Option<&str>| {
        let here = ask.source.as_deref() == source;
        let target = Ask {
            source: source.map(str::to_string),
            limit: ask.limit,
            cursor: None,
        };
        let _ = write!(
            s,
            "<li>{}</li>",
            link(
                &target.href(title, None),
                &esc(&label),
                if here { " aria-current=\"page\"" } else { "" }
            )
        );
    };
    item(m.get("ts-history-all-sources"), None);
    for g in graphs {
        let label = if g == "local" {
            m.get("ts-history-made-here")
        } else {
            graph_name(site, g)
        };
        item(label, Some(g));
    }
    s.push_str("</ul></nav>");
    s
}

/// The note that a mirror keeps only its latest state (19 §3.5).
fn latest_only_html(m: &Messages, site: &SiteInfo, provenance: Option<&Provenance>) -> String {
    let Some(p) = provenance else {
        return String::new();
    };
    let latest: Vec<String> = p
        .graphs
        .iter()
        .filter(|g| g.graph.starts_with("mirror/") && g.history.as_deref() == Some("latest"))
        .map(|g| graph_name(site, &g.graph))
        .collect();
    if latest.is_empty() {
        return String::new();
    }
    format!(
        "<aside class=\"ts-history__note\"><p>{}</p></aside>",
        esc(&m.with("ts-history-latest-only", &[&latest.join(", ")]))
    )
}

/// The pager: **Newest** when on a later page, **Older** when there is one.
fn pager_html(m: &Messages, title: &str, ask: &Ask, next: Option<&str>) -> String {
    if ask.cursor.is_none() && next.is_none() {
        return String::new();
    }
    let mut s = format!(
        "<nav class=\"ts-pager\" aria-label=\"{}\">",
        esc(&m.get("ts-search-pages-label"))
    );
    if ask.cursor.is_some() {
        let _ = write!(
            s,
            "{} ",
            link(
                &ask.href(title, None),
                &esc(&m.get("ts-changes-newest")),
                ""
            )
        );
    }
    if let Some(n) = next {
        s.push_str(&link(
            &ask.href(title, Some(n)),
            &esc(&m.get("ts-changes-older")),
            " rel=\"next\"",
        ));
    }
    s.push_str("</nav>");
    s
}

/// The tab's content.
#[must_use]
pub fn body(
    m: &Messages,
    site: &SiteInfo,
    title: &str,
    ask: &Ask,
    provenance: Option<&Provenance>,
    page: &ActivityPage,
) -> String {
    let graphs = contributing(provenance);
    let mut s = String::from("<div class=\"ts-history\">");
    s.push_str(&switch_html(m, site, title, ask, &graphs));
    s.push_str(&latest_only_html(m, site, provenance));
    if page.rows.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-history-none"))
        );
    } else {
        s.push_str(&list_html(m, site, &page.rows));
        s.push_str(&pager_html(m, title, ask, page.next.as_deref()));
    }
    s.push_str("</div>");
    s
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
