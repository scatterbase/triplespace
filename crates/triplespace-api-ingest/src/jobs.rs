//! The job queue and the runner (0104 §4, §8; 16 §2.2; 03 §4.12; 0083 §5): the jobs that
//! are not interactive, submitted as rows of `ops.job_queue` with their job ID minted at
//! submission, claimed by a runner task one running job per partition, run as the command
//! line would run them, and finished with the job's own records.
//!
//! **Submitting** ([`submit`]) mints the ID and writes the row in one transaction, so the
//! API answers with an ID that `Special:Jobs/{id}` shows `queued`. **Claiming**
//! ([`claim`]) takes the oldest row whose partition has no running job, under the
//! database's row lock (`FOR UPDATE SKIP LOCKED`), so several runner tasks, in one
//! process or several, share the queue without a coordinator. A claimed row's `heartbeat`
//! is refreshed while the job runs; a row whose heartbeat is older than [`STALE`] was a
//! runner that died, and the next claim writes `job/interrupt` under the dead job's key
//! (0011 A36) and runs the job again under a new ID, since the jobs here are idempotent
//! (an import skips what it wrote, 0008 §9). **Finishing** deletes the row; the job's
//! finish or fail record is the job's own.
//!
//! Each kind of job is a function of the row ([`Kind`]); this phase carries `import`.
//! The follower, sync and adoption jobs still run where the command line runs them.
//!
//! implements 0104 §8 (A1)

use std::time::Duration;

use scatter_ingest::IngestError;
use scatter_ingest::IngestStore as _;
use scatter_ingest::job::Job;
use scatter_ingest::store::Sequence;
use scatter_ingest::write::Attestation;
use scatter_mediawiki_xml::XmlDump;
use scatter_projection::{Backend as _, Pipeline};
use tokio_postgres::GenericClient;

use crate::PgIngest;
use crate::import::{ImportRequest, Stage, run_request};

/// How long a claimed row may go without a heartbeat before it counts as dead.
pub const STALE: Duration = Duration::from_mins(2);

/// How often a running job's heartbeat is written.
pub const HEARTBEAT: Duration = Duration::from_secs(20);

/// The kinds of job the queue carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A MediaWiki XML import (0008 §9): the file in `payload`, an [`ImportRequest`] in
    /// `params`.
    Import,
}

impl Kind {
    /// The kind's name, as `ops.job_queue.kind` holds it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Import => "import",
        }
    }

    /// The kind a name is.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "import" => Some(Self::Import),
            _ => None,
        }
    }
}

/// What to submit.
#[derive(Debug, Clone)]
pub struct Submission {
    /// The tenant; `""` for an instance job.
    pub tenant: String,
    /// The kind.
    pub kind: Kind,
    /// What the job needs, by kind.
    pub params: serde_json::Value,
    /// An uploaded file, where the kind takes one.
    pub payload: Option<Vec<u8>>,
    /// Who submitted it: the job's actor.
    pub actor: String,
}

fn store_err(e: impl std::fmt::Display) -> IngestError {
    IngestError::Store(e.to_string())
}

/// Submits a job: mints its ID and writes the queue row in one transaction. Returns the
/// job ID.
///
/// # Errors
///
/// The store's; the tenant has no `log` partition.
pub async fn submit(store: &PgIngest, s: &Submission) -> Result<u64, IngestError> {
    let mut cx = store.begin().await?;
    let partition = store
        .partition(&mut cx, &s.tenant, "log")
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: s.tenant.clone(),
            graph: "log".to_string(),
        })?;
    let id = store.next_id(&mut cx, "", &Sequence::Job).await?;
    cx.conn()
        .execute(
            "INSERT INTO ops.job_queue (tenant, job_id, partition, kind, params, payload, actor_key)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                &s.tenant,
                &i64::try_from(id).map_err(store_err)?,
                &scatter_log_postgres::ids::partition_to_db(partition),
                &s.kind.name(),
                &s.params,
                &s.payload.as_deref(),
                &s.actor,
            ],
        )
        .await
        .map_err(store_err)?;
    store.commit(cx).await?;
    Ok(id)
}

/// A queued job, as the API and the job page read it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QueuedJob {
    /// The tenant.
    pub tenant: String,
    /// The job ID.
    pub job_id: u64,
    /// The kind.
    pub kind: String,
    /// What the job needs, without any payload.
    pub params: serde_json::Value,
    /// Who submitted it.
    pub actor: String,
    /// When, in microseconds.
    pub submitted: u64,
    /// Whether a runner holds it now.
    pub running: bool,
    /// The running job's progress.
    pub progress: Option<serde_json::Value>,
    /// Its place in the queue, 1-based, among what waits; `None` once claimed.
    pub position: Option<u64>,
    /// How many times a runner has claimed it.
    pub attempts: u32,
}

fn micros(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

const QUEUED_COLUMNS: &str = "q.tenant, q.job_id, q.kind, q.params, q.actor_key, q.submitted,
       q.claimed_at IS NOT NULL AND q.heartbeat >= now() - make_interval(secs => $1::double precision), q.progress, q.attempts,
       CASE WHEN q.claimed_at IS NOT NULL AND q.heartbeat >= now() - make_interval(secs => $1::double precision) THEN NULL
            ELSE (SELECT count(*) FROM ops.job_queue w
                  WHERE w.tenant = q.tenant
                    AND (w.claimed_at IS NULL OR w.heartbeat < now() - make_interval(secs => $1::double precision))
                    AND (w.submitted, w.job_id) <= (q.submitted, q.job_id)) END";

fn queued_row(r: &tokio_postgres::Row) -> QueuedJob {
    let job_id: i64 = r.get(1);
    let submitted: std::time::SystemTime = r.get(5);
    let position: Option<i64> = r.get(9);
    let attempts: i32 = r.get(8);
    QueuedJob {
        tenant: r.get(0),
        job_id: u64::try_from(job_id).unwrap_or(0),
        kind: r.get(2),
        params: r.get(3),
        actor: r.get(4),
        submitted: micros(submitted),
        running: r.get(6),
        progress: r.get(7),
        position: position.and_then(|p| u64::try_from(p).ok()),
        attempts: u32::try_from(attempts).unwrap_or(0),
    }
}

/// The stale threshold in seconds, as the queries take it.
fn stale_interval() -> f64 {
    STALE.as_secs_f64()
}

/// A tenant's queued jobs, oldest first.
///
/// # Errors
///
/// The store's.
pub async fn queued<C: GenericClient>(
    client: &C,
    tenant: &str,
) -> Result<Vec<QueuedJob>, tokio_postgres::Error> {
    let sql = format!(
        "SELECT {QUEUED_COLUMNS} FROM ops.job_queue q WHERE q.tenant = $2
         ORDER BY q.submitted, q.job_id"
    );
    Ok(client
        .query(&sql, &[&stale_interval(), &tenant])
        .await?
        .iter()
        .map(queued_row)
        .collect())
}

/// One queued job, if it is in the queue.
///
/// # Errors
///
/// The store's.
pub async fn queued_one<C: GenericClient>(
    client: &C,
    tenant: &str,
    job_id: u64,
) -> Result<Option<QueuedJob>, tokio_postgres::Error> {
    let sql = format!(
        "SELECT {QUEUED_COLUMNS} FROM ops.job_queue q WHERE q.tenant = $2 AND q.job_id = $3"
    );
    let id = i64::try_from(job_id).unwrap_or(i64::MAX);
    Ok(client
        .query_opt(&sql, &[&stale_interval(), &tenant, &id])
        .await?
        .as_ref()
        .map(queued_row))
}

/// Removes a job that waits in the queue. A job a runner holds is not removed, and
/// `Ok(false)` says so; nor is one that is not queued.
///
/// # Errors
///
/// The store's.
pub async fn cancel<C: GenericClient>(
    client: &C,
    tenant: &str,
    job_id: u64,
) -> Result<bool, tokio_postgres::Error> {
    let id = i64::try_from(job_id).unwrap_or(i64::MAX);
    let n = client
        .execute(
            "DELETE FROM ops.job_queue WHERE tenant = $1 AND job_id = $2
               AND (claimed_at IS NULL OR heartbeat < now() - make_interval(secs => $3::double precision))",
            &[&tenant, &id, &stale_interval()],
        )
        .await?;
    Ok(n > 0)
}

/// A claimed row: what the runner runs.
#[derive(Debug, Clone)]
pub struct Claimed {
    /// The tenant.
    pub tenant: String,
    /// The job ID the job runs under.
    pub job_id: u64,
    /// The kind.
    pub kind: Kind,
    /// What the job needs.
    pub params: serde_json::Value,
    /// The payload.
    pub payload: Option<Vec<u8>>,
    /// The job's actor.
    pub actor: String,
    /// The job ID an earlier runner ran it under before it died, if any: marked
    /// interrupted by this claim.
    pub resumes: Option<u64>,
}

/// Claims the oldest job whose partition has no running job, for `runner`. A row whose
/// claim went stale is claimed again under a new job ID, and the dead run is marked
/// `job/interrupt` where it had written its start record.
///
/// # Errors
///
/// The store's.
pub async fn claim(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    runner: &str,
    now: u64,
) -> Result<Option<Claimed>, IngestError> {
    let mut cx = store.begin().await?;
    let stale = stale_interval();
    let row = cx
        .conn()
        .query_opt(
            "SELECT q.tenant, q.job_id, q.kind, q.params, q.payload, q.actor_key, q.claimed_at IS NOT NULL
             FROM ops.job_queue q
             WHERE (q.claimed_at IS NULL OR q.heartbeat < now() - make_interval(secs => $1::double precision))
               AND NOT q.cancel
               AND NOT EXISTS (SELECT 1 FROM ops.job_queue r
                               WHERE r.partition = q.partition AND r.job_id <> q.job_id
                                 AND r.claimed_at IS NOT NULL AND r.heartbeat >= now() - make_interval(secs => $1::double precision))
             ORDER BY q.submitted, q.job_id
             LIMIT 1
             FOR UPDATE OF q SKIP LOCKED",
            &[&stale],
        )
        .await
        .map_err(store_err)?;
    let Some(row) = row else {
        store.rollback(cx).await?;
        return Ok(None);
    };
    let tenant: String = row.get(0);
    let old_id: i64 = row.get(1);
    let kind_name: String = row.get(2);
    let Some(kind) = Kind::parse(&kind_name) else {
        // A kind this build does not run: left for one that does, never claimed here.
        store.rollback(cx).await?;
        return Ok(None);
    };
    let params: serde_json::Value = row.get(3);
    let payload: Option<Vec<u8>> = row.get(4);
    let actor: String = row.get(5);
    let was_claimed: bool = row.get(6);
    let old = u64::try_from(old_id).map_err(store_err)?;
    let (job_id, resumes) = if was_claimed {
        // The earlier runner died. Its job is interrupted where it had started, and the
        // work runs again under a new ID (0104 §4).
        let started: bool = cx
            .conn()
            .query_opt(
                "SELECT 1 FROM view.job WHERE tenant = $1 AND job_id = $2 AND status = 'running'",
                &[&tenant, &old_id],
            )
            .await
            .map_err(store_err)?
            .is_some();
        let new_id = store.next_id(&mut cx, "", &Sequence::Job).await?;
        if started {
            let handle = Job::handle(store, &mut cx, &tenant, old, Attestation::by(&actor)).await?;
            handle
                .interrupt(store, pipeline, &mut cx, new_id, now)
                .await?;
        }
        cx.conn()
            .execute(
                "UPDATE ops.job_queue SET job_id = $3, claimed_by = $4, claimed_at = now(), heartbeat = now(),
                        attempts = attempts + 1, progress = NULL
                 WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &old_id, &i64::try_from(new_id).map_err(store_err)?, &runner],
            )
            .await
            .map_err(store_err)?;
        (new_id, Some(old))
    } else {
        cx.conn()
            .execute(
                "UPDATE ops.job_queue SET claimed_by = $3, claimed_at = now(), heartbeat = now(),
                        attempts = attempts + 1
                 WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &old_id, &runner],
            )
            .await
            .map_err(store_err)?;
        (old, None)
    };
    store.commit(cx).await?;
    Ok(Some(Claimed {
        tenant,
        job_id,
        kind,
        params,
        payload,
        actor,
        resumes,
    }))
}

/// Writes a claimed row's heartbeat and progress.
async fn beat<C: GenericClient>(
    client: &C,
    tenant: &str,
    job_id: u64,
    progress: Option<&serde_json::Value>,
) -> Result<(), tokio_postgres::Error> {
    let id = i64::try_from(job_id).unwrap_or(i64::MAX);
    match progress {
        Some(p) => client
            .execute(
                "UPDATE ops.job_queue SET heartbeat = now(), progress = $3 WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &id, p],
            )
            .await
            .map(|_| ()),
        None => client
            .execute(
                "UPDATE ops.job_queue SET heartbeat = now() WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &id],
            )
            .await
            .map(|_| ()),
    }
}

/// Deletes a finished job's row.
async fn finish<C: GenericClient>(
    client: &C,
    tenant: &str,
    job_id: u64,
) -> Result<(), tokio_postgres::Error> {
    let id = i64::try_from(job_id).unwrap_or(i64::MAX);
    client
        .execute(
            "DELETE FROM ops.job_queue WHERE tenant = $1 AND job_id = $2",
            &[&tenant, &id],
        )
        .await
        .map(|_| ())
}

/// How a run ended, for the runner's log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The tenant.
    pub tenant: String,
    /// The job ID.
    pub job_id: u64,
    /// The kind.
    pub kind: Kind,
    /// The error, where the job failed.
    pub error: Option<String>,
}

/// Runs a claimed job to its end: the kind's work under the job's ID, with the heartbeat
/// written as it goes, then the row deleted. A job that fails gets its `job/fail` record
/// where it had started, and its row is deleted too: the jobs here fail on their input,
/// not on the weather, so nothing is retried.
///
/// # Errors
///
/// The store's, where the queue itself could not be written; a job's own failure is in
/// the result.
pub async fn run_claimed(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    claimed: Claimed,
    now: fn() -> u64,
) -> Result<Ran, IngestError> {
    let pool = store.backend().pool().clone();
    let tenant = claimed.tenant.clone();
    let job_id = claimed.job_id;
    // The heartbeat, from a connection of its own, until the job ends.
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let beating = {
        let pool = pool.clone();
        let tenant = tenant.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(HEARTBEAT);
            every.tick().await;
            loop {
                tokio::select! {
                    _ = every.tick() => {
                        if let Ok(client) = pool.get().await {
                            let _ = beat(&**client, &tenant, job_id, None).await;
                        }
                    }
                    r = stopped.changed() => {
                        if r.is_err() || *stopped.borrow() { break; }
                    }
                }
            }
        })
    };
    let result = run_kind(store, pipeline, &claimed, now).await;
    let _ = stop.send(true);
    let _ = beating.await;
    let error = match result {
        Ok(()) => None,
        Err(e) => {
            let message = e.to_string();
            // The job's own failure record, where it had started (0011 §6.3).
            let mut cx = store.begin().await?;
            let started: bool = cx
                .conn()
                .query_opt(
                    "SELECT 1 FROM view.job WHERE tenant = $1 AND job_id = $2 AND status = 'running'",
                    &[&tenant, &i64::try_from(job_id).map_err(store_err)?],
                )
                .await
                .map_err(store_err)?
                .is_some();
            if started {
                let handle = Job::handle(
                    store,
                    &mut cx,
                    &tenant,
                    job_id,
                    Attestation::by(&claimed.actor),
                )
                .await?;
                handle
                    .fail(store, pipeline, &mut cx, &message, now())
                    .await?;
            }
            store.commit(cx).await?;
            Some(message)
        }
    };
    let client = pool.get().await.map_err(store_err)?;
    finish(&**client, &tenant, job_id)
        .await
        .map_err(store_err)?;
    Ok(Ran {
        tenant,
        job_id,
        kind: claimed.kind,
        error,
    })
}

/// The kind's work.
async fn run_kind(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    claimed: &Claimed,
    now: fn() -> u64,
) -> Result<(), IngestError> {
    match claimed.kind {
        Kind::Import => {
            let request: ImportRequest =
                serde_json::from_value(claimed.params.clone()).map_err(store_err)?;
            let bytes = claimed
                .payload
                .as_deref()
                .ok_or_else(|| store_err("an import job carries its file"))?;
            let mut pages = Vec::new();
            for page in XmlDump::new(bytes) {
                pages.push(page.map_err(|e| {
                    store_err(format!("the file is not a MediaWiki XML export: {e}"))
                })?);
            }
            let total = pages.len();
            let pool = store.backend().pool().clone();
            let client = pool.get().await.map_err(store_err)?;
            let tenant = claimed.tenant.clone();
            let job_id = claimed.job_id;
            let progress_pool = pool.clone();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Stage>();
            let reporter = tokio::spawn(async move {
                while let Some(stage) = rx.recv().await {
                    let p = serde_json::json!({"stage": stage.name(), "pages": total});
                    if let Ok(c) = progress_pool.get().await {
                        let _ = beat(&**c, &tenant, job_id, Some(&p)).await;
                    }
                }
            });
            let mut progress = move |stage: Stage| {
                let _ = tx.send(stage);
            };
            let result = run_request(
                store,
                pipeline,
                &**client,
                &claimed.tenant,
                &claimed.actor,
                &request,
                Some(claimed.job_id),
                pages,
                now(),
                &mut progress,
            )
            .await;
            drop(progress);
            let _ = reporter.await;
            result.map(|_| ())
        }
    }
}

/// One pass of a runner: claims a job and runs it. `Ok(None)` when nothing waits.
///
/// # Errors
///
/// The store's.
pub async fn run_once(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    runner: &str,
    now: fn() -> u64,
) -> Result<Option<Ran>, IngestError> {
    match claim(store, pipeline, runner, now()).await? {
        Some(claimed) => Ok(Some(run_claimed(store, pipeline, claimed, now).await?)),
        None => Ok(None),
    }
}

/// Whether the runner is paused (`jobs.paused`, an instance setting): nothing is claimed
/// while it is, and what is queued waits.
///
/// # Errors
///
/// The settings' store error.
pub async fn paused<C: GenericClient>(client: &C) -> Result<bool, String> {
    let (settings, _) = triplespace_accounts::settings::load(client, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(settings
        .value("jobs.paused")
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

/// Runs the runner until the process ends: a pass whenever a job waits, a wait of `idle`
/// when none does, and nothing while paused.
pub async fn run_forever(
    store: PgIngest,
    pipeline: Pipeline<PgIngest>,
    runner: String,
    idle: Duration,
    now: fn() -> u64,
) {
    loop {
        let paused = match store.backend().pool().get().await {
            Ok(client) => paused(&**client).await.unwrap_or(false),
            Err(_) => false,
        };
        if paused {
            tokio::time::sleep(idle).await;
            continue;
        }
        match run_once(&store, &pipeline, &runner, now).await {
            Ok(Some(ran)) => match ran.error {
                None => eprintln!(
                    "{runner}: job {} ({}) for `{}` finished",
                    ran.job_id,
                    ran.kind.name(),
                    ran.tenant
                ),
                Some(e) => eprintln!(
                    "{runner}: job {} ({}) for `{}` failed: {e}",
                    ran.job_id,
                    ran.kind.name(),
                    ran.tenant
                ),
            },
            Ok(None) => tokio::time::sleep(idle).await,
            Err(e) => {
                eprintln!("{runner}: {e}");
                tokio::time::sleep(idle).await;
            }
        }
    }
}
