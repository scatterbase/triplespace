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
/// the feature each needs.
pub const PAGES: &[(&str, &str, &str, Feature)] = &[(
    "Special:Search",
    "ts-search-title",
    "ts-specialpages-search",
    Feature::Search,
)];

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
        if cx.features.has(*feature) {
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
