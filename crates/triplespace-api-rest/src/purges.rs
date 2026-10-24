//! `GET /purges` (03 §9.6; 0057 §5): the cache purges queued since an ID, for the web
//! tier, a CDN hook or an operator's script, which purges the tags and remembers the last
//! ID it saw. Needs `ts-config`, since the queue names restricted targets by tag. Nothing
//! in the instance serves from a cache yet; the queue says what a layer would have to drop.
//! `since` (an ID, 0 by default) and `limit` (100, at most 1000).

use std::collections::BTreeMap;

use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::json;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};

use crate::entity_route::{error, internal};
use crate::settings::prelude;

/// `GET /purges`.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
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
    if caller.require("ts-config").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "the purge queue needs the \"ts-config\" right",
        );
    }
    let since: i64 = q.get("since").and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit: i64 = q
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
        .clamp(1, 1000);
    let rows = match client
        .query(
            "SELECT id, tag, reason, partition, \"offset\",
                    (extract(epoch FROM at) * 1e6)::bigint
             FROM ops.purge WHERE tenant = $1 AND id > $2 ORDER BY id LIMIT $3",
            &[&tenant.slug, &since, &limit],
        )
        .await
    {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let epoch: i64 = match client
        .query_opt(
            "SELECT epoch FROM ops.visibility_epoch WHERE tenant = $1",
            &[&tenant.slug],
        )
        .await
    {
        Ok(r) => r.map_or(0, |r| r.get(0)),
        Err(e) => return internal(&e),
    };
    let purges: Vec<_> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<_, i64>(0),
                "tag": r.get::<_, String>(1),
                "reason": r.get::<_, String>(2),
                "record": {"partition": r.get::<_, i64>(3).cast_unsigned().to_string(), "offset": r.get::<_, i64>(4)},
                "at": triplespace_projections::feed::iso8601(u64::try_from(r.get::<_, i64>(5)).unwrap_or(0)),
            })
        })
        .collect();
    let last = purges
        .last()
        .and_then(|p| p["id"].as_i64())
        .unwrap_or(since);
    let body = json!({"purges": purges, "last": last, "visibility_epoch": epoch});
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}
