//! Special pages (0047): those the site serves so far, by name.
//!
//! - [`changes`]: `Special:RecentChanges` and `Special:Contributions`, two feeds
//!   (0020 §4; 19 §3.9).
//! - [`index`]: `Special:SpecialPages`, the list of the others.
//! - [`login`]: `Special:UserLogin` and `Special:UserLogout`.
//! - [`new_entity`]: `Special:NewItem` and `Special:NewProperty`, the forms that create
//!   an entity (0047 §5).
//! - [`search`]: `Special:Search`, the full results page, and its **Go to** rule
//!   (0010 §3; 0047 §9).
//! - [`version`]: `Special:Version` and its `Credits` and `License` subpages (0077).
//! - [`preferences`]: `Special:Preferences` and the Appearance menu's form (0085 §5–6).
//! - [`settings`]: `Special:WikiSettings`, `Special:InstanceSettings` and
//!   `Special:Tenancy` (0085 §4), with [`controls`], the form controls drawn from a type.
//!
//! A special page the site does not serve is a `404`, as MediaWiki answers for a special
//! page it does not have.

pub mod changes;
pub mod controls;
pub mod index;
pub mod login;
pub mod new_entity;
pub mod preferences;
pub mod search;
pub mod settings;
pub mod version;

use std::collections::BTreeMap;

use axum::http::HeaderMap;
use axum::response::Response;

use crate::Site;
use crate::pages::{Params, Peer};

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
    params: &Params,
) -> Option<Response> {
    let query = &params.map;
    let name = name_of(title)?;
    if let Some(kind) = new_entity::Kind::of(&name) {
        return Some(new_entity::serve(site, headers, peer, kind, title, params).await);
    }
    match name.as_str() {
        "Search" => Some(search::serve(site, headers, peer, query).await),
        "RecentChanges" => Some(changes::recent_changes(site, headers, peer, params).await),
        "Contributions" => Some(changes::contributions(site, headers, peer, title, params).await),
        "SpecialPages" => Some(index::serve(site, headers, peer, query).await),
        "UserLogin" => Some(login::login(site, headers, peer, query).await),
        "UserLogout" => Some(login::logout(site, headers, peer, query).await),
        "Version" => Some(version::serve(site, headers, peer, title, query).await),
        "Preferences" => Some(preferences::serve(site, headers, peer, params).await),
        "WikiSettings" => {
            Some(settings::serve(site, headers, peer, settings::Scope::Wiki, params).await)
        }
        "InstanceSettings" => {
            Some(settings::serve(site, headers, peer, settings::Scope::Instance, params).await)
        }
        "Tenancy" => Some(settings::tenancy(site, headers, peer, query).await),
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
    params: &Params,
) -> Option<Response> {
    let form = &params.map;
    let name = name_of(title)?;
    if let Some(kind) = new_entity::Kind::of(&name) {
        return Some(new_entity::post(site, headers, peer, kind, query, params).await);
    }
    match name.as_str() {
        "UserLogin" => Some(login::post_login(site, headers, peer, query, form).await),
        "UserLogout" => Some(login::post_logout(site, headers, peer, query, form).await),
        "Preferences" => Some(preferences::post(site, headers, peer, query, params).await),
        "WikiSettings" => {
            Some(settings::post(site, headers, peer, settings::Scope::Wiki, query, params).await)
        }
        "InstanceSettings" => Some(
            settings::post(
                site,
                headers,
                peer,
                settings::Scope::Instance,
                query,
                params,
            )
            .await,
        ),
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
