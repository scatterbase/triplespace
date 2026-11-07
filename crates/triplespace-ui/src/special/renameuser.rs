//! `Special:RenameUser` (19 §2.3; 0010 §10; 21 §6.1): renaming an account by hand, for a
//! holder of `renameuser`, MediaWiki's address with `Special:RenameUser/{name}` prefilled.
//!
//! The account, the new name and a reason, which goes to the public
//! `renameuser/renameuser` log event; the page says that the old name stays reserved for
//! good, since a name is never reused. The form posts one `action=renameuser` with the
//! `csrf` token, and the API's refusal comes back on the form: `nosuchuser`,
//! `userexists`, `invaliduser`, `ts-name-reserved` (a reserved prefix or a tombstone),
//! `ts-strict-name` under strict names, `permissiondenied`, `badtoken`.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, RenameForm};

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::moderation::GateModel;
use crate::pages::{self, Context, Params, Peer};

const TITLE: &str = "Special:RenameUser";

/// The name a title carries: `Special:RenameUser/Alice`.
fn subject_of(title: &str) -> Option<String> {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `Special:RenameUser` as Plain's `special/renameuser.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RenameUserPage {
    /// The notice it came back with.
    pub notices: Vec<MessageModel>,
    /// Where it posts.
    pub action: String,
    /// The `csrf` token.
    pub token: String,
    /// The account.
    pub user: FieldModel,
    /// The new name.
    pub newuser: FieldModel,
    /// The reason.
    pub reason: FieldModel,
    /// That the old name stays reserved.
    pub note: MessageModel,
    /// **Rename**.
    pub submit: ButtonModel,
}

/// What the form shows when it comes back.
#[derive(Debug, Default)]
struct Returned {
    notice: Option<(MessageKind, String)>,
    user: String,
    newuser: String,
    reason: String,
}

fn text_field(
    m: &Messages,
    id: &str,
    name: &str,
    value: &str,
    label: &str,
    help: &str,
    required: bool,
) -> FieldModel {
    FieldModel {
        id: id.into(),
        name: name.into(),
        kind: "text".into(),
        value: value.into(),
        label: m.get(label),
        description: None,
        help: Some(m.get(help)),
        required,
        autocomplete: Some("off".into()),
    }
}

fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "permissiondenied" => m.get("ts-renameuser-denied"),
        "badtoken" => m.get("ts-login-expired"),
        "nosuchuser" | "nosuchuserid" => m.get("ts-userrights-nosuchuser"),
        "userexists" => m.get("ts-renameuser-taken"),
        "invaliduser" => m.get("ts-renameuser-invalid"),
        "ts-name-reserved" => m.get("ts-renameuser-reserved"),
        "ts-strict-name" => m.get("ts-renameuser-strict"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

fn form_model(m: &Messages, token: &str, back: &Returned) -> RenameUserPage {
    RenameUserPage {
        notices: back
            .notice
            .iter()
            .map(|(k, t)| Message::block(*k, t).model())
            .collect(),
        action: format!("/w/index.php?title={}", title_url(TITLE)),
        token: token.to_string(),
        user: text_field(
            m,
            "ts-renameuser-user",
            "user",
            &back.user,
            "ts-renameuser-user",
            "ts-renameuser-user-help",
            true,
        ),
        newuser: text_field(
            m,
            "ts-renameuser-newuser",
            "newuser",
            &back.newuser,
            "ts-renameuser-newuser",
            "ts-renameuser-newuser-help",
            true,
        ),
        reason: text_field(
            m,
            "ts-renameuser-reason",
            "reason",
            &back.reason,
            "ts-renameuser-reason",
            "ts-renameuser-reason-help",
            false,
        ),
        note: Message::block(MessageKind::Notice, &m.get("ts-renameuser-note")).model(),
        submit: Button {
            label: &m.get("ts-renameuser-submit"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
    }
}

/// The form, or the gate for a reader or a viewer without the right.
async fn form_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: Returned,
) -> Response {
    let heading = cx.m.get("ts-renameuser-title");
    if cx.user.is_none() {
        let href = format!("/wiki/Special:UserLogin?returnto={}", title_url(TITLE));
        let model = GateModel::login(&cx.m, "ts-renameuser-login", href.clone());
        let body = cx.draw("moderation/gate.html", &model);
        return pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.clone(),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    if !cx.holds("renameuser") {
        let text = cx.m.get("ts-renameuser-denied");
        let body = cx.draw(
            "moderation/gate.html",
            GateModel::notice(MessageKind::Error, &text),
        );
        return pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| Page {
            title: heading.clone(),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let model = form_model(&cx.m, &token.value.value, &back);
    let body = cx.draw("special/renameuser.html", &model);
    let r = pages::respond(cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

/// The context, or the response that ends the request: a 404 where the API does not
/// serve the module.
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::RenameUser) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    Ok(cx)
}

/// `GET Special:RenameUser[/{name}]`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let notice = params
        .map
        .get("done")
        .map(|to| (MessageKind::Success, cx.m.with("ts-renameuser-done", &[to])));
    let back = Returned {
        notice,
        user: subject_of(title).unwrap_or_default(),
        ..Returned::default()
    };
    form_page(site, &mut cx, headers, &incoming, back).await
}

/// `POST Special:RenameUser`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.map.get(k).map_or("", String::as_str).trim();
    let user = field("user");
    let newuser = field("newuser");
    let reason = field("reason");
    let sent = RenameForm {
        user,
        newuser,
        reason,
        token: field("token"),
    };
    match site.client().renameuser(&incoming, &sent).await {
        Ok(Saved::Ok(v)) => {
            let to = v["renameuser"]["to"].as_str().unwrap_or(newuser);
            pages::see_other(
                &format!(
                    "/wiki/{}?done={}",
                    title_url(&format!("{TITLE}/{to}")),
                    title_url(to)
                ),
                &[],
            )
        }
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal_text(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            let back = Returned {
                notice: Some((MessageKind::Error, text)),
                user: user.to_string(),
                newuser: newuser.to_string(),
                reason: reason.to_string(),
            };
            form_page(site, &mut cx, headers, &incoming, back).await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subject_is_the_subpage() {
        assert_eq!(
            subject_of("Special:RenameUser/Alice_B").as_deref(),
            Some("Alice B")
        );
        assert_eq!(subject_of("Special:RenameUser"), None);
    }
}
