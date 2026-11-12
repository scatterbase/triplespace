//! A login's email addresses (0100 §1–§4, §7; 07 §2.9; 18 §3.2), holder only, `no-store`
//! like everything under `/auth`:
//!
//! - `GET /auth/email`: the addresses, each with `verified`, `primary`, `added`, for an
//!   unverified one `code_expires`, and `undeliverable` after a bounce or complaint;
//!   whether the instance `sends_mail`; and why the primary cannot be removed now
//!   (`primary_removal`), for the page's disabled button.
//! - `POST /auth/email` with `address`: adds it and sends a code, or sends a new code to
//!   one the login holds unverified; `ts-email-invalid`, `ts-email-limit`,
//!   `ts-email-disabled`, `ratelimited` with `wait`.
//! - `POST /auth/email/verify` with `address` and `code`: `ts-email-code` with `left`,
//!   `ts-email-taken`.
//! - `POST /auth/email/primary` and `POST /auth/email/remove` with `address`:
//!   `ts-email-unverified`; `ts-email-primary`, `ts-email-last`, `ts-email-required`.
//!
//! Every write takes the CSRF token as `token` in a JSON body. Verifying, making primary
//! and removing stage the security notices to every address verified at that moment, the
//! removed one included. Messages are written in the language of the request, from the
//! wiki's interface messages, and name the instance, never the login name.
//!
//! **The login.** An address belongs to the instance login the session holds (0089 §2;
//! 0100 A3), stored by its ID; the routes are served on the tenant's host as `/account`
//! is, since a single-tenant instance's tenant base is its auth base (0089 §12).
//!
//! The five routes are what implements 0100 §7 (A2); the security notices they stage,
//! with `triplespace_notify::mail::texts`, are what implements 0100 §4.
//! `email.required` is read for the removal rule only: the creation-time
//! flow of 0100 §1, where `action=createaccount` asks for a code, is not built yet.
//!
//! `POST /mail/postmark` is Postmark's bounce and spam-complaint webhook (0100 A2):
//! authenticated by the HTTP Basic password `mail.postmark.webhook_secret_file` holds, it
//! marks the address undeliverable after a hard bounce or a complaint, on every login that
//! holds it verified, and answers `200` to everything it understood.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use base64::Engine as _;
use serde_json::{Map, Value, json};
use triplespace_accounts::email::{self, Added, EmailError, RemovalRules, Undeliverable};
use triplespace_accounts::{bindings, password};
use triplespace_api_action::App;
use triplespace_api_action::auth::Caller;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::interface::Texts;
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_notify::mail::{self, Notice, texts};

use crate::entity_route::{error, internal};
use crate::settings::check_token;

/// Everything a request on the addresses needs.
struct Scope {
    tenant: Tenant,
    caller: Caller,
    /// The login's ID, as `private.email.login` stores it.
    login: String,
    /// The login's ID.
    login_id: i64,
}

async fn holder(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
) -> Result<Scope, Response> {
    let tenant = match tenant::resolve(app, client, origin, headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(error(
                StatusCode::MISDIRECTED_REQUEST,
                "misdirected",
                "not a tenant of this instance",
            ));
        }
        Err(e) => return Err(internal(&e)),
    };
    let caller = Caller::resolve(app.accounts_pool(), &tenant.slug, headers)
        .await
        .map_err(|e| internal(&e.info))?;
    if caller.identity.actor_key.is_none() {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "notloggedin",
            "email addresses belong to a login; log in",
        ));
    }
    let Some(login_id) = caller
        .identity
        .login
        .filter(|_| caller.identity.session.is_some())
    else {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "email addresses are managed in a session of the login, never by key",
        ));
    };
    Ok(Scope {
        tenant,
        caller,
        login: login_id.to_string(),
        login_id,
    })
}

fn no_store(headers: &HeaderMap, body: &Value) -> Response {
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}

fn seconds(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The body of a write: a JSON object, its CSRF token checked.
#[allow(clippy::result_large_err)]
fn write_body(
    app: &App,
    caller: &Caller,
    headers: &HeaderMap,
    bytes: &Bytes,
) -> Result<Map<String, Value>, Response> {
    let object: Map<String, Value> = if bytes.is_empty() {
        Map::new()
    } else {
        match serde_json::from_slice(bytes) {
            Ok(Value::Object(o)) => o,
            _ => {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "badbody",
                    "the body is a JSON object",
                ));
            }
        }
    };
    check_token(app, caller, headers, &object)?;
    Ok(object)
}

/// A refusal as the API answers it.
fn refused(e: &EmailError) -> Response {
    let (status, message) = match e {
        EmailError::Invalid => (
            StatusCode::BAD_REQUEST,
            "not an email address: one local part, an @, and a domain with a dot".to_string(),
        ),
        EmailError::Limit => (
            StatusCode::BAD_REQUEST,
            "the login holds as many addresses as the instance allows".into(),
        ),
        EmailError::Code { left } => (
            StatusCode::BAD_REQUEST,
            if *left > 0 {
                format!("wrong code; {left} entries left")
            } else {
                "the code is wrong, expired or spent; send a new one".into()
            },
        ),
        EmailError::Unverified => (
            StatusCode::BAD_REQUEST,
            "an unverified address cannot be the primary".into(),
        ),
        EmailError::Taken => (
            StatusCode::CONFLICT,
            "another login verified this address first".into(),
        ),
        EmailError::Primary => (
            StatusCode::BAD_REQUEST,
            "make another address primary before removing this one".into(),
        ),
        EmailError::Last => (
            StatusCode::BAD_REQUEST,
            "the last verified address can be removed only with a password set and no external sign-in method".into(),
        ),
        EmailError::Required => (
            StatusCode::BAD_REQUEST,
            "this instance requires a verified email address".into(),
        ),
        EmailError::Disabled => (
            StatusCode::BAD_REQUEST,
            "this instance does not send email".into(),
        ),
        EmailError::RateLimited { wait } => (
            StatusCode::TOO_MANY_REQUESTS,
            format!("too many codes; wait {wait} seconds"),
        ),
        EmailError::Unknown => (StatusCode::NOT_FOUND, "the login holds no such address".into()),
        EmailError::Accounts(e) => return internal(e),
        _ => (StatusCode::BAD_REQUEST, e.to_string()),
    };
    let mut body = json!({"code": e.code(), "message": message});
    match e {
        EmailError::Code { left } => body["left"] = json!(left),
        EmailError::RateLimited { wait } => body["wait"] = json!(wait),
        _ => {}
    }
    let mut r = error(status, e.code(), message);
    *r.body_mut() = axum::body::Body::from(body.to_string());
    r
}

/// The instance's name, for messages: the primary tenant's `site.name`, or the farm
/// slug.
async fn instance_name(app: &App, db: &tokio_postgres::Client, tenant: &Tenant) -> String {
    let primary = triplespace_projections::tenants::primary(db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| tenant.slug.clone());
    triplespace_accounts::settings::load(db, Some(&primary))
        .await
        .ok()
        .and_then(|(snap, _)| snap.value("site.name"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| app.config().farm.slug.clone())
}

/// Where the login is reviewed: the auth host's `Special:Account`.
fn account_url(app: &App, tenant: &Tenant) -> String {
    let base = app
        .config()
        .auth_base
        .clone()
        .unwrap_or_else(|| tenant.public_base.clone());
    format!("{}/wiki/Special:Account", base.trim_end_matches('/'))
}

/// The texts of the request's language: the preference negotiated from
/// `Accept-Language`, the wiki's Interface pages over the shipped messages.
async fn texts_for(
    db: &tokio_postgres::Client,
    tenant: &Tenant,
    login: &str,
    headers: &HeaderMap,
) -> Texts {
    let accept = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok());
    let site_lang = triplespace_accounts::settings::load(db, Some(&tenant.slug))
        .await
        .ok()
        .and_then(|(snap, _)| snap.value("site.language"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "en".into());
    let lang = triplespace_api_action::prefs::resolve(db, &tenant.slug, Some(login), None, accept)
        .await
        .ok()
        .and_then(|r| {
            r.values
                .get("language")
                .and_then(|v| v.value.as_ref().and_then(Value::as_str).map(str::to_owned))
        })
        .unwrap_or_else(|| site_lang.clone());
    Texts::load(db, &tenant.slug, &lang, &site_lang)
        .await
        .unwrap_or_else(|_| Texts::shipped(&lang))
}

/// The instance settings an address operation reads.
struct Policy {
    max_addresses: i64,
    required: bool,
}

async fn policy(db: &tokio_postgres::Client, tenant: &Tenant) -> Policy {
    let snap = triplespace_accounts::settings::load(db, Some(&tenant.slug))
        .await
        .ok()
        .map(|(s, _)| s);
    Policy {
        max_addresses: snap
            .as_ref()
            .and_then(|s| s.value("email.max_addresses"))
            .and_then(|v| v.as_i64())
            .unwrap_or(3),
        required: snap
            .as_ref()
            .and_then(|s| s.value("email.required"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

async fn removal_rules(
    db: &tokio_postgres::Client,
    login: i64,
    required: bool,
) -> Result<RemovalRules, Response> {
    Ok(RemovalRules {
        required,
        has_password: password::has(db, login).await.map_err(|e| internal(&e))?,
        has_external: !bindings::of_login(db, login)
            .await
            .map_err(|e| internal(&e))?
            .is_empty(),
    })
}

fn address_json(a: &email::Address) -> Value {
    let mut v = json!({
        "address": a.address,
        "verified": a.is_verified(),
        "primary": a.primary,
        "added": seconds(a.added),
    });
    if let Some(t) = a.code_expires {
        v["code_expires"] = json!(seconds(t));
    }
    if let Some((reason, at)) = &a.undeliverable {
        v["undeliverable"] = json!(reason.as_str());
        v["undeliverable_at"] = json!(seconds(*at));
    }
    v
}

/// `GET /auth/email`.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let s = match holder(&app, &client, &origin, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let addresses = match email::list(&**client, &s.login).await {
        Ok(a) => a,
        Err(e) => return refused(&e),
    };
    let p = policy(&client, &s.tenant).await;
    let rules = match removal_rules(&client, s.login_id, p.required).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let blocker = addresses
        .iter()
        .any(|a| a.primary)
        .then(|| email::primary_removal_blocker(&addresses, rules))
        .flatten()
        .map(|e| e.code());
    no_store(
        &headers,
        &json!({
            "addresses": addresses.iter().map(address_json).collect::<Vec<_>>(),
            "sends_mail": app.mail().sends(),
            "max_addresses": p.max_addresses,
            "required": p.required,
            "primary_removal": blocker,
        }),
    )
}

/// The common start of a write: the holder, the body, and the address it names.
async fn write(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<(Scope, Map<String, Value>, String), Response> {
    let s = holder(app, client, origin, headers).await?;
    let object = write_body(app, &s.caller, headers, body)?;
    let address = object
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((s, object, address))
}

/// Stages the security notices (0100 §4) to every address in `to`.
async fn notify_all(
    app: &App,
    db: &tokio_postgres::Client,
    tenant: &Tenant,
    t: &Texts,
    to: &[String],
    what: Notice,
    address: &str,
) -> Result<(), Response> {
    let m = app.mail();
    let Some(from) = m.from.clone().filter(|_| m.sends()) else {
        return Ok(());
    };
    let instance = instance_name(app, db, tenant).await;
    let url = account_url(app, tenant);
    let now = SystemTime::now();
    for recipient in to {
        let o = texts::notice(t, &instance, &url, recipient, what, address, now);
        mail::stage(db, &from, &o).await.map_err(|e| internal(&e))?;
    }
    Ok(())
}

/// `POST /auth/email`.
pub async fn add(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (scope, _, address) = match write(&app, &client, &origin, &headers, &body).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let mail = app.mail();
    let Some(from) = mail.from.clone().filter(|_| mail.sends()) else {
        return refused(&EmailError::Disabled);
    };
    let p = policy(&client, &scope.tenant).await;
    let (addr, added) = match email::add(
        &**client,
        app.secret(),
        &scope.login,
        &address,
        p.max_addresses,
    )
    .await
    {
        Ok(x) => x,
        Err(e) => return refused(&e),
    };
    let texts = texts_for(&client, &scope.tenant, &scope.login, &headers).await;
    let instance = instance_name(&app, &client, &scope.tenant).await;
    let url = account_url(&app, &scope.tenant);
    let sent = match &added {
        Added::Code { code, expires } => {
            let outgoing = texts::verification(
                &texts,
                &instance,
                &url,
                &addr.address,
                &email::display_code(code),
                *expires,
            );
            if let Err(e) = mail::stage(&**client, &from, &outgoing).await {
                return internal(&e);
            }
            true
        }
        Added::HeldElsewhere => {
            let outgoing = texts::held_elsewhere(&texts, &instance, &url, &addr.address);
            if let Err(e) = mail::stage(&**client, &from, &outgoing).await {
                return internal(&e);
            }
            true
        }
        Added::AlreadyVerified => false,
    };
    let mut v = address_json(&addr);
    v["sent"] = json!(sent);
    no_store(&headers, &v)
}

/// `POST /auth/email/verify`.
pub async fn verify(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (s, object, address) = match write(&app, &client, &origin, &headers, &body).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let code = object
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (a, newly) = match email::verify(&**client, app.secret(), &s.login, &address, code).await {
        Ok(x) => x,
        Err(e) => return refused(&e),
    };
    if newly
        || a.verified
            .is_some_and(|t| seconds(t) + 2 >= seconds(SystemTime::now()))
    {
        let t = texts_for(&client, &s.tenant, &s.login, &headers).await;
        let to = match email::verified(&**client, &s.login).await {
            Ok(v) => v,
            Err(e) => return refused(&e),
        };
        if let Err(r) = notify_all(
            &app,
            &client,
            &s.tenant,
            &t,
            &to,
            Notice::Verified,
            &a.address,
        )
        .await
        {
            return r;
        }
    }
    no_store(&headers, &address_json(&a))
}

/// `POST /auth/email/primary`.
pub async fn primary(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (s, _, address) = match write(&app, &client, &origin, &headers, &body).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let a = match email::make_primary(&**client, &s.login, &address).await {
        Ok(a) => a,
        Err(e) => return refused(&e),
    };
    let t = texts_for(&client, &s.tenant, &s.login, &headers).await;
    let to = match email::verified(&**client, &s.login).await {
        Ok(v) => v,
        Err(e) => return refused(&e),
    };
    if let Err(r) = notify_all(
        &app,
        &client,
        &s.tenant,
        &t,
        &to,
        Notice::MadePrimary,
        &a.address,
    )
    .await
    {
        return r;
    }
    no_store(&headers, &address_json(&a))
}

/// `POST /auth/email/remove`.
pub async fn remove(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (s, _, address) = match write(&app, &client, &origin, &headers, &body).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let p = policy(&client, &s.tenant).await;
    let rules = match removal_rules(&client, s.login_id, p.required).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    // Who hears of it: every address verified at this moment, the removed one included.
    let to = match email::verified(&**client, &s.login).await {
        Ok(v) => v,
        Err(e) => return refused(&e),
    };
    let a = match email::remove(&**client, &s.login, &address, rules).await {
        Ok(a) => a,
        Err(e) => return refused(&e),
    };
    let t = texts_for(&client, &s.tenant, &s.login, &headers).await;
    if let Err(r) = notify_all(
        &app,
        &client,
        &s.tenant,
        &t,
        &to,
        Notice::Removed,
        &a.address,
    )
    .await
    {
        return r;
    }
    no_store(&headers, &json!({"address": a.address, "removed": true}))
}

/// Whether the webhook's HTTP Basic password is the secret, compared in constant time.
fn webhook_authorized(headers: &HeaderMap, secret: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(encoded) = value.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
        return false;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return false;
    };
    let presented = text.split_once(':').map_or(text.as_str(), |(_, p)| p);
    presented.len() == secret.len()
        && presented
            .bytes()
            .zip(secret.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// `POST /mail/postmark` (0100 A2).
pub async fn postmark_webhook(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(secret) = app.mail().postmark_webhook_secret else {
        return error(
            StatusCode::NOT_FOUND,
            "ts-mail-no-webhook",
            "this instance has no Postmark webhook secret",
        );
    };
    if !webhook_authorized(&headers, &secret) {
        return error(
            StatusCode::UNAUTHORIZED,
            "badauth",
            "the webhook password is wrong",
        );
    }
    let event: scatter_mail::postmark::Event = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(e) => {
            return error(
                StatusCode::BAD_REQUEST,
                "badbody",
                format!("not a Postmark event: {e}"),
            );
        }
    };
    let reason = match event.verdict() {
        scatter_mail::postmark::Verdict::Bounced => Some(Undeliverable::Bounce),
        scatter_mail::postmark::Verdict::Complained => Some(Undeliverable::Complaint),
        scatter_mail::postmark::Verdict::Nothing => None,
    };
    let mut marked = 0;
    if let Some(reason) = reason {
        let client = match app.accounts_pool().get().await {
            Ok(c) => c,
            Err(e) => return internal(&e),
        };
        match email::mark_undeliverable(&**client, &event.email, reason).await {
            Ok(logins) => marked = logins.len(),
            Err(EmailError::Invalid) => {}
            Err(e) => return refused(&e),
        }
        eprintln!(
            "mail: postmark {} for {}: {} login(s) marked undeliverable",
            event.kind, event.email, marked
        );
    }
    no_store(
        &headers,
        &json!({"received": event.record_type, "marked": marked}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_basic_auth() {
        let mut h = HeaderMap::new();
        assert!(!webhook_authorized(&h, "s3cret"));
        let enc = base64::engine::general_purpose::STANDARD.encode("postmark:s3cret");
        h.insert(
            header::AUTHORIZATION,
            format!("Basic {enc}").parse().unwrap(),
        );
        assert!(webhook_authorized(&h, "s3cret"));
        assert!(!webhook_authorized(&h, "other"));
        let enc = base64::engine::general_purpose::STANDARD.encode("s3cret");
        h.insert(
            header::AUTHORIZATION,
            format!("Basic {enc}").parse().unwrap(),
        );
        assert!(webhook_authorized(&h, "s3cret"));
        h.insert(header::AUTHORIZATION, "Bearer s3cret".parse().unwrap());
        assert!(!webhook_authorized(&h, "s3cret"));
    }
}
