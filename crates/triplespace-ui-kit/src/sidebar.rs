//! The wiki's own links in the menus (0097 §11; 19 §5.10): the interface message
//! `sidebar`, written on the page `Interface:Sidebar` as MediaWiki's `MediaWiki:Sidebar`
//! is, the instance's from the primary tenant's page and a wiki's over it (0095 §2).
//!
//! The text is MediaWiki's: a line `* heading` opens a section, and a line
//! `** target|label` is a link in it. The target is a message whose text is the link's
//! target, or else the target itself; either is a URL (`https://…`, `//…`) or a page's
//! title. The label is a message, or else the text itself. MediaWiki's placeholders for
//! the parts it draws itself (`SEARCH`, `TOOLBOX`, `LANGUAGES`) are sections with nothing
//! to add, since the search box, the tools and the language links are drawn anyway.
//!
//! Plain has no sidebar, so every section's links join the global header's links, in
//! order, after the site's own. A link whose key is one of the site's (MediaWiki's
//! default sidebar names `recentchanges`) takes that link's place, since the wiki's
//! choice comes first. The key is MediaWiki's: the label as written, spaces as hyphens,
//! which Plain draws as `n-{key}` as MediaWiki does.

use crate::frame::{Item, title_url};
use crate::i18n::Messages;

/// The sections MediaWiki fills itself, which add no links.
const PLACEHOLDERS: [&str; 3] = ["SEARCH", "TOOLBOX", "LANGUAGES"];

/// One link of the sidebar, as written: its target and its label, each a message key or
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The section it is in, as written.
    pub section: String,
    /// The target: a message key, a URL or a title.
    pub target: String,
    /// The label: a message key or text.
    pub label: String,
}

/// The links of a sidebar's text, in order. A link before any heading, a line with no
/// `|`, an empty target or label, and every link of a placeholder section are left out.
#[must_use]
pub fn parse(text: &str) -> Vec<Line> {
    let mut section: Option<String> = None;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(rest) = line.strip_prefix("**") {
            let Some(section) = section.as_ref() else {
                continue;
            };
            if PLACEHOLDERS.contains(&section.as_str()) {
                continue;
            }
            let mut parts = rest.split('|').map(str::trim);
            let (Some(target), Some(label)) = (parts.next(), parts.next()) else {
                continue;
            };
            if target.is_empty() || label.is_empty() {
                continue;
            }
            lines.push(Line {
                section: section.clone(),
                target: target.to_string(),
                label: label.to_string(),
            });
        } else if let Some(rest) = line.strip_prefix('*') {
            section = Some(rest.trim().to_string());
        }
    }
    lines
}

/// Whether a target is a URL rather than a title: a scheme MediaWiki links to, or `//`.
fn is_url(target: &str) -> bool {
    target.starts_with("//")
        || [
            "http://", "https://", "mailto:", "irc://", "ircs://", "ftp://",
        ]
        .iter()
        .any(|s| {
            target
                .get(..s.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(s))
        })
}

/// A message's text where the wiki or the build has it and it is not MediaWiki's `-`,
/// which turns a link off.
fn message(m: &Messages, key: &str) -> Option<String> {
    m.has(key)
        .then(|| m.get(key))
        .filter(|t| !t.trim().is_empty())
}

/// The links of the wiki's sidebar as items: its `sidebar` message read and resolved in
/// the page's language. None where the wiki has written no sidebar.
#[must_use]
pub fn items(m: &Messages) -> Vec<Item> {
    let Some(text) = message(m, "sidebar") else {
        return Vec::new();
    };
    parse(&text)
        .into_iter()
        .filter_map(|l| {
            let target = message(m, &l.target).unwrap_or_else(|| l.target.clone());
            let target = target.trim();
            if target == "-" {
                return None;
            }
            let href = if is_url(target) {
                target.to_string()
            } else {
                format!("/wiki/{}", title_url(target))
            };
            let (msg, label) = match message(m, &l.label) {
                Some(text) => (l.label.clone(), text),
                None => (String::new(), l.label.clone()),
            };
            Some(Item::labelled(
                &l.label.replace(' ', "-"),
                &msg,
                label,
                href,
            ))
        })
        .collect()
}

/// The global header's links with the wiki's added: each replaces the site's link of
/// the same key in its place, and the rest follow the site's, each key once.
#[must_use]
pub fn merge(mut site: Vec<Item>, wiki: Vec<Item>) -> Vec<Item> {
    for item in wiki {
        if let Some(at) = site.iter().position(|i| i.key == item.key) {
            site[at] = item;
        } else {
            site.push(item);
        }
    }
    site
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn with_sidebar(text: &str, more: &[(&str, &str)]) -> Messages {
        let mut o: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        o.entry("sidebar".into())
            .or_default()
            .insert(String::new(), text.into());
        for (k, v) in more {
            o.entry((*k).into())
                .or_default()
                .insert(String::new(), (*v).into());
        }
        Messages::for_language("en").with_overrides(o, "en")
    }

    // 0097 §11: MediaWiki's sidebar text, its placeholders left out.
    #[test]
    fn parses_mediawikis_text() {
        let lines = parse(
            "* navigation\n** mainpage|mainpage-description\n** https://example.org|Example site\n** nolabel\n* SEARCH\n** x|y\n* TOOLBOX\n* Projects\n** Project:Index|Index\n",
        );
        let got: Vec<_> = lines
            .iter()
            .map(|l| (l.section.as_str(), l.target.as_str(), l.label.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("navigation", "mainpage", "mainpage-description"),
                ("navigation", "https://example.org", "Example site"),
                ("Projects", "Project:Index", "Index"),
            ]
        );
    }

    // 0097 §11: targets and labels resolve through messages, else stand as written; a
    // wiki's link replaces the site's of the same key.
    #[test]
    fn resolves_and_merges() {
        let m = with_sidebar(
            "* navigation\n** recentchanges-url|recentchanges\n** Help:Contents|Help pages\n** https://example.org|Example\n** off|gone\n",
            &[
                ("recentchanges-url", "Special:RecentChanges"),
                ("recentchanges", "What changed"),
                ("off", "-"),
            ],
        );
        let wiki = items(&m);
        let got: Vec<_> = wiki
            .iter()
            .map(|i| (i.key.as_str(), i.label.as_str(), i.href.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "recentchanges",
                    "What changed",
                    "/wiki/Special:RecentChanges"
                ),
                ("Help-pages", "Help pages", "/wiki/Help:Contents"),
                ("Example", "Example", "https://example.org"),
            ]
        );
        let site = vec![
            Item::labelled("recentchanges", "", "Recent changes".into(), "/rc"),
            Item::labelled("specialpages", "", "Special pages".into(), "/sp"),
        ];
        let merged = merge(site, wiki);
        let keys: Vec<_> = merged.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            ["recentchanges", "specialpages", "Help-pages", "Example"]
        );
        assert_eq!(merged[0].label, "What changed");
        // No sidebar written: nothing added.
        assert!(items(&Messages::for_language("en")).is_empty());
    }
}
