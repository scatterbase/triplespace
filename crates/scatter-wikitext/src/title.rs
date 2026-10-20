//! Link targets: parsing `[[…]]` targets into titles, and the resolver the caller supplies
//! (22 §2.1: "the link resolver is passed in as a trait").

use crate::config::{Config, MAIN};
use crate::html::decode_entities;

/// A title a link points at.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Title {
    /// The interwiki prefix, lowercased, for a link that leaves the wiki.
    pub interwiki: Option<String>,
    /// The namespace.
    pub namespace: i32,
    /// The title within the namespace, with spaces, first letter normalized by the
    /// namespace's rule.
    pub text: String,
    /// The fragment after `#`, as written.
    pub fragment: Option<String>,
}

impl Title {
    /// A title with no fragment or interwiki prefix.
    #[must_use]
    pub fn new(namespace: i32, text: &str) -> Self {
        Self {
            interwiki: None,
            namespace,
            text: text.to_string(),
            fragment: None,
        }
    }

    /// The title with its namespace's local name, as MediaWiki writes it in a `title`
    /// attribute: `Project:Scope`, `User talk:Harej`.
    #[must_use]
    pub fn prefixed(&self, config: &Config) -> String {
        let local = if self.namespace == MAIN {
            self.text.clone()
        } else {
            let ns = config
                .namespace(self.namespace)
                .map_or("", |n| n.name.as_str());
            format!("{ns}:{}", self.text)
        };
        match &self.interwiki {
            Some(prefix) => format!("{prefix}:{local}"),
            None => local,
        }
    }

    /// The same title with no fragment.
    #[must_use]
    pub fn without_fragment(&self) -> Self {
        Self {
            fragment: None,
            ..self.clone()
        }
    }
}

/// What a resolver says about a local title.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageInfo {
    /// Whether the page exists (or the entity, for an entity namespace).
    pub exists: bool,
    /// Whether it is a redirect: its links carry `mw-redirect`.
    pub redirect: bool,
    /// The text a link without its own text shows instead of the title: an entity's
    /// label, as Wikibase renders it outside the entity namespaces.
    pub label: Option<String>,
}

/// Answers the questions rendering asks about titles. Implemented over the serving model
/// by the caller, and by fixtures in tests.
pub trait Resolver {
    /// Whether `title` exists, is a redirect, and the label its links show.
    fn info(&self, title: &Title) -> PageInfo;
    /// The URL of an existing page, without a fragment.
    fn url(&self, title: &Title) -> String;
    /// Where a link to a missing page goes.
    fn missing_url(&self, title: &Title) -> String;
}

/// A parsed link target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The title.
    pub title: Title,
    /// Whether the target began with `:` (`[[:Category:X]]`, a link and not membership).
    pub leading_colon: bool,
}

/// Parses a link target as MediaWiki resolves it: character references decoded, `_` as a
/// space, a leading `:`, `#fragment`, `/Sub` and `../Sibling` relative to the page where
/// its namespace has subpages, a namespace or interwiki prefix, and the namespace's
/// first-letter rule. `None` for a target that is not a valid title, which MediaWiki
/// renders as the literal text.
#[must_use]
pub fn parse_target(target: &str, config: &Config, page: &Title) -> Option<Target> {
    let decoded = decode_entities(target);
    let mut s = decoded.trim();
    let leading_colon = s.starts_with(':');
    if leading_colon {
        s = s[1..].trim_start();
    }
    let (body, fragment) = match s.split_once('#') {
        Some((b, f)) => (b, Some(f.to_string())),
        None => (s, None),
    };
    let mut body = clean(body);
    if body.is_empty() {
        // `[[#Section]]` links to the page itself.
        return fragment.map(|f| Target {
            title: Title {
                fragment: Some(f),
                ..page.without_fragment()
            },
            leading_colon,
        });
    }
    if body
        .chars()
        .any(|c| matches!(c, '[' | ']' | '{' | '}' | '|' | '<' | '>') || c.is_control())
    {
        return None;
    }

    let page_subpages = config.namespace(page.namespace).is_some_and(|n| n.subpages);
    if page_subpages
        && !leading_colon
        && let Some(rel) = relative(&body, &page.text)
    {
        return Some(Target {
            title: Title {
                fragment,
                ..Title::new(page.namespace, &rel)
            },
            leading_colon,
        });
    }

    let mut namespace = MAIN;
    let mut interwiki = None;
    if let Some((prefix, rest)) = body.split_once(':') {
        if let Some(ns) = config.namespace_by_name(prefix) {
            namespace = ns.id;
            body = clean(rest);
        } else if let Some(iw) = config.interwiki(prefix) {
            interwiki = Some(iw.prefix.to_lowercase());
            body = clean(rest);
        }
    }
    if body.is_empty() && interwiki.is_none() {
        return None;
    }
    let case_sensitive = interwiki.is_some()
        || config
            .namespace(namespace)
            .is_some_and(|n| n.case_sensitive);
    if !case_sensitive {
        body = upper_first(&body);
    }
    Some(Target {
        title: Title {
            interwiki,
            namespace,
            text: body,
            fragment,
        },
        leading_colon,
    })
}

/// `_` as a space, whitespace runs collapsed, ends trimmed.
fn clean(s: &str) -> String {
    s.replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Resolves `/Sub`, `../Sibling` and `../` against the page's title.
fn relative(target: &str, page: &str) -> Option<String> {
    if let Some(sub) = target.strip_prefix('/') {
        let sub = sub.trim_end_matches('/');
        return Some(if sub.is_empty() {
            page.to_string()
        } else {
            format!("{page}/{sub}")
        });
    }
    if !target.starts_with("../") {
        return None;
    }
    let mut base: Vec<&str> = page.split('/').collect();
    let mut rest = target;
    while let Some(r) = rest.strip_prefix("../") {
        if base.len() < 2 {
            return None;
        }
        base.pop();
        rest = r;
    }
    let rest = rest.trim_end_matches('/');
    let mut out = base.join("/");
    if !rest.is_empty() {
        out.push('/');
        out.push_str(rest);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigSource, Interwiki};

    fn config() -> Config {
        let mut s = ConfigSource::default();
        s.interwiki.push(Interwiki {
            prefix: "wikipedia".into(),
            url: "https://en.wikipedia.org/wiki/$1".into(),
        });
        Config::new(s)
    }

    #[test]
    fn targets() {
        let c = config();
        let page = Title::new(4, "Analytics");
        let t = |s: &str| {
            parse_target(s, &c, &page)
                .map(|t| (t.title.prefixed(&c), t.title.fragment, t.leading_colon))
        };
        assert_eq!(
            t("project:scope"),
            Some(("Project:Scope".into(), None, false))
        );
        assert_eq!(
            t("User_talk:harej#Top"),
            Some(("User talk:Harej".into(), Some("Top".into()), false))
        );
        assert_eq!(t(":Category:X"), Some(("Category:X".into(), None, true)));
        assert_eq!(
            t("/WCD"),
            Some(("Project:Analytics/WCD".into(), None, false))
        );
        assert_eq!(
            t("#Notes"),
            Some(("Project:Analytics".into(), Some("Notes".into()), false))
        );
        assert_eq!(
            t("wikipedia:Wikipedia:Consensus"),
            Some(("wikipedia:Wikipedia:Consensus".into(), None, false))
        );
        assert_eq!(t("foo bar"), Some(("Foo bar".into(), None, false)));
        assert_eq!(t("a<b"), None);
        assert_eq!(t("A&amp;B"), Some(("A&B".into(), None, false)));
        let sub = Title::new(4, "Analytics/WCD");
        assert_eq!(
            parse_target("../IABot", &c, &sub).map(|t| t.title.text),
            Some("Analytics/IABot".into())
        );
    }
}
