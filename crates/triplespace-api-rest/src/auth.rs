//! Login through external issuers, the site's door (0088 §5–§7; 18 §3.2): the browser
//! posts an issuer's button straight here, the provider returns here, and the API
//! redirects to the page the site passed, so the provider's code never reaches the web
//! tier (0057 §7).
//!
//! - `POST /auth/{issuer}/start`: `purpose=login` with the session's `logintoken`, or
//!   `purpose=link` with its CSRF `token`, and `returnto`, a title; `303` to the
//!   provider.
//! - `GET /auth/{issuer}/callback`: the URL registered with the provider. For a flow the
//!   site began, the outcome is applied to the session it began in and the answer is a
//!   `303` to `returnto`, to `Special:CreateAccount` for an identity with no account, or
//!   to `Special:UserLogin` (`Special:Account` for `link`) with `tserror`. For a flow
//!   the Action API began, a `303` to its return URL with `state`.
//! - `GET /auth/pending`: the session's pending identity, to that session only.
//! - `GET /account`: the holder's name and sign-in methods.
//!
//! Every answer is `no-store`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use percent_encoding::utf8_percent_encode;
use serde_json::{Value, json};
use triplespace_accounts::oauth::{Door, Purpose};
use triplespace_accounts::requests::Outcome as RequestOutcome;
use triplespace_accounts::{bindings, password};
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, set_cookie};
use triplespace_api_action::external::{self, Conclusion, Scope};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_api_action::{QUERY, TITLE_SEGMENT};

use crate::entity_route::{error, internal};

/// A `303 See Other`, never stored, with a session cookie where one is set.
fn see_other(location: &str, cookie: Option<String>) -> Response {
    let mut r = StatusCode::SEE_OTHER.into_response();
    let h = r.headers_mut();
    if let Ok(v) = HeaderValue::from_str(location) {
        h.insert(header::LOCATION, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    if let Some(c) = cookie
        && let Ok(v) = HeaderValue::from_str(&c)
    {
        h.insert(header::SET_COOKIE, v);
    }
    r
}

fn enc(s: &str) -> String {
    utf8_percent_encode(s, QUERY).to_string()
}

/// A page of the site by title, as a path.
fn wiki(title: &str) -> String {
    format!(
        "/wiki/{}",
        utf8_percent_encode(&title.replace(' ', "_"), TITLE_SEGMENT)
    )
}

/// A `returnto` the site passed: a title, never a URL or a path (0057 §7).
fn title_of(returnto: Option<&str>) -> Option<String> {
    let t = returnto?.trim();
    (!t.is_empty() && !t.contains("://") && !t.starts_with('/') && !t.starts_with('\\'))
        .then(|| t.to_string())
}

/// Where a refusal sends the person: the login page, or for `link` the account page.
fn refused(purpose: Purpose, code: &str, returnto: Option<&str>) -> Response {
    let location = if purpose == Purpose::Link {
        format!(
            "{}?tserror={}#sign-in-methods",
            wiki("Special:Account"),
            enc(code)
        )
    } else {
        let mut l = format!("{}?tserror={}", wiki("Special:UserLogin"), enc(code));
        if let Some(t) = returnto {
            let _ = write!(l, "&returnto={}", enc(t));
        }
        l
    };
    see_other(&location, None)
}

async fn prelude(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
) -> Result<(Tenant, Caller), Response> {
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
    let caller = Caller::resolve(client, &tenant.slug, headers)
        .await
        .map_err(|e| internal(&e.info))?;
    Ok((tenant, caller))
}

/// `POST /auth/{issuer}/start`.
pub async fn start(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    Path(issuer): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let form: BTreeMap<String, String> = url::form_urlencoded::parse(&body).into_owned().collect();
    let purpose = match form.get("purpose").map_or("login", String::as_str) {
        "link" => Purpose::Link,
        _ => Purpose::Login,
    };
    let returnto = title_of(form.get("returnto").map(String::as_str));
    let (token_type, field) = match purpose {
        Purpose::Link => ("csrf", "token"),
        _ => ("login", "logintoken"),
    };
    let session = caller.identity.session.as_ref().map(|s| s.id.clone());
    let token_ok = match (&session, form.get(field)) {
        (Some(s), Some(t)) => app.secret().check(token_type, s, t),
        _ => false,
    };
    let Some(session) = session.filter(|_| token_ok) else {
        return refused(purpose, "ts-oauth-state", returnto.as_deref());
    };
    if purpose == Purpose::Link
        && (caller.identity.actor_key.is_none() || caller.identity.operator.is_some())
    {
        return refused(purpose, "ts-oauth-state", returnto.as_deref());
    }
    let scope = Scope {
        app: &app,
        db: &client,
        tenant: &tenant,
    };
    match external::start(
        scope,
        &session,
        &issuer,
        purpose,
        Door::Site,
        None,
        returnto.as_deref(),
    )
    .await
    {
        Ok(Ok(url)) => see_other(&url, None),
        Ok(Err(r)) => refused(purpose, r.code, returnto.as_deref()),
        Err(e) => internal(&e.info),
    }
}

/// `GET /auth/{issuer}/callback`.
pub async fn callback(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    Path(issuer): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let scope = Scope {
        app: &app,
        db: &client,
        tenant: &tenant,
    };
    let Some(state) = q.get("state") else {
        return refused(Purpose::Login, "ts-oauth-state", None);
    };
    let flow = match external::answer(
        scope,
        &issuer,
        state,
        q.get("code").map(String::as_str),
        q.get("error").map(String::as_str),
    )
    .await
    {
        Ok(Ok(f)) => f,
        Ok(Err(r)) => return refused(Purpose::Login, r.code, None),
        Err(e) => return internal(&e.info),
    };
    if flow.door == Door::Action {
        let back = flow.returnto.clone().unwrap_or_default();
        let sep = if back.contains('?') { '&' } else { '?' };
        return see_other(&format!("{back}{sep}state={}", enc(state)), None);
    }
    let address = triplespace_accounts::challenge::address_key(origin.client.or(origin.peer));
    let conclusion = external::conclude(
        scope,
        &flow,
        external::Caller {
            session: caller.identity.session.as_ref().map(|s| s.id.as_str()),
            actor_key: caller.identity.actor_key.as_deref(),
            address: &address,
            noratelimit: caller.effective.holds("noratelimit"),
        },
    )
    .await;
    let returnto = flow.returnto.as_deref();
    match conclusion {
        Ok(Conclusion::LoggedIn {
            session, reclaimed, ..
        }) => {
            let cookie = set_cookie(
                &app,
                &session.id,
                triplespace_accounts::session::LIFETIME.as_secs(),
            );
            let location = if reclaimed {
                let mut l = format!("{}?tsnotice=ts-reclaimed", wiki("Special:UserLogin"));
                if let Some(t) = returnto {
                    let _ = write!(l, "&returnto={}", enc(t));
                }
                l
            } else {
                wiki(returnto.unwrap_or("Main Page"))
            };
            see_other(&location, Some(cookie))
        }
        Ok(Conclusion::Pending { .. }) => {
            let mut l = wiki("Special:CreateAccount");
            if let Some(t) = returnto {
                let _ = write!(l, "?returnto={}", enc(t));
            }
            see_other(&l, None)
        }
        Ok(Conclusion::Requested) => refused(flow.purpose, "ts-account-requested", returnto),
        Ok(Conclusion::Linked) => see_other(
            &format!(
                "{}?tsnotice=ts-linked#sign-in-methods",
                wiki("Special:Account")
            ),
            None,
        ),
        Ok(Conclusion::Refused(r)) => refused(flow.purpose, r.code, returnto),
        Ok(Conclusion::Request(RequestOutcome::Pending)) => {
            refused(flow.purpose, "ts-account-pending", returnto)
        }
        Ok(Conclusion::Request(RequestOutcome::Rejected(_))) => {
            refused(flow.purpose, "ts-account-rejected", returnto)
        }
        Ok(Conclusion::NameTaken) => refused(flow.purpose, "userexists", returnto),
        Err(e) => internal(&e.info),
    }
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

/// `GET /auth/pending`.
pub async fn pending(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let Some(session) = caller.identity.session.as_ref() else {
        return error(
            StatusCode::NOT_FOUND,
            "ts-no-pending",
            "no pending identity",
        );
    };
    let scope = Scope {
        app: &app,
        db: &client,
        tenant: &tenant,
    };
    match external::pending(scope, &session.id).await {
        Ok(Some((_, v))) => no_store(&headers, &v),
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            "ts-no-pending",
            "no pending identity",
        ),
        Err(e) => internal(&e.info),
    }
}

fn seconds(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `GET /account`: the holder's name and sign-in methods (0088 §7; 19 §2.4).
pub async fn account(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let Some(me) = caller.identity.actor_key.clone() else {
        return error(
            StatusCode::UNAUTHORIZED,
            "notloggedin",
            "an account page belongs to an account; log in",
        );
    };
    let scope = Scope {
        app: &app,
        db: &client,
        tenant: &tenant,
    };
    let result: Result<Value, triplespace_api_action::ApiError> = async {
        let offered = external::offered(scope).await?;
        let registry = scatter_actors::issuer::IssuerRegistry::default_registry();
        let mine = bindings::of(&**client, &me).await?;
        let has_password = password::has(&**client, &me).await?;
        let rows: Vec<Value> = mine
            .iter()
            .map(|b| {
                let label = registry
                    .by_code(&b.issuer)
                    .map_or_else(|| b.issuer.clone(), external::label);
                json!({
                    "issuer": b.issuer,
                    "label": label,
                    "added": seconds(b.added),
                    "last_used": b.last_used.map(seconds),
                    "available": offered.iter().any(|i| i.code == b.issuer),
                })
            })
            .collect();
        let can_add: Vec<Value> = offered
            .iter()
            .filter(|i| !mine.iter().any(|b| b.issuer == i.code))
            .map(|i| json!({"issuer": i.code, "label": external::label(i)}))
            .collect();
        Ok(json!({
            "actor": me,
            "name": caller.identity.name,
            "password": has_password,
            "bindings": rows,
            "can_add": can_add,
            "ways": rows.len() + usize::from(has_password),
        }))
    }
    .await;
    match result {
        Ok(v) => no_store(&headers, &v),
        Err(e) => internal(&e.info),
    }
}
