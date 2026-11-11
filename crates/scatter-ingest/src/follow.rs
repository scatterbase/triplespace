//! Following a provider by its changes (0092 §1–§3): a loop that asks a [`ChangeSource`]
//! for the next batch of changed entities, writes them through the sync job as `upsert`
//! items, and tells the source to save its position in the same unit of work's wake.
//!
//! The ingester names no API. A source is the surfaces' (`triplespace-api-ingest`), over
//! the adapters and the upstream client; this crate sees only [`SyncItem`]s and an opaque
//! position, as its `FilterHook` sees no filter language (0030 §7). The version cursor,
//! not the position, is what keeps a replayed row from writing anything (0092 §3): the
//! sync skips an entity whose recorded revision is not older.
//!
//! A follower writes under one job a day (0092 A6): the day's first batch starts it, every
//! batch that day commits under it, and the first batch of the next day, or the first run
//! after it, finishes it with the day's counts. The open job travels with the position, so
//! `follow --once` from a timer and a server's loop leave the same jobs.

use std::collections::BTreeMap;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;

use scatter_projection::Pipeline;
use scatter_providers::Registry;
use scatter_wikibase_changeset::{JobHeader, Mode};
use serde::{Deserialize, Serialize};

use crate::IngestError;
use crate::job::{Counts, Job, Reject};
use crate::store::IngestStore;
use crate::sync::{SyncItem, sync_batch};
use crate::write::Attestation;

/// One poll's worth of changes, as the source turned them into sync items.
#[derive(Debug, Default)]
pub struct ChangeBatch {
    /// The items: states, redirects and deletions.
    pub items: Vec<SyncItem>,
    /// Rows the poll read, for the job's counts.
    pub rows: u64,
    /// Rows the tail pass found that the head had missed (0092 §1, `late`).
    pub late: u64,
    /// States whose revision was older than the row that named them: a lagging replica,
    /// to be read again on the next poll (0092 §2, `stale`).
    pub stale: u64,
    /// Whether the source has caught up to the present: nothing more to read now.
    pub caught_up: bool,
}

/// Where changes come from, and where the position goes (0092 §1, §3).
///
/// A batch carries the position it leaves, so that the loop can read the next batch while
/// this one is written and still save each position only after its own batch.
pub trait ChangeSource {
    /// Where the source stands after a batch, as `save` stores it.
    type Position: Send;

    /// The next batch, with the position it leaves. An empty batch with `caught_up` set
    /// means "nothing new".
    fn next(
        &mut self,
    ) -> impl Future<Output = Result<(ChangeBatch, Self::Position), IngestError>> + Send;

    /// The follower's open job, as the last `save` left it.
    fn job(&mut self) -> impl Future<Output = Result<Option<FollowJob>, IngestError>> + Send;

    /// Saves a position a batch left, with the follower's open job as it stands after
    /// that batch: called after that batch's records are committed, never before, so that
    /// a crash between the two replays rather than skips (0092 §3).
    fn save(
        &mut self,
        at: Self::Position,
        job: Option<FollowJob>,
    ) -> impl Future<Output = Result<(), IngestError>> + Send;
}

/// A follower's open job, the day's (0092 A6): what its finish record will say, kept with
/// the position between batches and between runs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FollowJob {
    /// The job ID.
    pub id: u64,
    /// The UTC day it is for, in days since 1970-01-01.
    pub day: u64,
    /// The counts so far.
    pub counts: Counts,
    /// Batches written under it.
    pub batches: u64,
    /// Change rows read.
    pub rows: u64,
    /// Late rows.
    pub late: u64,
    /// Stale states.
    pub stale: u64,
    /// Rejected in all.
    pub rejected_total: u64,
    /// The first rejects, for the finish record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejects: Vec<Reject>,
    /// Upstream hashes kept, by value type.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hash_mismatches: BTreeMap<String, u64>,
}

const DAY_MICROS: u64 = 86_400_000_000;

/// The job line of a follower's job for `day`: `follow-YYYY-MM-DD` its version.
fn day_header(tenant: &str, slug: &str, day: u64) -> JobHeader {
    let version = format!("follow-{}", date_of(day));
    let mut header = if tenant.is_empty() {
        JobHeader::mirror(slug, &version, Mode::Upsert)
    } else {
        JobHeader::source(slug, &version, Mode::Upsert)
    };
    header
        .params
        .insert("follow".into(), serde_json::Value::Bool(true));
    header
}

/// `YYYY-MM-DD` of a day since 1970-01-01 (the proleptic Gregorian calendar).
fn date_of(day: u64) -> String {
    let z = i64::try_from(day).unwrap_or(i64::MAX / 2) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The handle to write under or finish `open`, reopened when this process did not start it.
async fn handle_of<S: IngestStore>(
    store: &S,
    tenant: &str,
    slug: &str,
    attestation: &Attestation,
    open: &FollowJob,
    job: Option<Job>,
) -> Result<Job, IngestError> {
    if let Some(job) = job.filter(|j| j.id == open.id) {
        return Ok(job);
    }
    let mut cx = store.begin().await?;
    let mut job = Job::reopen(
        store,
        &mut cx,
        tenant,
        open.id,
        day_header(tenant, slug, open.day),
        attestation.clone(),
    )
    .await;
    store.rollback(cx).await?;
    if let Ok(job) = job.as_mut() {
        job.rejects.clone_from(&open.rejects);
        job.rejected_total = open.rejected_total;
        job.hash_mismatches.clone_from(&open.hash_mismatches);
    }
    job
}

/// Finishes a day's job with its counts, and what the poll read, as its finish record.
async fn finish_day<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    job: &Job,
    open: &FollowJob,
    now: u64,
) -> Result<(), IngestError> {
    let mut extra = serde_json::Map::new();
    for (k, v) in [
        ("batches", open.batches),
        ("rows", open.rows),
        ("late", open.late),
        ("stale", open.stale),
    ] {
        extra.insert(k.into(), v.into());
    }
    let mut cx = store.begin().await?;
    job.finish(store, pipeline, &mut cx, &open.counts, extra, now)
        .await?;
    store.commit(cx).await?;
    Ok(())
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FollowOutcome {
    /// The open job's ID, the day's (0092 A6); 0 when nothing was written.
    pub job_id: u64,
    /// Batches read.
    pub batches: u64,
    /// Rows read across them.
    pub rows: u64,
    /// The sync's counts, summed.
    pub counts: Counts,
    /// Late rows, summed.
    pub late: u64,
    /// Stale states, summed.
    pub stale: u64,
}

/// Runs a follower until the source is caught up, or forever when `once` is false.
///
/// Each batch is a sync in `upsert` mode (0092 §2) under the follower's job for the day
/// (A6), committed as its own unit of work under the load lock; the position, and the job
/// as it stands, are saved once the batch is committed. The day's first batch starts the
/// job, and the first batch of a later day finishes it first. While a batch is written,
/// the next one is read: the wait on the upstream overlaps the writing, batches are still
/// written one at a time and in order, and each position is saved after its own batch, so
/// a crash replays at most the batch read ahead. `tenant` is `""` for a registry
/// provider's mirror and the tenant for its entity source `slug` (0078 §4), with `registry`
/// the tenant's table of names.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_follow<S: IngestStore, C: ChangeSource>(
    store: &S,
    pipeline: &Pipeline<S>,
    tenant: &str,
    slug: &str,
    attestation: Attestation,
    source: &mut C,
    registry: &Registry,
    now: impl Fn() -> u64,
    batch: usize,
    once: bool,
) -> Result<FollowOutcome, IngestError> {
    let mut out = FollowOutcome::default();
    let mut open = source.job().await?;
    let mut job: Option<Job> = None;
    let mut ahead = source.next().await?;
    loop {
        let (b, at) = ahead;
        let ChangeBatch {
            items,
            rows,
            late,
            stale,
            caught_up,
        } = b;
        out.batches += 1;
        out.rows += rows;
        out.late += late;
        out.stale += stale;
        let last = caught_up && once;
        // Boxed: with the read ahead beside it, the pair would make a large future.
        let write = Box::pin(async {
            let today = now() / DAY_MICROS;
            // A day's job ends with the day, whether or not the new day has changes yet.
            if let Some(f) = open.as_ref().filter(|f| f.day != today) {
                let handle = handle_of(store, tenant, slug, &attestation, f, job.take()).await?;
                finish_day(store, pipeline, &handle, f, now()).await?;
                open = None;
            }
            if items.is_empty() {
                return Ok::<_, IngestError>(None);
            }
            let handle = match open.as_ref() {
                Some(f) => Some(handle_of(store, tenant, slug, &attestation, f, job.take()).await?),
                None => None,
            };
            let (o, j) = sync_batch(
                store,
                pipeline,
                tenant,
                day_header(tenant, slug, today),
                attestation.clone(),
                handle,
                items,
                registry,
                now(),
                batch,
            )
            .await?;
            let f = open.get_or_insert_with(|| FollowJob {
                id: j.id,
                day: today,
                ..FollowJob::default()
            });
            f.counts.add(&o.counts);
            f.batches += 1;
            f.rejected_total = j.rejected_total;
            f.rejects.clone_from(&j.rejects);
            f.hash_mismatches.clone_from(&j.hash_mismatches);
            job = Some(j);
            Ok(Some(o))
        });
        let (written, next) = if last {
            (write.await, None)
        } else {
            let (written, next) = both(write, source.next()).await;
            (written, Some(next))
        };
        if let Some(o) = written? {
            out.counts.add(&o.counts);
        }
        if let Some(f) = open.as_mut() {
            out.job_id = f.id;
            f.rows += rows;
            f.late += late;
            f.stale += stale;
        }
        source.save(at, open.clone()).await?;
        match next {
            None => return Ok(out),
            Some(next) => ahead = next?,
        }
    }
}

/// Runs two futures together on the calling task and returns both outputs, with `std`
/// alone rather than a new dependency (22 §1.2 rule 7).
async fn both<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
    let mut a = pin!(a);
    let mut b = pin!(b);
    let mut ra = None;
    let mut rb = None;
    poll_fn(|cx| {
        if ra.is_none()
            && let Poll::Ready(v) = a.as_mut().poll(cx)
        {
            ra = Some(v);
        }
        if rb.is_none()
            && let Poll::Ready(v) = b.as_mut().poll(cx)
        {
            rb = Some(v);
        }
        if ra.is_some() && rb.is_some() {
            Poll::Ready((ra.take().unwrap(), rb.take().unwrap()))
        } else {
            Poll::Pending
        }
    })
    .await
}
