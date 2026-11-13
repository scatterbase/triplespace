//! The export job (21 §4.2; 0104 A9) and `GET /export/{id}`: an export over
//! `export.sync_revisions`, queued for the runner, its file kept in the blob store for
//! the account that asked.
//!
//! **Queuing** (`queue`). `POST /export` hands a selection over the limit here. Where
//! the caller is signed in and this server keeps a blob store, the request is submitted
//! as an `export` job, its `params` the request body with `format` written out, and the
//! answer is `202` with `{"job", "status": "queued", "url"}`, `url` being where the file
//! is fetched from. An anonymous caller has nobody to keep a file for, and a server
//! without a blob store nowhere to keep it: both are still refused `413
//! export-too-large`.
//!
//! **Running** ([`ExportWork`], which the process gives its runner for
//! [`Kind::Export`]). The job reads the
//! stored request again with the rights its submitter holds when it runs, not those held
//! when it was queued, and with no revision limit; `export.history`, `export.max_history`
//! and the other export settings hold as they do in place. MediaWiki XML is streamed into
//! the blob store at `{tenant}/job/{job_id}/export.xml` while it is written, a chunk at a
//! time, the document never held in memory; the `records` format, which is one CBOR
//! value, is built and written at `{tenant}/job/{job_id}/export.cbor`. The job's finish
//! record names the file: `export: {format, file, bytes, missing, notices}`. A failure
//! leaves nothing at the key.
//!
//! **Keeping** (`export.keep_days`). Each export job first removes every object under the
//! tenant's `job/` prefix written more than `export.keep_days` days ago, and
//! `GET /export/{id}` treats a file of that age as gone, removing it, so that a wiki
//! where no further export runs still forgets its files.
//!
//! **Fetching** ([`get`]). Only the account that asked may fetch the file, or learn that
//! the job exists: any other caller, an anonymous one included, is answered `404
//! export-not-found`, as for an ID that is no export job. The submitter is answered `202`
//! with the status while the job waits or runs, `200` with `status: "failed"` and the
//! error where it failed, `200` with the file where it finished, and `410 export-expired`
//! once the file is gone.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::body::{Body, Bytes};
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt as _;
use scatter_actors::Effective;
use scatter_blob::{BlobError, BlobStore, Key, Prefix, Scope};
use scatter_ingest::IngestError;
use scatter_ingest::job::{Counts, Job};
use scatter_ingest::write::Attestation;
use scatter_projection::Backend as _;
use scatter_wikibase_changeset::job::JobHeader;
use serde_json::{Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::Caller;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::tenant::Tenant;
use triplespace_api_ingest::jobs::{self, Claimed, Kind, Outcome, Submission, Work, WorkFuture};
use triplespace_rdf::records::{BuildError, MEDIA_TYPE};
use triplespace_rdf::xml;

use crate::entity_route::{error, internal};
use crate::export::{For, Prepared, format_of, prepare, setting_usize, too_large};
use crate::settings::prelude;

/// The registry default of `export.keep_days`.
const KEEP_DAYS: usize = 7;

/// How many chunks may wait for the blob store: the export's writer stops reading the
/// database while this many are unwritten.
const WAITING_CHUNKS: usize = 2;

/// The media type of the XML file.
const XML_TYPE: &str = "application/xml; charset=utf-8";

fn failed(e: impl std::fmt::Display) -> IngestError {
    IngestError::Store(e.to_string())
}

/// The name of a job's file, by format.
fn file_name(format: &str) -> &'static str {
    if format == "records" {
        "export.cbor"
    } else {
        "export.xml"
    }
}

/// Where a job's file is fetched from.
fn url_of(tenant: &Tenant, job: u64) -> String {
    format!(
        "{}{}/export/{job}",
        tenant.public_base.trim_end_matches('/'),
        crate::BASES[0]
    )
}

fn json_answer(status: StatusCode, body: &Value) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        body.to_string(),
    )
        .into_response()
}

/// How many export jobs one account may have queued or running on a tenant at once.
pub const MAX_QUEUED: i64 = 5;

/// Answers a selection over `export.sync_revisions`: queued as an export job for a
/// signed-in caller where this server keeps a blob store, refused otherwise.
pub(crate) async fn queue(
    app: &App,
    tenant: &Tenant,
    caller: &Caller,
    mut request: Value,
    over: &BuildError,
) -> Response {
    let Some(actor) = caller.identity.actor_key.clone() else {
        return too_large(
            over,
            "sign in and an export this large is queued as a job, or narrow the selection",
        );
    };
    if app.blobs().is_none() {
        return too_large(
            over,
            "this server keeps no job output, so an export this large cannot be queued: narrow the selection",
        );
    }
    // No more than a few at once for one account: queuing an export costs its
    // submitter nothing, and each is a long read of the database.
    let waiting = match app.pool().get().await {
        Ok(client) => client
            .query_one(
                "SELECT count(*) FROM ops.job_queue
                 WHERE tenant = $1 AND kind = 'export' AND actor_key = $2",
                &[&tenant.slug, &actor],
            )
            .await
            .map(|r| r.get::<_, i64>(0)),
        Err(e) => return internal(&e),
    };
    match waiting {
        Ok(n) if n >= MAX_QUEUED => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "export-queue-full",
                format!(
                    "{MAX_QUEUED} of your exports are already queued or running; wait for one to finish"
                ),
            );
        }
        Ok(_) => {}
        Err(e) => return internal(&e),
    }
    // The format written out, so that the job's record and a reader of the queue need
    // not know the default.
    let format = format_of(&request).unwrap_or("xml");
    if let Some(object) = request.as_object_mut() {
        object.insert("format".into(), format.into());
    }
    let submitted = jobs::submit(
        app.store(),
        &Submission {
            tenant: tenant.slug.clone(),
            kind: Kind::Export,
            params: request,
            payload: None,
            actor,
        },
    )
    .await;
    let job = match submitted {
        Ok(id) => id,
        Err(e) => return internal(&e),
    };
    let url = url_of(tenant, job);
    let mut r = json_answer(
        StatusCode::ACCEPTED,
        &json!({"job": job, "status": "queued", "url": url}),
    );
    if let Ok(v) = header::HeaderValue::from_str(&url) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

/// The work of an export job: what a runner is given for
/// [`Kind::Export`].
#[derive(Clone)]
pub struct ExportWork {
    app: App,
}

/// The export job's work over `app`, whose blob store ([`App::set_blobs`]) the files go
/// to: `Runner::with_work(Kind::Export, export_work(app))`.
#[must_use]
pub fn export_work(app: App) -> Arc<dyn Work> {
    Arc::new(ExportWork { app })
}

impl Work for ExportWork {
    fn run<'a>(&'a self, job: &'a Claimed, now: fn() -> u64) -> WorkFuture<'a> {
        Box::pin(run(&self.app, job, now))
    }
}

/// `export.keep_days` of a tenant, as a duration.
async fn keep(client: &tokio_postgres::Client, tenant: &str) -> Result<Duration, String> {
    let (snap, _) = triplespace_accounts::settings::load(client, Some(tenant))
        .await
        .map_err(|e| e.to_string())?;
    let days = u64::try_from(setting_usize(&snap, "export.keep_days", KEEP_DAYS)).unwrap_or(0);
    Ok(Duration::from_hours(days.saturating_mul(24)))
}

/// Whether an object written at `written` is past its keeping.
fn expired(written: SystemTime, keep: Duration) -> bool {
    SystemTime::now()
        .duration_since(written)
        .is_ok_and(|age| age > keep)
}

/// The retention sweep (0104 A9): removes every job output of the scope written more
/// than `keep` ago. Returns how many it removed.
///
/// # Errors
///
/// The blob store's.
pub async fn sweep(blobs: &BlobStore, scope: &Scope, keep: Duration) -> Result<u64, BlobError> {
    let mut listing = blobs.list(&Prefix::jobs(scope));
    let mut old = Vec::new();
    while let Some(entry) = listing.next().await {
        let entry = entry?;
        if expired(entry.meta.last_modified, keep)
            && let Some(key) = entry.key
        {
            old.push(key);
        }
    }
    for key in &old {
        blobs.delete(key).await?;
    }
    Ok(old.len() as u64)
}

/// Streams a planned XML export into the blob store at `key`: the writer hands each
/// chunk to the store's multipart write through a bounded channel, and reads no further
/// while [`WAITING_CHUNKS`] are unwritten. A failure of either side ends the other, and
/// the write is aborted. Returns the bytes written.
async fn store_xml(
    blobs: &BlobStore,
    key: &Key,
    client: &tokio_postgres::Client,
    plan: &xml::Plan,
) -> Result<u64, IngestError> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(WAITING_CHUNKS);
    let source = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|chunk| (chunk, rx))
    });
    let writing = async move {
        let written = xml::write_chunks(client, plan, xml::CHUNK, |chunk| {
            let tx = tx.clone();
            async move {
                tx.send(Ok(Bytes::from(chunk)))
                    .await
                    .map_err(|_| BuildError::Io("the blob store stopped taking the export".into()))
            }
        })
        .await;
        // A document that failed part-way fails the write: nothing is left at the key.
        if let Err(e) = &written {
            let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
        }
        written
        // `tx` is dropped here, which ends the stream.
    };
    let (written, stored) = tokio::join!(writing, blobs.put_stream_at(key, source));
    match (written, stored) {
        (Ok(()), Ok(bytes)) => Ok(bytes),
        // The store's own failure is the cause where the writer only saw it stop.
        (_, Err(e)) if !matches!(e, BlobError::Source(_)) => Err(failed(e)),
        (Err(e), _) => Err(failed(e)),
        (Ok(()), Err(e)) => Err(failed(e)),
    }
}

/// The tenant's registered base and what the job's submitter may do now: an account
/// since removed, or one that may no longer read, gets no export.
async fn submitter(
    client: &tokio_postgres::Client,
    tenant: &str,
    actor: &str,
) -> Result<(String, Effective), IngestError> {
    let base = triplespace_api_action::tenant::registered(client)
        .await
        .map_err(failed)?
        .into_iter()
        .find(|t| t.slug == tenant)
        .map(|t| t.base)
        .ok_or_else(|| failed(format!("no tenant `{tenant}`")))?;
    let effective = triplespace_accounts::identity::effective_of(client, tenant, actor)
        .await
        .map_err(failed)?
        .ok_or_else(|| {
            failed(format!(
                "the account that asked for the export, `{actor}`, is no longer an account here"
            ))
        })?;
    if !effective.holds("read") {
        return Err(failed(
            "the account that asked for the export may no longer read this wiki",
        ));
    }
    Ok((base, effective))
}

/// Runs a claimed export job.
async fn run(app: &App, job: &Claimed, now: fn() -> u64) -> Result<Outcome, IngestError> {
    let blobs = app
        .blobs()
        .ok_or_else(|| failed("this server keeps no blob store for an export's file"))?;
    let client = app.pool().get().await.map_err(failed)?;
    let client: &tokio_postgres::Client = &client;
    let tenant = job.tenant.as_str();
    let (base, effective) = submitter(client, tenant, &job.actor).await?;
    let scope = Scope::tenant(tenant).map_err(failed)?;
    let format = format_of(&job.params).map_err(failed)?;

    let (store, pipeline) = (app.store(), app.pipeline());
    let mut header = JobHeader::local("export");
    header.graph = "pages".into();
    header.params.insert("format".into(), format.into());
    let mut cx = store.begin().await?;
    let record = Job::start_as(
        store,
        pipeline,
        &mut cx,
        tenant,
        job.job_id,
        header,
        Attestation::by(&job.actor),
        now(),
    )
    .await?;
    store.commit(cx).await?;

    // Retention first: what this tenant's earlier jobs left past `export.keep_days`.
    let keep = keep(client, tenant).await.map_err(failed)?;
    sweep(blobs, &scope, keep).await.map_err(failed)?;

    let who = For {
        tenant,
        base: &base,
        effective,
    };
    let prepared = Box::pin(prepare(
        app,
        client,
        who,
        &job.params,
        Some(usize::MAX),
        now(),
    ))
    .await
    .map_err(failed)?;
    let file = file_name(format);
    let key = Key::job(scope, job.job_id, file).map_err(failed)?;
    let (bytes, missing, notices) = match prepared {
        Prepared::Xml { plan, missing } => {
            let bytes = Box::pin(store_xml(blobs, &key, client, &plan)).await?;
            (bytes, missing.len(), plan.notices)
        }
        Prepared::Records(export) => {
            let encoded = export.encode();
            let bytes = encoded.len() as u64;
            blobs
                .put_at(&key, Bytes::from(encoded))
                .await
                .map_err(failed)?;
            (bytes, export.missing.len(), Vec::new())
        }
    };

    let mut extra = serde_json::Map::new();
    extra.insert(
        "export".into(),
        json!({
            "format": format,
            "file": file,
            "bytes": bytes,
            "missing": missing,
            "notices": notices,
        }),
    );
    let mut cx = store.begin().await?;
    let finished = record
        .finish(store, pipeline, &mut cx, &Counts::default(), extra, now())
        .await;
    if let Err(e) = finished {
        // No record names the file, so nothing would fetch it: it is not kept.
        let _ = blobs.delete(&key).await;
        return Err(e);
    }
    store.commit(cx).await?;
    Ok(Outcome::Done)
}

fn not_found(id: &str) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "export-not-found",
        format!("no export job `{id}` of yours on this wiki"),
    )
}

fn expired_answer(job: u64) -> Response {
    error(
        StatusCode::GONE,
        "export-expired",
        format!(
            "the file of export job {job} is no longer kept (`export.keep_days`); ask for the export again"
        ),
    )
}

/// `GET /export/{id}`: an export job's state and, when it is ready, its file, for the
/// account that asked for it and nobody else.
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
    let client: &tokio_postgres::Client = &client;
    // Whatever is wrong, the answer is the same: only the submitter learns of the job.
    let (Some(blobs), Some(actor), Ok(job)) = (
        app.blobs(),
        caller.identity.actor_key.as_deref(),
        id.parse::<u64>(),
    ) else {
        return not_found(&id);
    };
    let Ok(id_db) = i64::try_from(job) else {
        return not_found(&id);
    };
    // While it waits or runs, the queue row says whose it is.
    match jobs::queued_one(client, &tenant.slug, job).await {
        Ok(Some(q)) => {
            if q.kind != Kind::Export.name() || q.actor != actor {
                return not_found(&id);
            }
            let status = if q.running { "running" } else { "queued" };
            return json_answer(StatusCode::ACCEPTED, &json!({"job": job, "status": status}));
        }
        Ok(None) => {}
        Err(e) => return internal(&e),
    }
    let row = match client
        .query_opt(
            "SELECT source, actor_key, status, counts, params->>'format' FROM view.job WHERE tenant = $1 AND job_id = $2",
            &[&tenant.slug, &id_db],
        )
        .await
    {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let Some(row) = row else {
        return not_found(&id);
    };
    let (source, owner, status): (String, String, String) = (row.get(0), row.get(1), row.get(2));
    let counts: Option<Value> = row.get(3);
    let format: Option<String> = row.get(4);
    if source != Kind::Export.name() || owner != actor {
        return not_found(&id);
    }
    let detail = |field: &str| {
        counts
            .as_ref()
            .and_then(|c| c.get(field))
            .cloned()
            .unwrap_or(Value::Null)
    };
    match status.as_str() {
        "finished" => {}
        // Between its last record and its row's removal, or the reverse.
        "running" => {
            return json_answer(
                StatusCode::ACCEPTED,
                &json!({"job": job, "status": "running"}),
            );
        }
        "failed" => {
            return json_answer(
                StatusCode::OK,
                &json!({"job": job, "status": "failed", "error": detail("error")}),
            );
        }
        // A run whose runner died: the work goes on under the job named.
        "interrupted" => {
            let by = detail("by");
            let mut v = json!({"job": job, "status": "interrupted", "interrupted_by": by});
            if let Some(next) = by.as_u64() {
                v["url"] = json!(url_of(&tenant, next));
            }
            return json_answer(StatusCode::OK, &v);
        }
        other => return json_answer(StatusCode::OK, &json!({"job": job, "status": other})),
    }
    // The format is in the job's start record, which names the file.
    let format = if format.as_deref() == Some("records") {
        "records"
    } else {
        "xml"
    };
    file_answer(blobs, client, &tenant.slug, job, format).await
}

/// The file of a finished export job, or `410 export-expired` where it is no longer
/// kept: removed by a sweep, or past `export.keep_days` and removed here.
async fn file_answer(
    blobs: &BlobStore,
    client: &tokio_postgres::Client,
    tenant: &str,
    job: u64,
    format: &str,
) -> Response {
    let Ok(key) = Scope::tenant(tenant).and_then(|s| Key::job(s, job, file_name(format))) else {
        return not_found(&job.to_string());
    };
    // A file past `export.keep_days` is gone, whether or not a sweep has met it yet.
    let keep = match keep(client, tenant).await {
        Ok(k) => k,
        Err(e) => return internal(&e),
    };
    match blobs.head(&key).await {
        Ok(Some(meta)) if expired(meta.last_modified, keep) => {
            let _ = blobs.delete(&key).await;
            return expired_answer(job);
        }
        Ok(Some(_)) => {}
        Ok(None) => return expired_answer(job),
        Err(e) => return internal(&e),
    }
    let reader = match blobs.read(&key, None).await {
        Ok(r) => r,
        Err(e) if e.is_not_found() => return expired_answer(job),
        Err(e) => return internal(&e),
    };
    let (content_type, extension) = if format == "records" {
        (MEDIA_TYPE, "cbor")
    } else {
        (XML_TYPE, "xml")
    };
    let disposition = format!("attachment; filename=\"{tenant}-{job}.{extension}\"");
    let mut r = Response::new(Body::from_stream(reader.stream));
    let h = r.headers_mut();
    if let Ok(v) = header::HeaderValue::from_str(content_type) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    h.insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from(reader.size),
    );
    if let Ok(v) = header::HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0104 A9: a job's file is past its keeping once it is older than `export.keep_days`.
    #[test]
    fn a_file_expires_after_its_keeping() {
        let week = Duration::from_hours(7 * 24);
        let now = SystemTime::now();
        assert!(!expired(now, week));
        assert!(!expired(now - Duration::from_hours(6 * 24), week));
        assert!(expired(now - Duration::from_hours(8 * 24), week));
        // A clock that ran backwards expires nothing.
        assert!(!expired(now + Duration::from_mins(1), week));
    }

    #[test]
    fn the_file_is_named_by_format() {
        assert_eq!(file_name("xml"), "export.xml");
        assert_eq!(file_name("records"), "export.cbor");
    }
}
