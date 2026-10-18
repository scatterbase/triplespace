//! `Special:CreateAccount` (with its alias `Special:RequestAccount`) and
//! `Special:ConfirmAccounts` (0087 §5, §6; 19 §2.3).
//!
//! - **The form** asks for a name, a password twice and, where the wiki reviews new
//!   accounts, a statement. Where the wiki guards registration with a challenge, the
//!   form carries the challenge the API issued to the browser's session, an empty field
//!   for its solution, a polite live region the site's script reports its progress in,
//!   and a `<noscript>` block that says what is asked and why. The script solves the
//!   challenge in a Web Worker while the person types. Submitted without a solution, the
//!   API asks again, and the page explains that the browser needs JavaScript for this,
//!   and what else the person can do.
//! - **A honeypot field**, off-screen and outside the accessibility tree, is on the form
//!   in every configuration; a submission that fills it is answered as a refused
//!   creation without calling `createaccount`.
//! - **After creating**, a person who was not logged in is logged in with the name and
//!   password they chose and returned to the page they came from. Creating for someone
//!   else says so and stays. Where the wiki reviews accounts, the page says the request
//!   was filed and that its outcome is learned by logging in.
//! - **`Special:ConfirmAccounts`** lists the open requests, oldest first, each with its
//!   statement, its age, and its address for holders of `ts-viewip`, and a form to
//!   approve it or to reject it with a reason.
//!
//! Every page holding a token is the viewer's alone.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use triplespace_client::settings::Saved;
use triplespace_client::{
    CacheInfo, ChallengeInfo, CreateFields, CreateResult, Incoming, LoginResult,
};

use crate::Site;
use crate::codex::{Action, Button, Field, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};

const TITLE: &str = "Special:CreateAccount";
const REVIEW: &str = "Special:ConfirmAccounts";

/// The honeypot's field name: a word a form-filler fills.
pub const HONEYPOT: &str = "wpWebsite";

/// The challenge's part of a form: the challenge, the solution field, the live region
/// the script reports in, and the `<noscript>` explanation. `solution` is kept where the
/// form comes back for a reason that did not spend it.
#[must_use]
pub fn challenge_fields(
    m: &Messages,
    challenge: Option<&ChallengeInfo>,
    solution: &str,
    nojs_key: &str,
) -> String {
    let Some(c) = challenge else {
        return String::new();
    };
    format!(
        "<input type=\"hidden\" name=\"tschallengeid\" value=\"{}\" data-ts-challenge-bits=\"{}\"><input type=\"hidden\" name=\"tschallengesolution\" value=\"{}\"><p class=\"ts-challenge__status\" role=\"status\" aria-live=\"polite\" data-ts-challenge-status data-working=\"{}\" data-waiting=\"{}\" data-done=\"{}\" data-failed=\"{}\"></p><noscript><div class=\"cdx-message cdx-message--block cdx-message--notice\"><span class=\"cdx-message__icon\"></span><div class=\"cdx-message__content\">{}</div></div></noscript>",
        esc(&c.id),
        c.bits,
        esc(solution),
        esc(&m.get("ts-challenge-working")),
        esc(&m.get("ts-challenge-waiting")),
        esc(&m.get("ts-challenge-done")),
        esc(&m.get("ts-challenge-failed")),
        esc(&m.get(nojs_key))
    )
}

/// The challenge a form came back with, and its solution.
#[must_use]
pub fn posted_challenge(form: &BTreeMap<String, String>) -> Option<(String, String)> {
    let id = form.get("tschallengeid").filter(|s| !s.is_empty())?;
    Some((
        id.clone(),
        form.get("tschallengesolution").cloned().unwrap_or_default(),
    ))
}

/// The challenge a form came back with, as the next form carries it.
fn challenge_of(id: &str) -> Option<ChallengeInfo> {
    let bits = id.split('.').nth(3)?.parse().ok()?;
    Some(ChallengeInfo {
        id: id.to_string(),
        bits,
    })
}

/// What a form is drawn from.
struct FormState<'a> {
    token: &'a str,
    back: &'a str,
    username: &'a str,
    statement: Option<(&'a str, &'a str)>,
    challenge: Option<ChallengeInfo>,
    solution: &'a str,
    approval: bool,
    for_another: bool,
}

fn text_field(
    m: &Messages,
    id: &str,
    name: &str,
    kind: &str,
    value: &str,
    key: &str,
    auto: &str,
) -> String {
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
}

fn create_form(m: &Messages, s: &FormState<'_>) -> String {
    let mut out = String::new();
    if s.approval {
        let _ = write!(out, "<p>{}</p>", esc(&m.get("ts-createaccount-reviewed")));
    }
    let _ = write!(
        out,
        "<form class=\"ts-form ts-createaccount\" method=\"post\" action=\"/w/index.php?title={}\"{}><input type=\"hidden\" name=\"createtoken\" value=\"{}\"><input type=\"hidden\" name=\"returnto\" value=\"{}\">",
        title_url(TITLE),
        s.challenge
            .as_ref()
            .map(|c| format!(" data-ts-challenge=\"{}\"", c.bits))
            .unwrap_or_default(),
        esc(s.token),
        esc(s.back)
    );
    out.push_str(&text_field(
        m,
        "wpName2",
        "username",
        "text",
        s.username,
        "ts-createaccount-username",
        "username",
    ));
    out.push_str(&text_field(
        m,
        "wpPassword2",
        "password",
        "password",
        "",
        "ts-createaccount-password",
        "new-password",
    ));
    out.push_str(&text_field(
        m,
        "wpRetype",
        "retype",
        "password",
        "",
        "ts-createaccount-retype",
        "new-password",
    ));
    if let Some((mode, value)) = s.statement {
        let required = mode == "required";
        let _ = write!(
            out,
            "<input type=\"hidden\" name=\"tsstatementmode\" value=\"{}\">",
            if required { "required" } else { "optional" }
        );
        let _ = write!(
            out,
            "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"wpStatement\"><span class=\"cdx-label__label__text\">{}</span>{}</label><span class=\"cdx-label__description\" id=\"wpStatement-description\">{}</span></div><div class=\"cdx-field__control\"><div class=\"cdx-text-area\"><textarea class=\"cdx-text-area__textarea\" id=\"wpStatement\" name=\"tsstatement\" rows=\"4\" maxlength=\"1000\" aria-describedby=\"wpStatement-description\"{}>{}</textarea></div></div></div>",
            esc(&m.get("ts-createaccount-statement")),
            if required {
                String::new()
            } else {
                format!(
                    "<span class=\"cdx-label__label__optional-flag\"> {}</span>",
                    esc(&m.get("ts-createaccount-optional"))
                )
            },
            esc(&m.get("ts-createaccount-statement-help")),
            if required { " required" } else { "" },
            esc(value)
        );
    }
    // The honeypot: off-screen, out of the tab order and the accessibility tree.
    let _ = write!(
        out,
        "<div class=\"ts-honeypot\" aria-hidden=\"true\"><label for=\"{HONEYPOT}\">{}</label><input type=\"text\" id=\"{HONEYPOT}\" name=\"{HONEYPOT}\" value=\"\" tabindex=\"-1\" autocomplete=\"off\"></div>",
        esc(&m.get("ts-createaccount-honeypot"))
    );
    out.push_str(&challenge_fields(
        m,
        s.challenge.as_ref(),
        s.solution,
        "ts-challenge-nojs",
    ));
    let label = if s.approval {
        m.get("ts-createaccount-request-submit")
    } else if s.for_another {
        m.get("ts-createaccount-other-submit")
    } else {
        m.get("ts-createaccount-submit")
    };
    let _ = write!(
        out,
        "<div class=\"ts-form__actions\">{}</div></form>",
        Button {
            label: &label,
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default()
    );
    out
}

fn page(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    notice: Option<(MessageKind, String)>,
    body: &str,
    back: &str,
) -> Response {
    let mut html = String::new();
    if let Some((kind, text)) = notice {
        html.push_str(&pages::message(kind, &text));
    }
    html.push_str(body);
    let back = back.to_string();
    pages::respond(cx, headers, status, |m| Page {
        title: m.get("ts-createaccount-title"),
        returnto: Some(back),
        body: format!("<div class=\"ts-account-form\">{html}</div>"),
        ..Page::default()
    })
}

/// The page to return to: `returnto` where it is a title, else the main page.
fn returnto(cx: &Context, value: Option<&String>) -> String {
    let t = value.map_or("", |v| v.trim()).replace('_', " ");
    let lower = t.to_ascii_lowercase();
    if t.is_empty()
        || t.contains("://")
        || t.starts_with('/')
        || lower.starts_with("special:userlog")
        || lower.starts_with("special:createaccount")
        || lower.starts_with("special:requestaccount")
    {
        cx.site.mainpage.clone()
    } else {
        t
    }
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    title: &str,
) -> Result<Context, Response> {
    let mut cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Register) {
        return Err(pages::no_page(&cx, headers, title.to_string()));
    }
    cx.inputs.push(CacheInfo::default());
    Ok(cx)
}

/// A fresh form, or the page that says the viewer may not create an account.
#[allow(clippy::too_many_arguments)]
async fn fresh(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: &str,
    username: &str,
    statement: &str,
    notice: Option<(MessageKind, String)>,
    status: StatusCode,
    mut cookies: Vec<HeaderValue>,
) -> Response {
    let f = match site.client().create_form(incoming).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cookies.extend(f.set_cookies);
    let f = f.value;
    if !f.can_create {
        let body = format!(
            "<p>{}</p>",
            esc(&cx.m.get(if cx.user.is_some() {
                "ts-createaccount-not-allowed-user"
            } else {
                "ts-createaccount-not-allowed"
            }))
        );
        return pages::with_cookies(
            page(cx, headers, StatusCode::FORBIDDEN, None, &body, back),
            &cookies,
        );
    }
    let approval = cx.site.account_approval && !holds_confirm(cx);
    let state = FormState {
        token: &f.token,
        back,
        username,
        statement: f.statement.as_deref().map(|mode| (mode, statement)),
        challenge: f.challenge,
        solution: "",
        approval,
        for_another: cx.user.is_some(),
    };
    let body = create_form(&cx.m, &state);
    pages::with_cookies(page(cx, headers, status, notice, &body, back), &cookies)
}

/// Whether the viewer holds `confirmaccount`, which the site reads from the open-request
/// count the API reports to holders alone; a holder creates accounts at once.
fn holds_confirm(cx: &Context) -> bool {
    cx.user.is_some() && cx.account_requests.is_some()
}

/// `GET Special:CreateAccount`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, TITLE).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, query.get("returnto"));
    let notice = cx.user.clone().map(|name| {
        (
            MessageKind::Notice,
            cx.m.with("ts-createaccount-as-other", &[&name]),
        )
    });
    fresh(
        site,
        &mut cx,
        headers,
        &incoming,
        &back,
        "",
        "",
        notice,
        StatusCode::OK,
        Vec::new(),
    )
    .await
}

/// The message for a refused creation.
fn refusal(m: &Messages, code: &str, message: &str) -> String {
    match code {
        "userexists" => m.get("ts-createaccount-userexists"),
        "noname" => m.get("ts-createaccount-noname"),
        "badretype" => m.get("ts-createaccount-badretype"),
        "passwordtooshort" => m.get("ts-createaccount-passwordtooshort"),
        "acct_creation_throttle_hit" => m.get("ts-createaccount-throttle"),
        "ts-account-statement-required" => m.get("ts-createaccount-statement-required"),
        "ts-account-statement-long" => m.get("ts-createaccount-statement-long"),
        "permissiondenied" => m.get("ts-createaccount-not-allowed"),
        _ => message.to_string(),
    }
}

/// `POST Special:CreateAccount`.
#[allow(clippy::too_many_lines)]
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, TITLE).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, form.get("returnto"));
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let username = field("username").trim().to_string();
    let statement = field("tsstatement").to_string();
    // A filled honeypot is answered as a refused creation, without asking the API.
    if !field(HONEYPOT).is_empty() {
        let notice = Some((MessageKind::Error, cx.m.get("ts-createaccount-failed")));
        return fresh(
            site,
            &mut cx,
            headers,
            &incoming,
            &back,
            &username,
            &statement,
            notice,
            StatusCode::OK,
            Vec::new(),
        )
        .await;
    }
    let posted = posted_challenge(form);
    let return_url = format!(
        "{}/wiki/{}",
        cx.site.server.trim_end_matches('/'),
        title_url(&back)
    );
    let fields = CreateFields {
        username: &username,
        password: field("password"),
        retype: field("retype"),
        token: field("createtoken"),
        return_url: &return_url,
        challenge: posted.as_ref().map(|(i, s)| (i.as_str(), s.as_str())),
        statement: form.get("tsstatement").map(String::as_str),
    };
    let result = match site.client().create_account(&incoming, &fields).await {
        Ok(r) => r,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let mut cookies = result.set_cookies;
    let approval = cx.site.account_approval && !holds_confirm(&cx);
    let again = |cx: &Context,
                 challenge: Option<ChallengeInfo>,
                 solution: &str,
                 notice: (MessageKind, String)| {
        let state = FormState {
            token: field("createtoken"),
            back: &back,
            username: &username,
            statement: form
                .get("tsstatementmode")
                .map(|mode| (mode.as_str(), statement.as_str())),
            challenge,
            solution,
            approval,
            for_another: cx.user.is_some(),
        };
        page(
            cx,
            headers,
            StatusCode::OK,
            Some(notice),
            &create_form(&cx.m, &state),
            &back,
        )
    };
    match result.value {
        CreateResult::Pass { username: name } => {
            if cx.user.is_some() {
                let body = format!(
                    "<p>{}</p>",
                    link(
                        &format!("/wiki/User:{}", title_url(&name)),
                        &esc(&cx.m.with("ts-createaccount-userpage", &[&name])),
                        ""
                    )
                );
                let notice = Some((
                    MessageKind::Success,
                    cx.m.with("ts-createaccount-created-other", &[&name]),
                ));
                return pages::with_cookies(
                    page(&cx, headers, StatusCode::OK, notice, &body, &back),
                    &cookies,
                );
            }
            // Log the new account in, with the name and password just chosen.
            let token = match site.client().token(&incoming, "login").await {
                Ok(t) => t,
                Err(e) => return pages::api_failed(&cx, headers, &e),
            };
            cookies.extend(token.set_cookies);
            match site
                .client()
                .client_login(
                    &incoming,
                    &name,
                    field("password"),
                    &token.value.value,
                    &return_url,
                    None,
                )
                .await
            {
                Ok(r) if matches!(r.value, LoginResult::Pass { .. }) => {
                    cookies.extend(r.set_cookies);
                    pages::see_other(&format!("/wiki/{}", title_url(&back)), &cookies)
                }
                Ok(r) => {
                    cookies.extend(r.set_cookies);
                    pages::see_other(
                        &format!("/wiki/Special:UserLogin?returnto={}", title_url(&back)),
                        &cookies,
                    )
                }
                Err(e) => pages::api_failed(&cx, headers, &e),
            }
        }
        CreateResult::Requested => {
            let body = format!(
                "<p>{}</p><p>{}</p>",
                esc(&cx.m.get("ts-createaccount-requested-next")),
                link(
                    &format!("/wiki/{}", title_url(&back)),
                    &esc(&cx.m.with("ts-login-continue", &[&back])),
                    ""
                )
            );
            let notice = Some((MessageKind::Success, cx.m.get("ts-createaccount-requested")));
            pages::with_cookies(
                page(&cx, headers, StatusCode::OK, notice, &body, &back),
                &cookies,
            )
        }
        CreateResult::Challenge(c) => {
            // No solution came: the browser ran no script. One came and was refused: it
            // went stale or was spent, and the script works on the new one.
            let key = if posted.as_ref().is_none_or(|(_, s)| s.is_empty()) {
                "ts-challenge-nojs"
            } else {
                "ts-challenge-retry"
            };
            let notice = (MessageKind::Error, cx.m.get(key));
            pages::with_cookies(again(&cx, Some(c), "", notice), &cookies)
        }
        CreateResult::Fail { code, message } => {
            let notice = (MessageKind::Error, refusal(&cx.m, &code, &message));
            // The challenge was not spent: the form keeps it, and its solution.
            let (challenge, solution) = posted
                .as_ref()
                .map_or((None, String::new()), |(i, s)| (challenge_of(i), s.clone()));
            pages::with_cookies(again(&cx, challenge, &solution, notice), &cookies)
        }
        CreateResult::BadToken => {
            let notice = Some((MessageKind::Warning, cx.m.get("ts-login-expired")));
            fresh(
                site,
                &mut cx,
                headers,
                &incoming,
                &back,
                &username,
                &statement,
                notice,
                StatusCode::OK,
                cookies,
            )
            .await
        }
    }
}

// ---- Special:ConfirmAccounts ---------------------------------------------------------

fn age(m: &Messages, filed: i64) -> String {
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
    .unwrap_or(i64::MAX);
    let days = (now - filed).max(0) / 86_400;
    if days == 0 {
        m.get("ts-confirmaccounts-today")
    } else {
        m.with("ts-confirmaccounts-days", &[&days.to_string()])
    }
}

fn review_page(
    cx: &Context,
    headers: &HeaderMap,
    notice: Option<(MessageKind, String)>,
    body: &str,
) -> Response {
    let mut html = String::new();
    if let Some((kind, text)) = notice {
        html.push_str(&pages::message(kind, &text));
    }
    html.push_str(body);
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-confirmaccounts-title"),
        returnto: Some(REVIEW.to_string()),
        body: format!("<div class=\"ts-confirmaccounts\">{html}</div>"),
        ..Page::default()
    })
}

/// One open request: its name, age, address where shown, statement, and the forms that
/// approve it or reject it with a reason.
fn request_card(m: &Messages, r: &triplespace_client::AccountRequest, token: &str) -> String {
    let mut body = String::new();
    let _ = write!(
        body,
        "<section class=\"ts-request\" aria-labelledby=\"req-{id}\"><h2 id=\"req-{id}\">{}</h2><p class=\"ts-request__meta\">{}{}</p>",
        esc(&r.name),
        esc(&age(m, r.filed)),
        r.address
            .as_ref()
            .map(|a| format!(" · <span class=\"ts-request__address\">{}</span>", esc(a)))
            .unwrap_or_default(),
        id = esc(&r.id),
    );
    match &r.statement {
        Some(s) => {
            let _ = write!(
                body,
                "<blockquote class=\"ts-request__statement\">{}</blockquote>",
                esc(s)
            );
        }
        None => {
            let _ = write!(
                body,
                "<p class=\"ts-request__statement--none\">{}</p>",
                esc(&m.get("ts-confirmaccounts-no-statement"))
            );
        }
    }
    let _ = write!(
        body,
        "<form class=\"ts-request__approve\" method=\"post\" action=\"/w/index.php?title={title}\"><input type=\"hidden\" name=\"token\" value=\"{token}\"><input type=\"hidden\" name=\"id\" value=\"{id}\"><input type=\"hidden\" name=\"decision\" value=\"approve\">{}</form><form class=\"ts-request__reject\" method=\"post\" action=\"/w/index.php?title={title}\"><input type=\"hidden\" name=\"token\" value=\"{token}\"><input type=\"hidden\" name=\"id\" value=\"{id}\"><input type=\"hidden\" name=\"decision\" value=\"reject\">{}{}</form></section>",
        Button {
            label: &m.with("ts-confirmaccounts-approve", &[&r.name]),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default(),
        Field {
            id: &format!("reason-{}", r.id),
            name: "reason",
            kind: "text",
            value: "",
            label: &m.get("ts-confirmaccounts-reason"),
            description: None,
            help: Some(&m.get("ts-confirmaccounts-reason-help")),
            required: true,
            autocomplete: Some("off"),
        }
        .render()
        .unwrap_or_default(),
        Button {
            label: &m.with("ts-confirmaccounts-reject", &[&r.name]),
            kind: "submit",
            action: Action::Destructive,
            weight: Weight::Normal,
        }
        .render()
        .unwrap_or_default(),
        title = title_url(REVIEW),
        token = esc(token),
        id = esc(&r.id),
    );
    body
}

/// `GET Special:ConfirmAccounts`.
pub async fn review(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match start(site, headers, &incoming, query, REVIEW).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if cx.user.is_none() {
        let body = format!(
            "<p>{}</p>",
            link(
                &format!("/wiki/Special:UserLogin?returnto={}", title_url(REVIEW)),
                &esc(&cx.m.get("ts-login")),
                ""
            )
        );
        let notice = Some((MessageKind::Notice, cx.m.get("ts-confirmaccounts-login")));
        return review_page(&cx, headers, notice, &body);
    }
    let (list, token) = tokio::join!(
        site.client().account_requests(&incoming),
        site.client().token(&incoming, "csrf")
    );
    let (list, token) = match (list, token) {
        (Ok(l), Ok(t)) => (l, t),
        (Err(e), _) | (_, Err(e)) => return pages::api_failed(&cx, headers, &e),
    };
    let m = &cx.m;
    let notice = match query.get("done").map(String::as_str) {
        Some("approved") => Some((MessageKind::Success, m.get("ts-confirmaccounts-approved"))),
        Some("rejected") => Some((MessageKind::Success, m.get("ts-confirmaccounts-rejected"))),
        _ => None,
    };
    let mut body = String::new();
    if list.is_empty() {
        let _ = write!(body, "<p>{}</p>", esc(&m.get("ts-confirmaccounts-none")));
    }
    for r in &list {
        body.push_str(&request_card(m, r, &token.value.value));
    }
    let r = review_page(&cx, headers, notice, &body);
    pages::with_cookies(r, &token.set_cookies)
}

/// `POST Special:ConfirmAccounts`: approves or rejects one request.
pub async fn post_review(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match start(site, headers, &incoming, query, REVIEW).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let approve = field("decision") == "approve";
    let result = site
        .client()
        .decide_request(
            &incoming,
            field("id"),
            approve,
            field("token"),
            field("reason").trim(),
        )
        .await;
    match result {
        Ok(Saved::Ok(_)) => pages::see_other(
            &format!(
                "/wiki/{}?done={}",
                title_url(REVIEW),
                if approve { "approved" } else { "rejected" }
            ),
            &[],
        ),
        Ok(Saved::Refused(r)) => {
            let text = match r.first().map(|x| x.code.as_str()) {
                Some("ts-reason-required") => cx.m.get("ts-confirmaccounts-reason-required"),
                Some("ts-no-request") => cx.m.get("ts-confirmaccounts-gone"),
                Some("userexists") => cx.m.get("ts-createaccount-userexists"),
                Some("badtoken") => cx.m.get("ts-login-expired"),
                _ => r.first().map(|x| x.message.clone()).unwrap_or_default(),
            };
            review_page(
                &cx,
                headers,
                Some((MessageKind::Error, text)),
                &format!(
                    "<p>{}</p>",
                    link(
                        &format!("/wiki/{}", title_url(REVIEW)),
                        &esc(&cx.m.get("ts-confirmaccounts-back")),
                        ""
                    )
                ),
            )
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_part_of_a_form() {
        let m = Messages::for_language("en");
        let c = ChallengeInfo {
            id: "1.w.createaccount.12.9.s\"x.m".into(),
            bits: 12,
        };
        let html = challenge_fields(&m, Some(&c), "", "ts-challenge-nojs");
        assert!(
            html.contains("name=\"tschallengeid\" value=\"1.w.createaccount.12.9.s&quot;x.m\"")
        );
        assert!(html.contains("data-ts-challenge-bits=\"12\""));
        assert!(html.contains("role=\"status\" aria-live=\"polite\""));
        assert!(html.contains("<noscript>"));
        assert!(challenge_fields(&m, None, "", "ts-challenge-nojs").is_empty());
        assert_eq!(
            challenge_of("1.w.badlogin.9.1.s.m").map(|c| c.bits),
            Some(9)
        );
    }

    #[test]
    fn the_form_has_its_honeypot() {
        let m = Messages::for_language("en");
        let html = create_form(
            &m,
            &FormState {
                token: "t+\\",
                back: "Main Page",
                username: "Bob",
                statement: Some(("required", "Hi")),
                challenge: None,
                solution: "",
                approval: true,
                for_another: false,
            },
        );
        assert!(html.contains(&format!("name=\"{HONEYPOT}\"")));
        assert!(html.contains("tabindex=\"-1\" autocomplete=\"off\""));
        assert!(html.contains("aria-hidden=\"true\""));
        assert!(html.contains("name=\"tsstatement\""));
        assert!(html.contains(" required>Hi</textarea>"));
        assert!(!html.contains("data-ts-challenge="));
    }
}
