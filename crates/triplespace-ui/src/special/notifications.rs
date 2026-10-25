//! `Special:Notifications` (0021 §6; 19 §6.2): the viewer's inbox as a page, newest first,
//! fifty to a page, everything or the unread only, with **Mark as read** on each unread
//! item and **Mark all as read** above them. Opening it is looking: the bell's mark goes
//! (`echomarkseen`). Each item says who did what where, and links to it: a thread's post,
//! or the user page an item about an account names.
//!
//! The list is what the API lets the viewer see now: an item about a thread they may no
//! longer read, or a post since hidden, is not there (16 §5.2).

use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::Notification;
use triplespace_client::settings::Saved;

use crate::Site;
use crate::codex::MessageKind;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};
use crate::special::changes::{clock, date};

const TITLE: &str = "Special:Notifications";

/// How many items a page shows.
const PER_PAGE: u32 = 50;

/// One item's sentence, its links in place.
fn sentence(m: &Messages, n: &Notification) -> String {
    let who = match &n.agent {
        Some(agent) => link(&format!("/wiki/User:{}", title_url(agent)), &esc(agent), ""),
        None => esc(&m.get("ts-notif-someone")),
    };
    let thread = match (&n.title, &n.subject) {
        (Some(title), Some(subject)) => {
            let href = match n.revid {
                Some(rev) => format!("/wiki/{}#post-{rev}", title_url(title)),
                None => format!("/wiki/{}", title_url(title)),
            };
            link(
                &href,
                &format!("“{}”", esc(subject)),
                " class=\"ts-notif__thread\"",
            )
        }
        _ => String::new(),
    };
    let account = n.title.as_deref().map_or_else(String::new, |t| {
        let name = t.split_once(':').map_or(t, |(_, n)| n);
        link(&format!("/wiki/{}", title_url(t)), &esc(name), "")
    });
    let key = match n.reason.as_str() {
        "talk" => "ts-notif-talk",
        "reply" => "ts-notif-reply",
        "mention" => "ts-notif-mention",
        "thread-status" => "ts-notif-thread-status",
        "rights" => "ts-notif-rights",
        "group-join" => "ts-notif-group-join",
        _ => "ts-notif-other",
    };
    let second = if n.reason == "group-join" {
        &account
    } else {
        &thread
    };
    esc(&m.get(key)).replace("$1", &who).replace("$2", second)
}

fn mark_form(token: &str, id: Option<i64>, label: &str, unread_only: bool) -> String {
    let (name, value) = match id {
        Some(i) => ("markread", i.to_string()),
        None => ("markall", "1".to_string()),
    };
    format!(
        "<form class=\"ts-notif__mark\" method=\"post\" action=\"/w/index.php?title={TITLE}\"><input type=\"hidden\" name=\"wpEditToken\" value=\"{}\"><input type=\"hidden\" name=\"{name}\" value=\"{value}\">{}<button class=\"cdx-button cdx-button--weight-quiet cdx-button--size-small\" type=\"submit\">{}</button></form>",
        esc(token),
        if unread_only {
            "<input type=\"hidden\" name=\"unread\" value=\"1\">"
        } else {
            ""
        },
        esc(label),
    )
}

/// `GET Special:Notifications`.
#[allow(clippy::too_many_lines)]
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Notifications) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    if cx.user.is_none() {
        let m = &cx.m;
        let body = format!(
            "{}<p>{}</p>",
            pages::message(MessageKind::Notice, &m.get("ts-notifications-login")),
            link(
                &format!("/wiki/Special:UserLogin?returnto={TITLE}"),
                &esc(&m.get("ts-login")),
                ""
            )
        );
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-notifications-title"),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    let unread_only = params.map.get("unread").is_some_and(|v| v == "1");
    let from = params.map.get("from").map(String::as_str);
    let listing = match site
        .client()
        .notifications(&incoming, unread_only, PER_PAGE, from)
        .await
    {
        Ok(l) => l,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(listing.cache);
    let token = match site.client().token(&incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache.clone());
    let cookies = token.set_cookies.clone();
    let tok = token.value.value;
    // Opening the inbox is looking at it: the bell's mark goes.
    if let Ok(Saved::Ok(_)) = site.client().mark_seen(&incoming, &tok).await
        && let Some(n) = cx.notifications.as_mut()
    {
        n.seen = n.newest_unread.clone();
    }
    let m = &cx.m;
    let mut body =
        String::from("<div class=\"ts-notifications\"><div class=\"ts-notifications__bar\">");
    let filter = |label: &str, href: &str, current: bool| {
        format!(
            "<a class=\"ts-notifications__filter\" href=\"{href}\"{}>{}</a>",
            if current {
                " aria-current=\"page\""
            } else {
                ""
            },
            esc(label)
        )
    };
    let _ = write!(
        body,
        "<nav class=\"ts-notifications__filters\" aria-label=\"{}\">{} {}</nav>",
        esc(&m.get("ts-notifications-filter")),
        filter(
            &m.get("ts-notifications-all"),
            &format!("/wiki/{TITLE}"),
            !unread_only
        ),
        filter(
            &m.get("ts-notifications-unread"),
            &format!("/wiki/{TITLE}?unread=1"),
            unread_only
        ),
    );
    if listing.value.items.iter().any(|n| !n.read) {
        body.push_str(&mark_form(
            &tok,
            None,
            &m.get("ts-notifications-markall"),
            unread_only,
        ));
    }
    body.push_str("</div>");
    if listing.value.items.is_empty() {
        let key = if unread_only {
            "ts-notifications-none-unread"
        } else {
            "ts-notifications-empty"
        };
        body.push_str(&pages::message(MessageKind::Notice, &m.get(key)));
    } else {
        body.push_str("<ul class=\"ts-notifications__list\">");
        for n in &listing.value.items {
            let _ = write!(
                body,
                "<li class=\"ts-notif ts-notif--{}{}\"><p class=\"ts-notif__text\">{}</p><p class=\"ts-notif__meta\"><time datetime=\"{}\">{}, {}</time>{}</p>{}</li>",
                esc(&n.reason),
                if n.read { "" } else { " ts-notif--unread" },
                sentence(m, n),
                esc(&n.timestamp),
                esc(clock(&n.timestamp)),
                esc(&date(m, &n.timestamp)),
                n.talk.as_deref().map_or_else(String::new, |t| format!(
                    " · {}",
                    link(&format!("/wiki/{}", title_url(t)), &esc(t), "")
                )),
                if n.read {
                    String::new()
                } else {
                    mark_form(
                        &tok,
                        Some(n.id),
                        &m.get("ts-notifications-markread"),
                        unread_only,
                    )
                },
            );
        }
        body.push_str("</ul>");
    }
    if let Some(next) = &listing.value.next {
        let mut href = format!("/wiki/{TITLE}?from={}", esc(&url_q(next)));
        if unread_only {
            href.push_str("&amp;unread=1");
        }
        let _ = write!(
            body,
            "<p class=\"ts-notifications__more\"><a href=\"{href}\">{}</a></p>",
            esc(&m.get("ts-notifications-older"))
        );
    }
    body.push_str("</div>");
    let r = pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-notifications-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &cookies)
}

fn url_q(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `POST Special:Notifications`: **Mark as read** (`markread`, an ID) or **Mark all as
/// read** (`markall`), then back to the list.
pub async fn post(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Notifications) || cx.user.is_none() {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let form = &params.map;
    let token = form.get("wpEditToken").map_or("", String::as_str);
    let one = form.get("markread").and_then(|i| i.parse::<i64>().ok());
    let ids = one.map(|i| vec![i]);
    if ids.is_none() && !form.contains_key("markall") {
        return pages::see_other(&format!("/wiki/{TITLE}"), &[]);
    }
    match site
        .client()
        .mark_read(&incoming, ids.as_deref(), None, token)
        .await
    {
        Ok(Saved::Ok(_)) => {
            let back = if form.get("unread").is_some_and(|v| v == "1") {
                format!("/wiki/{TITLE}?unread=1")
            } else {
                format!("/wiki/{TITLE}")
            };
            pages::see_other(&back, &[])
        }
        Ok(Saved::Refused(r)) => {
            let text = r
                .iter()
                .map(|x| x.message.clone())
                .collect::<Vec<_>>()
                .join(" ");
            pages::respond(&cx, headers, StatusCode::BAD_REQUEST, |m| Page {
                title: m.get("ts-notifications-title"),
                returnto: Some(TITLE.to_string()),
                body: pages::message(MessageKind::Error, &text),
                ..Page::default()
            })
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
