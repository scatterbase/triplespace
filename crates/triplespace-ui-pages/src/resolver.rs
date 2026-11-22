//! Viewing a title in a resolver namespace (06 §3.3; 0029 §3, A12):
//! `DOI:10.1000/xyz` is looked up through `GET /resolve`, never stored.
//!
//! - **One entity, holding one value for the key**: a `303` to its page.
//! - **No entity**: a page saying so, with the external link and, for a signed-in reader,
//!   *Create an item with this DOI*, which opens `Special:NewItem` with
//!   `statement={property}:{key}` filled in. A `404`, as a missing page is.
//! - **Several entities**: a disambiguation page listing them with their labels and
//!   descriptions, and a note that they may be duplicates to merge.
//! - **One entity with several values**: a page showing the entity and each of its values,
//!   each linked in the resolver form.
//! - **Not a valid key**: a page saying so, a `404`.
//!
//! The title is read as the path gives it, before underscores become spaces, since a
//! resolver's grammar, not MediaWiki's title rules, decides what a key may contain (06
//! §3.2).

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use triplespace_client::resolve::Resolution;
use triplespace_ui_kit::Site;
use triplespace_ui_kit::frame::{Page, title_url};
use triplespace_ui_kit::pages::{self, Context, Params, Peer};

/// Whether a raw title is in a resolver namespace: its prefix is the canonical name or an
/// alias of a `resolver` namespace, in any case, and something follows it.
#[must_use]
pub fn is_resolver_title(raw: &str) -> bool {
    let Some((prefix, rest)) = raw.split_once(':') else {
        return false;
    };
    let prefix = prefix.replace('_', " ");
    !rest.trim().is_empty()
        && scatter_titles::Namespaces::embedded()
            .iter()
            .filter(|n| n.kind == scatter_titles::Kind::Resolver)
            .any(|n| {
                n.canonical.eq_ignore_ascii_case(prefix.trim())
                    || n.aliases
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(prefix.trim()))
            })
}

/// A link the template draws.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Link {
    /// Where it goes.
    pub href: String,
    /// Its text.
    pub text: String,
}

/// One entry of the page's list: an entity with its description, or a value.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    /// The entity or the value, linked.
    pub link: Link,
    /// The entity's ID, beside its label.
    pub id: Option<String>,
    /// The entity's description.
    pub description: Option<String>,
}

/// What `resolver.html` draws.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ResolverPage {
    /// What the page says of the key.
    pub text: String,
    /// The entities or values listed.
    pub entries: Vec<Entry>,
    /// The note under the list.
    pub note: Option<String>,
    /// Where the key resolves outside the instance.
    pub external: Option<Link>,
    /// *Create an item with this DOI*, for a signed-in reader.
    pub create: Option<Link>,
}

/// A `303` to a title's page (06 §3.3): the lookup's answer, which a later edit may
/// change, so the redirect is not permanent.
fn see_other(title: &str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, format!("/wiki/{}", title_url(title))),
            (header::CACHE_CONTROL, "private, no-cache".to_string()),
        ],
    )
        .into_response()
}

/// The page's model for a resolver's answer that is not a redirect; the status beside it.
fn model(
    cx: &Context,
    r: &Resolution,
    labels: &std::collections::BTreeMap<String, triplespace_client::entity::Label>,
    descriptions: &std::collections::BTreeMap<String, triplespace_client::entity::Label>,
) -> (StatusCode, ResolverPage) {
    let m = &cx.m;
    let label = r.label.clone().unwrap_or_default();
    let key = r.key.clone().unwrap_or_default();
    let namespace = r.namespace.clone().unwrap_or_default();
    let external = r.external.clone().map(|href| Link {
        text: m.with("ts-resolver-external", &[&label, &key]),
        href,
    });
    let entity = |id: &str, title: &str| Entry {
        link: Link {
            href: format!("/wiki/{}", title_url(title)),
            text: labels
                .get(id)
                .map_or_else(|| id.to_string(), |l| l.value.clone()),
        },
        id: labels.contains_key(id).then(|| id.to_string()),
        description: descriptions.get(id).map(|d| d.value.clone()),
    };
    if r.invalid.is_some() {
        let page = ResolverPage {
            text: m.with("ts-resolver-invalid", &[&label]),
            ..ResolverPage::default()
        };
        return (StatusCode::NOT_FOUND, page);
    }
    match r.candidates.as_slice() {
        [] => {
            let create = cx
                .user
                .is_some()
                .then(|| r.properties.first())
                .flatten()
                .map(|p| {
                    let statement = percent_encoding::utf8_percent_encode(
                        &format!("{p}:{key}"),
                        percent_encoding::NON_ALPHANUMERIC,
                    )
                    .to_string();
                    Link {
                        href: format!("/wiki/Special:NewItem?statement={statement}"),
                        text: m.with("ts-resolver-create", &[&label]),
                    }
                });
            let page = ResolverPage {
                text: m.with("ts-resolver-none", &[&label, &key]),
                external,
                create,
                ..ResolverPage::default()
            };
            (StatusCode::NOT_FOUND, page)
        }
        [one] => {
            let e = entity(&one.id, &one.title);
            let entries: Vec<Entry> = one
                .values
                .iter()
                .map(|v| Entry {
                    link: Link {
                        href: format!("/wiki/{}", title_url(&format!("{namespace}:{v}"))),
                        text: v.clone(),
                    },
                    id: None,
                    description: None,
                })
                .collect();
            let page = ResolverPage {
                text: m.with("ts-resolver-values", &[&e.link.text, &label, &key]),
                entries: std::iter::once(e).chain(entries).collect(),
                external,
                ..ResolverPage::default()
            };
            (StatusCode::OK, page)
        }
        several => {
            let page = ResolverPage {
                text: m.with("ts-resolver-several", &[&label, &key]),
                entries: several.iter().map(|c| entity(&c.id, &c.title)).collect(),
                note: Some(m.get("ts-resolver-duplicates")),
                external,
                ..ResolverPage::default()
            };
            (StatusCode::OK, page)
        }
    }
}

/// `GET /wiki/{namespace}:{key}` in a resolver namespace; `raw` is the title as the path
/// gives it.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    raw: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let title = pages::display_title(raw);
    let client = site.client();
    let resolved = match client.resolve(&incoming, raw.trim()).await {
        Ok(r) => r,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(resolved.cache);
    let Some(r) = resolved.value.filter(|r| r.kind == "resolver") else {
        return pages::no_page(&cx, headers, title);
    };
    if let Some(t) = &r.target {
        return see_other(&t.title);
    }
    let ids: Vec<String> = r.candidates.iter().map(|c| c.id.clone()).collect();
    let (labels, descriptions) = if ids.is_empty() {
        Default::default()
    } else {
        let chain = cx.m.term_chain();
        match client.labels_with(&incoming, &ids, &chain, &ids).await {
            Ok(t) => {
                cx.inputs.push(t.cache);
                t.value
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    };
    let (status, model) = model(&cx, &r, &labels, &descriptions);
    let body = cx.draw("resolver.html", &model);
    let heading = r
        .key
        .as_ref()
        .zip(r.namespace.as_ref())
        .map_or_else(|| title.clone(), |(k, ns)| format!("{ns}:{k}"));
    pages::respond(&cx, headers, status, |_| Page {
        title: heading,
        returnto: Some(title.clone()),
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::is_resolver_title;

    // 06 §3.2: the DOI namespace is a resolver's, in any case; a page namespace is not.
    #[test]
    fn resolver_titles_are_recognized() {
        assert!(is_resolver_title("DOI:10.1000/xyz"));
        assert!(is_resolver_title("doi:10.1000/a_b"));
        assert!(!is_resolver_title("DOI:"));
        assert!(!is_resolver_title("Item:Q5"));
        assert!(!is_resolver_title("Main Page"));
    }
}
