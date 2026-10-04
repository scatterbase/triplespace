//! `Special:UserLogin` and `Special:UserLogout` (0010 §10; 0047 §9; 0057 §4), without
//! JavaScript.
//!
//! - **Log in.** The form carries the API's login token for the viewer's session. The
//!   site posts the form to `clientlogin` with the browser's cookie, relays the API's
//!   `Set-Cookie` verbatim, and answers `303` to the page the reader came from (post,
//!   redirect, get). A refusal shows the form again, with the name kept; an expired
//!   token gets a fresh one.
//! - **Log out** is a form too, as in MediaWiki: a `GET` only asks, and the `POST`
//!   carries the CSRF token to `logout`.
//!
//! A page holding a token is never cacheable by anyone but the viewer, and every
//! response that relays a cookie is `private, no-store` (0057 §6; plan §6 Phase 6).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use triplespace_client::{CacheInfo, ClientError, Incoming, LoginResult};

use crate::Site;
use crate::codex::{Action, Button, Field, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};

/// The page to return to: `returnto` where it is a title, else the main page; never the
/// login or logout page itself.
fn returnto(cx: &Context, value: Option<&String>) -> String {
    let t = value.map_or("", |v| v.trim()).replace('_', " ");
    if t.is_empty()
        || t.contains("://")
        || t.starts_with('/')
        || t.to_ascii_lowercase().starts_with("special:userlog")
    {
        cx.site.mainpage.clone()
    } else {
        t
    }
}

/// The response that takes the reader on after a login or logout.
fn onward(returnto: &str, cookies: &[HeaderValue]) -> Response {
    pages::see_other(&format!("/wiki/{}", title_url(returnto)), cookies)
}

/// A page that holds a token is the viewer's alone.
fn private(cx: &mut Context) {
    cx.inputs.push(CacheInfo::default());
}

/// The login form.
fn login_form(m: &Messages, token: &str, back: &str, username: &str) -> String {
    let field = |id: &str, name: &str, kind: &str, value: &str, key: &str, auto: &str| {
        Field {
            id,
            name,
            kind,
            value,
            label: &m.get(key),
            description: None,
            help: None,
            required: true,
            autocomplete: Some(auto),
        }
        .render()
        .unwrap_or_default()
    };
    format!(
        "<form class=\"ts-form\" method=\"post\" action=\"/w/index.php?title=Special:UserLogin\"><input type=\"hidden\" name=\"logintoken\" value=\"{}\"><input type=\"hidden\" name=\"returnto\" value=\"{}\">{}{}<div class=\"ts-form__actions\">{}</div></form>",
        esc(token),
        esc(back),
        field(
            "wpName1",
            "username",
            "text",
            username,
            "ts-login-username",
            "username"
        ),
        field(
            "wpPassword1",
            "password",
            "password",
            "",
            "ts-login-password",
            "current-password"
        ),
        Button {
            label: &m.get("ts-login-submit"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default()
    )
}

/// The login page: a notice, if any, and the form.
fn login_page(
    cx: &Context,
    headers: &HeaderMap,
    notice: Option<(MessageKind, String)>,
    form: &str,
    back: String,
) -> Response {
    let mut body = String::new();
    if let Some((kind, text)) = notice {
        body.push_str(&pages::message(kind, &text));
    }
    body.push_str(form);
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-login-title"),
        returnto: Some(back),
        body: format!("<div class=\"ts-account-form\">{body}</div>"),
        ..Page::default()
    })
}

/// A fresh login token and the form around it, with the cookies its session needs.
async fn fresh_form(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    back: &str,
    username: &str,
) -> Result<(String, Vec<HeaderValue>), ClientError> {
    let t = site.client().token(incoming, "login").await?;
    cx.inputs.push(t.value.cache);
    Ok((
        login_form(&cx.m, &t.value.value, back, username),
        t.set_cookies,
    ))
}

/// The context, or the response that ends the request: the API away, or logging in not
/// offered.
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    title: &str,
) -> Result<Context, Response> {
    let mut cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Login) {
        return Err(pages::no_page(&cx, headers, title.to_string()));
    }
    private(&mut cx);
    Ok(cx)
}

/// `GET Special:UserLogin`.
pub async fn login(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, "Special:UserLogin").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, query.get("returnto"));
    if let Some(name) = cx.user.clone() {
        let body = format!(
            "{}<p>{}</p>",
            pages::message(
                MessageKind::Notice,
                &cx.m.with("ts-login-already", &[&name])
            ),
            link(
                &format!("/wiki/{}", title_url(&back)),
                &esc(&cx.m.with("ts-login-continue", &[&back])),
                ""
            )
        );
        return login_page(&cx, headers, None, &body, back);
    }
    match fresh_form(site, &mut cx, &incoming, &back, "").await {
        Ok((form, cookies)) => {
            pages::with_cookies(login_page(&cx, headers, None, &form, back), &cookies)
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `POST Special:UserLogin`.
pub async fn post_login(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, "Special:UserLogin").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, form.get("returnto"));
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let (username, token) = (field("username").trim().to_string(), field("logintoken"));
    let return_url = format!(
        "{}/wiki/{}",
        cx.site.server.trim_end_matches('/'),
        title_url(&back)
    );
    let result = site
        .client()
        .client_login(&incoming, &username, field("password"), token, &return_url)
        .await;
    match result {
        Ok(r) => match r.value {
            LoginResult::Pass { .. } => onward(&back, &r.set_cookies),
            LoginResult::Fail { message, code } => {
                let text = match code.as_str() {
                    "wrongpassword" => cx.m.get("ts-login-wrongpassword"),
                    "ts-subsidiary-password" => cx.m.with("ts-login-subsidiary", &[&username]),
                    _ => message,
                };
                let page = login_page(
                    &cx,
                    headers,
                    Some((MessageKind::Error, text)),
                    &login_form(&cx.m, token, &back, &username),
                    back,
                );
                pages::with_cookies(page, &r.set_cookies)
            }
            LoginResult::BadToken => {
                match fresh_form(site, &mut cx, &incoming, &back, &username).await {
                    Ok((f, cookies)) => {
                        let notice = Some((MessageKind::Warning, cx.m.get("ts-login-expired")));
                        let mut all = r.set_cookies;
                        all.extend(cookies);
                        pages::with_cookies(login_page(&cx, headers, notice, &f, back), &all)
                    }
                    Err(e) => pages::api_failed(&cx, headers, &e),
                }
            }
        },
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The logout form, or the note that there is no one to log out.
fn logout_body(m: &Messages, token: &str, back: &str) -> String {
    let mut s = format!(
        "<form class=\"ts-form\" method=\"post\" action=\"/w/index.php?title=Special:UserLogout\"><p>{}</p><input type=\"hidden\" name=\"token\" value=\"{}\"><input type=\"hidden\" name=\"returnto\" value=\"{}\"><div class=\"ts-form__actions\">",
        esc(&m.get("ts-logout-question")),
        esc(token),
        esc(back)
    );
    let _ = write!(
        s,
        "{}</div></form>",
        Button {
            label: &m.get("ts-logout-submit"),
            kind: "submit",
            action: Action::Destructive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default()
    );
    s
}

/// The logout page.
fn logout_page(cx: &Context, headers: &HeaderMap, body: &str, back: String) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-logout-title"),
        returnto: Some(back),
        body: format!("<div class=\"ts-account-form\">{body}</div>"),
        ..Page::default()
    })
}

/// A CSRF token and the logout form around it, or the note that no one is logged in.
async fn logout_form(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    back: &str,
    notice: Option<String>,
) -> Result<(String, Vec<HeaderValue>), ClientError> {
    let mut body = notice.map_or_else(String::new, |n| pages::message(MessageKind::Warning, &n));
    if cx.user.is_none() {
        let _ = write!(
            body,
            "<p>{}</p><p>{}</p>",
            esc(&cx.m.get("ts-logout-not-logged-in")),
            link(
                &format!("/wiki/{}", title_url(back)),
                &esc(&cx.m.with("ts-login-continue", &[back])),
                ""
            )
        );
        return Ok((body, Vec::new()));
    }
    let t = site.client().token(incoming, "csrf").await?;
    cx.inputs.push(t.value.cache);
    body.push_str(&logout_body(&cx.m, &t.value.value, back));
    Ok((body, t.set_cookies))
}

/// `GET Special:UserLogout`: asks.
pub async fn logout(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, "Special:UserLogout").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, query.get("returnto"));
    match logout_form(site, &mut cx, &incoming, &back, None).await {
        Ok((body, cookies)) => {
            pages::with_cookies(logout_page(&cx, headers, &body, back), &cookies)
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `POST Special:UserLogout`: logs out.
pub async fn post_logout(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, "Special:UserLogout").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, form.get("returnto"));
    let token = form.get("token").map_or("", String::as_str);
    match site.client().logout(&incoming, token).await {
        Ok(r) => onward(&back, &r.set_cookies),
        Err(ClientError::Api { code, .. }) if code == "badtoken" => {
            let notice = Some(cx.m.get("ts-login-expired"));
            match logout_form(site, &mut cx, &incoming, &back, notice).await {
                Ok((body, cookies)) => {
                    pages::with_cookies(logout_page(&cx, headers, &body, back), &cookies)
                }
                Err(e) => pages::api_failed(&cx, headers, &e),
            }
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
