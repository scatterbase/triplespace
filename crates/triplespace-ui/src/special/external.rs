//! Logging in through external issuers on the site (0088 §5–§7; 19 §2.3, §2.4), without
//! JavaScript.
//!
//! - **Buttons.** `Special:UserLogin` and `Special:CreateAccount` offer one form per
//!   issuer the wiki offers, posting the session's login token straight to the API's
//!   `/auth/{issuer}/start`; the provider returns to the API, which redirects back here.
//! - **Answers.** The API comes back with `tserror` or `tsnotice` in the query, which the
//!   pages show as a message.
//! - **`Special:Account`**, its Sign-in methods section: each binding with when it was
//!   added and last used, **Remove** (disabled for the last way to log in), and **Add**,
//!   a form to `/auth/{issuer}/start` with `purpose=link` and the CSRF token.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::issuers::start_path;
use triplespace_client::{AccountInfo, CacheInfo, LoginIssuer};

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::esc;
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};

/// One form per issuer: a button that posts the token to the API's start route.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn buttons(
    m: &Messages,
    issuers: &[LoginIssuer],
    token_field: &str,
    token: &str,
    purpose: &str,
    back: &str,
    heading_key: &str,
    button_key: &str,
) -> String {
    if issuers.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<section class=\"ts-login__issuers\" aria-labelledby=\"ts-issuers-{purpose}\"><h2 class=\"ts-login__issuers-heading\" id=\"ts-issuers-{purpose}\">{}</h2>",
        esc(&m.get(heading_key))
    );
    for i in issuers {
        let _ = write!(
            out,
            "<form class=\"ts-form ts-login__issuer\" method=\"post\" action=\"{}\"><input type=\"hidden\" name=\"purpose\" value=\"{}\"><input type=\"hidden\" name=\"{}\" value=\"{}\"><input type=\"hidden\" name=\"returnto\" value=\"{}\">{}</form>",
            esc(&start_path(&i.issuer)),
            esc(purpose),
            esc(token_field),
            esc(token),
            esc(back),
            Button {
                label: &m.with(button_key, &[&i.label]),
                kind: "submit",
                action: Action::Default,
                weight: Weight::Normal,
            }
            .render()
            .unwrap_or_default()
        );
    }
    let _ = write!(
        out,
        "<p class=\"ts-login__issuers-note\">{}</p></section>",
        esc(&m.get("ts-login-external-note"))
    );
    out
}

/// The site's policy with `form-action` widened to the authorization origins of the
/// issuers a page's buttons post for (0088 §5): the start route answers `303` to the
/// provider, and a browser applies `form-action` to a form's redirects.
#[must_use]
pub fn csp_for(issuers: &[LoginIssuer]) -> Option<String> {
    let mut origins: Vec<String> = issuers
        .iter()
        .filter_map(|i| i.origin.as_deref())
        .filter_map(|o| url::Url::parse(o).ok())
        .filter(|u| u.scheme() == "https")
        .map(|u| u.origin().ascii_serialization())
        .collect();
    origins.sort();
    origins.dedup();
    if origins.is_empty() {
        return None;
    }
    Some(pages::CSP.replace(
        "form-action 'self'",
        &format!("form-action 'self' {}", origins.join(" ")),
    ))
}

/// A page response that carries issuers' buttons, with its `form-action` widened.
#[must_use]
pub fn allow_issuer_forms(cx: &Context, mut r: Response) -> Response {
    if let Some(csp) = csp_for(&cx.site.login_issuers)
        && let Ok(v) = axum::http::HeaderValue::from_str(&csp)
    {
        r.headers_mut()
            .insert(axum::http::header::CONTENT_SECURITY_POLICY, v);
    }
    r
}

/// The message key of an answer code the API sends back in `tserror` or `tsnotice`.
fn key_of(code: &str) -> Option<&'static str> {
    Some(match code {
        "ts-oauth-denied" => "ts-oauth-denied",
        "ts-oauth-state" => "ts-oauth-state",
        "ts-oauth-failed" => "ts-oauth-failed",
        "ts-oauth-unavailable" => "ts-oauth-unavailable",
        "ts-oauth-bound" => "ts-oauth-bound",
        "ts-oauth-bound-elsewhere" => "ts-oauth-bound-elsewhere",
        "ts-last-binding" => "ts-last-binding",
        "ts-reclaimed" => "ts-reclaimed",
        "ts-reclaim-taken" => "ts-reclaim-taken",
        "ts-reclaim-unavailable" => "ts-reclaim-unavailable",
        "ts-account-pending" => "ts-login-pending",
        "ts-account-rejected" => "ts-login-rejected-external",
        "ts-account-requested" => "ts-createaccount-requested",
        "ts-linked" => "ts-account-linked",
        "ts-unlinked" => "ts-account-unlinked",
        "userexists" => "ts-createaccount-userexists",
        "acct_creation_throttle_hit" => "ts-createaccount-throttle",
        _ => return None,
    })
}

/// The message an answer in the query asks for: `tserror` as an error, `tsnotice` as a
/// notice. An unknown code shows nothing.
#[must_use]
pub fn answer(m: &Messages, query: &BTreeMap<String, String>) -> Option<(MessageKind, String)> {
    if let Some(k) = query.get("tserror").and_then(|c| key_of(c)) {
        return Some((MessageKind::Error, m.get(k)));
    }
    query
        .get("tsnotice")
        .and_then(|c| key_of(c))
        .map(|k| (MessageKind::Success, m.get(k)))
}

/// `YYYY-MM-DD` of seconds since the epoch, in UTC (the civil-from-days algorithm).
#[must_use]
pub fn iso_date(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}")
}

const TITLE: &str = "Special:Account";

fn account_page(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    notice: Option<(MessageKind, String)>,
    body: &str,
) -> Response {
    let mut html = String::new();
    if let Some((kind, text)) = notice {
        html.push_str(&pages::message(kind, &text));
    }
    html.push_str(body);
    allow_issuer_forms(
        cx,
        pages::respond(cx, headers, status, |m| Page {
            title: m.get("ts-account-title"),
            body: format!("<div class=\"ts-account\">{html}</div>"),
            ..Page::default()
        }),
    )
}

fn remove_form(m: &Messages, issuer: &str, token: &str, last: bool) -> String {
    let mut s = format!(
        "<form class=\"ts-signin__remove\" method=\"post\" action=\"/w/index.php?title={}\"><input type=\"hidden\" name=\"issuer\" value=\"{}\"><input type=\"hidden\" name=\"token\" value=\"{}\">",
        title_url(TITLE),
        esc(issuer),
        esc(token)
    );
    if last {
        let _ = write!(
            s,
            "<button class=\"cdx-button\" type=\"submit\" disabled aria-describedby=\"ts-signin-last\">{}</button>",
            esc(&m.get("ts-account-remove"))
        );
    } else {
        s.push_str(
            &Button {
                label: &m.get("ts-account-remove"),
                kind: "submit",
                action: Action::Destructive,
                weight: Weight::Normal,
            }
            .render()
            .unwrap_or_default(),
        );
    }
    s.push_str("</form>");
    s
}

/// The Sign-in methods section.
fn sign_in_methods(m: &Messages, a: &AccountInfo, token: &str) -> String {
    let last = a.ways <= 1;
    let mut s = format!(
        "<section class=\"ts-signin\" id=\"sign-in-methods\" aria-labelledby=\"ts-signin-heading\"><h2 id=\"ts-signin-heading\">{} <span class=\"ts-signin__private\">{}</span></h2><ul class=\"ts-signin__list\">",
        esc(&m.get("ts-account-signin-heading")),
        esc(&m.get("ts-account-private"))
    );
    if a.password {
        let _ = write!(
            s,
            "<li class=\"ts-signin__item\"><span class=\"ts-signin__name\">{}</span></li>",
            esc(&m.get("ts-account-password"))
        );
    }
    for b in &a.bindings {
        let used = b.last_used.map_or_else(
            || m.get("ts-account-never-used"),
            |t| m.with("ts-account-last-used", &[&iso_date(t)]),
        );
        let _ = write!(
            s,
            "<li class=\"ts-signin__item\"><span class=\"ts-signin__name\">{}</span> <span class=\"ts-signin__dates\">{} · {}</span>{}{}</li>",
            esc(&b.label),
            esc(&m.with("ts-account-added", &[&iso_date(b.added)])),
            esc(&used),
            if b.available {
                String::new()
            } else {
                format!(
                    " <span class=\"ts-signin__unavailable\">{}</span>",
                    esc(&m.get("ts-account-unavailable"))
                )
            },
            remove_form(m, &b.issuer, token, last)
        );
    }
    s.push_str("</ul>");
    if last {
        let _ = write!(
            s,
            "<p class=\"ts-signin__last\" id=\"ts-signin-last\">{}</p>",
            esc(&m.get("ts-account-remove-last"))
        );
    } else if !a.bindings.is_empty() {
        let _ = write!(
            s,
            "<p class=\"ts-signin__note\">{}</p>",
            esc(&m.get("ts-account-remove-note"))
        );
    }
    s.push_str(&buttons(
        m,
        &a.can_add,
        "token",
        token,
        "link",
        TITLE,
        "ts-account-add-heading",
        "ts-account-add-button",
    ));
    s.push_str("</section>");
    s
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Result<(Context, triplespace_client::Incoming), Response> {
    let incoming = pages::incoming(headers, peer);
    let mut cx = pages::context(site, &incoming, query).await?;
    if !cx.features.has(Feature::Login) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    cx.inputs.push(CacheInfo::default());
    Ok((cx, incoming))
}

/// The page, with a fresh CSRF token.
async fn render(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &triplespace_client::Incoming,
    notice: Option<(MessageKind, String)>,
) -> Response {
    if cx.user.is_none() {
        let body = format!(
            "<p>{}</p><p><a href=\"/wiki/Special:UserLogin?returnto={}\">{}</a></p>",
            esc(&cx.m.get("ts-account-login-needed")),
            title_url(TITLE),
            esc(&cx.m.get("ts-login"))
        );
        return account_page(cx, headers, StatusCode::OK, notice, &body);
    }
    let account = match site.client().account(incoming).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            let body = format!("<p>{}</p>", esc(&cx.m.get("ts-account-login-needed")));
            return account_page(cx, headers, StatusCode::OK, notice, &body);
        }
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let t = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let body = sign_in_methods(&cx.m, &account, &t.value.value);
    pages::with_cookies(
        account_page(cx, headers, StatusCode::OK, notice, &body),
        &t.set_cookies,
    )
}

/// `GET Special:Account`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let (mut cx, incoming) = match start(site, headers, peer, query).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let notice = answer(&cx.m, query);
    render(site, &mut cx, headers, &incoming, notice).await
}

/// `POST Special:Account`: removes a sign-in method.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let (mut cx, incoming) = match start(site, headers, peer, query).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    match site
        .client()
        .unlink(&incoming, field("issuer"), field("token"))
        .await
    {
        Ok(Ok(())) => pages::see_other(
            &format!(
                "/wiki/{}?tsnotice=ts-unlinked#sign-in-methods",
                title_url(TITLE)
            ),
            &[],
        ),
        Ok(Err(code)) => {
            let text = match code.as_str() {
                "ts-last-binding" => cx.m.get("ts-last-binding"),
                "badtoken" => cx.m.get("ts-login-expired"),
                _ => cx.m.get("ts-account-remove-failed"),
            };
            render(
                site,
                &mut cx,
                headers,
                &incoming,
                Some((MessageKind::Error, text)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(1_792_281_600), "2026-10-18");
        assert_eq!(iso_date(951_782_400), "2000-02-29");
    }

    #[test]
    fn buttons_post_to_the_api() {
        let m = Messages::for_language("en");
        let html = buttons(
            &m,
            &[LoginIssuer {
                issuer: "wikimedia-central".into(),
                label: "Wikimedia".into(),
                origin: None,
            }],
            "logintoken",
            "tok+\\",
            "login",
            "Main Page",
            "ts-login-external-heading",
            "ts-login-external-button",
        );
        assert!(
            html.contains("action=\"/w/rest.php/triplespace/v0/auth/wikimedia-central/start\"")
        );
        assert!(html.contains("name=\"logintoken\" value=\"tok+\\\""));
        assert!(html.contains("Log in with Wikimedia"));
        assert!(
            buttons(
                &m,
                &[],
                "t",
                "t",
                "login",
                "x",
                "ts-login-external-heading",
                "ts-login-external-button"
            )
            .is_empty()
        );
    }

    #[test]
    fn form_action_widens_to_https_origins_only() {
        let i = |o: &str| LoginIssuer {
            issuer: "x".into(),
            label: "X".into(),
            origin: Some(o.into()),
        };
        let csp = csp_for(&[
            i("https://meta.wikimedia.org"),
            i("https://meta.wikimedia.org"),
            i("http://insecure.example"),
        ])
        .unwrap();
        assert!(
            csp.ends_with("form-action 'self' https://meta.wikimedia.org"),
            "{csp}"
        );
        assert_eq!(csp_for(&[]), None);
        assert_eq!(csp_for(&[i("javascript:alert(1)")]), None);
    }

    #[test]
    fn answers_are_known_codes_only() {
        let m = Messages::for_language("en");
        let q = |k: &str, v: &str| BTreeMap::from([(k.to_string(), v.to_string())]);
        assert!(matches!(
            answer(&m, &q("tserror", "ts-oauth-denied")),
            Some((MessageKind::Error, _))
        ));
        assert!(matches!(
            answer(&m, &q("tsnotice", "ts-linked")),
            Some((MessageKind::Success, _))
        ));
        assert!(answer(&m, &q("tserror", "<script>")).is_none());
    }
}
