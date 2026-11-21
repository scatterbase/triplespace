//! `GET /entity-sources` (18 §3.2; 0078 §10): the tenant's entity sources as it declares
//! them, the list `siprop=triplespace` carries as `entity_sources`. A reader on another
//! instance reads it to rewrite a reference to one of the tenant's source entities by its
//! IRI (05 §6.7). Public, as the tenant's configuration is: it needs the `read` right and
//! nothing more.

use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::json;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};

use crate::entity_route::internal;
use crate::settings::prelude;

/// `GET /entity-sources`.
pub async fn handle(
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
    let sources =
        match triplespace_projections::entity_sources::declared(&**client, &tenant.slug).await {
            Ok(s) => s,
            Err(e) => return internal(&e),
        };
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        json!({ "sources": triplespace_projections::entity_sources::public(&sources) }).to_string(),
        Class::Private,
        &[],
    )
}
