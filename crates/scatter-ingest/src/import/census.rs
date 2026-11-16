//! The template census that comes before an import (0042 §5; 10 §6; 11 §2.1): a pure
//! function over the export's pages. It says what the file holds by namespace and by
//! kind, which templates and modules the pages' latest revisions use and which of those
//! the file contains, which variables, parser functions and tags they call and with what
//! status, and which pages the job will not take: entity pages, which only adoption
//! brings (0008 A48), and pages in namespaces the tenant has no place for. The surface
//! adds which titles already exist, since only its title index can say.
//!
//! How a text is scanned is injected: the registry of variables, functions and tags is
//! `scatter-wikitext-expand`'s, which this crate does not depend on (22 §1.2 rule 3).
//! implements 0008 §9.2 (A49)

use std::collections::{BTreeMap, BTreeSet};

use scatter_mediawiki_xml::DumpPage;
use serde::{Deserialize, Serialize};

/// What the registry says of a call, as the scanner reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Expanded or rendered as MediaWiki does.
    Implemented,
    /// Recognised and rendered as a chip.
    Chip,
    /// Recognised and without effect.
    Ignored,
    /// Not in the registry.
    Unregistered,
}

/// What one text uses, as the scanner reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextUses {
    /// Template names as the Template namespace titles them.
    pub templates: BTreeSet<String>,
    /// Module names.
    pub modules: BTreeSet<String>,
    /// Variables and parser functions, with their status.
    pub functions: BTreeMap<String, Status>,
    /// Extension tags, with their status.
    pub tags: BTreeMap<String, Status>,
}

/// What the import will do with a page, as the surface's reader classifies it (10 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageClass {
    /// A document page the job imports.
    Page,
    /// A talk page, converted to threads after the job (0090 §10).
    Talk,
    /// An entity page, refused: only adoption brings entities (0008 A48).
    Entity,
    /// Left out, with the reason: a namespace or model the tenant has no place for.
    Deferred(String),
    /// Left out as something the instance does not use (0095 A1), with the reason.
    Ignored(String),
}

/// The census of an export.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Census {
    /// Pages in the file.
    pub pages: u64,
    /// Revisions in the file.
    pub revisions: u64,
    /// Whether the file is a current-revision export: every page has one revision while
    /// revisions name parents. An import of it brings no history.
    pub current_only: bool,
    /// Pages by namespace number.
    pub by_namespace: BTreeMap<i64, u64>,
    /// Document pages the job imports.
    pub imported: u64,
    /// Talk pages, converted to threads.
    pub talk: u64,
    /// Entity pages, refused.
    pub entities: u64,
    /// Pages deferred, by reason.
    pub deferred: BTreeMap<String, u64>,
    /// Pages ignored, by reason.
    pub ignored: BTreeMap<String, u64>,
    /// The titles the job imports, as the file spells them, with their namespace: what
    /// the surface checks for existence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub titles: Vec<(i64, String)>,
    /// Titles that already exist here, filled in by the surface; the job merges into them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub existing: Vec<String>,
    /// Templates the latest revisions use, with how many pages use each.
    pub templates_used: BTreeMap<String, u64>,
    /// Template pages the file contains.
    pub templates_in_file: BTreeSet<String>,
    /// Templates used but not in the file: what step 3 of 10 §6 cannot bring.
    pub templates_missing: BTreeSet<String>,
    /// Modules the latest revisions invoke, with how many pages invoke each.
    pub modules_used: BTreeMap<String, u64>,
    /// Module pages the file contains.
    pub modules_in_file: BTreeSet<String>,
    /// Modules invoked but not in the file.
    pub modules_missing: BTreeSet<String>,
    /// Variables and parser functions called, with their status.
    pub functions: BTreeMap<String, Status>,
    /// Extension tags found, with their status.
    pub tags: BTreeMap<String, Status>,
}

impl Census {
    /// Variables, functions and tags outside the registry.
    #[must_use]
    pub fn unregistered(&self) -> Vec<String> {
        self.functions
            .iter()
            .chain(self.tags.iter())
            .filter(|(_, s)| **s == Status::Unregistered)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Whether the templates and modules the pages use are all in the file, so that
    /// importing them is enough and nothing need be flattened (10 §6 step 3).
    #[must_use]
    pub fn self_contained(&self) -> bool {
        self.templates_missing.is_empty() && self.modules_missing.is_empty()
    }
}

/// The Template and Module namespace numbers, MediaWiki's.
const TEMPLATE: i64 = 10;
const MODULE: i64 = 828;

/// Takes the census of an export's pages, each with its class, scanning the latest
/// revision of every page the job imports or converts with `scan`.
pub fn census<'a>(
    pages: impl IntoIterator<Item = (&'a DumpPage, PageClass)>,
    scan: &dyn Fn(&str) -> TextUses,
) -> Census {
    let mut c = Census::default();
    let mut parents = false;
    let mut history = false;
    for (page, class) in pages {
        c.pages += 1;
        c.revisions += page.revisions.len() as u64;
        *c.by_namespace.entry(page.ns).or_default() += 1;
        if page.revisions.len() > 1 {
            history = true;
        }
        if page.revisions.iter().any(|r| r.parent_id.is_some()) {
            parents = true;
        }
        match class {
            PageClass::Page | PageClass::Talk => {
                if class == PageClass::Talk {
                    c.talk += 1;
                } else {
                    c.imported += 1;
                    c.titles.push((page.ns, page.title.clone()));
                    if page.ns == TEMPLATE {
                        c.templates_in_file.insert(local_title(page));
                    } else if page.ns == MODULE {
                        c.modules_in_file.insert(local_title(page));
                    }
                }
                let Some(latest) = page.latest() else {
                    continue;
                };
                if latest.text_hidden || latest.model.as_deref().is_some_and(|m| m != "wikitext") {
                    continue;
                }
                let uses = scan(&latest.text);
                for t in uses.templates {
                    *c.templates_used.entry(t).or_default() += 1;
                }
                for m in uses.modules {
                    *c.modules_used.entry(m).or_default() += 1;
                }
                for (f, s) in uses.functions {
                    c.functions.entry(f).or_insert(s);
                }
                for (t, s) in uses.tags {
                    c.tags.entry(t).or_insert(s);
                }
            }
            PageClass::Entity => c.entities += 1,
            PageClass::Deferred(why) => *c.deferred.entry(why).or_default() += 1,
            PageClass::Ignored(why) => *c.ignored.entry(why).or_default() += 1,
        }
    }
    c.current_only = c.pages > 0 && !history && parents;
    c.templates_missing = c
        .templates_used
        .keys()
        .filter(|t| !c.templates_in_file.contains(*t))
        .cloned()
        .collect();
    c.modules_missing = c
        .modules_used
        .keys()
        .filter(|m| !c.modules_in_file.contains(*m))
        .cloned()
        .collect();
    c
}

/// A page's title without its namespace prefix.
fn local_title(page: &DumpPage) -> String {
    if page.ns == 0 {
        return page.title.clone();
    }
    page.title
        .split_once(':')
        .map_or_else(|| page.title.clone(), |(_, t)| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_mediawiki_xml::Revision;

    fn page(ns: i64, title: &str, texts: &[&str]) -> DumpPage {
        DumpPage {
            title: title.into(),
            ns,
            id: 1,
            redirect: None,
            revisions: texts
                .iter()
                .enumerate()
                .map(|(i, t)| Revision {
                    id: i as u64 + 1,
                    parent_id: (i > 0).then_some(i as u64),
                    text: (*t).to_string(),
                    model: Some("wikitext".into()),
                    ..Revision::default()
                })
                .collect(),
        }
    }

    fn scanner(text: &str) -> TextUses {
        let mut u = TextUses::default();
        for word in text.split_whitespace() {
            if let Some(t) = word.strip_prefix("T:") {
                u.templates.insert(t.into());
            } else if let Some(m) = word.strip_prefix("M:") {
                u.modules.insert(m.into());
            } else if let Some(f) = word.strip_prefix("F:") {
                u.functions.insert(f.into(), Status::Unregistered);
            } else if let Some(t) = word.strip_prefix("X:") {
                u.tags.insert(t.into(), Status::Chip);
            }
        }
        u
    }

    #[test]
    fn counts_classes_and_what_the_file_holds() {
        let a = page(0, "A", &["T:Old", "T:Cite M:String F:#foo X:math"]);
        let t = page(10, "Template:Cite", &["x"]);
        let talk = page(1, "Talk:A", &["T:Box"]);
        let item = page(120, "Item:Q1", &["{}"]);
        let css = page(8, "MediaWiki:Common.css", &["body{}"]);
        let c = census(
            [
                (&a, PageClass::Page),
                (&t, PageClass::Page),
                (&talk, PageClass::Talk),
                (&item, PageClass::Entity),
                (
                    &css,
                    PageClass::Ignored("`MediaWiki` pages of model `css`".into()),
                ),
            ],
            &scanner,
        );
        assert_eq!((c.pages, c.revisions), (5, 6));
        assert_eq!((c.imported, c.talk, c.entities), (2, 1, 1));
        assert_eq!(c.ignored.values().sum::<u64>(), 1);
        assert!(!c.current_only);
        assert_eq!(c.by_namespace[&0], 1);
        assert_eq!(c.templates_used["Cite"], 1);
        assert!(
            !c.templates_used.contains_key("Old"),
            "the latest revision only"
        );
        assert_eq!(
            c.templates_used["Box"], 1,
            "the talk page's latest text counts"
        );
        assert!(c.templates_in_file.contains("Cite"));
        assert_eq!(
            c.templates_missing.iter().collect::<Vec<_>>(),
            [&"Box".to_string()]
        );
        assert!(c.modules_missing.contains("String"));
        assert!(c.templates_used.contains_key("Cite"));
        assert_eq!(c.tags["math"], Status::Chip);
        assert_eq!(c.unregistered(), vec!["#foo".to_string()]);
        assert!(!c.self_contained());
        assert_eq!(c.titles.len(), 2);
    }

    #[test]
    fn a_current_revision_export_is_told_apart() {
        let mut p = page(0, "A", &["one", "two"]);
        p.revisions.remove(0);
        let c = census([(&p, PageClass::Page)], &scanner);
        assert!(
            c.current_only,
            "one revision per page while revisions name parents"
        );
        let c = census([(&page(0, "B", &["one"]), PageClass::Page)], &scanner);
        assert!(
            !c.current_only,
            "a page with one revision and no parent is a new page"
        );
    }
}
