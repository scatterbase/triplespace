//! `/group/{name}` (09 §3.2): read, define and retire a tenant's group. The logic is
//! [`triplespace_api_action::group_api`]; this is the HTTP around it, as [`crate::acl`]
//! is for ACLs.

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::group_api;

use crate::acl::{context, ok, refused, write_body};

/// `GET /group/{name}`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    match group_api::read(&ctx, &name).await {
        Ok(v) => ok(&headers, &v),
        Err(e) => refused(&e),
    }
}

/// `PUT /group/{name}`.
pub async fn put(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
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
    match group_api::write(&ctx, &name, &body).await {
        Ok(v) => ok(&headers, &v),
        Err(e) => refused(&e),
    }
}

/// `DELETE /group/{name}`.
pub async fn delete(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
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
    match group_api::retire(&ctx, &name, &body).await {
        Ok(v) => ok(&headers, &v),
        Err(e) => refused(&e),
    }
}
