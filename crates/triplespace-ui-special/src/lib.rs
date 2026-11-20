//! The site's other special pages (0113 §1; 0047): the feeds, the lists and reports,
//! search, the redirecting pages, and the forms that create and edit entities.
//!
//! - [`changes`]: `Special:RecentChanges`, `Special:Contributions` and `Special:Log`, three
//!   feeds (0020 §4; 19 §3.9, §6.3).
//! - [`index`]: `Special:SpecialPages`, the list of the others.
//! - [`new_entity`]: `Special:NewItem` and `Special:NewProperty`, the forms that create
//!   an entity (0047 §5).
//! - [`search`]: `Special:Search`, the full results page, and its **Go to** rule
//!   (0010 §3; 0047 §9).
//! - [`undelete`]: `Special:Undelete`, the deletions and the way back (09 §6.2).
//! - [`movepage`]: `Special:MovePage`, the Move form (10 §2.5).
//! - [`blank`]: `Special:BlankPage`, the frame around nothing.
//! - [`info`]: `Special:NamespaceInfo`, `Special:ListGroupRights`, `Special:ListDatatypes`
//!   and `Special:MyLanguageFallbackChain`, read-only pages about the wiki.
//! - [`compare`]: `Special:ComparePages`, any two pages or revisions.
//! - [`set_terms`]: `Special:SetLabel`, `Special:SetDescription`, `Special:SetAliases` and
//!   `Special:SetLabelDescriptionAliases`, Wikibase's term forms.
//! - `Special:NewPages` is a fourth feed in [`changes`].
//! - [`allpages`]: `Special:AllPages` and `Special:PrefixIndex`.
//! - [`whatlinkshere`]: `Special:WhatLinksHere`, the Links here tab.
//! - [`reports`]: the report pages, `Special:WantedPages`, `Special:ShortPages`,
//!   `Special:ListProperties` and the rest, one page from `list=querypage` (0047 §4).
//! - [`redirects`]: MediaWiki's redirecting pages (`MyPage`, `EditPage`, `TalkPage`,
//!   `PermanentLink`, `Diff`, `Redirect`, `EntityPage` and the rest; 21 §6.1).
//!
//! [`serve`] and [`post`] answer for these pages by name and decline every other, so the
//! router (`triplespace-ui`) can ask each special-page crate in turn; none depends on
//! another. A special page no crate serves is a `404`, as MediaWiki answers for a special
//! page it does not have.

#![forbid(unsafe_code)]

pub mod allpages;
pub mod blank;
pub mod changes;
pub mod compare;
pub mod create_entities;
pub mod index;
pub mod info;
pub mod movepage;
pub mod new_entity;
pub mod redirects;
pub mod reports;
pub mod search;
pub mod set_terms;
pub mod undelete;
pub mod whatlinkshere;

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
    if let Some(kind) = new_entity::Kind::of(&name) {
        return Some(new_entity::serve(site, headers, peer, kind, title, params).await);
    }
    if let Some(which) = redirects::Which::of(&name) {
        return Some(redirects::serve(site, headers, peer, which, title, params).await);
    }
    if let Some(which) = info::Which::of(&name) {
        return Some(info::serve(site, headers, peer, which, params).await);
    }
    if let Some(which) = allpages::Which::of(&name) {
        return Some(allpages::serve(site, headers, peer, which, title, params).await);
    }
    if let Some(which) = set_terms::Which::of(&name) {
        return Some(set_terms::serve(site, headers, peer, which, title, params).await);
    }
    match name.as_str() {
        "BlankPage" => Some(blank::serve(site, headers, peer, params).await),
        "WhatLinksHere" | "Whatlinkshere" => {
            Some(whatlinkshere::serve(site, headers, peer, title, params).await)
        }
        "ComparePages" => Some(compare::serve(site, headers, peer, params).await),
        "NewPages" => Some(changes::new_pages(site, headers, peer, params).await),
        "Search" => Some(search::serve(site, headers, peer, query).await),
        "RecentChanges" => Some(changes::recent_changes(site, headers, peer, params).await),
        "Contributions" => Some(changes::contributions(site, headers, peer, title, params).await),
        "Log" => Some(changes::log(site, headers, peer, title, params).await),
        "SpecialPages" => Some(index::serve(site, headers, peer, query).await),
        "Undelete" => Some(undelete::serve(site, headers, peer, title, params).await),
        "MovePage" => Some(movepage::serve(site, headers, peer, title, params).await),
        "CreateManyEntities" => {
            Some(create_entities::serve(site, headers, peer, title, params).await)
        }
        "CreateEntitiesFromFile" => {
            Some(create_entities::from_file::serve(site, headers, peer, title, params).await)
        }
        "QuickStatements" => {
            Some(create_entities::quickstatements::serve(site, headers, peer, params).await)
        }
        // The page's name until 0104 A5: a GET is sent on to the name it has now, with
        // its subpage (0047 §2).
        "CreateEntities" => {
            use axum::response::IntoResponse as _;
            let rest = title.split_once('/').map_or("", |(_, r)| r);
            let mut to = "/wiki/Special:CreateManyEntities".to_string();
            if !rest.is_empty() {
                to.push('/');
                to.push_str(&triplespace_ui_kit::frame::title_url(rest));
            }
            Some(
                (
                    axum::http::StatusCode::MOVED_PERMANENTLY,
                    [(axum::http::header::LOCATION, to)],
                )
                    .into_response(),
            )
        }
        _ => match reports::Report::of(&name) {
            Some(report) => Some(reports::serve(site, headers, peer, report, params).await),
            None => None,
        },
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
    let name = special_name(title)?;
    if let Some(kind) = new_entity::Kind::of(&name) {
        return Some(new_entity::post(site, headers, peer, kind, query, params).await);
    }
    if let Some(which) = set_terms::Which::of(&name) {
        return Some(set_terms::post(site, headers, peer, which, query, params).await);
    }
    match name.as_str() {
        "MovePage" => Some(movepage::post(site, headers, peer, title, params).await),
        "CreateManyEntities" | "CreateEntities" => {
            Some(create_entities::post(site, headers, peer, query, params).await)
        }
        "CreateEntitiesFromFile" => {
            Some(create_entities::from_file::post(site, headers, peer, title, query, params).await)
        }
        "QuickStatements" => {
            Some(create_entities::quickstatements::post(site, headers, peer, query, params).await)
        }
        _ => None,
    }
}
