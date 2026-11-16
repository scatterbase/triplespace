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
use scatter_ingest::store::IngestStore as _;
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
    entitled_to(app, tenant, caller, RIGHT)
}

/// The farm base and an instance right, or the refusal.
#[allow(clippy::result_large_err)] // a ready response, as the async routes return
fn entitled_to(
    app: &App,
    tenant: &Tenant,
    caller: &Caller,
    right: &str,
) -> Result<String, Response> {
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
    if !instance_rights(Some(&caller.effective), None).contains(right) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            format!("this needs `{right}`, an instance right"),
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
        act: text(&object, "act").unwrap_or_default(),
        ..Default::default()
    };
    if ask.act() == tenant_requests::act::ACCEPT_PRIMARY {
        return error(
            StatusCode::BAD_REQUEST,
            "ts-tenant-request-invalid",
            "the primary role is accepted on the offered tenant, with POST /tenancy/acceptance",
        );
    }
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

/// A write's body, with its CSRF token checked.
#[allow(clippy::result_large_err)] // a ready response, as the async routes return
fn body_of(
    app: &App,
    caller: &Caller,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Map<String, Value>, Response> {
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(body) else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "badbody",
            "the body is a JSON object",
        ));
    };
    check_token(app, caller, headers, &object)?;
    Ok(object)
}

/// Runs `write` in a unit of work of the public store, and commits it.
async fn in_unit<T>(
    app: &App,
    write: impl AsyncFnOnce(&mut triplespace_projections::PgCx) -> Result<T, String>,
) -> Result<T, Response> {
    let store = app.store();
    let mut cx = store.begin().await.map_err(|e| internal(&e))?;
    match write(&mut cx).await {
        Ok(v) => {
            store.commit(cx).await.map_err(|e| internal(&e))?;
            Ok(v)
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(internal(&e))
        }
    }
}

/// `POST /tenancy/primary/offer` with `{tenant, reason?, token}` (0046 §5; 08 §7.5): at the
/// farm base, for a holder of `ts-primary`, offers the primary role to another tenant for
/// seven days.
pub async fn offer(
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
    let actor = match entitled_to(&app, &tenant, &caller, "ts-primary") {
        Ok(a) => a,
        Err(r) => return r,
    };
    let object = match body_of(&app, &caller, &headers, &body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let Some(offered) = text(&object, "tenant") else {
        return error(StatusCode::BAD_REQUEST, "badbody", "`tenant` is required");
    };
    match triplespace_projections::tenants::tenant(&**client, &offered).await {
        Ok(Some(t)) if t.writable() && offered != tenant.slug => {}
        Ok(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "ts-tenant-invalid",
                "the role is offered to another active tenant of this instance",
            );
        }
        Err(e) => return internal(&e),
    }
    let reason = text(&object, "reason");
    let now = triplespace_api_action::edit::now_micros();
    let filed = in_unit(&app, async |cx| {
        tenant_requests::offer(
            app.store(),
            app.pipeline(),
            cx,
            &offered,
            &actor,
            reason.as_deref(),
            now,
        )
        .await
        .map_err(|e| e.to_string())
    })
    .await;
    match filed {
        Ok(f) => json_response(
            StatusCode::OK,
            &json!({"offer": {"id": f.offset, "tenant": offered,
                              "expires": now + tenant_requests::OFFER_DAYS * 86_400 * 1_000_000}}),
        ),
        Err(r) => r,
    }
}

/// `POST /tenancy/primary/withdraw` with `{offer, token}`: withdraws an open offer.
pub async fn withdraw(
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
    let actor = match entitled_to(&app, &tenant, &caller, "ts-primary") {
        Ok(a) => a,
        Err(r) => return r,
    };
    let object = match body_of(&app, &caller, &headers, &body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let Some(id) = object.get("offer").and_then(Value::as_u64) else {
        return error(StatusCode::BAD_REQUEST, "badbody", "`offer` is required");
    };
    let now = triplespace_api_action::edit::now_micros();
    let open = match tenant_requests::open_offers(&**client, now).await {
        Ok(o) => o,
        Err(e) => return internal(&e),
    };
    if !open.iter().any(|o| o.offset == id) {
        return error(StatusCode::NOT_FOUND, "ts-no-offer", "no such open offer");
    }
    let done = in_unit(&app, async |cx| {
        let partition = app
            .store()
            .partition(cx, "", "log")
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no instance log".to_string())?;
        let offer = tenant_requests::Filed {
            partition,
            offset: id,
            leaf: [0; 32],
        };
        tenant_requests::withdraw(app.store(), app.pipeline(), cx, &offer, &actor, now)
            .await
            .map_err(|e| e.to_string())
    })
    .await;
    match done {
        Ok(_) => json_response(StatusCode::OK, &json!({"withdrawn": id})),
        Err(r) => r,
    }
}

/// `POST /tenancy/acceptance` with `{offer, token}` (08 §7.5; 0111 §10): on the offered
/// tenant, for a member of its `owner` group, files the acceptance of the primary role as a
/// tenant request, which the provisioner carries out.
pub async fn accept(
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
    let Some(actor) = caller.identity.actor_key.clone() else {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "accepting the primary role needs a member of `owner` on this tenant",
        );
    };
    if !caller.effective.groups.contains("owner") {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "accepting the primary role needs a member of `owner` on this tenant",
        );
    }
    let object = match body_of(&app, &caller, &headers, &body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let Some(offer) = object.get("offer").and_then(Value::as_u64) else {
        return error(StatusCode::BAD_REQUEST, "badbody", "`offer` is required");
    };
    let mut ask = Ask::on(tenant_requests::act::ACCEPT_PRIMARY, &tenant.slug);
    ask.offer = Some(offer);
    let now = triplespace_api_action::edit::now_micros();
    let filed = in_unit(&app, async |cx| {
        tenant_requests::file(app.store(), app.pipeline(), cx, &ask, &actor, None, now)
            .await
            .map_err(|e| e.to_string())
    })
    .await;
    let filed = match filed {
        Ok(f) => f,
        Err(r) => return r,
    };
    if let Err(e) = client.batch_execute(&format!("NOTIFY {CHANNEL}")).await {
        return internal(&e);
    }
    let entry = tenant_requests::Entry {
        filed,
        actor: Some(actor),
        time: now,
        reason: None,
        ask,
        state: tenant_requests::State::Pending,
    };
    json_response(StatusCode::ACCEPTED, &json!({"request": entry.to_json()}))
}

/// `GET /tenancy/acceptance` (08 §7.5): on any tenant, the open offers of the primary role
/// that name it, for the page that accepts one.
pub async fn offers_here(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, _) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let now = triplespace_api_action::edit::now_micros();
    match tenant_requests::open_offers(&**client, now).await {
        Ok(offers) => json_response(
            StatusCode::OK,
            &json!({"offers": offers
                .iter()
                .filter(|o| o.tenant == tenant.slug)
                .map(|o| json!({"id": o.offset, "tenant": o.tenant, "expires": o.expires, "actor": o.actor}))
                .collect::<Vec<_>>()}),
        ),
        Err(e) => internal(&e),
    }
}
