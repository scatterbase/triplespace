//! `GET /jobs` and `GET /jobs/{id}` (0012 §5; 0104 §6; 18 §3.2; 0010 §9): the tenant's
//! jobs, running and recent, and one job's page: its record from `view.job` (0011 §6.3),
//! its first rejects, and, while it waits for or runs under the runner, its row in
//! `ops.job_queue` (0104 §4), which is where a `queued` job and a running job's progress
//! come from. `status=` narrows the list; `status=queued` is what waits, in order.
//! Reading needs `read`; a job's actor is shown as the feed shows actors, by key and name.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{Value, json};
use tokio_postgres::GenericClient;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_ingest::jobs::{QueuedJob, queued, queued_one};
use triplespace_projections::read::iso8601;

use crate::entity_route::{error, internal};
use crate::settings::prelude;

/// How many jobs a listing holds at most.
const LIMIT: i64 = 200;

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

fn micros(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// A `view.job` row as the API shows it.
fn job_json(r: &tokio_postgres::Row) -> Value {
    let job_id: i64 = r.get(0);
    let actor_key: String = r.get(1);
    let actor_name: Option<String> = r.get(2);
    let requested_by: Option<String> = r.get(3);
    let source: String = r.get(4);
    let version: Option<String> = r.get(5);
    let adapter: Option<String> = r.get(6);
    let mode: Option<String> = r.get(7);
    let graph: String = r.get(8);
    let params: Value = r.get(9);
    let params = params.clone();
    let status: String = r.get(10);
    let started: std::time::SystemTime = r.get(11);
    let finished: Option<std::time::SystemTime> = r.get(12);
    let counts: Option<Value> = r.get(13);
    let checkpoint_size: Option<i64> = r.get(14);
    let sweep_count: Option<i32> = r.get(15);
    let sweep_threshold: Option<i32> = r.get(16);
    let mut v = json!({
        "id": job_id,
        "status": status,
        "mode": mode,
        "source": source,
        "version": version,
        "adapter": adapter,
        "graph": graph,
        "params": params,
        "actor": {"key": actor_key, "name": actor_name},
        "requested_by": requested_by,
        "started": iso8601(micros(started)),
        "finished": finished.map(|t| iso8601(micros(t))),
        "links": {"page": format!("/wiki/Special:Jobs/{job_id}")},
    });
    match status.as_str() {
        "finished" => {
            v["counts"] = counts.unwrap_or(Value::Null);
            if let Some(n) = checkpoint_size {
                v["checkpoint_size"] = json!(n);
            }
            if let (Some(c), Some(t)) = (sweep_count, sweep_threshold) {
                v["sweep"] = json!({"count": c, "threshold": t});
            }
        }
        "failed" => {
            v["error"] = counts
                .as_ref()
                .and_then(|c| c.get("error"))
                .cloned()
                .unwrap_or(Value::Null);
        }
        "interrupted" => {
            v["interrupted_by"] = counts
                .as_ref()
                .and_then(|c| c.get("by"))
                .cloned()
                .unwrap_or(Value::Null);
        }
        "rejected" => {
            v["reason"] = counts
                .as_ref()
                .and_then(|c| c.get("reason"))
                .cloned()
                .unwrap_or(Value::Null);
            v["revert"] = counts
                .as_ref()
                .and_then(|c| c.get("revert"))
                .cloned()
                .unwrap_or(Value::Null);
        }
        "reverted" => {
            v["reverted_by"] = counts
                .as_ref()
                .and_then(|c| c.get("reverted_by"))
                .cloned()
                .unwrap_or(Value::Null);
        }
        _ => {}
    }
    // A batch's hash and operator, never its value (0104 §1).
    if let Some(b) = params.get("batch") {
        v["batch"] = b.clone();
        v["operator"] = params.get("operator").cloned().unwrap_or(Value::Null);
    }
    v
}

/// A queue row as the API shows it: a job that waits, or the running side of one.
fn queued_json(q: &QueuedJob, actor_name: Option<&str>) -> Value {
    let mut v = json!({
        "id": q.job_id,
        "status": if q.running { "running" } else { "queued" },
        "mode": q.kind,
        "kind": q.kind,
        "source": q.params.get("source").cloned().unwrap_or(Value::Null),
        "version": q.params.get("version").cloned().unwrap_or(Value::Null),
        "graph": "pages",
        "params": q.params,
        "actor": {"key": q.actor, "name": actor_name},
        "submitted": iso8601(q.submitted),
        "attempts": q.attempts,
        "links": {"page": format!("/wiki/Special:Jobs/{}", q.job_id)},
    });
    if let Some(p) = q.position {
        v["position"] = json!(p);
    }
    if let Some(p) = &q.progress {
        v["progress"] = p.clone();
    }
    v
}

const JOB_COLUMNS: &str =
    "j.job_id, j.actor_key, a.name, j.requested_by, j.source, j.source_version, j.adapter_version,
       j.mode, j.graph, j.params, j.status, j.started, j.finished, j.counts, j.checkpoint_size,
       j.sweep_count, j.sweep_threshold";

async fn actor_names<C: GenericClient>(
    client: &C,
    tenant: &str,
    keys: &[String],
) -> Result<BTreeMap<String, String>, tokio_postgres::Error> {
    if keys.is_empty() {
        return Ok(BTreeMap::new());
    }
    Ok(client
        .query(
            "SELECT actor_key, name FROM view.actor WHERE tenant = $1 AND actor_key = ANY($2)",
            &[&tenant, &keys],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect())
}

/// `GET /jobs`.
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
    if caller.require("read").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reading jobs needs the \"read\" right",
        );
    }
    let status = q
        .get("status")
        .map(String::as_str)
        .filter(|s| !s.is_empty());
    let limit = q
        .get("limit")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(50)
        .clamp(1, LIMIT);
    let mut jobs: Vec<Value> = Vec::new();
    // What waits for the runner, and what runs under it, oldest first.
    if status.is_none_or(|s| s == "queued" || s == "running") {
        let rows = match queued(&**client, &tenant.slug).await {
            Ok(r) => r,
            Err(e) => return internal(&e),
        };
        let keys: Vec<String> = rows.iter().map(|r| r.actor.clone()).collect();
        let names = match actor_names(&**client, &tenant.slug, &keys).await {
            Ok(n) => n,
            Err(e) => return internal(&e),
        };
        for r in &rows {
            if status.is_none_or(|s| (s == "running") == r.running) {
                jobs.push(queued_json(r, names.get(&r.actor).map(String::as_str)));
            }
        }
    }
    if status != Some("queued") {
        let sql = format!(
            "SELECT {JOB_COLUMNS} FROM view.job j
             LEFT JOIN view.actor a ON a.tenant = j.tenant AND a.actor_key = j.actor_key
             WHERE j.tenant = $1 AND ($2::text IS NULL OR j.status = $2)
             ORDER BY j.job_id DESC LIMIT $3"
        );
        let rows = match client.query(&sql, &[&tenant.slug, &status, &limit]).await {
            Ok(r) => r,
            Err(e) => return internal(&e),
        };
        // A running job the runner holds is already listed from its queue row, with its
        // progress; its record adds the start time.
        let held: BTreeMap<u64, usize> = jobs
            .iter()
            .enumerate()
            .filter_map(|(i, j)| j["id"].as_u64().map(|id| (id, i)))
            .collect();
        for r in &rows {
            let v = job_json(r);
            if let Some(&i) = v["id"].as_u64().and_then(|id| held.get(&id)) {
                jobs[i]["started"] = v["started"].clone();
                jobs[i]["graph"] = v["graph"].clone();
            } else {
                jobs.push(v);
            }
        }
    }
    ok(&headers, &json!({"jobs": jobs}))
}

/// `GET /jobs/{id}`.
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
    if caller.require("read").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reading jobs needs the \"read\" right",
        );
    }
    let Ok(job_id) = id.parse::<u64>() else {
        return error(
            StatusCode::NOT_FOUND,
            "job-not-found",
            format!("`{id}` is not a job ID"),
        );
    };
    let Ok(id_db) = i64::try_from(job_id) else {
        return error(
            StatusCode::NOT_FOUND,
            "job-not-found",
            format!("`{id}` is not a job ID"),
        );
    };
    let sql = format!(
        "SELECT {JOB_COLUMNS} FROM view.job j
         LEFT JOIN view.actor a ON a.tenant = j.tenant AND a.actor_key = j.actor_key
         WHERE j.tenant = $1 AND j.job_id = $2"
    );
    let record = match client.query_opt(&sql, &[&tenant.slug, &id_db]).await {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let in_queue = match queued_one(&**client, &tenant.slug, job_id).await {
        Ok(q) => q,
        Err(e) => return internal(&e),
    };
    let mut v = match (record.as_ref(), in_queue.as_ref()) {
        (Some(r), None) => job_json(r),
        (record, Some(q)) => {
            let names =
                match actor_names(&**client, &tenant.slug, std::slice::from_ref(&q.actor)).await {
                    Ok(n) => n,
                    Err(e) => return internal(&e),
                };
            let mut v = queued_json(q, names.get(&q.actor).map(String::as_str));
            if let Some(r) = record {
                let j = job_json(r);
                v["started"] = j["started"].clone();
                v["graph"] = j["graph"].clone();
                v["mode"] = j["mode"].clone();
            }
            v
        }
        (None, None) => {
            return error(
                StatusCode::NOT_FOUND,
                "job-not-found",
                format!("no job {job_id} on this wiki"),
            );
        }
    };
    let rejects = match client
        .query(
            "SELECT line, match_key, reason FROM view.job_reject
             WHERE tenant = $1 AND job_id = $2 ORDER BY line",
            &[&tenant.slug, &id_db],
        )
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|r| {
                let line: i64 = r.get(0);
                let key: Option<String> = r.get(1);
                let reason: String = r.get(2);
                json!({
                    "line": line,
                    "match": key.and_then(|k| serde_json::from_str::<Value>(&k).ok()),
                    "reason": reason,
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => return internal(&e),
    };
    v["rejects"] = Value::Array(rejects);
    ok(&headers, &v)
}

// ---------------------------------------------------------------------------------------
// The actions (0104 §2, §3, §6): reject, revert, close.
// implements 0104 §5
// implements 0104 §7
// ---------------------------------------------------------------------------------------

use axum::body::Bytes;
use scatter_ingest::job::Job;
use scatter_ingest::write::Attestation;
use scatter_projection::Backend as _;
use triplespace_api_action::edit::now_micros;
use triplespace_api_ingest::jobs::{self, Kind, Submission};

/// A job's row, as the actions need it.
struct JobRow {
    status: String,
    mode: Option<String>,
    actor: String,
    operator: Option<String>,
}

async fn job_row<C: GenericClient>(
    client: &C,
    tenant: &str,
    id: u64,
) -> Result<Option<JobRow>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT status, mode, actor_key, params->>'operator' FROM view.job WHERE tenant = $1 AND job_id = $2",
            &[&tenant, &i64::try_from(id).unwrap_or(i64::MAX)],
        )
        .await?;
    Ok(row.map(|r| JobRow {
        status: r.get(0),
        mode: r.get(1),
        actor: r.get(2),
        operator: r.get(3),
    }))
}

fn parse_id(id: &str) -> Result<u64, Response> {
    id.parse::<u64>().map_err(|_| {
        error(
            StatusCode::NOT_FOUND,
            "job-not-found",
            format!("`{id}` is not a job ID"),
        )
    })
}

/// Queues a revert of `reverts` as `actor`; the revert job's ID.
async fn queue_revert(
    app: &App,
    tenant: &str,
    actor: &str,
    reverts: u64,
    reason: Option<&str>,
) -> Result<u64, Response> {
    let mut params = json!({"reverts": reverts});
    if let Some(r) = reason.filter(|r| !r.is_empty()) {
        params["reason"] = json!(r);
    }
    jobs::submit(
        app.store(),
        &Submission {
            tenant: tenant.to_string(),
            kind: Kind::Revert,
            params,
            payload: None,
            actor: actor.to_string(),
        },
    )
    .await
    .map_err(|e| internal(&e))
}

/// `POST /jobs/{id}/reject`: `{reason, revert?}`; needs `ts-revertjob`.
pub async fn reject(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    if ctx.caller.require("ts-revertjob").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "rejecting a job needs the \"ts-revertjob\" right",
        );
    }
    let Some(actor) = ctx.caller.identity.actor_key.clone() else {
        return error(StatusCode::FORBIDDEN, "notloggedin", "an account is needed");
    };
    let job_id = match parse_id(&id) {
        Ok(j) => j,
        Err(r) => return r,
    };
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if reason.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "ts-reason-required",
            "a reason is required",
        );
    }
    let revert = object
        .get("revert")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tenant = ctx.tenant.slug.clone();
    let row = match job_row(ctx.db(), &tenant, job_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "job-not-found",
                format!("no job {job_id} on this wiki"),
            );
        }
        Err(e) => return internal(&e),
    };
    if row.status != "running" {
        return error(
            StatusCode::CONFLICT,
            "job-not-running",
            format!(
                "job {job_id} is {}; only a running job is rejected",
                row.status
            ),
        );
    }
    let revert_job = if revert {
        match queue_revert(&app, &tenant, &actor, job_id, Some(&reason)).await {
            Ok(j) => Some(j),
            Err(r) => return r,
        }
    } else {
        None
    };
    let store = app.store();
    let mut cx = match store.begin().await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let written = async {
        let handle = Job::handle(store, &mut cx, &tenant, job_id, Attestation::by(&actor)).await?;
        handle
            .reject_job(
                store,
                app.pipeline(),
                &mut cx,
                &reason,
                revert_job,
                now_micros(),
            )
            .await
    }
    .await;
    if let Err(e) = written {
        let _ = store.rollback(cx).await;
        return internal(&e);
    }
    if let Err(e) = store.commit(cx).await {
        return internal(&e);
    }
    let mut v = json!({"id": job_id, "status": "rejected", "reason": reason});
    if let Some(r) = revert_job {
        v["revert"] = json!(r);
    }
    ok(&headers, &v)
}

/// `POST /jobs/{id}/revert`: `{reason?}`; needs `ts-revertjob`. The revert runs under
/// the runner (0104 §4): the answer names the revert job, queued.
pub async fn revert(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    if ctx.caller.require("ts-revertjob").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reverting a job needs the \"ts-revertjob\" right",
        );
    }
    let Some(actor) = ctx.caller.identity.actor_key.clone() else {
        return error(StatusCode::FORBIDDEN, "notloggedin", "an account is needed");
    };
    let job_id = match parse_id(&id) {
        Ok(j) => j,
        Err(r) => return r,
    };
    let tenant = ctx.tenant.slug.clone();
    let row = match job_row(ctx.db(), &tenant, job_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "job-not-found",
                format!("no job {job_id} on this wiki"),
            );
        }
        Err(e) => return internal(&e),
    };
    if row.status == "running" {
        return error(
            StatusCode::CONFLICT,
            "job-running",
            format!("job {job_id} is running; reject or close it first"),
        );
    }
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|r| !r.is_empty());
    match queue_revert(&app, &tenant, &actor, job_id, reason).await {
        Ok(r) => ok(
            &headers,
            &json!({"job": r, "status": "queued", "reverts": job_id, "links": {"page": format!("/wiki/Special:Jobs/{r}")}}),
        ),
        Err(r) => r,
    }
}

/// `POST /jobs/{id}/close`: the batch's submitter, their operator, or a holder of
/// `ts-revertjob` ends a running batch.
pub async fn close(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let ctx = match crate::acl::context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = crate::acl::write_body(&app, &ctx, &headers, &body) {
        return r;
    }
    let Some(actor) = ctx.caller.identity.actor_key.clone() else {
        return error(StatusCode::FORBIDDEN, "notloggedin", "an account is needed");
    };
    let job_id = match parse_id(&id) {
        Ok(j) => j,
        Err(r) => return r,
    };
    let tenant = ctx.tenant.slug.clone();
    let row = match job_row(ctx.db(), &tenant, job_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "job-not-found",
                format!("no job {job_id} on this wiki"),
            );
        }
        Err(e) => return internal(&e),
    };
    if row.mode.as_deref() != Some("batch") {
        return error(
            StatusCode::CONFLICT,
            "job-not-batch",
            format!("job {job_id} is not a batch; only a batch is closed"),
        );
    }
    if row.status != "running" {
        return error(
            StatusCode::CONFLICT,
            "job-not-running",
            format!("job {job_id} is {}", row.status),
        );
    }
    let own = row.actor == actor
        || row.operator.as_deref() == Some(actor.as_str())
        || ctx.caller.require("ts-revertjob").is_ok();
    if !own {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "only the batch's submitter, their operator or a holder of \"ts-revertjob\" closes it",
        );
    }
    let store = app.store();
    let mut cx = match store.begin().await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let written = async {
        let handle = Job::handle(store, &mut cx, &tenant, job_id, Attestation::by(&actor)).await?;
        handle
            .close(store, app.pipeline(), &mut cx, now_micros())
            .await
    }
    .await;
    if let Err(e) = written {
        let _ = store.rollback(cx).await;
        return internal(&e);
    }
    if let Err(e) = store.commit(cx).await {
        return internal(&e);
    }
    ok(&headers, &json!({"id": job_id, "status": "closed"}))
}
