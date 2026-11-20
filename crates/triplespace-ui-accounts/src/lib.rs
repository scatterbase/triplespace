//! The site's special pages about an account (0113 §1; 0047): signing in and out,
//! registration, preferences, notifications, the watchlist, and the pages that act on
//! another account.
//!
//! - [`login`]: `Special:UserLogin` and `Special:UserLogout`.
//! - [`external`]: logging in through external issuers, and `Special:Account`'s Sign-in
//!   methods (0088 §5–§7).
//! - [`register`]: `Special:CreateAccount`, its alias `Special:RequestAccount`, and
//!   `Special:ConfirmAccounts` (0087 §5, §6).
//! - [`preferences`]: `Special:Preferences` and the Appearance menu's form (0085 §5–6).
//! - [`userrights`]: `Special:UserRights`, an account's groups (09 §3.3).
//! - [`renameuser`]: `Special:RenameUser`, renaming an account by hand (19 §2.3).
//! - [`block`]: `Special:Block`, `Special:Unblock` and `Special:BlockList` (09 §3.3).
//! - [`notifications`]: `Special:Notifications`, the viewer's inbox (19 §6.2).
//! - [`watchlist`]: `Special:Watchlist` and `Special:EditWatchlist` (19 §6.2).
//!
//! [`serve`] and [`post`] answer for these pages by name and decline every other, so the
//! router (`triplespace-ui`) can ask each special-page crate in turn; none depends on
//! another. A special page no crate serves is a `404`, as MediaWiki answers for a special
//! page it does not have.

#![forbid(unsafe_code)]

pub mod block;
pub mod external;
pub mod invitation;
pub mod login;
pub mod notifications;
pub mod preferences;
pub mod register;
pub mod renameuser;
pub mod subsidiaries;
pub mod userrights;
pub mod watchlist;

use std::collections::BTreeMap;

use axum::http::HeaderMap;
use axum::response::Response;
use triplespace_ui_kit::Site;
use triplespace_ui_kit::pages::{Params, Peer};
use triplespace_ui_kit::titles::special_name;

/// Serves one of this crate's special pages, or `None` for a title that names none of them.
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
        "UserLogin" => Some(login::login(site, headers, peer, query).await),
        "UserLogout" => Some(login::logout(site, headers, peer, query).await),
        "Account" => Some(external::serve(site, headers, peer, query).await),
        "CreateAccount" | "RequestAccount" => {
            Some(register::serve(site, headers, peer, query).await)
        }
        "ConfirmAccounts" => Some(register::review(site, headers, peer, query).await),
        "Preferences" => Some(preferences::serve(site, headers, peer, params).await),
        "UserRights" => Some(userrights::serve(site, headers, peer, title, params).await),
        "RenameUser" | "Renameuser" => {
            Some(renameuser::serve(site, headers, peer, title, params).await)
        }
        "Block" => {
            Some(block::serve(site, headers, peer, block::Which::Block, title, params).await)
        }
        "Unblock" => {
            Some(block::serve(site, headers, peer, block::Which::Unblock, title, params).await)
        }
        "BlockList" => Some(block::list(site, headers, peer, params).await),
        "Notifications" => Some(notifications::serve(site, headers, peer, params).await),
        "Watchlist" => Some(watchlist::serve(site, headers, peer, params).await),
        "EditWatchlist" => Some(watchlist::edit(site, headers, peer, params).await),
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
        "UserLogin" => Some(login::post_login(site, headers, peer, query, form).await),
        "UserLogout" => Some(login::post_logout(site, headers, peer, query, form).await),
        "Account" => Some(external::post(site, headers, peer, query, form).await),
        "CreateAccount" | "RequestAccount" => {
            Some(register::post(site, headers, peer, query, form).await)
        }
        "ConfirmAccounts" => Some(register::post_review(site, headers, peer, query, form).await),
        "Preferences" => Some(preferences::post(site, headers, peer, query, params).await),
        "UserRights" => Some(userrights::post(site, headers, peer, title, query, params).await),
        "RenameUser" | "Renameuser" => {
            Some(renameuser::post(site, headers, peer, query, params).await)
        }
        "Block" => Some(block::post(site, headers, peer, block::Which::Block, query, params).await),
        "Unblock" => {
            Some(block::post(site, headers, peer, block::Which::Unblock, query, params).await)
        }
        "Notifications" => Some(notifications::post(site, headers, peer, params).await),
        "Watchlist" => Some(watchlist::post(site, headers, peer, params).await),
        "EditWatchlist" => Some(watchlist::edit_post(site, headers, peer, params).await),
        _ => None,
    }
}
