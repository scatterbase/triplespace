//! A tenant owner's invitation on `Special:CreateAccount?invite={code}` (0111 §5, A2;
//! 08 §1.4): the page says which wiki the invitation is for and the owner account's name,
//! and asks for a password, twice. Posting it redeems the invitation
//! (`POST /auth/invitation`), which creates a login for the owner account with that
//! password, and then logs the person in with the owner's name and the password, as
//! `Special:CreateAccount` does after a creation. An unknown, used or expired code says
//! so. It is served before the page's registration gate, since a wiki that closes
//! registration still takes its owner's invitation.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, LoginResult};

use triplespace_ui_kit::Site;
use triplespace_ui_kit::codex::{
    Action, Button, ButtonModel, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use triplespace_ui_kit::frame::{Page, title_url};
use triplespace_ui_kit::i18n::Messages;
use triplespace_ui_kit::pages::{self, Context, Peer};

const TITLE: &str = "Special:CreateAccount";

/// The invitation as Plain's `special/invitation.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InvitationPage {
    /// A refusal it came back with.
    pub notices: Vec<MessageModel>,
    /// Which wiki, as whom.
    pub intro: String,
    /// Where it posts.
    pub action: String,
    /// The code, carried by the form.
    pub code: String,
    /// The password.
    pub password: FieldModel,
    /// The password again.
    pub retype: FieldModel,
    /// **Accept and sign in**.
    pub submit: ButtonModel,
}

fn password_field(m: &Messages, name: &str) -> FieldModel {
    FieldModel {
        id: format!("ts-invitation-{name}"),
        name: name.into(),
        kind: "password".into(),
        value: String::new(),
        label: m.get(&format!("ts-invitation-{name}")),
        description: None,
        help: None,
        required: true,
        autocomplete: Some("new-password".into()),
    }
}

fn respond(cx: &Context, headers: &HeaderMap, status: StatusCode, body: String) -> Response {
    let heading = cx.m.get("ts-invitation-title");
    pages::respond(cx, headers, status, |_| Page {
        title: heading.clone(),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

async fn page(
    site: &Site,
    cx: &Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    code: &str,
    notice: Option<String>,
) -> Response {
    let info = match site.client().invitation(incoming, code).await {
        Ok(i) => i,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let Some(info) = info else {
        let body = cx.message(MessageKind::Error, &cx.m.get("ts-invitation-invalid"));
        return respond(cx, headers, StatusCode::NOT_FOUND, body);
    };
    let m = &cx.m;
    let model = InvitationPage {
        notices: notice
            .iter()
            .map(|t| Message::block(MessageKind::Error, t).model())
            .collect(),
        intro: m.with(
            "ts-invitation-intro",
            &[&info.wiki, info.owner.as_deref().unwrap_or("")],
        ),
        action: format!("/w/index.php?title={}", title_url(TITLE)),
        code: code.to_string(),
        password: password_field(m, "password"),
        retype: password_field(m, "retype"),
        submit: Button {
            label: &m.get("ts-invitation-submit"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
    };
    // A login token now, so that the session it opens carries the post's login after the
    // invitation is redeemed, as `Special:CreateAccount`'s form does.
    let token = match site.client().token(incoming, "login").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let body = cx.draw("special/invitation.html", &model);
    pages::with_cookies(
        respond(cx, headers, StatusCode::OK, body),
        &token.set_cookies,
    )
}

/// `GET Special:CreateAccount?invite={code}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    code: &str,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    page(site, &cx, headers, &incoming, code, None).await
}

/// `POST Special:CreateAccount` with `invite`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let code = field("invite").trim();
    let password = field("password");
    if password != field("retype") {
        let notice = cx.m.get("ts-invitation-mismatch");
        return page(site, &cx, headers, &incoming, code, Some(notice)).await;
    }
    let name = match site
        .client()
        .redeem_invitation(&incoming, code, password)
        .await
    {
        Ok(Saved::Ok(name)) => name,
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let notice = match first.map(|x| x.code.as_str()) {
                Some("passwordtooshort") => cx.m.get("ts-invitation-tooshort"),
                Some("ts-invitation-invalid") => cx.m.get("ts-invitation-invalid"),
                _ => first.map_or_else(String::new, |x| x.message.clone()),
            };
            return page(site, &cx, headers, &incoming, code, Some(notice)).await;
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    // Log the owner in with the name and the password just chosen.
    let token = match site.client().token(&incoming, "login").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let mut cookies = token.set_cookies;
    match site
        .client()
        .client_login(&incoming, &name, password, &token.value.value, "/", None)
        .await
    {
        Ok(r) if matches!(r.value, LoginResult::Pass { .. }) => {
            cookies.extend(r.set_cookies);
            pages::see_other("/", &cookies)
        }
        Ok(r) => {
            cookies.extend(r.set_cookies);
            pages::see_other("/wiki/Special:UserLogin", &cookies)
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
