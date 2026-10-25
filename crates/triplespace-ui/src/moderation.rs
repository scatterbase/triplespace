//! The Protect, Delete and Undelete forms, `index.php?title={title}&action=protect|delete|
//! undelete` (09 §4.5, §6.2; 0016 §7; 0023 §9; 0056 §13; 19 §6.3), for pages and entities
//! alike, and the view of a deleted page.
//!
//! **Protect:**
//!
//! - **One row per action** the title may be restricted on (`restrictiontypes`): a group
//!   picker whose first choice lifts the restriction. **Read** is among them: a
//!   confidential restriction (09 §5.2), with the note that the viewer must be in the
//!   group they choose (the lock-out of 09 §4.5), the viewer's own groups named so that
//!   the choice is made with eyes open.
//! - **Also the talk page** goes with the Read row, on by default, since people who hide
//!   a page usually mean its discussion too (0056 A24); it is `tstalk` on the request.
//! - **Expiry** applies to every restriction the form sets, as MediaWiki's form does
//!   without its per-row expiries; **reason** is the log's.
//! - **Enclosing restrictions** (a namespace's, a set's, the tenant's) are listed above
//!   the form and cannot be changed here: their targets have their own pages.
//!
//! The form posts to the same address; the API's answer decides, and its refusal is
//! shown on the form with what was typed kept (`ts-locked-out`, `ts-policy`,
//! `ts-unknown-group`, `protectedpage`, `badtoken`, …). A page the viewer may not read,
//! or that does not exist, answers as the API does: the form is still offered for a free
//! title, since a title may be create-protected before it exists.
//!
//! **Delete** asks for a reason and, for a page, whether the talk page goes too
//! (`deletetalk`), as MediaWiki's form does; it lands on the page, which now shows its
//! deletion. **Undelete** is a reason and a button, offered on the deleted page and at
//! `action=undelete` to a holder of `undelete`.
//!
//! **A deleted page** ([`deleted`]) shows everyone the deletion log entry in place of its
//! content (who, when, why, from the public `delete` log), and the deletion group the
//! content of the newest deleted revision under that banner (`prop=deletedrevisions`,
//! rendered as a preview), with the Undelete form. A title with no deletion the viewer may
//! see is the ordinary missing page (09 §5.3).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::Value;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, ProtectForm, Protection, Restriction};

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params};
use crate::special::controls::{Control, Kind, render};

/// What the form shows when it comes back: a notice, and what was typed.
#[derive(Debug, Default)]
struct Returned {
    notice: Option<(MessageKind, String)>,
    levels: BTreeMap<String, String>,
    expiry: String,
    reason: String,
    talk: Option<bool>,
}

/// A message for an API refusal.
fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "ts-locked-out" => m.get("ts-protect-locked-out"),
        "ts-policy" => m.get("ts-protect-policy"),
        "ts-unknown-group" | "badvalue" => m.get("ts-protect-unknown-group"),
        "invalidexpiry" => m.get("ts-protect-invalid-expiry"),
        "badtoken" => m.get("ts-login-expired"),
        "permissiondenied" | "protectedpage" | "protectednamespace" => m.get("ts-protect-denied"),
        "ts-restricted-title" | "missingtitle" | "nosuchpageid" => m.get("ts-protect-missing"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// The restrictions an enclosing target set, as a list the viewer cannot change here.
fn enclosing_html(m: &Messages, restrictions: &[Restriction]) -> String {
    let from_above: Vec<&Restriction> =
        restrictions.iter().filter(|r| r.source.is_some()).collect();
    if from_above.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "<section class=\"ts-protect__enclosing\"><h2>{}</h2><ul>",
        esc(&m.get("ts-protect-enclosing"))
    );
    for r in from_above {
        let _ = write!(
            s,
            "<li>{}</li>",
            esc(&m.with(
                "ts-protect-enclosing-row",
                &[
                    &action_label(m, &r.kind),
                    &r.level,
                    r.source.as_deref().unwrap_or_default(),
                    &expiry_text(m, &r.expiry),
                ]
            ))
        );
    }
    s.push_str("</ul></section>");
    s
}

fn action_label(m: &Messages, kind: &str) -> String {
    let key = format!("ts-protect-action-{kind}");
    if m.has(&key) {
        m.get(&key)
    } else {
        kind.to_string()
    }
}

fn expiry_text(m: &Messages, expiry: &str) -> String {
    if expiry == "infinity" || expiry == "infinite" {
        m.get("ts-protect-expiry-never")
    } else {
        expiry.to_string()
    }
}

/// The form.
#[allow(clippy::too_many_lines)]
fn form_html(
    cx: &Context,
    title: &str,
    p: &Protection,
    groups: &[String],
    token: &str,
    back: &Returned,
) -> String {
    let m = &cx.m;
    let mut s = String::new();
    if let Some((kind, text)) = &back.notice {
        s.push_str(&pages::message(*kind, text));
    }
    if !p.exists {
        s.push_str(&pages::message(
            MessageKind::Notice,
            &m.get("ts-protect-new-title"),
        ));
    }
    s.push_str(&enclosing_html(m, &p.restrictions));
    let own: BTreeMap<&str, &Restriction> = p
        .restrictions
        .iter()
        .filter(|r| r.source.is_none())
        .map(|r| (r.kind.as_str(), r))
        .collect();
    let _ = write!(
        s,
        "<form class=\"ts-protect\" method=\"post\" action=\"/w/index.php?title={}&amp;action=protect\"><input type=\"hidden\" name=\"token\" value=\"{}\">",
        esc(&title_url(title)),
        esc(token)
    );
    let lift = m.get("ts-protect-level-all");
    for kind in &p.types {
        // Creation is restricted on a title with no page (09 §4.8); a page that exists
        // has nothing to create.
        if kind == "create" && p.exists {
            continue;
        }
        let id = format!("ts-protect-level-{kind}");
        let help_id = format!("{id}-help");
        let current = back
            .levels
            .get(kind)
            .cloned()
            .or_else(|| own.get(kind.as_str()).map(|r| r.level.clone()))
            .unwrap_or_default();
        let value = Value::String(current.clone());
        let control = render(&Control {
            id: &id,
            name: &format!("level-{kind}"),
            kind: Kind::Choice,
            choices: groups,
            value: Some(&value),
            empty_choice: Some(&lift),
            described_by: Some(&help_id),
            choice_label: None,
        });
        let help = if kind == "read" {
            let mine = cx.groups.join(", ");
            m.with("ts-protect-read-help", &[&mine])
        } else {
            m.get(&format!("ts-protect-help-{kind}"))
        };
        let _ = write!(
            s,
            "<div class=\"cdx-field ts-protect__row\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"{id}\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\">{control}</div><div class=\"cdx-field__help-text\" id=\"{help_id}\">{}</div>",
            esc(&action_label(m, kind)),
            esc(&help),
        );
        if let Some(r) = own.get(kind.as_str())
            && r.expiry != "infinity"
        {
            let _ = write!(
                s,
                "<div class=\"cdx-field__help-text\">{}</div>",
                esc(&m.with("ts-protect-current-expiry", &[&r.expiry]))
            );
        }
        if kind == "read" {
            let checked = back
                .talk
                .unwrap_or_else(|| own.get("read").is_none_or(|r| r.talk));
            let _ = write!(
                s,
                "<div class=\"ts-protect__talk\"><span class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"ts-protect-talk\" name=\"talk\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><label class=\"cdx-checkbox__label\" for=\"ts-protect-talk\">{}</label></span><div class=\"cdx-field__help-text\">{}</div></div>",
                if checked { " checked" } else { "" },
                esc(&m.get("ts-protect-talk")),
                esc(&m.get("ts-protect-talk-help")),
            );
        }
        s.push_str("</div>");
    }
    let expiry = if back.expiry.is_empty() {
        "infinite".to_string()
    } else {
        back.expiry.clone()
    };
    let _ = write!(
        s,
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-protect-expiry\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"ts-protect-expiry\" name=\"expiry\" value=\"{}\" aria-describedby=\"ts-protect-expiry-help\"></div></div><div class=\"cdx-field__help-text\" id=\"ts-protect-expiry-help\">{}</div></div>",
        esc(&m.get("ts-protect-expiry")),
        esc(&expiry),
        esc(&m.get("ts-protect-expiry-help")),
    );
    let _ = write!(
        s,
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-protect-reason\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"ts-protect-reason\" name=\"reason\" value=\"{}\"></div></div></div>",
        esc(&m.get("ts-protect-reason")),
        esc(&back.reason),
    );
    let _ = write!(
        s,
        "<div class=\"ts-protect__actions\">{}{}</div></form>",
        Button {
            label: &m.get("ts-protect-submit"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default(),
        link(
            &format!("/wiki/{}", title_url(title)),
            &esc(&m.get("ts-protect-cancel")),
            " class=\"ts-protect__cancel\""
        ),
    );
    s
}

/// The groups a restriction may name: the tenant's, implicit ones left out.
async fn group_names(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
) -> Result<Vec<String>, Response> {
    match site.client().groups(incoming).await {
        Ok(g) => {
            cx.inputs.push(g.cache);
            Ok(g.value
                .into_iter()
                .filter(|g| !g.implicit && g.name != "*")
                .map(|g| g.name)
                .collect())
        }
        Err(e) => Err(pages::api_failed(cx, headers, &e)),
    }
}

/// The page around the form, or the page that says the viewer may not protect.
async fn page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    back: Returned,
    status: StatusCode,
) -> Response {
    let m = &cx.m;
    let heading = m.with("ts-protect-title", &[title]);
    if cx.user.is_none() {
        let body = format!(
            "{}<p>{}</p>",
            pages::message(MessageKind::Notice, &m.get("ts-protect-login")),
            link(
                &format!(
                    "/wiki/Special:UserLogin?returnto={}&returntoquery=action%3Dprotect",
                    title_url(title)
                ),
                &esc(&m.get("ts-login")),
                ""
            )
        );
        return pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.clone(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        });
    }
    if !cx.holds("protect") {
        let body = pages::message(MessageKind::Error, &m.get("ts-protect-denied"));
        return pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| Page {
            title: heading.clone(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        });
    }
    let (p, token) = tokio::join!(
        site.client().protection(incoming, title),
        site.client().token(incoming, "csrf")
    );
    let (p, token) = match (p, token) {
        (Ok(p), Ok(t)) => (p, t),
        (Err(e), _) | (_, Err(e)) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(p.cache);
    cx.inputs.push(token.value.cache);
    let groups = match group_names(site, cx, headers, incoming).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let body = format!(
        "<div class=\"ts-protect-page\">{}</div>",
        form_html(
            cx,
            &p.value.title,
            &p.value,
            &groups,
            &token.value.value,
            &back
        )
    );
    let r = pages::respond(cx, headers, status, |_| Page {
        title: heading.clone(),
        returnto: Some(title.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

/// `GET …&action=protect`.
async fn serve_protect(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: String,
    params: &Params,
) -> Response {
    if !cx.features.has(Feature::Protect) {
        return pages::no_page(&cx, headers, title);
    }
    let notice = params
        .map
        .get("done")
        .map(|_| (MessageKind::Success, cx.m.get("ts-protect-done")));
    let back = Returned {
        notice,
        ..Returned::default()
    };
    page(
        site,
        &mut cx,
        headers,
        incoming,
        &title,
        back,
        StatusCode::OK,
    )
    .await
}

/// `POST …&action=protect`.
async fn post_protect(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: String,
    form: &Params,
) -> Response {
    if !cx.features.has(Feature::Protect) {
        return pages::no_page(&cx, headers, title);
    }
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let mut levels = BTreeMap::new();
    let mut protections = Vec::new();
    for (k, v) in &form.map {
        if let Some(kind) = k.strip_prefix("level-") {
            levels.insert(kind.to_string(), v.clone());
            let group = if v.trim().is_empty() { "all" } else { v.trim() };
            protections.push((kind.to_string(), group.to_string()));
        }
    }
    let talk = form.map.contains_key("talk");
    let expiry = field("expiry").trim();
    let reason = field("reason").trim();
    let sent = ProtectForm {
        title: &title,
        protections,
        expiry: if expiry.is_empty() {
            "infinite"
        } else {
            expiry
        },
        reason,
        talk,
        token: field("token"),
    };
    match site.client().protect(incoming, &sent).await {
        Ok(Saved::Ok(_)) => pages::see_other(
            &format!(
                "/w/index.php?title={}&action=protect&done=1",
                title_url(&title)
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
                levels,
                expiry: expiry.to_string(),
                reason: reason.to_string(),
                talk: Some(talk),
            };
            page(
                site,
                &mut cx,
                headers,
                incoming,
                &title,
                back,
                StatusCode::OK,
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// Which form an `action=` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `action=protect`.
    Protect,
    /// `action=delete`.
    Delete,
    /// `action=undelete`.
    Undelete,
}

impl Form {
    /// The form an `action=` names, if any.
    #[must_use]
    pub fn of(action: &str) -> Option<Self> {
        match action {
            "protect" => Some(Self::Protect),
            "delete" => Some(Self::Delete),
            "undelete" => Some(Self::Undelete),
            _ => None,
        }
    }
}

/// `GET …&action=protect|delete|undelete`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: Context,
    form: Form,
    title: String,
    params: &Params,
) -> Response {
    match form {
        Form::Protect => serve_protect(site, headers, incoming, cx, title, params).await,
        Form::Delete => delete_page(site, cx, headers, incoming, &title, None, None).await,
        Form::Undelete => undelete_page(site, cx, headers, incoming, &title, None).await,
    }
}

/// `POST …&action=protect|delete|undelete`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: Context,
    form: Form,
    title: String,
    fields: &Params,
) -> Response {
    match form {
        Form::Protect => post_protect(site, headers, incoming, cx, title, fields).await,
        Form::Delete => post_delete(site, cx, headers, incoming, &title, fields).await,
        Form::Undelete => post_undelete(site, cx, headers, incoming, &title, fields).await,
    }
}

/// A page that asks the viewer to log in, or says they lack a right.
pub(crate) fn gate(
    cx: &Context,
    headers: &HeaderMap,
    title: &str,
    heading: &str,
    right: &str,
    action: &str,
) -> Option<Response> {
    let m = &cx.m;
    if cx.user.is_none() {
        let body = format!(
            "{}<p>{}</p>",
            pages::message(MessageKind::Notice, &m.get("ts-moderation-login")),
            link(
                &format!(
                    "/wiki/Special:UserLogin?returnto={}&returntoquery=action%3D{action}",
                    title_url(title)
                ),
                &esc(&m.get("ts-login")),
                ""
            )
        );
        return Some(pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.to_string(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        }));
    }
    if !cx.holds(right) {
        let body = pages::message(
            MessageKind::Error,
            &m.with("ts-moderation-denied", &[right]),
        );
        return Some(pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| {
            Page {
                title: heading.to_string(),
                returnto: Some(title.to_string()),
                body,
                ..Page::default()
            }
        }));
    }
    None
}

pub(crate) fn reason_field(m: &Messages, id: &str, value: &str) -> String {
    format!(
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"{id}\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"{id}\" name=\"reason\" value=\"{}\"></div></div></div>",
        esc(&m.get("ts-moderation-reason")),
        esc(value),
    )
}

/// The Delete form.
async fn delete_page(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    notice: Option<(MessageKind, String)>,
    typed: Option<(&str, bool)>,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let heading = cx.m.with("ts-delete-title", &[title]);
    if let Some(r) = gate(&cx, headers, title, &heading, "delete", "delete") {
        return r;
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let m = &cx.m;
    let (reason, talk) = typed.unwrap_or(("", true));
    let mut body = String::new();
    if let Some((kind, text)) = notice {
        body.push_str(&pages::message(kind, &text));
    }
    body.push_str(&pages::message(
        MessageKind::Warning,
        &m.get("ts-delete-warning"),
    ));
    let _ = write!(
        body,
        "<form class=\"ts-delete\" method=\"post\" action=\"/w/index.php?title={}&amp;action=delete\"><input type=\"hidden\" name=\"token\" value=\"{}\">{}<div class=\"ts-protect__talk\"><span class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"ts-delete-talk\" name=\"talk\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><label class=\"cdx-checkbox__label\" for=\"ts-delete-talk\">{}</label></span><div class=\"cdx-field__help-text\">{}</div></div><div class=\"ts-protect__actions\">{}{}</div></form>",
        esc(&title_url(title)),
        esc(&token.value.value),
        reason_field(m, "ts-delete-reason", reason),
        if talk { " checked" } else { "" },
        esc(&m.get("ts-delete-talk")),
        esc(&m.get("ts-delete-talk-help")),
        Button {
            label: &m.get("ts-delete-submit"),
            kind: "submit",
            action: Action::Destructive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default(),
        link(
            &format!("/wiki/{}", title_url(title)),
            &esc(&m.get("ts-protect-cancel")),
            " class=\"ts-protect__cancel\""
        ),
    );
    let r = pages::respond(&cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(title.to_string()),
        body: format!("<div class=\"ts-protect-page\">{body}</div>"),
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

fn deletion_refusal(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "missingtitle" | "nosuchpageid" | "cantundelete" | "ts-restricted-title" => {
            m.get("ts-protect-missing")
        }
        "permissiondenied" | "protectedpage" => m.with("ts-moderation-denied", &["delete"]),
        "badtoken" => m.get("ts-login-expired"),
        "ts-locked-out" => m.get("ts-protect-locked-out"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// `POST …&action=delete`.
async fn post_delete(
    site: &Site,
    cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    fields: &Params,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let field = |k: &str| fields.map.get(k).map_or("", String::as_str);
    let reason = field("reason").trim();
    let talk = fields.map.contains_key("talk");
    match site
        .client()
        .delete_page(incoming, title, reason, talk, field("token"))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{}", title_url(title)), &[]),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = deletion_refusal(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            delete_page(
                site,
                cx,
                headers,
                incoming,
                title,
                Some((MessageKind::Error, text)),
                Some((reason, talk)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The Undelete form on its own page (`action=undelete`): the deleted page's view, which
/// carries it, or the missing page.
async fn undelete_page(
    site: &Site,
    cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    notice: Option<(MessageKind, String)>,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let heading = cx.m.with("ts-undelete-title", &[title]);
    if let Some(r) = gate(&cx, headers, title, &heading, "undelete", "undelete") {
        return r;
    }
    let mut cx = cx;
    match deleted(site, &mut cx, headers, incoming, title, notice).await {
        Some(r) => r,
        None => pages::no_page(&cx, headers, title.to_string()),
    }
}

/// `POST …&action=undelete`.
async fn post_undelete(
    site: &Site,
    cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    fields: &Params,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let field = |k: &str| fields.map.get(k).map_or("", String::as_str);
    match site
        .client()
        .undelete_page(incoming, title, field("reason").trim(), field("token"))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{}", title_url(title)), &[]),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = deletion_refusal(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            undelete_page(
                site,
                cx,
                headers,
                incoming,
                title,
                Some((MessageKind::Error, text)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The view of a deleted title (19 §6.3), or `None` where the viewer may see no deletion
/// of it: the banner naming who deleted it, when and why; for the deletion group the
/// newest deleted revision's content under it; and the Undelete form for a holder of
/// `undelete`. An entity's deleted content is not shown: `deletedrevisions` has none for it.
pub async fn deleted(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    notice: Option<(MessageKind, String)>,
) -> Option<Response> {
    if !cx.features.has(Feature::Delete) {
        return None;
    }
    let deletion = match site.client().deletion_of(incoming, title).await {
        Ok(d) => d,
        Err(e) => return Some(pages::api_failed(cx, headers, &e)),
    };
    cx.inputs.push(deletion.cache);
    let d = deletion.value?;
    let m = &cx.m;
    let mut body = String::new();
    if let Some((kind, text)) = notice {
        body.push_str(&pages::message(kind, &text));
    }
    let by =
        d.by.clone()
            .unwrap_or_else(|| m.get("ts-changes-hidden-user"));
    let mut banner = esc(&m.with("ts-deleted-banner", &[&by, &d.time, &d.reason]));
    if d.reason.is_empty() {
        banner = esc(&m.with("ts-deleted-banner-no-reason", &[&by, &d.time]));
    }
    let _ = write!(
        banner,
        " {}",
        link(
            &format!("/wiki/Special:Log?type=delete&page={}", title_url(title)),
            &esc(&m.get("ts-deleted-log-link")),
            ""
        )
    );
    let _ = write!(
        body,
        "<div class=\"cdx-message cdx-message--block cdx-message--warning ts-deleted__banner\" aria-live=\"polite\"><span class=\"cdx-message__icon\"></span><div class=\"cdx-message__content\">{banner}</div></div>"
    );
    // The deletion group reads the content; others see nothing more.
    let revision = match site.client().deleted_revision(incoming, title).await {
        Ok(r) => r,
        Err(e) => return Some(pages::api_failed(cx, headers, &e)),
    };
    cx.inputs.push(revision.cache);
    let member = revision.value.is_some();
    if let Some(rev) = &revision.value
        && let Some(text) = &rev.content
    {
        match site.client().preview(incoming, title, text).await {
            Ok(parsed) => {
                let _ = write!(
                    body,
                    "<div class=\"ts-document ts-deleted__content\"><div class=\"ts-document__content\">{}</div></div>",
                    parsed.html
                );
            }
            Err(_) => {
                let _ = write!(
                    body,
                    "<pre class=\"ts-deleted__source\">{}</pre>",
                    esc(text)
                );
            }
        }
    }
    let mut cookies = Vec::new();
    if member && cx.holds("undelete") {
        let token = match site.client().token(incoming, "csrf").await {
            Ok(t) => t,
            Err(e) => return Some(pages::api_failed(cx, headers, &e)),
        };
        cx.inputs.push(token.value.cache);
        cookies = token.set_cookies;
        let _ = write!(
            body,
            "<form class=\"ts-undelete\" method=\"post\" action=\"/w/index.php?title={}&amp;action=undelete\"><input type=\"hidden\" name=\"token\" value=\"{}\">{}<div class=\"ts-protect__actions\">{}</div></form>",
            esc(&title_url(title)),
            esc(&token.value.value),
            reason_field(m, "ts-undelete-reason", ""),
            Button {
                label: &m.get("ts-undelete-submit"),
                kind: "submit",
                action: Action::Progressive,
                weight: Weight::Primary,
            }
            .render()
            .unwrap_or_default(),
        );
    }
    // The group has a page to read; everyone else a title with no page, as the API says.
    let status = if member {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    };
    let heading = title.to_string();
    let r = pages::respond(cx, headers, status, |_| Page {
        title: heading.clone(),
        returnto: Some(title.to_string()),
        body: format!("<div class=\"ts-deleted\">{body}</div>"),
        ..Page::default()
    });
    Some(pages::with_cookies(r, &cookies))
}
