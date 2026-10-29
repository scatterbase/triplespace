//! `Special:SpecialPages` (0047 §9): the special pages the site offers, each with what it
//! is for. A page appears only where its feature is offered.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::Site;
use crate::features::Feature;
use crate::frame::Page;
use crate::html::{esc, link};
use crate::pages::{self, Peer};

/// The special pages the site serves: title, label message, description message, and
/// the feature each needs, if any.
pub const PAGES: &[(&str, &str, &str, Option<Feature>)] = &[
    (
        "Special:Search",
        "ts-search-title",
        "ts-specialpages-search",
        Some(Feature::Search),
    ),
    (
        "Special:UserLogin",
        "ts-login-title",
        "ts-specialpages-login",
        Some(Feature::Login),
    ),
    (
        "Special:CreateAccount",
        "ts-createaccount-title",
        "ts-specialpages-createaccount",
        Some(Feature::Register),
    ),
    (
        "Special:ConfirmAccounts",
        "ts-confirmaccounts-title",
        "ts-specialpages-confirmaccounts",
        Some(Feature::Register),
    ),
    (
        "Special:Watchlist",
        "ts-watchlist-title",
        "ts-specialpages-watchlist",
        Some(Feature::Watchlist),
    ),
    (
        "Special:EditWatchlist",
        "ts-editwatchlist-title",
        "ts-specialpages-editwatchlist",
        Some(Feature::Watchlist),
    ),
    (
        "Special:Notifications",
        "ts-notifications-title",
        "ts-specialpages-notifications",
        Some(Feature::Notifications),
    ),
    (
        "Special:Log",
        "ts-log-title",
        "ts-specialpages-log",
        Some(Feature::RecentChanges),
    ),
    (
        "Special:UserRights",
        "ts-userrights-title",
        "ts-specialpages-userrights",
        Some(Feature::UserRights),
    ),
    (
        "Special:ProtectedPages",
        "ts-protectedpages-title",
        "ts-specialpages-protectedpages",
        Some(Feature::Protect),
    ),
    (
        "Special:Block",
        "ts-block-title",
        "ts-specialpages-block",
        Some(Feature::Block),
    ),
    (
        "Special:BlockList",
        "ts-blocklist-title",
        "ts-specialpages-blocklist",
        Some(Feature::Block),
    ),
    (
        "Special:Undelete",
        "ts-undelete-index-title",
        "ts-specialpages-undelete",
        Some(Feature::Delete),
    ),
    (
        "Special:MovePage",
        "ts-move-index-title",
        "ts-specialpages-movepage",
        Some(Feature::Move),
    ),
    (
        "Special:NewItem",
        "ts-newitem-title",
        "ts-specialpages-newitem",
        Some(Feature::Create),
    ),
    (
        "Special:NewProperty",
        "ts-newproperty-title",
        "ts-specialpages-newproperty",
        Some(Feature::Create),
    ),
    (
        "Special:RecentChanges",
        "ts-recentchanges-title",
        "ts-specialpages-recentchanges",
        Some(Feature::RecentChanges),
    ),
    (
        "Special:Contributions",
        "ts-contributions-title-none",
        "ts-specialpages-contributions",
        Some(Feature::RecentChanges),
    ),
    (
        "Special:Version",
        "ts-version-title",
        "ts-specialpages-version",
        Some(Feature::Version),
    ),
    (
        "Special:Preferences",
        "ts-preferences-title",
        "ts-specialpages-preferences",
        Some(Feature::Preferences),
    ),
    (
        "Special:WikiSettings",
        "ts-wikisettings-title",
        "ts-specialpages-wikisettings",
        Some(Feature::Settings),
    ),
    (
        "Special:InstanceSettings",
        "ts-instancesettings-title",
        "ts-specialpages-instancesettings",
        Some(Feature::Settings),
    ),
    (
        "Special:Redirect",
        "ts-redirect-title",
        "ts-specialpages-redirect",
        Some(Feature::History),
    ),
    (
        "Special:EntityPage",
        "ts-entitypage-title",
        "ts-specialpages-entitypage",
        None,
    ),
    (
        "Special:ComparePages",
        "ts-comparepages-title",
        "ts-specialpages-comparepages",
        Some(Feature::Pages),
    ),
    (
        "Special:ProtectedTitles",
        "ts-protectedtitles-title",
        "ts-specialpages-protectedtitles",
        Some(Feature::Protect),
    ),
    (
        "Special:NamespaceInfo",
        "ts-namespaceinfo-title",
        "ts-specialpages-namespaceinfo",
        None,
    ),
    (
        "Special:ListGroupRights",
        "ts-listgrouprights-title",
        "ts-specialpages-listgrouprights",
        None,
    ),
    (
        "Special:AllMessages",
        "ts-allmessages-title",
        "ts-specialpages-allmessages",
        None,
    ),
    (
        "Special:ListDatatypes",
        "ts-listdatatypes-title",
        "ts-specialpages-listdatatypes",
        None,
    ),
    (
        "Special:MyLanguageFallbackChain",
        "ts-fallbackchain-title",
        "ts-specialpages-fallbackchain",
        None,
    ),
];

/// `Special:SpecialPages`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let m = &cx.m;
    let mut list = String::new();
    for (title, label, description, feature) in PAGES {
        // The instance's settings are served at the farm base only (0085 §4).
        let elsewhere = *title == "Special:InstanceSettings" && !cx.site.at_farm;
        if feature.is_none_or(|f| cx.features.has(f)) && !elsewhere {
            let _ = write!(
                list,
                "<li><p class=\"ts-specialpages__name\">{}</p><p class=\"ts-specialpages__description\">{}</p></li>",
                link(&format!("/wiki/{title}"), &esc(&m.get(label)), ""),
                esc(&m.get(description))
            );
        }
    }
    let body = if list.is_empty() {
        format!(
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-specialpages-none"))
        )
    } else {
        format!("<ul class=\"ts-specialpages\">{list}</ul>")
    };
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-nav-specialpages"),
        returnto: Some("Special:SpecialPages".into()),
        body,
        ..Page::default()
    })
}
