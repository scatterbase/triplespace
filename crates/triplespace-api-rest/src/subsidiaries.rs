//! The operator's subsidiaries and their keys (0024 §8; 18 §3.2; 07 §4.2, §4.4, §5):
//! `GET` and `POST /account/subsidiaries`, `POST /account/subsidiaries/{name}/rename`,
//! `…/retire` and `…/reactivate`, and `GET`, `POST` and `DELETE
//! /account/subsidiaries/{name}/keys`. `Special:Account`'s Subsidiaries section is drawn
//! from these.
//!
//! Holder only: the caller is a primary account, never a subsidiary or a temporary
//! account (07 §4.1, §4.2). Creating needs `createaccount`; the other writes need only
//! that the caller operates the subsidiary (07 §4.5). Each write carries the session's
//! CSRF token as `token`, in the body or, for `DELETE`, in the query. Keys are private
//! state: the answers are never cached, and a key's secret is in the answer that issued
//! it and nowhere else (07 §5.1).

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_actors::actor::ActorKind;
use serde_json::{Map, Value, json};
use triplespace_accounts::keys;
use triplespace_api_action::App;
use triplespace_api_action::auth::Caller;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_api_ingest::accounts::CreateError;
use triplespace_api_ingest::subsidiaries::{self, Change, Subsidiary, SubsidiaryError};

use crate::entity_route::{error, internal};
use crate::settings::check_token;

/// `subsidiaries.max_per_account`'s default (23).
const DEFAULT_LIMIT: i64 = 10;

/// The caller's tenant and actor key, where the caller is a primary account.
async fn operator(
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
            "subsidiaries belong to an account; log in to manage them",
        ));
    };
    if caller.identity.kind != Some(ActorKind::Registered) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "ts-not-primary",
            "only a primary account operates subsidiaries",
        ));
    }
    Ok((tenant, caller, actor))
}

fn private_json(headers: &HeaderMap, status: StatusCode, body: &Value) -> Response {
    http_cache::respond(
        headers,
        status,
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}

/// The JSON object of a write's body, with its token checked.
#[allow(clippy::result_large_err)]
fn write_body(
    app: &App,
    caller: &Caller,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<Map<String, Value>, Response> {
    let object = if body.is_empty() {
        Map::new()
    } else {
        match serde_json::from_slice(body) {
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

/// Seconds since the epoch.
fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn key_json(k: &keys::KeyInfo) -> Value {
    json!({
        "key_id": k.key_id,
        "label": k.label,
        "grants": k.grants,
        "created": secs(k.created),
        "expires": k.expires.map(secs),
        "last_used": k.last_used.map(secs),
        "revoked": k.revoked_at.map(secs),
    })
}

fn subsidiary_json(s: &Subsidiary, keys: Option<&[keys::KeyInfo]>) -> Value {
    let mut v = json!({
        "name": s.name,
        "actor": s.actor_key,
        "status": s.status,
        "groups": s.groups,
    });
    if let Some(keys) = keys {
        v["keys"] = keys.iter().map(key_json).collect();
    }
    v
}

/// `subsidiaries.max_per_account` for the tenant.
async fn limit(client: &tokio_postgres::Client, tenant: &Tenant) -> i64 {
    triplespace_accounts::settings::load(client, Some(&tenant.slug))
        .await
        .ok()
        .and_then(|(s, _)| s.value("subsidiaries.max_per_account"))
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LIMIT)
}

/// A refusal of the writer, as the routes answer it.
fn refused(e: SubsidiaryError) -> Response {
    match e {
        SubsidiaryError::Limit(_) => {
            error(StatusCode::CONFLICT, "ts-subsidiary-limit", e.to_string())
        }
        SubsidiaryError::NotFound => {
            error(StatusCode::NOT_FOUND, "ts-no-subsidiary", e.to_string())
        }
        SubsidiaryError::Retired => {
            error(StatusCode::CONFLICT, "ts-subsidiary-retired", e.to_string())
        }
        SubsidiaryError::NotRetired => {
            error(StatusCode::CONFLICT, "ts-subsidiary-active", e.to_string())
        }
        SubsidiaryError::Create(c) => match c {
            CreateError::InvalidName(_) => {
                error(StatusCode::BAD_REQUEST, "invaliduser", c.to_string())
            }
            CreateError::Taken => error(StatusCode::CONFLICT, "userexists", c.to_string()),
            CreateError::Reserved(_) | CreateError::Tombstoned => {
                error(StatusCode::CONFLICT, "ts-name-reserved", c.to_string())
            }
            other => internal(&other),
        },
    }
}

/// `GET /account/subsidiaries`: the caller's subsidiaries, each with its keys; whether
/// and how many more they may create; and the grants a key may carry.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match operator(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let subs = match subsidiaries::list(&**client, &tenant.slug, &actor).await {
        Ok(s) => s,
        Err(e) => return internal(&e),
    };
    let mut out = Vec::with_capacity(subs.len());
    for s in &subs {
        let k = match keys::list(&**client, &s.actor_key).await {
            Ok(k) => k,
            Err(e) => return internal(&e),
        };
        out.push(subsidiary_json(s, Some(&k)));
    }
    let max = limit(&client, &tenant).await;
    let held = subs.iter().filter(|s| s.status != "retired").count();
    let name = caller.identity.name.clone().unwrap_or_default();
    // The grants a key may carry, for the issue form; `basic` is always included.
    let grants: Vec<Value> = scatter_actors::grant::GrantRegistry::default_registry()
        .grants()
        .iter()
        .filter(|g| g.name != scatter_actors::grant::BASIC)
        .map(|g| json!({"name": g.name, "description": g.description}))
        .collect();
    private_json(
        &headers,
        StatusCode::OK,
        &json!({
            "subsidiaries": out,
            "can_create": caller.require("createaccount").is_ok(),
            "max": max,
            "held": held,
            "suggested": format!("{name}Bot"),
            "grants": grants,
        }),
    )
}

/// `POST /account/subsidiaries` with `name`: creates a subsidiary operated by the caller.
pub async fn create(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match operator(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let object = match write_body(&app, &caller, &headers, &body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    if caller.require("createaccount").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "creating a subsidiary needs `createaccount`",
        );
    }
    let Some(name) = object.get("name").and_then(Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "badbody", "`name` is required");
    };
    let max = limit(&client, &tenant).await;
    let store = app.accounts_store();
    let mut cx = match scatter_projection::Backend::begin(store).await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let made = subsidiaries::create(
        store,
        app.pipeline(),
        &mut cx,
        app.secret(),
        &tenant.slug,
        name,
        &actor,
        max,
        triplespace_api_action::edit::now_micros(),
    )
    .await;
    match made {
        Ok(key) => match scatter_projection::Backend::commit(store, cx).await {
            Ok(()) => {
                let name = scatter_actors::actor::normalize_name(name);
                private_json(
                    &headers,
                    StatusCode::CREATED,
                    &json!({"created": {"name": name, "actor": key}}),
                )
            }
            Err(e) => internal(&e),
        },
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            refused(e)
        }
    }
}

/// One of `…/rename` (with `to`), `…/retire` and `…/reactivate`.
async fn change(
    app: &App,
    origin: &Origin,
    headers: &HeaderMap,
    name: &str,
    body: &Bytes,
    what: &str,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match operator(app, &client, origin, headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let object = match write_body(app, &caller, headers, body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let to = object.get("to").and_then(Value::as_str).unwrap_or_default();
    let change = match what {
        "rename" if to.is_empty() => {
            return error(StatusCode::BAD_REQUEST, "badbody", "`to` is required");
        }
        "rename" => Change::Rename(to),
        "retire" => Change::Retire,
        _ => Change::Reactivate,
    };
    let store = app.accounts_store();
    let mut cx = match scatter_projection::Backend::begin(store).await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let done = subsidiaries::change(
        store,
        app.pipeline(),
        &mut cx,
        app.secret(),
        &tenant.slug,
        &actor,
        name,
        change,
        triplespace_api_action::edit::now_micros(),
    )
    .await;
    match done {
        Ok(s) => match scatter_projection::Backend::commit(store, cx).await {
            Ok(()) => private_json(
                headers,
                StatusCode::OK,
                &json!({"subsidiary": subsidiary_json(&s, None)}),
            ),
            Err(e) => internal(&e),
        },
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            refused(e)
        }
    }
}

/// `POST /account/subsidiaries/{name}/rename` with `to`.
pub async fn rename(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    change(&app, &origin, &headers, &name, &body, "rename").await
}

/// `POST /account/subsidiaries/{name}/retire`: revokes every key and retires it.
pub async fn retire(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    change(&app, &origin, &headers, &name, &body, "retire").await
}

/// `POST /account/subsidiaries/{name}/reactivate`.
pub async fn reactivate(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    change(&app, &origin, &headers, &name, &body, "reactivate").await
}

/// The caller's subsidiary `name`, for the key routes.
async fn owned(
    client: &tokio_postgres::Client,
    tenant: &Tenant,
    actor: &str,
    name: &str,
) -> Result<Subsidiary, Response> {
    subsidiaries::find(client, &tenant.slug, actor, name)
        .await
        .map_err(refused)
}

/// `GET /account/subsidiaries/{name}/keys`.
pub async fn keys_list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, _caller, actor) = match operator(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let sub = match owned(&client, &tenant, &actor, &name).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    match keys::list(&**client, &sub.actor_key).await {
        Ok(k) => private_json(
            &headers,
            StatusCode::OK,
            &json!({"keys": k.iter().map(key_json).collect::<Vec<_>>()}),
        ),
        Err(e) => internal(&e),
    }
}

/// `POST /account/subsidiaries/{name}/keys` with `label`, `grants` and optionally
/// `expires_days`: issues a key. The answer holds the secret, once.
pub async fn keys_issue(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match operator(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let object = match write_body(&app, &caller, &headers, &body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let sub = match owned(&client, &tenant, &actor, &name).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if sub.status == "retired" {
        return refused(SubsidiaryError::Retired);
    }
    let label = object
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let grants: Vec<String> = object
        .get("grants")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let expires = match object.get("expires_days") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64().filter(|d| (1..=3650).contains(d)) {
            Some(d) => Some(SystemTime::now() + Duration::from_secs(d * 86_400)),
            None => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "badbody",
                    "`expires_days` is a whole number of days from 1 to 3650",
                );
            }
        },
    };
    match keys::issue(&**client, &sub.actor_key, label, &grants, expires).await {
        Ok(issued) => {
            let secret = issued
                .bearer
                .split_once('.')
                .map(|(_, s)| s.to_string())
                .unwrap_or_default();
            private_json(
                &headers,
                StatusCode::CREATED,
                &json!({"issued": {
                    "key_id": issued.key_id,
                    "label": label,
                    "login": format!("{}@{label}", sub.name),
                    "secret": secret,
                    "bearer": issued.bearer,
                }}),
            )
        }
        Err(triplespace_accounts::AccountsError::Invalid(m)) => {
            error(StatusCode::BAD_REQUEST, "ts-bad-key", m)
        }
        Err(e) => internal(&e),
    }
}

/// `DELETE /account/subsidiaries/{name}/keys?key_id=…&token=…`: revokes a key and every
/// session opened with it.
pub async fn keys_revoke(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match operator(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    // A DELETE has no body to carry the token, so a session's token comes as `token=`.
    let token: Map<String, Value> = q
        .get("token")
        .map(|t| Map::from_iter([("token".to_string(), Value::String(t.clone()))]))
        .unwrap_or_default();
    if let Err(r) = check_token(&app, &caller, &headers, &token) {
        return r;
    }
    let sub = match owned(&client, &tenant, &actor, &name).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(key_id) = q.get("key_id") else {
        return error(StatusCode::BAD_REQUEST, "badbody", "`key_id` is required");
    };
    match keys::revoke(&**client, &sub.actor_key, key_id).await {
        Ok(true) => private_json(
            &headers,
            StatusCode::OK,
            &json!({"revoked": {"key_id": key_id}}),
        ),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "ts-no-key",
            "the subsidiary has no unrevoked key with that ID",
        ),
        Err(e) => internal(&e),
    }
}
