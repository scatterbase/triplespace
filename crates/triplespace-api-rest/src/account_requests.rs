//! `GET /account/requests`, `POST /account/requests/{id}/approve` and
//! `POST /account/requests/{id}/reject` (0087 §6; 18 §3.2): the review of account
//! requests, with `confirmaccount`, `no-store` like everything under `/account`.
//!
//! Approving creates the account with the request's password and its binding as one
//! change set attested by the reviewer, `newusers/create2`, and deletes the request in the
//! same transaction; rejecting keeps the request a week with the reason, which its
//! requester is told at login. The address a request was filed from is shown to holders
//! of `ts-viewip` alone.

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{Map, Value, json};
use triplespace_accounts::requests;
use triplespace_api_action::App;
use triplespace_api_action::auth::Caller;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_api_ingest::accounts::{CreateError, Credential, create};

use crate::entity_route::{error, internal};
use crate::settings::check_token;

/// The tenant and a caller who holds `confirmaccount`, or the refusal.
async fn reviewer(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
) -> Result<(Tenant, Caller, String), Response> {
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
    let Some(actor) = caller.identity.actor_key.clone() else {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "notloggedin",
            "reviewing account requests needs an account; log in",
        ));
    };
    if caller.require("confirmaccount").is_err() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reviewing account requests needs `confirmaccount`",
        ));
    }
    Ok((tenant, caller, actor))
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

/// The body of a write: a JSON object, its CSRF token checked.
#[allow(clippy::result_large_err)] // a ready response, as the async routes return
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

/// `GET /account/requests`: the open requests, oldest first.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, _) = match reviewer(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let open = match requests::open(&**client, &tenant.slug).await {
        Ok(o) => o,
        Err(e) => return internal(&e),
    };
    let viewip = caller.effective.holds("ts-viewip");
    let rows: Vec<Value> = open
        .into_iter()
        .map(|r| {
            let mut v = json!({
                "id": r.id,
                "name": r.name,
                "statement": r.statement,
                "filed": r.filed,
                "expires": r.expires,
            });
            if viewip {
                v["address"] = json!(r.address_key);
            }
            v
        })
        .collect();
    no_store(&headers, &json!({"count": rows.len(), "requests": rows}))
}

/// `POST /account/requests/{id}/approve`.
pub async fn approve(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match reviewer(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Err(r) = write_body(&app, &caller, &headers, &body) {
        return r;
    }
    let store = app.store();
    let mut cx = match scatter_projection::Backend::begin(store).await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let result: Result<Option<(String, String)>, Response> = async {
        let Some((request, hash)) = requests::take(cx.conn(), &tenant.slug, &id)
            .await
            .map_err(|e| internal(&e))?
        else {
            return Ok(None);
        };
        requests::delete(cx.conn(), &tenant.slug, &id)
            .await
            .map_err(|e| internal(&e))?;
        let key = create(
            store,
            app.pipeline(),
            &mut cx,
            &tenant.slug,
            &request.name,
            Credential::Hash(&hash),
            Some(&actor),
            "account request approved",
            triplespace_api_action::edit::now_micros(),
        )
        .await
        .map_err(|e| match e {
            CreateError::Taken => error(
                StatusCode::CONFLICT,
                "userexists",
                "an account holds the name already",
            ),
            other => internal(&other),
        })?;
        Ok(Some((request.name, key)))
    }
    .await;
    match result {
        Ok(Some((name, key))) => match scatter_projection::Backend::commit(store, cx).await {
            Ok(()) => no_store(
                &headers,
                &json!({"approved": {"id": id, "name": name, "actor": key}}),
            ),
            Err(e) => internal(&e),
        },
        Ok(None) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            error(
                StatusCode::NOT_FOUND,
                "ts-no-request",
                "no open account request has that ID",
            )
        }
        Err(r) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            r
        }
    }
}

/// `POST /account/requests/{id}/reject`, with `reason`.
pub async fn reject(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match reviewer(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let object = match write_body(&app, &caller, &headers, &body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .map_or("", str::trim);
    if reason.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "ts-reason-required",
            "a rejection gives its reason, which the requester is shown",
        );
    }
    match requests::reject(&**client, &tenant.slug, &id, &actor, reason).await {
        Ok(true) => no_store(&headers, &json!({"rejected": {"id": id}})),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "ts-no-request",
            "no open account request has that ID",
        ),
        Err(e) => internal(&e),
    }
}
