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

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::issuers::start_path;
use triplespace_client::{AccountInfo, CacheInfo, EmailInfo, LoginIssuer};

use crate::Site;
use crate::codex::{Action, Button, Field, FieldModel, Message, MessageKind, Weight};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::moderation::link_to;
use crate::pages::{self, Context, Peer};

/// The issuers' buttons as data (19 §5.10; 0097 A6, A11): `account.html`'s `issuers`
/// draws them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IssuerButtons {
    /// What the section is for: `login` or `link`, which also names its heading's id.
    pub purpose: String,
    /// The heading.
    pub heading: String,
    /// The field the token goes in.
    pub token_field: String,
    /// The token.
    pub token: String,
    /// Where the API sends the reader back to.
    pub back: String,
    /// One form per issuer.
    pub forms: Vec<IssuerForm>,
    /// The note under the buttons.
    pub note: String,
}

/// One issuer's form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IssuerForm {
    /// The API's start route for the issuer.
    pub action: String,
    /// Its button.
    pub button: crate::codex::ButtonModel,
}

/// The issuers' buttons, or nothing when the wiki offers none.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn issuer_buttons(
    m: &Messages,
    issuers: &[LoginIssuer],
    token_field: &str,
    token: &str,
    purpose: &str,
    back: &str,
    heading_key: &str,
    button_key: &str,
) -> Option<IssuerButtons> {
    (!issuers.is_empty()).then(|| IssuerButtons {
        purpose: purpose.to_string(),
        heading: m.get(heading_key),
        token_field: token_field.to_string(),
        token: token.to_string(),
        back: back.to_string(),
        forms: issuers
            .iter()
            .map(|i| IssuerForm {
                action: start_path(&i.issuer),
                button: Button {
                    label: &m.with(button_key, &[&i.label]),
                    kind: "submit",
                    action: Action::Default,
                    weight: Weight::Normal,
                }
                .model(),
            })
            .collect(),
        note: m.get("ts-login-external-note"),
    })
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
        "ts-email-added" => "ts-email-added",
        "ts-email-resent" => "ts-email-resent",
        "ts-email-verified" => "ts-email-verified",
        "ts-email-primary-set" => "ts-email-primary-set",
        "ts-email-removed" => "ts-email-removed",
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

/// `Special:Account` as data (19 §5.10): `special/account.html` draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AccountPage {
    /// What the API answered, above the page.
    pub notice: Option<crate::codex::MessageModel>,
    /// For a reader not logged in: why there is nothing here.
    pub login_needed: Option<String>,
    /// And the way to log in, where the reader has no session.
    pub login: Option<Choice>,
    /// The Sign-in methods section.
    pub methods: Option<SignInMethods>,
    /// The Email addresses section (0100 §6).
    pub email: Option<EmailSection>,
}

/// The Email addresses section (19 §2.4): each address with its state and its forms, and
/// the form that adds one. Every form posts back to the page with `email_action`. With
/// [`post_email`] this implements 0100 §6, the account page's half; the optional address
/// field on `Special:CreateAccount` is not built yet.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmailSection {
    /// The heading.
    pub heading: String,
    /// That the section is private to the reader.
    pub private: String,
    /// What the section is for.
    pub intro: String,
    /// The warning while no address is verified, or that the site sends no mail.
    pub warning: Option<String>,
    /// Where the forms post.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The addresses, the primary first.
    pub addresses: Vec<EmailItem>,
    /// The form that adds an address, where one can be added.
    pub add: Option<EmailAdd>,
    /// Why no address can be added: the limit.
    pub add_note: Option<String>,
}

/// One address on the page.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmailItem {
    /// The address.
    pub address: String,
    /// Its state: unverified, verified or primary.
    pub state: String,
    /// When it was added.
    pub added: String,
    /// That mail to it bounced or was reported as spam, and what to do.
    pub undeliverable: Option<String>,
    /// For an unverified address, or a bounced one: the code field and **Verify**.
    pub code: Option<FieldModel>,
    /// **Verify**.
    pub verify: Option<crate::codex::ButtonModel>,
    /// When the current code expires, or that it is spent.
    pub code_note: Option<String>,
    /// **Send a new code**.
    pub resend: Option<crate::codex::ButtonModel>,
    /// **Make primary**, for a verified address that is not the primary.
    pub make_primary: Option<crate::codex::ButtonModel>,
    /// **Remove**.
    pub remove: crate::codex::ButtonModel,
    /// Why Remove is disabled, for the primary while it cannot go.
    pub remove_disabled: Option<String>,
}

/// The form that adds an address.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmailAdd {
    /// The address field.
    pub field: FieldModel,
    /// **Add**.
    pub button: crate::codex::ButtonModel,
}

/// `YYYY-MM-DD HH:MM UTC` of seconds since the epoch.
fn utc_minute(secs: u64) -> String {
    let rem = secs % 86_400;
    format!(
        "{} {:02}:{:02} UTC",
        iso_date(secs),
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// The Email addresses section's model.
#[allow(clippy::too_many_lines)]
fn email_model(m: &Messages, e: &EmailInfo, token: &str) -> EmailSection {
    let any_verified = e.addresses.iter().any(|a| a.verified);
    let warning = if !e.sends_mail {
        Some(m.get("ts-account-email-disabled"))
    } else if !any_verified {
        Some(m.get("ts-account-email-none-verified"))
    } else {
        None
    };
    let button = |key: &str, action: Action| {
        Button {
            label: &m.get(key),
            kind: "submit",
            action,
            weight: Weight::Normal,
        }
        .model()
    };
    let addresses = e
        .addresses
        .iter()
        .map(|a| {
            let state = m.get(if a.primary {
                "ts-account-email-state-primary"
            } else if a.verified {
                "ts-account-email-state-verified"
            } else {
                "ts-account-email-state-unverified"
            });
            let needs_code = !a.verified || a.undeliverable.is_some();
            let field_id = format!("ts-email-code-{}", slug(&a.address));
            let code = (needs_code && e.sends_mail).then(|| {
                Field {
                    id: &field_id,
                    name: "code",
                    kind: "text",
                    value: "",
                    label: &m.get("ts-account-email-code-label"),
                    description: None,
                    help: None,
                    required: true,
                    autocomplete: Some("one-time-code"),
                }
                .model()
            });
            let code_note = needs_code.then(|| {
                a.code_expires.map_or_else(
                    || m.get("ts-account-email-code-spent"),
                    |t| m.with("ts-account-email-code-expires", &[&utc_minute(t)]),
                )
            });
            let remove_disabled = a
                .primary
                .then_some(e.primary_removal.as_deref())
                .flatten()
                .map(|why| {
                    m.get(match why {
                        "ts-email-last" => "ts-account-email-remove-last",
                        "ts-email-required" => "ts-account-email-remove-required",
                        _ => "ts-account-email-remove-primary",
                    })
                });
            EmailItem {
                address: a.address.clone(),
                state,
                added: m.with("ts-account-added", &[&iso_date(a.added)]),
                undeliverable: a.undeliverable.as_deref().map(|r| {
                    m.get(if r == "complaint" {
                        "ts-account-email-undeliverable-complaint"
                    } else {
                        "ts-account-email-undeliverable-bounce"
                    })
                }),
                verify: code
                    .is_some()
                    .then(|| button("ts-account-email-verify", Action::Progressive)),
                code,
                code_note,
                resend: (needs_code && e.sends_mail)
                    .then(|| button("ts-account-email-resend", Action::Default)),
                make_primary: (a.verified && !a.primary)
                    .then(|| button("ts-account-email-make-primary", Action::Default)),
                remove: button("ts-account-email-remove", Action::Destructive),
                remove_disabled,
            }
        })
        .collect();
    let at_limit = i64::try_from(e.addresses.len()).unwrap_or(i64::MAX) >= e.max_addresses;
    let add = (e.sends_mail && !at_limit).then(|| EmailAdd {
        field: Field {
            id: "ts-email-add",
            name: "address",
            kind: "email",
            value: "",
            label: &m.get("ts-account-email-add-label"),
            description: None,
            help: Some(&m.get("ts-account-email-add-help")),
            required: true,
            autocomplete: Some("email"),
        }
        .model(),
        button: button("ts-account-email-add-button", Action::Progressive),
    });
    EmailSection {
        heading: m.get("ts-account-email-heading"),
        private: m.get("ts-account-private"),
        intro: m.get("ts-account-email-intro"),
        warning,
        action: format!("/w/index.php?title={}", title_url(TITLE)),
        token: token.to_string(),
        addresses,
        add,
        add_note: (e.sends_mail && at_limit)
            .then(|| m.with("ts-account-email-limit", &[&e.max_addresses.to_string()])),
    }
}

/// An address as part of an element ID: letters, digits and hyphens.
fn slug(address: &str) -> String {
    address
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The Sign-in methods section.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SignInMethods {
    /// The heading.
    pub heading: String,
    /// That the section is private to the reader.
    pub private: String,
    /// The password's item, where the account has one.
    pub password: Option<String>,
    /// The bindings, each with its remove form.
    pub bindings: Vec<Binding>,
    /// Where the remove forms post.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// Remove, a destructive button; disabled for the last way to log in.
    pub remove: crate::codex::ButtonModel,
    /// Whether this is the last way to log in.
    pub last: bool,
    /// The note under the list: why Remove is disabled, or what removing does.
    pub note: Option<String>,
    /// Ways still to add.
    pub add: Option<IssuerButtons>,
}

/// One binding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Binding {
    /// The issuer's code.
    pub issuer: String,
    /// Its name.
    pub label: String,
    /// When it was added and last used.
    pub dates: String,
    /// That the wiki no longer offers it.
    pub unavailable: Option<String>,
}

/// The Sign-in methods section's model.
fn sign_in_model(m: &Messages, a: &AccountInfo, token: &str) -> SignInMethods {
    let last = a.ways <= 1;
    SignInMethods {
        heading: m.get("ts-account-signin-heading"),
        private: m.get("ts-account-private"),
        password: a.password.then(|| m.get("ts-account-password")),
        bindings: a
            .bindings
            .iter()
            .map(|b| {
                let used = b.last_used.map_or_else(
                    || m.get("ts-account-never-used"),
                    |t| m.with("ts-account-last-used", &[&iso_date(t)]),
                );
                Binding {
                    issuer: b.issuer.clone(),
                    label: b.label.clone(),
                    dates: format!(
                        "{} · {used}",
                        m.with("ts-account-added", &[&iso_date(b.added)])
                    ),
                    unavailable: (!b.available).then(|| m.get("ts-account-unavailable")),
                }
            })
            .collect(),
        action: format!("/w/index.php?title={}", title_url(TITLE)),
        token: token.to_string(),
        remove: Button {
            label: &m.get("ts-account-remove"),
            kind: "submit",
            action: Action::Destructive,
            weight: Weight::Normal,
        }
        .model(),
        last,
        note: if last {
            Some(m.get("ts-account-remove-last"))
        } else if a.bindings.is_empty() {
            None
        } else {
            Some(m.get("ts-account-remove-note"))
        },
        add: issuer_buttons(
            m,
            &a.can_add,
            "token",
            token,
            "link",
            TITLE,
            "ts-account-add-heading",
            "ts-account-add-button",
        ),
    }
}

fn account_page(
    cx: &Context,
    headers: &HeaderMap,
    notice: Option<(MessageKind, String)>,
    mut model: AccountPage,
) -> Response {
    model.notice = notice.map(|(k, t)| Message::block(k, &t).model());
    let body = cx.draw("special/account.html", &model);
    allow_issuer_forms(
        cx,
        pages::respond(cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-account-title"),
            body,
            ..Page::default()
        }),
    )
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
        let model = AccountPage {
            notice: None,
            login_needed: Some(cx.m.get("ts-account-login-needed")),
            login: Some(link_to(
                format!("/wiki/Special:UserLogin?returnto={}", title_url(TITLE)),
                cx.m.get("ts-login"),
            )),
            methods: None,
            email: None,
        };
        return account_page(cx, headers, notice, model);
    }
    let account = match site.client().account(incoming).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            let model = AccountPage {
                notice: None,
                login_needed: Some(cx.m.get("ts-account-login-needed")),
                login: None,
                methods: None,
                email: None,
            };
            return account_page(cx, headers, notice, model);
        }
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let email = match site.client().email_addresses(incoming).await {
        Ok(e) => e,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let t = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let model = AccountPage {
        notice: None,
        login_needed: None,
        login: None,
        methods: Some(sign_in_model(&cx.m, &account, &t.value.value)),
        email: email
            .as_ref()
            .map(|e| email_model(&cx.m, e, &t.value.value)),
    };
    pages::with_cookies(account_page(cx, headers, notice, model), &t.set_cookies)
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

/// The message of an email refusal the API answered with (07 §2.9).
fn email_refusal(m: &Messages, code: &str, body: &serde_json::Value) -> String {
    match code {
        "ts-email-code" => match body["left"].as_i64().unwrap_or(0) {
            left if left > 0 => m.with("ts-email-code-wrong", &[&left.to_string()]),
            _ => m.get("ts-email-code-spent"),
        },
        "ratelimited" => m.with(
            "ts-email-ratelimited",
            &[&body["wait"].as_u64().unwrap_or(60).to_string()],
        ),
        "badtoken" => m.get("ts-login-expired"),
        "ts-email-invalid"
        | "ts-email-limit"
        | "ts-email-unverified"
        | "ts-email-taken"
        | "ts-email-primary"
        | "ts-email-last"
        | "ts-email-required"
        | "ts-email-disabled"
        | "ts-email-unknown" => m.get(code),
        _ => m.get("ts-email-failed"),
    }
}

/// `POST Special:Account` with `email_action` (0100 §6): adds an address, sends a new
/// code, verifies, makes primary or removes, then returns to the section with a notice.
async fn post_email(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &triplespace_client::Incoming,
    form: &BTreeMap<String, String>,
) -> Response {
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let mut fields = serde_json::Map::new();
    fields.insert("address".into(), serde_json::json!(field("address")));
    let (action, notice) = match field("email_action") {
        "add" => ("", "ts-email-added"),
        "resend" => ("", "ts-email-resent"),
        "verify" => {
            fields.insert("code".into(), serde_json::json!(field("code")));
            ("verify", "ts-email-verified")
        }
        "primary" => ("primary", "ts-email-primary-set"),
        "remove" => ("remove", "ts-email-removed"),
        _ => {
            return render(
                site,
                cx,
                headers,
                incoming,
                Some((MessageKind::Error, cx.m.get("ts-email-failed"))),
            )
            .await;
        }
    };
    match site
        .client()
        .email_write(incoming, action, field("token"), fields)
        .await
    {
        Ok(Ok(body)) => {
            // Adding an address the login already holds verified sends nothing.
            let notice = if action.is_empty() && body["sent"] == serde_json::json!(false) {
                "ts-email-verified"
            } else {
                notice
            };
            pages::see_other(
                &format!(
                    "/wiki/{}?tsnotice={notice}#email-addresses",
                    title_url(TITLE)
                ),
                &[],
            )
        }
        Ok(Err((code, body))) => {
            let text = email_refusal(&cx.m, &code, &body);
            render(
                site,
                cx,
                headers,
                incoming,
                Some((MessageKind::Error, text)),
            )
            .await
        }
        Err(e) => pages::api_failed(cx, headers, &e),
    }
}

/// `POST Special:Account`: removes a sign-in method, or with `email_action` acts on an
/// email address.
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
    if form.contains_key("email_action") {
        return post_email(site, &mut cx, headers, &incoming, form).await;
    }
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

    /// What Plain draws: the issuers' buttons alone, or Special:Account.
    fn drawn(m: &Messages, template: &str, model: impl serde::Serialize) -> String {
        let skins = crate::skin::Skins::load(None);
        let plain = skins.get("plain").expect("plain");
        if template == "issuers" {
            plain
                .render_str(
                    "{% import \"account.html\" as account %}{{ account.issuers(x) }}",
                    m,
                    serde_json::json!({ "x": model }),
                )
                .unwrap()
        } else {
            plain.render(template, m, model).unwrap()
        }
    }

    #[test]
    fn buttons_post_to_the_api() {
        let m = Messages::for_language("en");
        let model = issuer_buttons(
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
        let html = drawn(&m, "issuers", model);
        assert!(
            html.contains("action=\"/w/rest.php/triplespace/v0/auth/wikimedia-central/start\""),
            "{html}"
        );
        assert!(
            html.contains("name=\"logintoken\" value=\"tok+\\\""),
            "{html}"
        );
        assert!(html.contains("Log in with Wikimedia"));
        assert!(issuer_buttons(&m, &[], "t", "t", "login", "x", "a", "b").is_none());
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

    /// Special:Account's sign-in methods: Remove disabled for the last way to log in, with
    /// the note that says why; a binding the wiki no longer offers says so.
    #[test]
    fn the_sign_in_methods() {
        let m = Messages::for_language("en");
        let binding = |code: &str, available: bool| triplespace_client::issuers::Binding {
            issuer: code.into(),
            label: code.to_uppercase(),
            added: 1_792_281_600,
            last_used: None,
            available,
        };
        let page = |a: &AccountInfo| AccountPage {
            notice: None,
            login_needed: None,
            login: None,
            methods: Some(sign_in_model(&m, a, "t&k")),
            email: None,
        };
        let last = AccountInfo {
            name: None,
            password: false,
            bindings: vec![binding("orcid", true)],
            can_add: Vec::new(),
            ways: 1,
        };
        let html = drawn(&m, "special/account.html", page(&last));
        assert!(
            html.contains("disabled aria-describedby=\"ts-signin-last\""),
            "{html}"
        );
        assert!(html.contains("id=\"ts-signin-last\""), "{html}");
        assert!(html.contains("2026-10-18"), "{html}");
        assert!(html.contains("value=\"t&amp;k\""), "{html}");
        let two = AccountInfo {
            password: true,
            bindings: vec![binding("orcid", true), binding("gone", false)],
            can_add: vec![LoginIssuer {
                issuer: "wikimedia-central".into(),
                label: "Wikimedia".into(),
                origin: None,
            }],
            ways: 3,
            ..last
        };
        let html = drawn(&m, "special/account.html", page(&two));
        assert!(!html.contains(" disabled"), "{html}");
        assert!(html.contains("cdx-button--action-destructive"), "{html}");
        assert!(html.contains("ts-signin__unavailable"), "{html}");
        assert!(html.contains("ts-signin__note"), "{html}");
        assert!(html.contains("Add Wikimedia"), "{html}");
    }
}
