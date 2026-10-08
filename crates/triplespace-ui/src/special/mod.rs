//! Special pages (0047): those the site serves so far, by name.
//!
//! - [`index`]: `Special:SpecialPages`, the list of the others.
//! - [`login`]: `Special:UserLogin` and `Special:UserLogout`, the site's only forms.
//! - [`search`]: `Special:Search`, the full results page, and its **Go to** rule
//!   (0010 §3; 0047 §9).
//! - [`version`]: `Special:Version` and its `Credits` and `License` subpages (0077).
//!
//! A special page the site does not serve is a `404`, as MediaWiki answers for a special
//! page it does not have.

pub mod index;
pub mod login;
pub mod search;
pub mod version;

use std::collections::BTreeMap;

use axum::http::HeaderMap;
use axum::response::Response;

use crate::Site;
use crate::pages::Peer;

/// A special page's name from its title: `Special:Search/foo` → `Search`, with the
/// first letter upper-cased, as MediaWiki's titles have it.
#[must_use]
pub fn name_of(title: &str) -> Option<String> {
    let rest = title.strip_prefix("Special:")?;
    let name = rest.split('/').next().unwrap_or(rest).trim();
    let mut chars = name.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// Serves a special page, or `None` for one the site does not serve.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
) -> Option<Response> {
    match name_of(title)?.as_str() {
        "Search" => Some(search::serve(site, headers, peer, query).await),
        "SpecialPages" => Some(index::serve(site, headers, peer, query).await),
        "UserLogin" => Some(login::login(site, headers, peer, query).await),
        "UserLogout" => Some(login::logout(site, headers, peer, query).await),
        "Version" => Some(version::serve(site, headers, peer, title, query).await),
        _ => None,
    }
}

/// Takes a form posted to a special page, or `None` for a page that takes none.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Option<Response> {
    match name_of(title)?.as_str() {
        "UserLogin" => Some(login::post_login(site, headers, peer, query, form).await),
        "UserLogout" => Some(login::post_logout(site, headers, peer, query, form).await),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::name_of;

    #[test]
    fn names() {
        assert_eq!(name_of("Special:Search").as_deref(), Some("Search"));
        assert_eq!(name_of("Special:search/x").as_deref(), Some("Search"));
        assert_eq!(name_of("Special:").as_deref(), None);
        assert_eq!(name_of("Item:Q6").as_deref(), None);
    }
}
