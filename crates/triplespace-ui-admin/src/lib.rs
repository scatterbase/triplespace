//! The site's special pages that run a wiki or an instance (0113 §1; 0047): settings,
//! tenants, jobs, import and export, protection, the version and the interface messages.
//!
//! - [`version`]: `Special:Version` and its `Credits` and `License` subpages (0077).
//! - [`settings`]: `Special:WikiSettings`, `Special:InstanceSettings` and
//!   `Special:Tenancy` (0085 §4), with [`triplespace_ui_kit::controls`], the form controls drawn from a type.
//! - [`export`]: `Special:Export`, MediaWiki XML or records of listed pages (21 §4.2).
//! - [`import`]: `Special:Import`, a MediaWiki XML export into this wiki, the census
//!   first (21 §4.3).
//! - [`protected`]: `Special:ProtectedPages`, the restrictions in force, the sets and the
//!   empty-group report (09 §4.5).
//! - [`allmessages`]: `Special:AllMessages`, the interface messages and their overrides.
//! - `Special:ProtectedTitles` is [`protected::titles`].
//! - [`providers`]: `Special:Providers`, the data providers this instance knows.
//!
//! [`serve`] and [`post`] answer for these pages by name and decline every other, so the
//! router (`triplespace-ui`) can ask each special-page crate in turn; none depends on
//! another. A special page no crate serves is a `404`, as MediaWiki answers for a special
//! page it does not have.

#![forbid(unsafe_code)]

pub mod allmessages;
pub mod export;
pub mod import;
pub mod jobs;
pub mod protected;
pub mod providers;
pub mod settings;
pub mod tenants;
pub mod version;

use std::collections::BTreeMap;

use axum::http::HeaderMap;
use axum::response::Response;
use triplespace_ui_kit::Site;
use triplespace_ui_kit::pages::{Params, Peer};
use triplespace_ui_kit::titles::special_name;

/// Serves one of this crate's special pages, or `None` for a title that names none of them.
#[allow(clippy::too_many_lines)] // a dispatch table, one arm per page
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Option<Response> {
    let query = &params.map;
    let name = special_name(title)?;
    match name.as_str() {
        "AllMessages" => Some(allmessages::serve(site, headers, peer, params).await),
        "ProtectedTitles" => Some(protected::titles(site, headers, peer, params).await),
        "Providers" => Some(providers::serve(site, headers, peer, params).await),
        "Version" => Some(version::serve(site, headers, peer, title, query).await),
        "WikiSettings" => {
            Some(settings::serve(site, headers, peer, settings::Scope::Wiki, params).await)
        }
        "InstanceSettings" => {
            Some(settings::serve(site, headers, peer, settings::Scope::Instance, params).await)
        }
        "Tenancy" => Some(settings::tenancy(site, headers, peer, query).await),
        "Tenants" => Some(tenants::serve(site, headers, peer, params).await),
        "ProtectedPages" => Some(protected::serve(site, headers, peer, params).await),
        "Export" => Some(export::serve(site, headers, peer, title, params).await),
        "Import" => Some(import::serve(site, headers, peer, params).await),
        "Jobs" => Some(jobs::serve(site, headers, peer, title, params).await),
        _ => None,
    }
}

/// Takes a form posted to one of this crate's special pages, or `None` for a page that
/// takes none here.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    params: &Params,
) -> Option<Response> {
    let form = &params.map;
    let name = special_name(title)?;
    match name.as_str() {
        "Tenants" => Some(tenants::post(site, headers, peer, query, form).await),
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
        "ProtectedPages" => Some(protected::post(site, headers, peer, query, params).await),
        "Export" => Some(export::post(site, headers, peer, query, params).await),
        "Import" => Some(import::post(site, headers, peer, query, params).await),
        "Jobs" => Some(jobs::post(site, headers, peer, title, query, params).await),
        _ => None,
    }
}
