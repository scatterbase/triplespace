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

/// A title in one of the wiki's own namespaces, by its canonical name or an alias, and
/// the rest of it.
fn own_split<'a>(
    title: &str,
    own: &'a [triplespace_client::OwnNamespace],
) -> Option<(&'a triplespace_client::OwnNamespace, String)> {
    let (prefix, rest) = title.split_once(':')?;
    let p = prefix.replace('_', " ");
    let p = p.trim();
    own.iter()
        .find(|n| {
            n.canonical.eq_ignore_ascii_case(p)
                || n.aliases.iter().any(|a| a.eq_ignore_ascii_case(p))
        })
        .map(|n| (n, rest.trim().to_string()))
}

/// A title's namespace number and canonical name, the wiki's own namespaces read before
/// the registry's (10 §1.3 rule 4); the main namespace where the prefix names none.
#[must_use]
pub fn namespace_of(title: &str, own: &[triplespace_client::OwnNamespace]) -> (i32, String) {
    if let Some((n, _)) = own_split(title, own) {
        return (n.id, n.canonical.clone());
    }
    split(title).map_or((0, String::new()), |(ns, _)| (ns.id, ns.canonical.clone()))
}

/// The talk page of a title on a wiki with namespaces of its own (10 §1.3 rule 4): a title
/// in one of `own` has its talk namespace's, `{canonical} talk:{rest}`, where the subject
/// holds pages; any other title is [`talk_of`]'s.
pub fn talk_of_with(title: &str, own: &[triplespace_client::OwnNamespace]) -> Option<String> {
    let Some((n, rest)) = own_split(title, own) else {
        return talk_of(title);
    };
    if n.id % 2 == 1 {
        return Some(format!("{}:{rest}", n.canonical));
    }
    let talk = own.iter().find(|t| t.id == n.id + 1)?;
    (n.kind == "pages" && talk.kind == "pages").then(|| format!("{}:{rest}", talk.canonical))
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

/// The key of a subject page's tab on its talk page, as MediaWiki names it for the ID
/// `ca-nstab-…` (19 §5.10): `nstab-main` for the main namespace, `nstab-image` for `File:`,
/// and otherwise the namespace's canonical name in lower case, spaces as underscores
/// (`nstab-user`, `nstab-project`, `nstab-item`).
/// The wiki's own namespaces are read first, as [`namespace_of`] reads them.
#[must_use]
pub fn nstab_key(title: &str, own: &[triplespace_client::OwnNamespace]) -> String {
    let name = match namespace_of(title, own).1.as_str() {
        "" => "main".to_string(),
        "File" => "image".to_string(),
        c => c.to_lowercase().replace(' ', "_"),
    };
    format!("nstab-{name}")
}

#[cfg(test)]
mod tests {
    use super::{namespace_of, nstab_key, special_name, talk_of_with};
    use triplespace_client::OwnNamespace;

    // 10 §1.3 rule 4: a title in the wiki's own namespace has its own talk namespace.
    #[test]
    fn own_namespaces_have_talk_pages() {
        let ns = |id, c: &str, a: &[&str], k: &str| OwnNamespace {
            id,
            canonical: c.into(),
            aliases: a.iter().map(|s| (*s).to_string()).collect(),
            kind: k.into(),
        };
        let own = [
            ns(3000, "Recipe", &["R"], "pages"),
            ns(3001, "Recipe talk", &[], "pages"),
            ns(3002, "MHC", &["mhc"], "resolver"),
            ns(3003, "MHC talk", &[], "reserved"),
        ];
        assert_eq!(
            talk_of_with("Recipe:Soup", &own).as_deref(),
            Some("Recipe talk:Soup")
        );
        assert_eq!(
            talk_of_with("r:Soup", &own).as_deref(),
            Some("Recipe talk:Soup")
        );
        assert_eq!(
            talk_of_with("Recipe talk:Soup", &own).as_deref(),
            Some("Recipe talk:Soup")
        );
        assert_eq!(talk_of_with("mhc:Q1", &own), None);
        assert_eq!(
            talk_of_with("Project:Home", &own).as_deref(),
            Some("Project talk:Home")
        );
    }

    #[test]
    fn subject_tab_keys() {
        assert_eq!(nstab_key("Main Page", &[]), "nstab-main");
        assert_eq!(nstab_key("User:Alice", &[]), "nstab-user");
        assert_eq!(nstab_key("Project:Home", &[]), "nstab-project");
        assert_eq!(nstab_key("File:A.png", &[]), "nstab-image");
        let own = [OwnNamespace {
            id: 3000,
            canonical: "Recipe Book".into(),
            aliases: vec!["R".into()],
            kind: "pages".into(),
        }];
        assert_eq!(nstab_key("r:Soup", &own), "nstab-recipe_book");
        assert_eq!(namespace_of("Recipe Book:Soup", &own).0, 3000);
        assert_eq!(namespace_of("Talk:Soup", &own).0, 1);
    }

    #[test]
    fn names() {
        assert_eq!(special_name("Special:Search").as_deref(), Some("Search"));
        assert_eq!(special_name("Special:search/x").as_deref(), Some("Search"));
        assert_eq!(special_name("Special:").as_deref(), None);
        assert_eq!(special_name("Item:Q6").as_deref(), None);
    }
}
