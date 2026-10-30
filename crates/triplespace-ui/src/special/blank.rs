//! `Special:BlankPage` (0047 §9; 21 §6.1): the frame around nothing, for testing the
//! frame, a skin or a gadget without a page's content in the way. It reads *No content*.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::Site;
use crate::frame::Page;
use crate::html::esc;
use crate::pages::{self, Params, Peer};

/// `GET Special:BlankPage`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-blankpage-title"),
        returnto: Some("Special:BlankPage".into()),
        body: format!(
            "<p class=\"ts-blankpage\"><i>{}</i></p>",
            esc(&m.get("ts-blankpage-text"))
        ),
        ..Page::default()
    })
}
