//! `Special:Undelete` (0023 §9; 19 §6.3): the deletions the viewer may see, newest first,
//! each leading to the deleted page's view, which carries the Undelete form for a member
//! of the deletion group holding `undelete` (`moderation::deleted`).
//! `Special:Undelete/{title}` goes straight to that view.
//!
//! The list is the `delete` log (`GET /activity?kind=log&log_type=delete`), so a deletion
//! of a confidential page is listed only to its group, as every feed does (09 §5.3), and a
//! page since undeleted still shows its past deletion; its own page says it is back.

use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::activity::ActivityQuery;

use crate::Site;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::pages::{self, Params, Peer};

const TITLE: &str = "Special:Undelete";

/// `GET Special:Undelete[/{title}]`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let subject = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| params.map.get("target").map(|t| t.trim().to_string()))
        .filter(|s| !s.is_empty());
    if let Some(page) = subject {
        if let Some(r) =
            crate::moderation::deleted(site, &mut cx, headers, &incoming, &page, None).await
        {
            return r;
        }
        let body = pages::message(
            crate::codex::MessageKind::Notice,
            &cx.m.with("ts-undelete-nothing", &[&page]),
        );
        return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: m.get("ts-undelete-index-title"),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    let query = ActivityQuery {
        kinds: vec!["log".into()],
        log_type: Some("delete".into()),
        limit: 100,
        lang: cx.m.lang().to_string(),
        ..ActivityQuery::default()
    };
    let feed = match site.client().activity(&incoming, &query).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(feed.cache);
    let m = &cx.m;
    let mut body = format!(
        "<p>{}</p><form class=\"ts-undelete__ask\" method=\"get\" action=\"/w/index.php\"><input type=\"hidden\" name=\"title\" value=\"{TITLE}\"><label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" name=\"target\"></label> <button class=\"cdx-button cdx-button--action-progressive\" type=\"submit\">{}</button></form>",
        esc(&m.get("ts-undelete-intro")),
        esc(&m.get("ts-undelete-page")),
        esc(&m.get("ts-undelete-go")),
    );
    let deletions: Vec<_> = feed
        .value
        .rows
        .iter()
        .filter(|r| r.summary.op.as_deref() == Some("delete/delete") || r.logid.is_some())
        .filter(|r| r.target.is_some())
        .collect();
    if deletions.is_empty() {
        let _ = write!(
            body,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-undelete-none"))
        );
    } else {
        body.push_str("<ul class=\"ts-changes__list ts-undelete__list\">");
        for r in deletions {
            let Some(t) = &r.target else { continue };
            let title = t.title.clone().unwrap_or_default();
            let _ = write!(
                body,
                "<li><time datetime=\"{}\">{}</time> {} — {} {}</li>",
                esc(&r.time),
                esc(&r.time),
                link(
                    &format!("/wiki/{}/{}", TITLE, title_url(&title)),
                    &esc(&title),
                    ""
                ),
                esc(r.actor.name.as_deref().unwrap_or("")),
                esc(r.comment.as_deref().unwrap_or("")),
            );
        }
        body.push_str("</ul>");
    }
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-undelete-index-title"),
        returnto: Some(TITLE.to_string()),
        body: format!("<div class=\"ts-changes\">{body}</div>"),
        ..Page::default()
    })
}
