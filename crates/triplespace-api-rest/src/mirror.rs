//! `GET /provider/{code}/mirror` (18 §3.2; 0070 §9; 0078 §10; 0092 §7, A15): how a
//! provider's mirror, or a tenant's entity source, is kept current. The live counters
//! that `siprop=providers` does not carry: how it is followed, the follower's position,
//! lag, `late` and `stale` counts and its jobs, a gap it could not cross (`follow-gap`),
//! and the last load's end and whether the follower has taken over from it.
//!
//! `{code}` is a registry provider's two-letter code (`WD`) or a source's name (`lb`):
//! uppercase codes and lowercase source names never clash. `queued` is how many entities
//! wait in the fetch queue for the follower (0070 §3, A9), counted to at most
//! [`QUEUED_CAP`] so that a crawl's long queue is not counted whole on each read. `types`
//! gives, per entity type, the `entities.mirror` setting the follower applies and how many
//! entities of the type the mirror holds, counted to the same cap (0070 §9, A12).
//!
//! `POST /provider/{code}/mirror/fetch` queues entities by hand, as `triplespace fetch`
//! does, with the `ts-runjob` right (0070 §9).

use axum::body::Bytes;
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

/// The provider or source `code` names on `tenant`, with the tenant its follower and queue
/// are kept under (`""` for a registry provider) and its kind.
async fn target(
    app: &App,
    client: &deadpool_postgres::Client,
    tenant: &str,
    code: &str,
) -> Result<Option<(&'static scatter_providers::Provider, String, &'static str)>, Response> {
    if code.bytes().all(|b| b.is_ascii_uppercase()) {
        return Ok(app
            .registry()
            .by_code(code)
            .map(|p| (p, String::new(), "provider")));
    }
    let registry = app
        .registry_for(&**client, tenant)
        .await
        .map_err(|e| internal(&e))?;
    Ok(registry
        .by_slug(code)
        .filter(|p| p.source)
        .map(|p| (p, tenant.to_string(), "source")))
}

fn not_found(code: &str) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "provider-not-found",
        format!("`{code}` is neither a provider's code nor a source of this tenant"),
    )
}

/// Per entity type, the setting the follower applies and the set's size, to [`QUEUED_CAP`].
async fn types(
    client: &tokio_postgres::Client,
    provider: &scatter_providers::Provider,
    tenant: &str,
    loaded: bool,
) -> Result<Value, tokio_postgres::Error> {
    let policy = triplespace_api_ingest::shallow::policy(client, tenant, provider, loaded).await?;
    let graph = if provider.source {
        format!("source/{}", provider.slug)
    } else {
        format!("mirror/{}", provider.slug)
    };
    let cap = i64::try_from(QUEUED_CAP).unwrap_or(i64::MAX);
    let mut out = serde_json::Map::new();
    for t in &provider.types {
        let prefix = if provider.source {
            format!("{}:{}", provider.slug, t.upstream_prefix)
        } else {
            format!("{}{}", provider.code, t.code)
        };
        // A prefix followed by digits: `WDQ` must not count `WDQX…` of another type.
        let size: i64 = client
            .query_one(
                "SELECT count(*) FROM (SELECT 1 FROM view.entity_source
                                       WHERE tenant = $1 AND graph = $2
                                         AND entity_id LIKE $3 || '%'
                                         AND substr(entity_id, length($3) + 1) ~ '^[0-9]+$'
                                       LIMIT $4) q",
                &[&tenant, &graph, &prefix, &cap],
            )
            .await?
            .get(0);
        out.insert(
            t.entity_type.clone(),
            json!({"setting": policy.setting(&t.entity_type), "size": size}),
        );
    }
    Ok(Value::Object(out))
}

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
    // A registry provider by its code, followed by the instance; a source by its name,
    // followed under the tenant.
    let (provider, cursor_tenant, kind) = match target(&app, &client, &tenant.slug, &code).await {
        Ok(Some(t)) => t,
        Ok(None) => return not_found(&code),
        Err(r) => return r,
    };
    let slug = provider.slug.clone();
    let api = provider.api.is_some();
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
    let loaded = row
        .as_ref()
        .and_then(|r| r.get::<_, Option<std::time::SystemTime>>("loaded_to"))
        .is_some();
    let types = match types(&client, provider, &cursor_tenant, loaded).await {
        Ok(t) => t,
        Err(e) => return internal(&e),
    };
    let mut body = status(&code, kind, &slug, api, row.as_ref());
    body["queued"] = json!(queued);
    body["types"] = types;
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

/// `POST /provider/{code}/mirror/fetch`: queues `ids`, as the provider or source writes
/// them (`Q42`) or as they are stored (`WDQ42`, `lb:Q42`), with the reason `manual`.
pub async fn fetch(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let ctx = match crate::acl::context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let object = match crate::acl::write_body(&app, &ctx, &headers, &body) {
        Ok(o) => o,
        Err(r) => return r,
    };
    if ctx.caller.require("ts-runjob").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "queuing entities to fetch needs the \"ts-runjob\" right",
        );
    }
    let (provider, queue_tenant, _) = match target(&app, &ctx.client, &ctx.tenant.slug, &code).await
    {
        Ok(Some(t)) => t,
        Ok(None) => return not_found(&code),
        Err(r) => return r,
    };
    if provider.api.is_none() {
        return error(
            StatusCode::CONFLICT,
            "ts-no-api",
            format!("`{code}` has no `api` to fetch from"),
        );
    }
    let Some(ids) = object.get("ids").and_then(Value::as_array) else {
        return error(
            StatusCode::BAD_REQUEST,
            "badbody",
            "`ids` is a list of entity IDs",
        );
    };
    let registry = match app.registry_for(ctx.db(), &ctx.tenant.slug).await {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let mut upstream = Vec::new();
    for id in ids {
        let Some(id) = id.as_str() else {
            return error(StatusCode::BAD_REQUEST, "badbody", "an ID is a string");
        };
        match upstream_id(registry, provider, id) {
            Some(u) => upstream.push(u),
            None => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "ts-not-this-provider",
                    format!("`{id}` is not an entity of `{code}`"),
                );
            }
        }
    }
    let queued = match triplespace_api_ingest::fetch::enqueue(
        ctx.db(),
        &queue_tenant,
        &provider.slug,
        &upstream,
        triplespace_api_ingest::fetch::Reason::Manual,
    )
    .await
    {
        Ok(n) => n,
        Err(e) => return internal(&e),
    };
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        json!({"queued": queued, "asked": upstream.len()}).to_string(),
        Class::Private,
        &[],
    )
}

/// `id` as `provider` writes it: a stored ID of the provider read back, or an ID in the
/// provider's own form, which one of its types' prefixes begins.
fn upstream_id(
    registry: &scatter_providers::Registry,
    provider: &scatter_providers::Provider,
    id: &str,
) -> Option<String> {
    if let Ok(f) = registry.parse_foreign_id(id) {
        return (f.provider().slug == provider.slug).then(|| f.upstream_id());
    }
    let own = !id.contains(':')
        && provider.types.iter().any(|t| {
            id.strip_prefix(t.upstream_prefix.as_str())
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        });
    own.then(|| id.to_string())
}
