//! A tenant owner's invitation, on the tenant it is for (0111 §5, A2; 08 §1.4):
//!
//! - `GET /auth/invitation?code=…` says what an open invitation is for: the wiki's name, the
//!   owner account's name, and when it expires; `404 ts-invitation-invalid` for a code that
//!   is unknown, used or expired. It reveals nothing to anyone without the code, which is
//!   32 random bytes.
//! - `POST /auth/invitation` with `{code, password}` redeems it: a login is created for the
//!   owner account, named after it, with that password, and the account is bound to it; the
//!   invitation is spent. Refused with `ts-external-required` where the instance creates
//!   logins only from external identities (0101 §1), `passwordtooshort` under eight
//!   characters, and `ts-invitation-invalid`. The site then logs the person in with the
//!   owner's name and the password, as `Special:CreateAccount` does after a creation.
//!
//! Signing in with a login the person already has is not offered: that waits for the auth
//! host (0089 §7); a request names such a login instead (0111 A2). With the provisioner's
//! binding, this implements 0111 §5 (A2).

use axum::body::Bytes;
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use scatter_projection::Backend as _;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::tenant::Tenant;

use crate::entity_route::{error, internal};
use crate::settings::prelude;

fn json_response(status: StatusCode, body: &Value) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        body.to_string(),
    )
        .into_response()
}

fn invalid() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "ts-invitation-invalid",
        "this invitation is unknown, used or expired",
    )
}

async fn owner_name<C: tokio_postgres::GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
) -> Result<Option<String>, tokio_postgres::Error> {
    Ok(client
        .query_opt(
            "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2",
            &[&tenant, &actor_key],
        )
        .await?
        .map(|r| r.get(0)))
}

async fn site_name<C: tokio_postgres::GenericClient>(client: &C, tenant: &Tenant) -> String {
    triplespace_accounts::settings::load(client, Some(&tenant.slug))
        .await
        .ok()
        .and_then(|(s, _)| s.value("site.name"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| tenant.slug.clone())
}

/// `GET /auth/invitation`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, _) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Some(code) = query.get("code") else {
        return invalid();
    };
    let accounts = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let open = match triplespace_accounts::invitations::open(&**accounts, &tenant.slug, code, false)
        .await
    {
        Ok(Some(o)) => o,
        Ok(None) => return invalid(),
        Err(e) => return internal(&e),
    };
    let name = match owner_name(&**client, &tenant.slug, &open.actor_key).await {
        Ok(n) => n,
        Err(e) => return internal(&e),
    };
    json_response(
        StatusCode::OK,
        &json!({"invitation": {
            "wiki": site_name(&**client, &tenant).await,
            "owner": name,
            "expires": open.expires,
        }}),
    )
}

/// `POST /auth/invitation`.
pub async fn post(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, _) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "badbody",
            "the body is a JSON object",
        );
    };
    let code = object
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let password = object
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let store = app.accounts_store();
    let mut cx = match store.begin().await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let result = redeem(&app, &tenant, &mut cx, code, password).await;
    match result {
        Ok(Ok(name)) => {
            if let Err(e) = store.commit(cx).await {
                return internal(&e);
            }
            json_response(
                StatusCode::OK,
                &json!({"invitation": {"result": "redeemed", "username": name}}),
            )
        }
        Ok(Err(refusal)) => {
            let _ = store.rollback(cx).await;
            refusal
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            internal(&e)
        }
    }
}

/// Redeems in the caller's unit of work: the owner's name, or a refusal.
async fn redeem(
    app: &App,
    tenant: &Tenant,
    cx: &mut triplespace_projections::PgCx,
    code: &str,
    password: &str,
) -> Result<Result<String, Response>, String> {
    use triplespace_accounts::{invitations, logins, password as pw};
    let Some(open) = invitations::open(cx.conn(), &tenant.slug, code, true)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(Err(invalid()));
    };
    if logins::external_required(cx.conn())
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(Err(error(
            StatusCode::FORBIDDEN,
            "ts-external-required",
            triplespace_api_action::external::message("ts-external-required"),
        )));
    }
    if password.chars().count() < 8 {
        return Ok(Err(error(
            StatusCode::BAD_REQUEST,
            "passwordtooshort",
            "Passwords must be at least 8 characters.",
        )));
    }
    if logins::login_of(cx.conn(), &tenant.slug, &open.actor_key)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(Err(invalid()));
    }
    let Some(name) = owner_name(cx.conn(), &tenant.slug, &open.actor_key)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(Err(invalid()));
    };
    let operator = app.config().farm.operator().to_string();
    let now = triplespace_api_action::edit::now_micros();
    let login = triplespace_api_ingest::accounts::create_login(
        app.accounts_store(),
        app.pipeline(),
        cx,
        &tenant.slug,
        &name,
        &operator,
        now,
    )
    .await
    .map_err(|e| e.to_string())?;
    triplespace_api_ingest::accounts::bind_account(
        app.accounts_store(),
        app.pipeline(),
        cx,
        &tenant.slug,
        &open.actor_key,
        login,
        &operator,
        now,
    )
    .await
    .map_err(|e| e.to_string())?;
    pw::set(cx.conn(), login, password)
        .await
        .map_err(|e| e.to_string())?;
    invitations::spend(cx.conn(), &tenant.slug, &open.actor_key)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Ok(name))
}
