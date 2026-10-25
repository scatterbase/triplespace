//! `Special:Block` and `Special:Unblock` (09 §3.3, §6.3; 0016 §7; 19 §6.3), MediaWiki's
//! addresses, with `Special:Block/{name}` prefilled.
//!
//! - **Block**: the account, an expiry (`infinite` by default, a time, or a span such as
//!   `1 week`), a reason, and the options the API serves: **also prevent account
//!   creation** (`nocreate`), **block everything but reading** (`tsallbutread`), and for a
//!   holder of `hideuser` **hide the username** (`hidename`), which the page says needs an
//!   indefinite block and is undone by an unblock from a holder of that right (0056 A25).
//!   A block in force is replaced when **replace the current block** is on (`reblock`).
//! - **Unblock**: the account and a reason. An administrator without `hideuser` is told
//!   that unblocking a hidden user leaves the name hidden.
//! - **`Special:BlockList`**: the blocks in force (`list=blocks`): who, by whom, when,
//!   until when, why, what the block removes; a hidden name shows as such to those outside
//!   the suppression group and carries a **Hidden** mark for those in it (19 §6.3); an
//!   **Unblock** link for a holder of `block`.
//!
//! The API's refusal comes back on the form: `alreadyblocked`, `ipb_expiry_temp`,
//! `ts-cantblock-self`, `cantunblock`, `nosuchuser`, `permissiondenied`, `badtoken`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::settings::Saved;
use triplespace_client::{BlockForm, Incoming};

use crate::Site;
use crate::codex::{Action, Button, Field, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};

/// Which form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:Block`.
    Block,
    /// `Special:Unblock`.
    Unblock,
}

impl Which {
    fn title(self) -> &'static str {
        match self {
            Self::Block => "Special:Block",
            Self::Unblock => "Special:Unblock",
        }
    }
}

/// The name a title carries: `Special:Block/Mallory`.
fn subject_of(title: &str) -> Option<String> {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// What the form shows when it comes back.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools, reason = "the form's four checkboxes")]
struct Returned {
    notice: Option<(MessageKind, String)>,
    user: String,
    expiry: String,
    reason: String,
    nocreate: bool,
    all_but_read: bool,
    hidename: bool,
    reblock: bool,
}

fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "alreadyblocked" => m.get("ts-block-already"),
        "ipb_expiry_temp" => m.get("ts-block-hide-indefinite"),
        "ts-cantblock-self" => m.get("ts-block-self"),
        "cantunblock" => m.get("ts-block-not-blocked"),
        "nosuchuser" | "nosuchuserid" => m.get("ts-userrights-nosuchuser"),
        "invalidexpiry" => m.get("ts-protect-invalid-expiry"),
        "badtoken" => m.get("ts-login-expired"),
        "permissiondenied" => m.get("ts-block-denied"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

fn checkbox(id: &str, name: &str, label: &str, help: &str, on: bool) -> String {
    format!(
        "<div class=\"ts-block__option\"><span class=\"cdx-checkbox\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"{id}\" name=\"{name}\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><label class=\"cdx-checkbox__label\" for=\"{id}\">{}</label></span><div class=\"cdx-field__help-text\">{}</div></div>",
        if on { " checked" } else { "" },
        esc(label),
        esc(help),
    )
}

#[allow(clippy::too_many_lines)] // one form, read top to bottom
fn form_html(cx: &Context, which: Which, token: &str, back: &Returned) -> String {
    let m = &cx.m;
    let mut s = String::new();
    if let Some((kind, text)) = &back.notice {
        s.push_str(&pages::message(*kind, text));
    }
    let _ = write!(
        s,
        "<form class=\"ts-block\" method=\"post\" action=\"/wiki/{}\"><input type=\"hidden\" name=\"token\" value=\"{}\">{}",
        esc(&title_url(which.title())),
        esc(token),
        Field {
            id: "ts-block-user",
            name: "user",
            kind: "text",
            value: &back.user,
            label: &m.get("ts-block-user"),
            description: None,
            help: None,
            required: true,
            autocomplete: Some("off"),
        }
        .render()
        .unwrap_or_default(),
    );
    if which == Which::Block {
        let expiry = if back.expiry.is_empty() {
            "infinite"
        } else {
            &back.expiry
        };
        s.push_str(
            &Field {
                id: "ts-block-expiry",
                name: "expiry",
                kind: "text",
                value: expiry,
                label: &m.get("ts-block-expiry"),
                description: None,
                help: Some(&m.get("ts-userrights-expiry-help")),
                required: false,
                autocomplete: Some("off"),
            }
            .render()
            .unwrap_or_default(),
        );
    }
    s.push_str(
        &Field {
            id: "ts-block-reason",
            name: "reason",
            kind: "text",
            value: &back.reason,
            label: &m.get("ts-block-reason"),
            description: None,
            help: Some(&m.get("ts-block-reason-help")),
            required: false,
            autocomplete: Some("off"),
        }
        .render()
        .unwrap_or_default(),
    );
    if which == Which::Block {
        s.push_str(&checkbox(
            "ts-block-nocreate",
            "nocreate",
            &m.get("ts-block-nocreate"),
            &m.get("ts-block-nocreate-help"),
            back.nocreate,
        ));
        s.push_str(&checkbox(
            "ts-block-allbutread",
            "allbutread",
            &m.get("ts-block-allbutread"),
            &m.get("ts-block-allbutread-help"),
            back.all_but_read,
        ));
        if cx.holds("hideuser") {
            s.push_str(&checkbox(
                "ts-block-hidename",
                "hidename",
                &m.get("ts-block-hidename"),
                &m.get("ts-block-hidename-help"),
                back.hidename,
            ));
        }
        s.push_str(&checkbox(
            "ts-block-reblock",
            "reblock",
            &m.get("ts-block-reblock"),
            &m.get("ts-block-reblock-help"),
            back.reblock,
        ));
    } else if !cx.holds("hideuser") {
        s.push_str(&pages::message(
            MessageKind::Notice,
            &m.get("ts-unblock-hidden-note"),
        ));
    }
    let _ = write!(
        s,
        "<div class=\"ts-block__actions\">{}</div></form>",
        Button {
            label: &m.get(match which {
                Which::Block => "ts-block-submit",
                Which::Unblock => "ts-unblock-submit",
            }),
            kind: "submit",
            action: match which {
                Which::Block => Action::Destructive,
                Which::Unblock => Action::Progressive,
            },
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default(),
    );
    let other = match which {
        Which::Block => Which::Unblock,
        Which::Unblock => Which::Block,
    };
    let _ = write!(
        s,
        "<p class=\"ts-block__links\">{}</p>",
        link(
            &format!("/wiki/{}", title_url(other.title())),
            &esc(&m.get(match other {
                Which::Block => "ts-block-title",
                Which::Unblock => "ts-unblock-title",
            })),
            ""
        )
    );
    s
}

async fn page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    which: Which,
    back: Returned,
) -> Response {
    let heading = cx.m.get(match which {
        Which::Block => "ts-block-title",
        Which::Unblock => "ts-unblock-title",
    });
    if cx.user.is_none() {
        let body = format!(
            "{}<p>{}</p>",
            pages::message(MessageKind::Notice, &cx.m.get("ts-block-login")),
            link(
                &format!(
                    "/wiki/Special:UserLogin?returnto={}",
                    title_url(which.title())
                ),
                &esc(&cx.m.get("ts-login")),
                ""
            )
        );
        return pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.clone(),
            returnto: Some(which.title().to_string()),
            body,
            ..Page::default()
        });
    }
    if !cx.holds("block") {
        let body = pages::message(MessageKind::Error, &cx.m.get("ts-block-denied"));
        return pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| Page {
            title: heading.clone(),
            returnto: Some(which.title().to_string()),
            body,
            ..Page::default()
        });
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let body = format!(
        "<div class=\"ts-block-page\">{}</div>",
        form_html(cx, which, &token.value.value, &back)
    );
    let r = pages::respond(cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(which.title().to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    which: Which,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Block) {
        return Err(pages::no_page(&cx, headers, which.title().to_string()));
    }
    Ok(cx)
}

/// `GET Special:Block[/{name}]` or `Special:Unblock[/{name}]`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map, which).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let notice = params.map.get("done").map(|_| {
        (
            MessageKind::Success,
            cx.m.get(match which {
                Which::Block => "ts-block-done",
                Which::Unblock => "ts-unblock-done",
            }),
        )
    });
    let back = Returned {
        notice,
        user: subject_of(title)
            .or_else(|| params.map.get("user").cloned())
            .unwrap_or_default(),
        ..Returned::default()
    };
    page(site, &mut cx, headers, &incoming, which, back).await
}

/// `POST Special:Block` or `Special:Unblock`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, which).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let on = |k: &str| form.map.contains_key(k);
    let user = field("user").trim().to_string();
    let expiry = field("expiry").trim().to_string();
    let reason = field("reason").trim().to_string();
    let back = Returned {
        notice: None,
        user: user.clone(),
        expiry: expiry.clone(),
        reason: reason.clone(),
        nocreate: on("nocreate"),
        all_but_read: on("allbutread"),
        hidename: on("hidename"),
        reblock: on("reblock"),
    };
    let result = match which {
        Which::Block => {
            site.client()
                .block(
                    &incoming,
                    &BlockForm {
                        user: &user,
                        expiry: if expiry.is_empty() {
                            "infinite"
                        } else {
                            &expiry
                        },
                        reason: &reason,
                        nocreate: back.nocreate,
                        all_but_read: back.all_but_read,
                        hidename: back.hidename,
                        reblock: back.reblock,
                        token: field("token"),
                    },
                )
                .await
        }
        Which::Unblock => {
            site.client()
                .unblock(&incoming, &user, &reason, field("token"))
                .await
        }
    };
    match result {
        Ok(Saved::Ok(_)) => pages::see_other(
            &format!(
                "/wiki/{}?done=1&user={}",
                title_url(which.title()),
                url::form_urlencoded::byte_serialize(user.as_bytes()).collect::<String>()
            ),
            &[],
        ),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal_text(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            let back = Returned {
                notice: Some((MessageKind::Error, text)),
                ..back
            };
            page(site, &mut cx, headers, &incoming, which, back).await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `GET Special:BlockList`.
pub async fn list(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map, Which::Block).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let blocks = match site.client().blocks(&incoming).await {
        Ok(b) => b,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(blocks.cache);
    let m = &cx.m;
    let may_unblock = cx.user.is_some() && cx.holds("block");
    let mut body = String::new();
    if blocks.value.is_empty() {
        let _ = write!(
            body,
            "<p class=\"ts-empty\">{}</p>",
            esc(&m.get("ts-blocklist-none"))
        );
    } else {
        let _ = write!(
            body,
            "<table class=\"cdx-table__table ts-blocklist__table\"><thead><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th>{}</tr></thead><tbody>",
            esc(&m.get("ts-blocklist-col-user")),
            esc(&m.get("ts-blocklist-col-by")),
            esc(&m.get("ts-blocklist-col-when")),
            esc(&m.get("ts-blocklist-col-expiry")),
            esc(&m.get("ts-blocklist-col-reason")),
            esc(&m.get("ts-blocklist-col-removes")),
            if may_unblock { "<th></th>" } else { "" },
        );
        for b in &blocks.value {
            let user = match &b.user {
                Some(u) => {
                    let mut h = link(
                        &format!("/wiki/Special:Contributions/{}", title_url(u)),
                        &esc(u),
                        "",
                    );
                    if b.hidden {
                        let _ = write!(
                            h,
                            " <span class=\"ts-blocklist__hidden\">{}</span>",
                            esc(&m.get("ts-blocklist-hidden"))
                        );
                    }
                    h
                }
                None => format!(
                    "<span class=\"ts-blocklist__hidden\">{}</span>",
                    esc(&m.get("ts-changes-hidden-user"))
                ),
            };
            let expiry = if b.expiry == "infinite" || b.expiry == "infinity" {
                m.get("ts-protect-expiry-never")
            } else {
                b.expiry.clone()
            };
            let removes = if b.removes == "all-but-read" {
                m.get("ts-blocklist-all-but-read")
            } else {
                b.removes.clone()
            };
            let _ = write!(
                body,
                "<tr><td>{user}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td>",
                esc(b.by.as_deref().unwrap_or("")),
                esc(&b.timestamp),
                esc(&expiry),
                esc(&b.reason),
                esc(&removes),
            );
            if may_unblock {
                let _ = write!(
                    body,
                    "<td>{}</td>",
                    match &b.user {
                        Some(u) => link(
                            &format!("/wiki/Special:Unblock/{}", title_url(u)),
                            &esc(&m.get("ts-unblock-submit")),
                            ""
                        ),
                        None => String::new(),
                    }
                );
            }
            body.push_str("</tr>");
        }
        body.push_str("</tbody></table>");
    }
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-blocklist-title"),
        returnto: Some("Special:BlockList".to_string()),
        body: format!("<div class=\"ts-protectedpages\">{body}</div>"),
        ..Page::default()
    })
}
