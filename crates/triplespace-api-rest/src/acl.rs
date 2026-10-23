//! `/acl/{kind}/{id}` and `/acl/tenant/{slug}` (0016 §7; 0023 §8; 0056 §13; 18 §3.2): read,
//! set and retire a target's ACL. The logic is [`triplespace_api_action::acl_api`]; this
//! module is the HTTP around it: the tenant and caller, the CSRF token of a session write,
//! and the status each refusal answers with.

#![allow(
    clippy::result_large_err,
    reason = "the helpers return a ready response, as the async routes do"
)]

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_actors::acl::Target;
use serde_json::{Map, Value};
use triplespace_api_action::App;
use triplespace_api_action::acl_api;
use triplespace_api_action::api::Ctx;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::response::ApiError;

use crate::entity_route::{error, internal};
use crate::settings::{check_token, prelude};

/// The status an [`ApiError`] answers with.
fn status_of(e: &ApiError) -> StatusCode {
    match e.code.as_str() {
        "ts-no-target" => StatusCode::NOT_FOUND,
        "permissiondenied" | "readapidenied" | "ts-locked-out" | "ts-policy" | "badtoken"
        | "protectedpage" | "protectednamespace" => StatusCode::FORBIDDEN,
        "ts-deleted" | "ts-no-acl" => StatusCode::CONFLICT,
        "internal" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    }
}

fn refused(e: &ApiError) -> Response {
    error(status_of(e), &e.code, e.info.clone())
}

/// The request's context, as the Action API's modules take it.
async fn context(app: &App, origin: Origin, headers: &HeaderMap) -> Result<Ctx, Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let (tenant, caller) = prelude(app, &client, &origin, headers).await?;
    Ok(Ctx {
        app: app.clone(),
        client,
        tenant,
        origin,
        params: triplespace_api_action::params::Params::default(),
        caller,
        formatversion: 2,
        set_cookie: None,
        warnings: Vec::new(),
        cache_class: Class::Private,
        cache_tags: Vec::new(),
        accept_language: None,
    })
}

/// A write's body, with its token checked and removed.
fn write_body(
    app: &App,
    ctx: &Ctx,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<Map<String, Value>, Response> {
    let mut object: Map<String, Value> = if body.is_empty() {
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
    check_token(app, &ctx.caller, headers, &object)?;
    object.remove("token");
    Ok(object)
}

fn ok(headers: &HeaderMap, body: &Value) -> Response {
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}

async fn get_target(
    app: App,
    origin: Origin,
    headers: HeaderMap,
    target: Result<Target, ApiError>,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let target = match target {
        Ok(t) => t,
        Err(e) => return refused(&e),
    };
    match acl_api::read(&ctx, &target).await {
        Ok(v) => ok(&headers, &v),
        Err(e) => refused(&e),
    }
}

async fn put_target(
    app: App,
    origin: Origin,
    headers: HeaderMap,
    target: Result<Target, ApiError>,
    body: Bytes,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let body = match write_body(&app, &ctx, &headers, &body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let target = match target {
        Ok(t) => t,
        Err(e) => return refused(&e),
    };
    match acl_api::write(&ctx, target, &body).await {
        Ok(v) => ok(&headers, &v),
        Err(e) => refused(&e),
    }
}

async fn delete_target(
    app: App,
    origin: Origin,
    headers: HeaderMap,
    target: Result<Target, ApiError>,
    body: Bytes,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let body = match write_body(&app, &ctx, &headers, &body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let target = match target {
        Ok(t) => t,
        Err(e) => return refused(&e),
    };
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match acl_api::retire(&ctx, target, reason).await {
        Ok(v) => ok(&headers, &v),
        Err(e) => refused(&e),
    }
}

/// `GET /acl/{kind}/{id}`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Response {
    get_target(app, origin, headers, acl_api::target_of(&kind, &id)).await
}

/// `PUT /acl/{kind}/{id}`.
pub async fn put(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    put_target(app, origin, headers, acl_api::target_of(&kind, &id), body).await
}

/// `DELETE /acl/{kind}/{id}`.
pub async fn delete(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    delete_target(app, origin, headers, acl_api::target_of(&kind, &id), body).await
}

/// `GET /acl/tenant/{slug}`.
pub async fn get_tenant(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Response {
    get_target(app, origin, headers, Ok(Target::Tenant(slug))).await
}

/// `PUT /acl/tenant/{slug}`.
pub async fn put_tenant(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Bytes,
) -> Response {
    put_target(app, origin, headers, Ok(Target::Tenant(slug)), body).await
}

/// `DELETE /acl/tenant/{slug}`.
pub async fn delete_tenant(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Bytes,
) -> Response {
    delete_target(app, origin, headers, Ok(Target::Tenant(slug)), body).await
}

/// Runs a write against the Action API's ACL functions with the request's context and
/// checked body.
async fn set_write<F, Fut>(
    app: App,
    origin: Origin,
    headers: HeaderMap,
    body: Bytes,
    f: F,
) -> Response
where
    F: FnOnce(Ctx, Map<String, Value>) -> Fut,
    Fut: std::future::Future<Output = Result<Value, ApiError>>,
{
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let body = match write_body(&app, &ctx, &headers, &body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match f(ctx, body).await {
        Ok(v) => ok(&headers, &v),
        Err(e) => refused(&e),
    }
}

/// `POST /acl/set`.
pub async fn create_set(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let h = headers.clone();
    set_write(app, origin, h, body, |ctx, body| async move {
        acl_api::create_set(&ctx, &body).await
    })
    .await
}

/// `POST /acl/set/{id}/members`.
pub async fn add_member(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    body: Bytes,
) -> Response {
    let h = headers.clone();
    set_write(app, origin, h, body, |ctx, body| async move {
        acl_api::add_member(&ctx, id, &body).await
    })
    .await
}

/// `DELETE /acl/set/{id}/members/{kind}/{member}`.
pub async fn remove_member(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((id, kind, member)): Path<(u64, String, String)>,
    body: Bytes,
) -> Response {
    let h = headers.clone();
    set_write(app, origin, h, body, |ctx, body| async move {
        let reason = body
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        acl_api::remove_member(&ctx, id, &kind, &member, &reason).await
    })
    .await
}
