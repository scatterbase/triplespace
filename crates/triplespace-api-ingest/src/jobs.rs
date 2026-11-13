//! The job queue and the runner (0104 §4, §8; 16 §2.2; 03 §4.12; 0083 §5): the jobs that
//! are not interactive, submitted as rows of `ops.job_queue` with their job ID minted at
//! submission, claimed by a runner task one running job per partition, run as the command
//! line would run them, and finished with the job's own records.
//!
//! **Submitting** ([`submit`]) mints the ID and writes the row in one transaction, so the
//! API answers with an ID that `Special:Jobs/{id}` shows `queued`. **Claiming**
//! ([`claim`]) takes the oldest row whose partition has no running job, under the
//! database's row lock (`FOR UPDATE SKIP LOCKED`) and, so that two claimers never decide
//! for one partition at once, a transaction's advisory lock on the partition
//! ([`PARTITION_LOCK`]; 0104 A7): several runner tasks, in one process or several, share
//! the queue without a coordinator, and one that finds a partition taken moves on to the
//! next. A claimed row's `heartbeat` is refreshed while the job runs; a row whose
//! heartbeat is older than [`STALE`] was a runner that died, and the next claim writes
//! `job/interrupt` under the dead job's key (0011 A36) and runs the job again under a new
//! ID, since the jobs here are idempotent (an import skips what it wrote, 0008 §9), until
//! it has been claimed `jobs.max_attempts` times: then it is failed, not run (A7).
//! **Finishing** deletes the row; the job's finish or fail record is the job's own, and a
//! job that fails before its start record gets both, so that an ID once answered is
//! always a job (A7). A runner whose claim was taken from it, its heartbeat having
//! stalled, writes nothing more about the row.
//!
//! Each kind of job is a function of the row ([`Kind`]): `import`, `revert` (0104 §3) and
//! `dump` (0104 A6). An export over `export.sync_revisions` and the follower, sync and
//! adoption jobs are not carried yet: the last three still run where the command line
//! runs them. A [`Runner`] names the kinds it runs, and claims no other: a row of a kind
//! no runner here runs waits, and holds nothing else back.
//!
//! implements 0104 §8 (A1)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use scatter_actors::{GrantRegistry, Principal, evaluate};
use scatter_ingest::IngestError;
use scatter_ingest::IngestStore as _;
use scatter_ingest::job::{Counts, Job};
use scatter_ingest::store::Sequence;
use scatter_ingest::write::Attestation;
use scatter_mediawiki_xml::XmlDump;
use scatter_projection::{Backend as _, Pipeline};
use scatter_wikibase_changeset::job::{JobHeader, Mode};
use tokio_postgres::GenericClient;
use triplespace_projections::PgCx;
use triplespace_projections::entities::EntityProjection;

use crate::PgIngest;
use crate::import::{ImportRequest, Stage, run_request};

/// How long a claimed row may go without a heartbeat before it counts as dead.
pub const STALE: Duration = Duration::from_mins(2);

/// How often a running job's heartbeat is written.
pub const HEARTBEAT: Duration = Duration::from_secs(20);

/// The registry default of `jobs.max_attempts` (0104 A7): how many times a job is claimed
/// before a stale claim on it is failed in place of another run.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// Takes the claim lock of the partition `$1` for the transaction, or answers `false`
/// where another claimer holds it (0104 A7). A claimer decides for a partition only
/// under it, so the claim another has made of the partition and not yet committed is
/// never missed: the lock is released by that commit, and the statement after sees it.
pub const PARTITION_LOCK: &str =
    "SELECT pg_try_advisory_xact_lock(hashtext('ops.job_queue'), hashint8($1))";

/// The kinds of job the queue carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A MediaWiki XML import (0008 §9): the file in `payload`, an [`ImportRequest`] in
    /// `params`.
    Import,
    /// A revert of another job (0104 §3): `reverts`, the job, and `reason` in `params`.
    Revert,
    /// A tenant's dumps (02 §4.1; 0013 A78), written into the runner's dumps directory as
    /// `triplespace dump` writes them; `keep` in `params` where it is not the tenant's
    /// `dumps.keep`.
    Dump,
}

impl Kind {
    /// The kind's name, as `ops.job_queue.kind` holds it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Revert => "revert",
            Self::Dump => "dump",
        }
    }

    /// The kind a name is.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "import" => Some(Self::Import),
            "revert" => Some(Self::Revert),
            "dump" => Some(Self::Dump),
            _ => None,
        }
    }
}

/// A runner: its name in the queue's `claimed_by`, the kinds it runs, and what those kinds
/// need of the process (0104 A6).
#[derive(Debug, Clone)]
pub struct Runner {
    /// The name.
    pub name: String,
    /// The kinds it claims.
    pub kinds: Vec<Kind>,
    /// The dumps directory (`dumps.dir`), for [`Kind::Dump`].
    pub dumps_dir: Option<PathBuf>,
    /// How often a running job's heartbeat is written: [`HEARTBEAT`], but for a test
    /// that cannot wait for it.
    pub heartbeat: Duration,
}

impl Runner {
    /// A runner of the kinds that need nothing of the process: imports and reverts.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kinds: vec![Kind::Import, Kind::Revert],
            dumps_dir: None,
            heartbeat: HEARTBEAT,
        }
    }

    /// Also runs dumps, into `dir`.
    #[must_use]
    pub fn with_dumps(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dumps_dir = Some(dir.into());
        if !self.kinds.contains(&Kind::Dump) {
            self.kinds.push(Kind::Dump);
        }
        self
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
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
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

/// The header of a job's start record where the runner writes it for the kind: a dump's
/// own, and that of a job that ended before the kind's work wrote its start (0104 A7),
/// with what the queue row says of it and nothing the work would have found out.
fn header_of(kind: Kind, params: &serde_json::Value) -> JobHeader {
    let text = |field: &str| {
        params
            .get(field)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
    };
    match kind {
        Kind::Import => {
            let mut header = JobHeader::local(text("source").unwrap_or("upload"));
            header.version = text("version").map(str::to_string);
            header.mode = Some(Mode::Import);
            header.graph = "pages".into();
            if let Some(summary) = text("summary") {
                header.params.insert("summary".into(), summary.into());
            }
            header
        }
        Kind::Revert => {
            let mut header = JobHeader::local("revert");
            header.mode = Some(Mode::Revert);
            if let Some(reverts) = params.get("reverts").and_then(serde_json::Value::as_u64) {
                header.params.insert("reverts".into(), reverts.into());
            }
            if let Some(reason) = text("reason") {
                header.params.insert("reason".into(), reason.into());
            }
            header
        }
        Kind::Dump => {
            let mut header = JobHeader::local("dump");
            header.graph = "local".into();
            header
        }
    }
}

/// The job's status in `view.job`; `None` where it has no start record.
async fn status_of<C: GenericClient>(
    client: &C,
    tenant: &str,
    job_id: u64,
) -> Result<Option<String>, IngestError> {
    Ok(client
        .query_opt(
            "SELECT status FROM view.job WHERE tenant = $1 AND job_id = $2",
            &[&tenant, &i64::try_from(job_id).map_err(store_err)?],
        )
        .await
        .map_err(store_err)?
        .map(|r| r.get(0)))
}

/// Ends job `job_id` as failed with `error`: its `job/fail` record, after the start
/// record where it had none (0104 A7). A job that already ended is left as it ended.
#[allow(clippy::too_many_arguments)]
async fn fail_job(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    job_id: u64,
    kind: Kind,
    params: &serde_json::Value,
    actor: &str,
    error: &str,
    now: u64,
) -> Result<(), IngestError> {
    let job = match status_of(cx.conn(), tenant, job_id).await?.as_deref() {
        None => {
            Job::start_as(
                store,
                pipeline,
                cx,
                tenant,
                job_id,
                header_of(kind, params),
                Attestation::by(actor),
                now,
            )
            .await?
        }
        Some("running") => Job::handle(store, cx, tenant, job_id, Attestation::by(actor)).await?,
        Some(_) => return Ok(()),
    };
    job.fail(store, pipeline, cx, error, now).await?;
    Ok(())
}

/// `jobs.max_attempts`, as the instance sets it.
async fn max_attempts<C: GenericClient>(client: &C) -> Result<u32, IngestError> {
    let (settings, _) = triplespace_accounts::settings::load(client, None)
        .await
        .map_err(store_err)?;
    Ok(settings
        .value("jobs.max_attempts")
        .and_then(|v| v.as_u64())
        .map_or(DEFAULT_MAX_ATTEMPTS, |n| {
            u32::try_from(n).unwrap_or(u32::MAX)
        }))
}

/// What one transaction of [`claim`] came to.
enum Step {
    /// A job to run.
    Claimed(Claimed),
    /// A job was failed in place of another run; look again.
    GaveUp,
    /// Nothing this runner may claim now.
    Nothing,
}

/// Claims the oldest job of a kind `runner` runs whose partition has no running job. A row whose
/// claim went stale is claimed again under a new job ID, and the dead run is marked
/// `job/interrupt` where it had written its start record; one already claimed
/// `jobs.max_attempts` times is failed and removed instead, and the next is looked for
/// (0104 A7). A partition another claimer is deciding for is passed over.
///
/// # Errors
///
/// The store's.
pub async fn claim(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    runner: &Runner,
    now: u64,
) -> Result<Option<Claimed>, IngestError> {
    loop {
        match claim_step(store, pipeline, runner, now).await? {
            Step::Claimed(claimed) => return Ok(Some(claimed)),
            Step::GaveUp => {}
            Step::Nothing => return Ok(None),
        }
    }
}

/// One transaction of a claim.
#[allow(clippy::too_many_lines)]
async fn claim_step(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    runner: &Runner,
    now: u64,
) -> Result<Step, IngestError> {
    let mut cx = store.begin().await?;
    let stale = stale_interval();
    // The kinds are part of the query, not a check on the row it returns: a row of a
    // kind this runner does not run must not be the oldest row it ever sees.
    let kinds: Vec<&str> = runner.kinds.iter().map(|k| k.name()).collect();
    let runner = runner.name.as_str();
    // The partitions with a job to claim, by their oldest. Each is decided under its
    // claim lock (0104 A7): without it a claimer skips the row another has locked and not
    // yet committed as claimed, sees no running job in the partition, and claims a second
    // row of it. A partition whose lock is taken is passed over, not waited for.
    let partitions = cx
        .conn()
        .query(
            "SELECT q.partition FROM ops.job_queue q
             WHERE (q.claimed_at IS NULL OR q.heartbeat < now() - make_interval(secs => $1::double precision))
               AND NOT q.cancel
               AND q.kind = ANY($2)
             GROUP BY q.partition
             ORDER BY min(q.submitted), min(q.job_id)",
            &[&stale, &kinds],
        )
        .await
        .map_err(store_err)?;
    let mut row = None;
    for p in &partitions {
        let partition: i64 = p.get(0);
        let locked: bool = cx
            .conn()
            .query_one(PARTITION_LOCK, &[&partition])
            .await
            .map_err(store_err)?
            .get(0);
        if !locked {
            continue;
        }
        // A statement of its own, after the lock: it sees every claim of the partition
        // committed before the lock was this transaction's.
        row = cx
            .conn()
            .query_opt(
                "SELECT q.tenant, q.job_id, q.kind, q.params, q.payload, q.actor_key, q.claimed_at IS NOT NULL, q.attempts
                 FROM ops.job_queue q
                 WHERE q.partition = $3
                   AND (q.claimed_at IS NULL OR q.heartbeat < now() - make_interval(secs => $1::double precision))
                   AND NOT q.cancel
                   AND q.kind = ANY($2)
                   AND NOT EXISTS (SELECT 1 FROM ops.job_queue r
                                   WHERE r.partition = q.partition AND r.job_id <> q.job_id
                                     AND r.claimed_at IS NOT NULL AND r.heartbeat >= now() - make_interval(secs => $1::double precision))
                 ORDER BY q.submitted, q.job_id
                 LIMIT 1
                 FOR UPDATE OF q SKIP LOCKED",
                &[&stale, &kinds, &partition],
            )
            .await
            .map_err(store_err)?;
        if row.is_some() {
            break;
        }
    }
    let Some(row) = row else {
        store.rollback(cx).await?;
        return Ok(Step::Nothing);
    };
    let tenant: String = row.get(0);
    let old_id: i64 = row.get(1);
    let kind_name: String = row.get(2);
    let Some(kind) = Kind::parse(&kind_name) else {
        // Not reached: the query asked for this runner's kinds.
        store.rollback(cx).await?;
        return Ok(Step::Nothing);
    };
    let params: serde_json::Value = row.get(3);
    let payload: Option<Vec<u8>> = row.get(4);
    let actor: String = row.get(5);
    let was_claimed: bool = row.get(6);
    let attempts = u32::try_from(row.get::<_, i32>(7)).unwrap_or(0);
    let old = u64::try_from(old_id).map_err(store_err)?;
    if !was_claimed {
        cx.conn()
            .execute(
                "UPDATE ops.job_queue SET claimed_by = $3, claimed_at = now(), heartbeat = now(),
                        attempts = attempts + 1
                 WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &old_id, &runner],
            )
            .await
            .map_err(store_err)?;
        store.commit(cx).await?;
        return Ok(Step::Claimed(Claimed {
            tenant,
            job_id: old,
            kind,
            params,
            payload,
            actor,
            resumes: None,
        }));
    }
    // The earlier runner died. Its job is interrupted where it had started, and the
    // work runs again under a new ID (0104 §4), unless it has had its attempts.
    let started = status_of(cx.conn(), &tenant, old).await?.as_deref() == Some("running");
    let give_up = attempts >= max_attempts(cx.conn()).await?;
    // A dead run that wrote no start record keeps its ID when it is failed: the ID the
    // submitter was answered with is then the failed job.
    let new_id = if started || !give_up {
        store.next_id(&mut cx, "", &Sequence::Job).await?
    } else {
        old
    };
    if started {
        let handle = Job::handle(store, &mut cx, &tenant, old, Attestation::by(&actor)).await?;
        handle
            .interrupt(store, pipeline, &mut cx, new_id, now)
            .await?;
    }
    if give_up {
        // 0104 A7: a job that takes its runner down with it is not run for ever.
        let error = format!(
            "given up after {attempts} runs that ended without a result (jobs.max_attempts)"
        );
        fail_job(
            store, pipeline, &mut cx, &tenant, new_id, kind, &params, &actor, &error, now,
        )
        .await?;
        cx.conn()
            .execute(
                "DELETE FROM ops.job_queue WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &old_id],
            )
            .await
            .map_err(store_err)?;
        store.commit(cx).await?;
        eprintln!("{runner}: job {new_id} ({kind_name}) for `{tenant}`: {error}");
        return Ok(Step::GaveUp);
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
    store.commit(cx).await?;
    Ok(Step::Claimed(Claimed {
        tenant,
        job_id: new_id,
        kind,
        params,
        payload,
        actor,
        resumes: Some(old),
    }))
}

/// Writes a claimed row's heartbeat and progress. `Ok(false)` where the row is no longer
/// this job's: the claim was taken (the row runs under a new ID) or the row is gone.
async fn beat<C: GenericClient>(
    client: &C,
    tenant: &str,
    job_id: u64,
    progress: Option<&serde_json::Value>,
) -> Result<bool, tokio_postgres::Error> {
    let id = i64::try_from(job_id).unwrap_or(i64::MAX);
    match progress {
        Some(p) => client
            .execute(
                "UPDATE ops.job_queue SET heartbeat = now(), progress = $3 WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &id, p],
            )
            .await
            .map(|n| n > 0),
        None => client
            .execute(
                "UPDATE ops.job_queue SET heartbeat = now() WHERE tenant = $1 AND job_id = $2",
                &[&tenant, &id],
            )
            .await
            .map(|n| n > 0),
    }
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
/// written as it goes, then the row deleted. A job that fails gets its `job/fail` record,
/// after its start record where it failed before writing one (0104 A7), and its row is
/// deleted too: the jobs here fail on their input, not on the weather, so nothing is
/// retried. Where the claim was taken meanwhile (the heartbeat stalled for [`STALE`] and
/// another runner claimed the row), nothing more is written about the row: it is the
/// other runner's, and the result says so.
///
/// # Errors
///
/// The store's, where the queue itself could not be written; a job's own failure is in
/// the result.
#[allow(clippy::too_many_lines)]
pub async fn run_claimed(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    runner: &Runner,
    claimed: Claimed,
    now: fn() -> u64,
) -> Result<Ran, IngestError> {
    let pool = store.backend().pool().clone();
    let tenant = claimed.tenant.clone();
    let job_id = claimed.job_id;
    // The heartbeat, from a connection of its own, until the job ends. One that finds
    // the row no longer this job's has lost the claim: it says so, and the work is
    // stopped (0104 A8).
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let lost = Arc::new(AtomicBool::new(false));
    let stopping = Arc::new(tokio::sync::Notify::new());
    let beating = {
        let pool = pool.clone();
        let tenant = tenant.clone();
        let lost = lost.clone();
        let stopping = stopping.clone();
        let name = runner.name.clone();
        let heartbeat = runner.heartbeat;
        tokio::spawn(async move {
            let mut every = tokio::time::interval(heartbeat);
            every.tick().await;
            loop {
                tokio::select! {
                    _ = every.tick() => {
                        let beaten = match pool.get().await {
                            Ok(client) => beat(&**client, &tenant, job_id, None)
                                .await
                                .map_err(|e| e.to_string()),
                            Err(e) => Err(e.to_string()),
                        };
                        match beaten {
                            Ok(true) => {}
                            Ok(false) => {
                                eprintln!(
                                    "{name}: job {job_id} for `{tenant}` LOST ITS CLAIM: its row was taken by \
                                     another runner or removed while it ran; its work is stopped, and \
                                     nothing more is written to the queue for it"
                                );
                                lost.store(true, Ordering::Relaxed);
                                stopping.notify_one();
                                break;
                            }
                            Err(e) => eprintln!(
                                "{name}: job {job_id} for `{tenant}`: the heartbeat was not written: {e}"
                            ),
                        }
                    }
                    r = stopped.changed() => {
                        if r.is_err() || *stopped.borrow() { break; }
                    }
                }
            }
        })
    };
    // The work, or the loss of the claim, whichever comes first. Losing the claim drops
    // the work where it stands: the unit of work it was in is rolled back with its
    // connection, and what it had committed is the run's that took the claim to go on
    // from, as after a runner's death. A dump's files are written on a thread that is
    // not stopped; it finishes them, and records nothing.
    let result = tokio::select! {
        result = Box::pin(run_kind(store, pipeline, runner, &claimed, now)) => result,
        () = stopping.notified() => Err(store_err("stopped: the claim was lost")),
    };
    let _ = stop.send(true);
    let _ = beating.await;
    // Whether the row is still this job's, read under its lock: a claim taken meanwhile
    // leaves the row under another ID, and its end is the other runner's to write.
    let id = i64::try_from(job_id).map_err(store_err)?;
    let mut cx = store.begin().await?;
    let held = cx
        .conn()
        .query_opt(
            "SELECT 1 FROM ops.job_queue WHERE tenant = $1 AND job_id = $2 FOR UPDATE",
            &[&tenant, &id],
        )
        .await
        .map_err(store_err)?
        .is_some();
    if !held {
        store.rollback(cx).await?;
        if !lost.load(Ordering::Relaxed) {
            eprintln!(
                "{}: job {job_id} for `{tenant}` LOST ITS CLAIM: its row was taken by another runner \
                 or removed while it ran",
                runner.name
            );
        }
        let ended = match result {
            Ok(()) => "the run ended".to_string(),
            Err(e) => format!("the run failed: {e}"),
        };
        return Ok(Ran {
            tenant,
            job_id,
            kind: claimed.kind,
            error: Some(format!(
                "the claim was lost while the job ran, and nothing was written to the queue for it ({ended})"
            )),
        });
    }
    let error = match result {
        Ok(()) => None,
        Err(e) => {
            let message = e.to_string();
            // The job's own failure record (0011 §6.3), and its start record first where
            // it failed before one: the ID is a job from submission on (0104 A7).
            fail_job(
                store,
                pipeline,
                &mut cx,
                &tenant,
                job_id,
                claimed.kind,
                &claimed.params,
                &claimed.actor,
                &message,
                now(),
            )
            .await?;
            Some(message)
        }
    };
    cx.conn()
        .execute(
            "DELETE FROM ops.job_queue WHERE tenant = $1 AND job_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(store_err)?;
    store.commit(cx).await?;
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
    runner: &Runner,
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
        Kind::Revert => {
            let reverts = claimed
                .params
                .get("reverts")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| store_err("a revert job names the job it reverts"))?;
            let reason = claimed
                .params
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            let pool = store.backend().pool().clone();
            let client = pool.get().await.map_err(store_err)?;
            crate::revert::run_revert(
                store,
                pipeline,
                &**client,
                &claimed.tenant,
                &claimed.actor,
                reverts,
                Some(claimed.job_id),
                reason.as_deref(),
                scatter_providers::Registry::default_registry(),
                now,
            )
            .await
            .map(|_| ())
        }
        Kind::Dump => {
            let dir = runner
                .dumps_dir
                .as_deref()
                .ok_or_else(|| store_err("this runner has no dumps directory"))?;
            run_dump(store, pipeline, dir, claimed, now).await
        }
    }
}

/// A dump job (0104 A6): the job's start record, the tenant's dumps written as
/// `triplespace dump` writes them, and the finish record naming the dump and its files. A
/// failure leaves no dump directory (the files are renamed into place last), and the
/// caller writes the job's `job/fail`.
async fn run_dump(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    dir: &Path,
    claimed: &Claimed,
    now: fn() -> u64,
) -> Result<(), IngestError> {
    let tenant = claimed.tenant.as_str();
    let mut cx = store.begin().await?;
    let job = Job::start_as(
        store,
        pipeline,
        &mut cx,
        tenant,
        claimed.job_id,
        header_of(Kind::Dump, &claimed.params),
        Attestation::by(&claimed.actor),
        now(),
    )
    .await?;
    store.commit(cx).await?;

    let mut client = store.backend().pool().get().await.map_err(store_err)?;
    let base = client
        .query_opt(
            "SELECT config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&tenant],
        )
        .await
        .map_err(store_err)?
        .and_then(|r| r.get::<_, Option<String>>(0))
        .ok_or_else(|| store_err(format!("no tenant `{tenant}`")))?;
    let registry = triplespace_projections::entity_sources::registry_for(&**client, tenant)
        .await
        .map_err(store_err)?;
    let slugs: Vec<&str> = registry
        .providers()
        .iter()
        .map(|p| p.slug.as_str())
        .collect();
    let order = EntityProjection::new(&slugs, &registry);
    let groups = triplespace_accounts::groups::registry(&**client, tenant)
        .await
        .map_err(store_err)?;
    let reader = evaluate(
        &Principal::anonymous(),
        &groups,
        GrantRegistry::default_registry(),
    );
    let keep = match claimed
        .params
        .get("keep")
        .and_then(serde_json::Value::as_u64)
    {
        Some(k) => k,
        None => triplespace_accounts::settings::load(&**client, Some(tenant))
            .await
            .map_err(store_err)?
            .0
            .value("dumps.keep")
            .and_then(|v| v.as_u64())
            .unwrap_or(7),
    };
    // On a thread of its own: the dump's future is not `Send` (it verifies its bundle
    // through borrowed lookups), and a runner is a spawned task.
    let runtime = tokio::runtime::Handle::current();
    let (tenant_owned, dir) = (tenant.to_string(), dir.join(tenant));
    let manifest = tokio::task::spawn_blocking(move || {
        runtime.block_on(async move {
            triplespace_rdf::dump(
                &mut client,
                &triplespace_rdf::Request {
                    tenant: &tenant_owned,
                    base: &base,
                    registry: &registry,
                    order: &order,
                    reader: &reader,
                    dir: &dir,
                    keep: usize::try_from(keep).unwrap_or(usize::MAX),
                    replica: false,
                },
            )
            .await
        })
    })
    .await
    .map_err(store_err)?
    .map_err(store_err)?;

    let mut extra = serde_json::Map::new();
    extra.insert(
        "dump".into(),
        serde_json::json!({
            "name": manifest.name,
            "taken": manifest.taken,
            "entities": manifest.entities,
            "local_entities": manifest.local_entities,
            "files": manifest.files.iter().map(|f| serde_json::json!({
                "name": f.name, "records": f.records, "bytes": f.bytes, "sha256": f.sha256,
            })).collect::<Vec<_>>(),
        }),
    );
    let mut cx = store.begin().await?;
    job.finish(store, pipeline, &mut cx, &Counts::default(), extra, now())
        .await?;
    store.commit(cx).await?;
    Ok(())
}

/// One pass of a runner: claims a job and runs it. `Ok(None)` when nothing waits.
///
/// # Errors
///
/// The store's.
pub async fn run_once(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    runner: &Runner,
    now: fn() -> u64,
) -> Result<Option<Ran>, IngestError> {
    match claim(store, pipeline, runner, now()).await? {
        Some(claimed) => Ok(Some(
            run_claimed(store, pipeline, runner, claimed, now).await?,
        )),
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
    runner: Runner,
    idle: Duration,
    now: fn() -> u64,
) {
    let name = runner.name.clone();
    loop {
        // A setting that cannot be read is not a setting that is off: the pass waits.
        let paused = match store.backend().pool().get().await {
            Ok(client) => paused(&**client).await,
            Err(e) => Err(e.to_string()),
        }
        .unwrap_or_else(|e| {
            eprintln!(
                "{name}: `jobs.paused` could not be read, so nothing is claimed this pass: {e}"
            );
            true
        });
        if paused {
            tokio::time::sleep(idle).await;
            continue;
        }
        match run_once(&store, &pipeline, &runner, now).await {
            Ok(Some(ran)) => match ran.error {
                None => eprintln!(
                    "{name}: job {} ({}) for `{}` finished",
                    ran.job_id,
                    ran.kind.name(),
                    ran.tenant
                ),
                Some(e) => eprintln!(
                    "{name}: job {} ({}) for `{}` failed: {e}",
                    ran.job_id,
                    ran.kind.name(),
                    ran.tenant
                ),
            },
            Ok(None) => tokio::time::sleep(idle).await,
            Err(e) => {
                eprintln!("{name}: {e}");
                tokio::time::sleep(idle).await;
            }
        }
    }
}
