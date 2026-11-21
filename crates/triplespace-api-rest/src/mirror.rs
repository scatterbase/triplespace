//! `GET /provider/{code}/mirror` (18 §3.2; 0070 §9; 0078 §10; 0092 §7, A15): how a
//! provider's mirror, or a tenant's entity source, is kept current. The live counters
//! that `siprop=providers` does not carry: how it is followed, the follower's position,
//! lag, `late` and `stale` counts and its jobs, a gap it could not cross (`follow-gap`),
//! and the last load's end and whether the follower has taken over from it.
//!
//! `{code}` is a registry provider's two-letter code (`WD`) or a source's name (`lb`):
//! uppercase codes and lowercase source names never clash. `queued` is how many entities
//! wait in the fetch queue for the follower (0070 §3, A9), counted to at most
//! [`QUEUED_CAP`] so that a crawl's long queue is not counted whole on each read. The per-type mirror settings
//! and set sizes of shallow mirroring (0070 §9) are not reported yet.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{Value, json};
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_projections::read::iso8601;

use crate::entity_route::{error, internal};
use crate::settings::prelude;

/// The most queued entities the route counts.
pub const QUEUED_CAP: u64 = 10_000;

/// The mirror's status.
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, _) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let not_found = || {
        error(
            StatusCode::NOT_FOUND,
            "provider-not-found",
            format!("`{code}` is neither a provider's code nor a source of this tenant"),
        )
    };
    // A registry provider by its code, followed by the instance; a source by its name,
    // followed under the tenant.
    let (slug, cursor_tenant, kind, api) = if code.bytes().all(|b| b.is_ascii_uppercase()) {
        let Some(p) = app.registry().by_code(&code) else {
            return not_found();
        };
        (p.slug.clone(), String::new(), "provider", p.api.is_some())
    } else {
        let registry = match app.registry_for(&client, &tenant.slug).await {
            Ok(r) => r,
            Err(e) => return internal(&e),
        };
        let Some(p) = registry.by_slug(&code).filter(|p| p.source) else {
            return not_found();
        };
        (
            p.slug.clone(),
            tenant.slug.clone(),
            "source",
            p.api.is_some(),
        )
    };
    let row = match client
        .query_opt(
            "SELECT rc_time, lag_seconds, late, stale, job, gap, loaded_to, handed_over, updated
             FROM ops.repo_cursor WHERE tenant = $1 AND kind = $2 AND name = $3",
            &[&cursor_tenant, &kind, &slug],
        )
        .await
    {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let queued =
        match triplespace_api_ingest::fetch::queued(&**client, &cursor_tenant, &slug, QUEUED_CAP)
            .await
        {
            Ok(n) => n,
            Err(e) => return internal(&e),
        };
    let mut body = status(&code, kind, &slug, api, row.as_ref());
    body["queued"] = json!(queued);
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}

/// The body, from the position row where there is one.
fn status(
    code: &str,
    kind: &str,
    slug: &str,
    api: bool,
    row: Option<&tokio_postgres::Row>,
) -> Value {
    let time = |name: &str| -> Option<String> {
        row.and_then(|r| r.get::<_, Option<std::time::SystemTime>>(name))
            .map(|t| {
                iso8601(
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX)),
                )
            })
    };
    let position = time("rc_time");
    let gap: Option<Value> = row.and_then(|r| r.get("gap"));
    let job: Option<Value> = row.and_then(|r| r.get("job"));
    let follow = position.as_ref().map(|p| {
        let r = row.expect("a position is read from a row");
        json!({
            "position": p,
            "lag_seconds": r.get::<_, Option<i32>>("lag_seconds"),
            "late": r.get::<_, Option<i64>>("late").unwrap_or(0),
            "stale": r.get::<_, Option<i64>>("stale").unwrap_or(0),
            "updated": time("updated"),
            "job": job.as_ref().and_then(|j| j.get("open")).map(|o| json!({
                "id": o.get("id"),
                "batches": o.get("batches"),
                "rows": o.get("rows"),
            })),
            "closing": job.as_ref().and_then(|j| j.get("closing")).map(|c| json!({
                "id": c.pointer("/job/id"),
                "compacted_to": c.get("at"),
                "to": c.get("to"),
                "removed": c.get("removed"),
            })),
        })
    });
    let load = time("loaded_to").map(|t| {
        json!({
            "loaded_to": t,
            "handed_over": row.and_then(|r| r.get::<_, Option<bool>>("handed_over")).unwrap_or(false),
        })
    });
    let state = if gap.is_some() {
        "follow-gap"
    } else if follow.is_some() {
        "following"
    } else {
        "not-followed"
    };
    json!({
        "code": code,
        "kind": kind,
        "slug": slug,
        "followed_by": api.then_some("recentchanges"),
        "status": state,
        "follow": follow,
        "gap": gap,
        "load": load,
    })
}
