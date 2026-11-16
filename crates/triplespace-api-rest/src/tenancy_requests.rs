//! Tenant requests at the farm base (0111 §7; 08 §1.4; 18 §3.2), for holders of
//! `ts-createtenant`, an instance right:
//!
//! - `POST /tenancy/requests` with `{slug, base, name?, owner_name, owner_login_source?,
//!   reason?, token}` files a request as the caller, announces it with
//!   `NOTIFY ts_tenant_request`, and answers `202` with it, `pending`. Only the shape is
//!   checked here: whether the slug is free, the base a host of its own and the owner login
//!   source a login's are the provisioner's checks, and a request failing them is filed and
//!   refused, with the reason, rather than turned away (0111 §4).
//! - `GET /tenancy/requests` lists every request, newest first, with its state: `pending`,
//!   `created`, or `refused` with the refusal's code and comment; `GET /tenancy/requests/{id}`
//!   reads one, by its offset in the instance `log`.
//!
//! A request holds what the instance `log` does not serve, the owner and the owner login
//! source (0111 A1), so reading is for holders of the right too, and nothing is cached.
//! With `Special:Tenants` in `triplespace-ui`, this implements 0111 §7 (A2).

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use scatter_actors::evaluate::instance_rights;
use scatter_projection::Backend as _;
use serde_json::{Map, Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::Caller;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::tenant::{Tenant, at_farm};
use triplespace_api_ingest::tenant_requests::{self, Ask, CHANNEL, RIGHT};

use crate::entity_route::{error, internal};
use crate::settings::{check_token, prelude};

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

/// The farm base and the right, or the refusal.
#[allow(clippy::result_large_err)] // a ready response, as the async routes return
fn entitled(app: &App, tenant: &Tenant, caller: &Caller) -> Result<String, Response> {
    if !at_farm(app, tenant) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "ts-instance-policy",
            "tenant requests are the instance's, served only at the farm base",
        ));
    }
    let Some(actor) = caller.identity.actor_key.clone() else {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "tenant requests need an account on the primary tenant",
        ));
    };
    if !instance_rights(Some(&caller.effective), None).contains(RIGHT) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "tenant requests need `ts-createtenant`, an instance right",
        ));
    }
    Ok(actor)
}

fn text(o: &Map<String, Value>, k: &str) -> Option<String> {
    o.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// `POST /tenancy/requests`.
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
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let actor = match entitled(&app, &tenant, &caller) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "badbody",
            "the body is a JSON object",
        );
    };
    if let Err(r) = check_token(&app, &caller, &headers, &object) {
        return r;
    }
    let ask = Ask {
        slug: text(&object, "slug").unwrap_or_default(),
        base: text(&object, "base").unwrap_or_default(),
        name: text(&object, "name"),
        owner_name: text(&object, "owner_name").unwrap_or_default(),
        owner_login_source: text(&object, "owner_login_source"),
        adopt: None,
        owner_id: None,
    };
    if let Err(why) = ask.check_shape() {
        return error(StatusCode::BAD_REQUEST, "ts-tenant-request-invalid", why);
    }
    let reason = text(&object, "reason");
    let store = app.store();
    let now = triplespace_api_action::edit::now_micros();
    let mut cx = match store.begin().await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let filed = match tenant_requests::file(
        store,
        app.pipeline(),
        &mut cx,
        &ask,
        &actor,
        reason.as_deref(),
        now,
    )
    .await
    {
        Ok(f) => f,
        Err(e) => {
            let _ = store.rollback(cx).await;
            return internal(&e);
        }
    };
    if let Err(e) = store.commit(cx).await {
        return internal(&e);
    }
    // The provisioner wakes on this; it also looks once a minute, so a lost notification
    // only delays the request (0111 §3).
    if let Err(e) = client.batch_execute(&format!("NOTIFY {CHANNEL}")).await {
        return internal(&e);
    }
    let entry = tenant_requests::Entry {
        filed,
        actor: Some(actor),
        time: now,
        reason,
        ask,
        state: tenant_requests::State::Pending,
    };
    json_response(StatusCode::ACCEPTED, &json!({"request": entry.to_json()}))
}

/// `GET /tenancy/requests`.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Err(r) = entitled(&app, &tenant, &caller) {
        return r;
    }
    match tenant_requests::list(&**client).await {
        Ok(entries) => json_response(
            StatusCode::OK,
            &json!({"requests": entries.iter().rev().map(tenant_requests::Entry::to_json).collect::<Vec<_>>()}),
        ),
        Err(e) => internal(&e),
    }
}

/// `GET /tenancy/requests/{id}`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Err(r) = entitled(&app, &tenant, &caller) {
        return r;
    }
    let Ok(id) = id.parse::<u64>() else {
        return error(
            StatusCode::BAD_REQUEST,
            "badid",
            "a request's ID is a number",
        );
    };
    match tenant_requests::list(&**client).await {
        Ok(entries) => match entries.iter().find(|e| e.filed.offset == id) {
            Some(e) => json_response(StatusCode::OK, &json!({"request": e.to_json()})),
            None => error(
                StatusCode::NOT_FOUND,
                "nosuchrequest",
                "no such tenant request",
            ),
        },
        Err(e) => internal(&e),
    }
}

/// `POST /tenancy/requests/{id}/invitation` (0111 §5, A2): for a created tenant whose
/// request named no owner login source and whose owner has no login yet, issues the owner's
/// invitation, replacing an unused one, and answers its code and the address to send, once.
pub async fn invite(
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
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let actor = match entitled(&app, &tenant, &caller) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let object = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(o)) => o,
        _ if body.is_empty() => Map::new(),
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "badbody",
                "the body is a JSON object",
            );
        }
    };
    if let Err(r) = check_token(&app, &caller, &headers, &object) {
        return r;
    }
    let Ok(id) = id.parse::<u64>() else {
        return error(
            StatusCode::BAD_REQUEST,
            "badid",
            "a request's ID is a number",
        );
    };
    let entries = match tenant_requests::list(&**client).await {
        Ok(e) => e,
        Err(e) => return internal(&e),
    };
    let Some(entry) = entries.into_iter().find(|e| e.filed.offset == id) else {
        return error(
            StatusCode::NOT_FOUND,
            "nosuchrequest",
            "no such tenant request",
        );
    };
    if entry.state != tenant_requests::State::Created {
        return error(
            StatusCode::CONFLICT,
            "ts-invitation-not-created",
            "an invitation is for a tenant the request created",
        );
    }
    if entry.ask.owner_login_source.is_some() {
        return error(
            StatusCode::CONFLICT,
            "ts-invitation-bound",
            "the request named the owner's login; the owner signs in with it",
        );
    }
    let (created, owner) = match owner_of(&**client, &entry).await {
        Ok(x) => x,
        Err(e) => return internal(&e),
    };
    let accounts = match app.accounts_pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    match triplespace_accounts::logins::login_of(&**accounts, &created.slug, &owner).await {
        Ok(Some(_)) => {
            return error(
                StatusCode::CONFLICT,
                "ts-invitation-bound",
                "the owner account already has a login",
            );
        }
        Ok(None) => {}
        Err(e) => return internal(&e),
    }
    let code =
        match triplespace_accounts::invitations::issue(&**accounts, &created.slug, &owner, &actor)
            .await
        {
            Ok(c) => c,
            Err(e) => return internal(&e),
        };
    let url = format!(
        "{}/wiki/Special:CreateAccount?invite={code}",
        created.base.trim_end_matches('/')
    );
    json_response(
        StatusCode::OK,
        &json!({"invitation": {
            "code": code, "url": url, "tenant": created.slug, "owner": owner,
            "valid_days": triplespace_accounts::invitations::VALID_DAYS,
        }}),
    )
}

/// The tenant a request created and its owner account's actor key.
async fn owner_of<C: tokio_postgres::GenericClient>(
    client: &C,
    entry: &tenant_requests::Entry,
) -> Result<(triplespace_projections::tenants::TenantEntry, String), String> {
    let created = triplespace_projections::tenants::tenants(client)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|t| t.slug == entry.ask.slug)
        .ok_or_else(|| "the created tenant is not projected yet".to_string())?;
    let owner =
        scatter_actors::key::ActorKey::numeric(&created.issuer, entry.ask.owner_id.unwrap_or(1))
            .to_string();
    Ok((created, owner))
}
