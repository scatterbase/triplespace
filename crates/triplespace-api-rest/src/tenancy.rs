//! `GET /tenancy` at the farm base (0028 §11; 0046 §9; 0079 §7; 0012 §5): the farm, the
//! primary tenant, the tenancy policy, and the tenant list `Special:Tenants` shows, each
//! tenant with its slug, issuer code, host, name and status.
//!
//! A tenant's name is its `site.name` setting (0085 §7). Every listed tenant is `active`:
//! freezing and deleting a tenant (0018 §10; 0028 §5) are not built yet. A tenant that
//! is private (0056 §3) is listed by the same right as any other, since the list names
//! tenants and holds none of their content.

use axum::extract::{Extension, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use serde_json::{Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::{self, at_farm, host_of};
use triplespace_projections::tenants;

use crate::entity_route::{error, internal};

/// `GET /tenancy`.
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let tenant = match tenant::resolve(&app, &**client, &origin, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return error(
                StatusCode::MISDIRECTED_REQUEST,
                "misdirected",
                "not a tenant of this instance",
            );
        }
        Err(e) => return internal(&e),
    };
    if !at_farm(&app, &tenant) {
        return error(
            StatusCode::BAD_REQUEST,
            "ts-instance-policy",
            "the tenancy policy and the tenant list are served only at the farm base",
        );
    }
    let caller = match Caller::resolve(&**client, &tenant.slug, &headers).await {
        Ok(c) => c,
        Err(e) => return internal(&e.info),
    };
    if caller.require("read").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reading needs the \"read\" right",
        );
    }
    match body(&app, &client).await {
        Ok(b) => respond(&headers, &method, &caller, &b),
        Err(e) => internal(&e),
    }
}

async fn body(app: &App, client: &tokio_postgres::Client) -> Result<Value, String> {
    let farm = &app.config().farm;
    let primary = tenants::primary(client).await.map_err(|e| e.to_string())?;
    let mut list = Vec::new();
    for t in tenants::tenants(client).await.map_err(|e| e.to_string())? {
        let (settings, _) = triplespace_accounts::settings::load(client, Some(&t.slug))
            .await
            .map_err(|e| e.to_string())?;
        let name = settings
            .value("site.name")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| t.slug.clone());
        list.push(json!({
            "slug": t.slug,
            "issuer": t.issuer,
            "host": host_of(&t.base),
            "base": t.base,
            "name": name,
            "status": "active",
        }));
    }
    Ok(json!({
        "farm": {"slug": farm.slug, "code": farm.code, "base": farm.base},
        "primary": primary,
        "policy": tenant::tenancy_report(client).await.map_err(|e| e.to_string())?,
        "tenants": list,
    }))
}

fn respond(headers: &HeaderMap, method: &Method, caller: &Caller, body: &Value) -> Response {
    let credentialed = caller.has_session() || bearer(headers).is_some();
    let class = Class::Public.permitted(method, credentialed, false, false);
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &[],
    )
}
