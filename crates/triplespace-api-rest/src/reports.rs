//! `GET /reports` and `GET /reports/{name}` (0047 §11; 18 §3.2): every report with its
//! state, and one report's rows, as `list=querypage` serves them
//! ([`triplespace_api_action::reports`]). Rows are the report's public form (0047 §4.5),
//! so the answer is cacheable as the public form is; a caller to whom every page of the
//! tenant is hidden is refused.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{Value, json};
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::pages::Site;
use triplespace_api_action::reports as served;
use triplespace_api_action::response::ApiError;
use triplespace_projections::reports::{REPORTS, state};

use crate::entity_route::{error, internal};
use crate::settings::prelude;

fn refused(e: &ApiError) -> Response {
    let status = match e.code.as_str() {
        "permissiondenied" => StatusCode::FORBIDDEN,
        "ts-report-unavailable" => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error(status, &e.code, e.info.clone())
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

/// `GET /reports`.
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
    let hidden = match triplespace_projections::acl_read::hidden(
        &**client,
        &tenant.slug,
        &caller.effective,
        scatter_actors::Timestamp(triplespace_api_action::edit::now_micros()),
    )
    .await
    {
        Ok(h) => h,
        Err(e) => return internal(&e),
    };
    if hidden.every_page() {
        return refused(&ApiError::permission_denied("read this wiki's reports"));
    }
    let mut out = Vec::new();
    for r in REPORTS {
        match state(&**client, &tenant.slug, r).await {
            Ok(s) => out.push(served::state_json(r, &s)),
            Err(e) => return internal(&e),
        }
    }
    ok(&headers, &json!({"reports": out}))
}

/// `GET /reports/{name}?limit=&continue=&lang=`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let report = match served::named(&name) {
        Ok(r) => r,
        Err(e) => return refused(&e),
    };
    let offset: i64 = q
        .get("continue")
        .and_then(|c| c.parse().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(0);
    let limit: i64 = q
        .get("limit")
        .and_then(|l| l.parse().ok())
        .unwrap_or(50)
        .clamp(1, served::MAX_LIMIT);
    let site = match Site::load_for(&client, &tenant.slug).await {
        Ok(s) => s,
        Err(e) => return refused(&e),
    };
    let lang = q.get("lang").cloned().unwrap_or_else(|| site.lang.clone());
    let page = match served::page(
        &app,
        &client,
        &tenant.slug,
        &caller.effective,
        report,
        &served::Window {
            offset,
            limit,
            lang,
        },
    )
    .await
    {
        Ok(p) => p,
        Err(e) => return refused(&e),
    };
    let rows: Vec<Value> = page
        .rows
        .iter()
        .map(|r| {
            // The Action API's row, with its `ts-` fields named plainly here.
            let mut v = served::row_json(&site, r);
            if let Some(m) = v.as_object_mut() {
                for k in ["target", "fragment", "final", "label"] {
                    if let Some(x) = m.remove(&format!("ts-{k}")) {
                        m.insert(k.into(), x);
                    }
                }
            }
            v
        })
        .collect();
    let mut body = json!({
        "report": served::state_json(report, &page.state),
        "rows": rows,
    });
    if page.more {
        body["continue"] = json!((offset + limit).to_string());
    }
    ok(&headers, &body)
}
