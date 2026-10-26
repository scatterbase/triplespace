//! `Special:Watchlist` and `Special:EditWatchlist` (0020 §4; 19 §6.2), for a signed-in
//! viewer, labelled **Private** as 0010 §1 asks.
//!
//! - **`Special:Watchlist`**: the changes to what the viewer watches, newest first, fifty
//!   to a page, as recent changes shows them, mirror syncs included (16 §4.2); a change
//!   they have not seen is marked, and **Mark all changes as seen** clears the marks.
//! - **`Special:EditWatchlist`**: every watch, with how long it lasts and whether it
//!   notifies, a box to tick beside each, and **Remove selected**.

use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::activity::ActivityQuery;
use triplespace_client::settings::Saved;
use triplespace_client::{ClientError, WatchForm};

use crate::Site;
use crate::codex::MessageKind;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::changes::{date, list_html};

const WATCHLIST: &str = "Special:Watchlist";
const EDIT: &str = "Special:EditWatchlist";

fn private_label(m: &Messages) -> String {
    format!(
        "<span class=\"ts-chip ts-chip--private\">{}</span>",
        esc(&m.get("ts-account-private"))
    )
}

/// The login page, for a viewer who is not signed in.
fn login(cx: &Context, headers: &HeaderMap, title: &str, key: &str) -> Response {
    let m = &cx.m;
    let body = format!(
        "{}<p>{}</p>",
        pages::message(MessageKind::Notice, &m.get("ts-watchlist-login")),
        link(
            &format!("/wiki/Special:UserLogin?returnto={title}"),
            &esc(&m.get("ts-login")),
            ""
        )
    );
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get(key),
        returnto: Some(title.to_string()),
        body,
        ..Page::default()
    })
}

fn tabs(m: &Messages, current: &str) -> String {
    let tab = |title: &str, key: &str| {
        format!(
            "<a class=\"ts-watchlist__tab\" href=\"/wiki/{title}\"{}>{}</a>",
            if title == current {
                " aria-current=\"page\""
            } else {
                ""
            },
            esc(&m.get(key))
        )
    };
    format!(
        "<nav class=\"ts-watchlist__tabs\" aria-label=\"{}\">{} {}</nav>",
        esc(&m.get("ts-watchlist-title")),
        tab(WATCHLIST, "ts-watchlist-changes"),
        tab(EDIT, "ts-editwatchlist-title"),
    )
}

async fn context(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    params: &Params,
) -> Result<(Context, triplespace_client::Incoming), Response> {
    let incoming = pages::incoming(headers, peer);
    let cx = pages::context(site, &incoming, &params.map).await?;
    Ok((cx, incoming))
}

/// `GET Special:Watchlist`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (mut cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) {
        return pages::no_page(&cx, headers, WATCHLIST.to_string());
    }
    if cx.user.is_none() {
        return login(&cx, headers, WATCHLIST, "ts-watchlist-title");
    }
    let query = ActivityQuery {
        set: Some("watchlist".into()),
        limit: 50,
        cursor: params.map.get("from").cloned(),
        lang: cx.m.lang().to_string(),
        ..ActivityQuery::default()
    };
    let page = match site.client().activity(&incoming, &query).await {
        Ok(p) => p,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(page.cache);
    let token = match site.client().token(&incoming, "csrf").await {
        Ok(t) => t.value.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let m = &cx.m;
    let mut body = format!(
        "<div class=\"ts-changes ts-watchlist\">{}",
        tabs(m, WATCHLIST)
    );
    let rows = &page.value.rows;
    if rows.iter().any(|r| r.unseen) {
        let _ = write!(
            body,
            "<form class=\"ts-watchlist__seen\" method=\"post\" action=\"/w/index.php?title={WATCHLIST}\"><input type=\"hidden\" name=\"wpEditToken\" value=\"{}\"><input type=\"hidden\" name=\"markseen\" value=\"1\"><button class=\"cdx-button\" type=\"submit\">{}</button></form>",
            esc(&token),
            esc(&m.get("ts-watchlist-markseen"))
        );
    }
    if rows.is_empty() {
        let _ = write!(
            body,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-watchlist-none"))
        );
    } else {
        body.push_str(&list_html(m, &cx.site, rows));
    }
    if let Some(next) = &page.value.next {
        let q: String = url::form_urlencoded::byte_serialize(next.as_bytes()).collect();
        let _ = write!(
            body,
            "<nav class=\"ts-changes__pager\">{}</nav>",
            link(
                &format!("/wiki/{WATCHLIST}?from={q}"),
                &esc(&m.get("ts-watchlist-older")),
                " rel=\"next\""
            )
        );
    }
    body.push_str("</div>");
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-watchlist-title"),
        returnto: Some(WATCHLIST.to_string()),
        identity_html: Some(private_label(m)),
        body,
        ..Page::default()
    })
}

/// `POST Special:Watchlist`: **Mark all changes as seen**.
pub async fn post(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) || cx.user.is_none() {
        return pages::no_page(&cx, headers, WATCHLIST.to_string());
    }
    let token = params.map.get("wpEditToken").map_or("", String::as_str);
    match site.client().mark_watch_seen(&incoming, None, token).await {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{WATCHLIST}"), &[]),
        Ok(Saved::Refused(r)) => refused(&cx, headers, WATCHLIST, &r),
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

fn refused(
    cx: &Context,
    headers: &HeaderMap,
    title: &str,
    r: &[triplespace_client::settings::Refusal],
) -> Response {
    let text = r
        .iter()
        .map(|x| x.message.clone())
        .collect::<Vec<_>>()
        .join(" ");
    pages::respond(cx, headers, StatusCode::BAD_REQUEST, |_| Page {
        title: title.to_string(),
        returnto: Some(title.to_string()),
        body: pages::message(MessageKind::Error, &text),
        ..Page::default()
    })
}

/// `GET Special:EditWatchlist`.
pub async fn edit(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (mut cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) {
        return pages::no_page(&cx, headers, EDIT.to_string());
    }
    if cx.user.is_none() {
        return login(&cx, headers, EDIT, "ts-editwatchlist-title");
    }
    let (watches, token) = tokio::join!(
        site.client().watches(&incoming),
        site.client().token(&incoming, "watch")
    );
    let watches = match watches {
        Ok(w) => w,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let token = match token {
        Ok(t) => t.value.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(watches.cache);
    let m = &cx.m;
    let mut body = format!("<div class=\"ts-watchlist\">{}", tabs(m, EDIT));
    let mut entries: Vec<_> = watches
        .value
        .iter()
        .filter_map(|w| w.title.clone().map(|t| (t, w)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.is_empty() {
        let _ = write!(
            body,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-editwatchlist-none"))
        );
    } else {
        let _ = write!(
            body,
            "<p>{}</p><form method=\"post\" action=\"/w/index.php?title={EDIT}\"><input type=\"hidden\" name=\"wpEditToken\" value=\"{}\"><table class=\"cdx-table__table ts-editwatchlist\"><caption class=\"ts-visually-hidden\">{}</caption><thead><tr><th scope=\"col\">{}</th><th scope=\"col\">{}</th><th scope=\"col\">{}</th><th scope=\"col\">{}</th></tr></thead><tbody>",
            esc(&m.with("ts-editwatchlist-count", &[&entries.len().to_string()])),
            esc(&token),
            esc(&m.get("ts-editwatchlist-title")),
            esc(&m.get("ts-editwatchlist-remove-col")),
            esc(&m.get("ts-editwatchlist-page")),
            esc(&m.get("ts-editwatchlist-expires")),
            esc(&m.get("ts-editwatchlist-notify")),
        );
        for (i, (title, w)) in entries.iter().enumerate() {
            let _ = write!(
                body,
                "<tr><td><span class=\"cdx-checkbox\"><span class=\"cdx-checkbox__wrapper\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"ts-unwatch-{i}\" name=\"titles\" value=\"{}\"><span class=\"cdx-checkbox__icon\"></span><span class=\"cdx-checkbox__label cdx-label\"><label class=\"cdx-label__label\" for=\"ts-unwatch-{i}\"><span class=\"cdx-label__label__text ts-visually-hidden\">{}</span></label></span></span></span></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(title),
                esc(&m.with("ts-editwatchlist-remove-one", &[title])),
                link(&format!("/wiki/{}", title_url(title)), &esc(title), ""),
                esc(&w
                    .expires
                    .as_deref()
                    .map_or_else(|| m.get("ts-watch-permanent"), |e| date(m, e))),
                esc(&m.get(if w.notify {
                    "ts-editwatchlist-yes"
                } else {
                    "ts-editwatchlist-no"
                })),
            );
        }
        let _ = write!(
            body,
            "</tbody></table><button class=\"cdx-button cdx-button--action-destructive\" type=\"submit\">{}</button></form>",
            esc(&m.get("ts-editwatchlist-remove"))
        );
    }
    body.push_str("</div>");
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-editwatchlist-title"),
        returnto: Some(EDIT.to_string()),
        identity_html: Some(private_label(m)),
        body,
        ..Page::default()
    })
}

/// `POST Special:EditWatchlist`: **Remove selected**.
pub async fn edit_post(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) || cx.user.is_none() {
        return pages::no_page(&cx, headers, EDIT.to_string());
    }
    let titles: Vec<String> = params
        .pairs
        .iter()
        .filter(|(k, _)| k == "titles")
        .map(|(_, v)| v.clone())
        .collect();
    if titles.is_empty() {
        return pages::see_other(&format!("/wiki/{EDIT}"), &[]);
    }
    let token = params.map.get("wpEditToken").map_or("", String::as_str);
    let r = site
        .client()
        .watch(
            &incoming,
            &WatchForm {
                titles: &titles,
                unwatch: true,
                expiry: None,
                notify: None,
                token,
            },
        )
        .await;
    match r {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{EDIT}"), &[]),
        Ok(Saved::Refused(r)) => refused(&cx, headers, EDIT, &r),
        Err(ClientError::Api { info, .. }) => refused(
            &cx,
            headers,
            EDIT,
            &[triplespace_client::settings::Refusal {
                code: "api".into(),
                key: None,
                message: info,
            }],
        ),
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
