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
        _ => {}
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
