//! Titles read against the namespace registry: a title's namespace, and its talk page.

use scatter_titles::{Kind as NsKind, Namespace, Namespaces};

/// A namespace by its prefix: the canonical name or the local one (`Interface`).
pub fn namespace_named(prefix: &str) -> Option<&'static Namespace> {
    let p = prefix.replace('_', " ");
    let p = p.trim();
    Namespaces::embedded().iter().find(|n| {
        !n.canonical.is_empty()
            && (n.canonical.eq_ignore_ascii_case(p)
                || n.name.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(p)))
    })
}

/// A title's namespace and the rest of it; the main namespace where the prefix names none.
pub fn split(title: &str) -> Option<(&'static Namespace, String)> {
    if let Some((prefix, rest)) = title.split_once(':')
        && let Some(ns) = namespace_named(prefix)
    {
        return Some((ns, rest.trim().to_string()));
    }
    Some((Namespaces::embedded().get(0)?, title.trim().to_string()))
}

/// The talk page of a title, from the namespace registry; a talk title is its own. `None`
/// where the namespace has no talk pages (`Thread:`, `DOI:`). The title is canonical
/// ([`crate::pages::canonical`]): `Project:Home`, not the site's own name for the namespace.
pub fn talk_of(title: &str) -> Option<String> {
    let (ns, rest) = split(title)?;
    if ns.default_model.as_deref() == Some("triplespace-talk") {
        return Some(title.to_string());
    }
    let talk = Namespaces::embedded().get(ns.talk?)?;
    (talk.kind == NsKind::Pages).then(|| format!("{}:{rest}", talk.canonical))
}

/// A special page's name from its title: `Special:Search/foo` → `Search`, with the
/// first letter upper-cased, as MediaWiki's titles have it.
#[must_use]
pub fn special_name(title: &str) -> Option<String> {
    let rest = title.strip_prefix("Special:")?;
    let name = rest.split('/').next().unwrap_or(rest).trim();
    let mut chars = name.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::special_name;

    #[test]
    fn names() {
        assert_eq!(special_name("Special:Search").as_deref(), Some("Search"));
        assert_eq!(special_name("Special:search/x").as_deref(), Some("Search"));
        assert_eq!(special_name("Special:").as_deref(), None);
        assert_eq!(special_name("Item:Q6").as_deref(), None);
    }
}
