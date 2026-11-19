//! Following a provider by its changes (0092 §1–§3): a loop that asks a [`ChangeSource`]
//! for the next batch of changed entities, writes them through the sync job as `upsert`
//! items, and saves the source's position in the unit of work that commits them.
//!
//! The ingester names no API. A source is the surfaces' (`triplespace-api-ingest`), over
//! the adapters and the upstream client; this crate sees only [`SyncItem`]s and an opaque
//! position, as its `FilterHook` sees no filter language (0030 §7). The source's
//! [`SavePosition`] writes the position, and the jobs with it, in the store's units of
//! work, so that they commit with the records and job records they describe (0092 A10).
//! The version cursor is still what keeps a replayed row from writing anything (§3): the
//! sync skips an entity whose recorded revision is not older.
//!
//! A follower writes under one job a day (0092 A6): the day's first batch starts it, every
//! batch that day commits under it, and the first batch of the next day, or the first run
//! after it, finishes it with the day's counts. The open job travels with the position, so
//! `follow --once` from a timer and a server's loop leave the same jobs.
//!
//! A registry provider's follower compacts its mirror as the day's job closes (0092 A7):
//! the keys written since the job opened keep their newest record, and the one carrying
//! their highest revision ID, and lose the rest ([`crate::compact`]). The compaction runs
//! beside the next day's batches, a window at a time, and the closed day's job is finished
//! once it is done (A8), so that following never waits for a whole day's compaction. A run
//! that stops when caught up leaves the rest to the next run (A9).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;

use scatter_projection::Pipeline;
use scatter_providers::Registry;
use scatter_wikibase_changeset::{JobHeader, Mode};
use serde::{Deserialize, Serialize};

use crate::IngestError;
use crate::compact::{WINDOW, compact, mirror_of};
use crate::job::{Counts, Job, Reject};
use crate::store::IngestStore;
use crate::sync::{BeforeCommit, SyncItem, sync_batch};
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

/// Where changes come from, and where the position goes (0092 §1, §3), for a follower
/// that writes to the store `S`.
///
/// A batch carries the position it leaves, so that the loop can read the next batch while
/// this one is written and still save each position with its own batch.
pub trait ChangeSource<S: IngestStore> {
    /// Where the source stands after a batch, as its saver stores it.
    type Position: Send + Sync;

    /// What saves the position and the jobs.
    type Saver: SavePosition<S, Position = Self::Position> + Send + Sync;

    /// The next batch, with the position it leaves. An empty batch with `caught_up` set
    /// means "nothing new". A source followed without `once` waits between polls once it is
    /// caught up, and the day's compaction runs beside that wait (0092 A9).
    fn next(
        &mut self,
    ) -> impl Future<Output = Result<(ChangeBatch, Self::Position), IngestError>> + Send;

    /// The follower's jobs, as the last save before this run left them.
    fn state(&mut self) -> impl Future<Output = Result<FollowState, IngestError>> + Send;

    /// The saver, which writes in the follower's units of work while the source reads on.
    fn saver(&self) -> Self::Saver;
}

/// Saves a follower's position and jobs in a unit of work of the store `S` (0092 A10).
pub trait SavePosition<S: IngestStore> {
    /// The source's position.
    type Position;

    /// Writes the follower's jobs as `state` has them, and `at` when given, in `cx`, to
    /// commit with it. Without `at` the position stays as it was: a unit of work before
    /// a batch's last, a day's close, or a window of its compaction.
    fn save(
        &self,
        cx: &mut S::Cx,
        at: Option<&Self::Position>,
        state: &FollowState,
    ) -> impl Future<Output = Result<(), IngestError>> + Send;
}

/// Saves in a unit of work of its own: a batch with nothing to write, or a compaction's
/// progress.
async fn save_alone<S: IngestStore, V: SavePosition<S>>(
    store: &S,
    saver: &V,
    at: Option<&V::Position>,
    state: &FollowState,
) -> Result<(), IngestError> {
    let mut cx = store.begin().await?;
    saver.save(&mut cx, at, state).await?;
    store.commit(cx).await?;
    Ok(())
}

/// A batch's [`BeforeCommit`] (0092 A10): the jobs in each of its units of work, with the
/// day's job as it was before the batch, or as the batch started it; and in its last, the
/// job with the batch counted, and the position the batch leaves. A crash in between
/// replays the batch under the same job.
struct BatchSave<'a, V: SavePosition<S>, S: IngestStore> {
    saver: &'a V,
    at: &'a V::Position,
    /// The day's job before the batch; `None` before the day's first.
    open: Option<FollowJob>,
    /// The closing day, as it stood when the batch began.
    closing: Option<Closing>,
    day: u64,
    compact_from: Option<u64>,
    rows: u64,
    late: u64,
    stale: u64,
    /// The day's job as the last unit of work saved it.
    saved: Option<FollowJob>,
    store: std::marker::PhantomData<fn(&S)>,
}

impl<V, S> BeforeCommit<S> for BatchSave<'_, V, S>
where
    S: IngestStore,
    V: SavePosition<S> + Sync,
    V::Position: Sync,
{
    async fn before_commit(
        &mut self,
        cx: &mut S::Cx,
        job: &Job,
        counts: &Counts,
        last: bool,
    ) -> Result<(), IngestError> {
        let mut f = self.open.clone().unwrap_or_else(|| FollowJob {
            id: job.id,
            day: self.day,
            compact_from: self.compact_from,
            ..FollowJob::default()
        });
        if last {
            f.counts.add(counts);
            f.batches += 1;
            f.rows += self.rows;
            f.late += self.late;
            f.stale += self.stale;
            f.rejected_total = job.rejected_total;
            f.rejects.clone_from(&job.rejects);
            f.hash_mismatches.clone_from(&job.hash_mismatches);
        }
        let state = FollowState {
            open: Some(f.clone()),
            closing: self.closing.clone(),
        };
        self.saver.save(cx, last.then_some(self.at), &state).await?;
        self.saved = Some(f);
        Ok(())
    }
}

/// A follower's jobs between batches and between runs: the day's open job (0092 A6), and
/// a closed day's job whose compaction is still running beside it (A8).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowState {
    /// The day's open job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<FollowJob>,
    /// A closed day's job, finished once its compaction is done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closing: Option<Closing>,
}

impl FollowState {
    /// Whether the follower has no job at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open.is_none() && self.closing.is_none()
    }
}

/// A closed day's job and its compaction's progress (0092 A8): the mirror's offsets from
/// the job's `compact_from` to `to` are compacted, `at` is where the compaction has got
/// to, and the job is finished, with `removed` as its `compacted`, once `at` reaches `to`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Closing {
    /// The day's job, as it stood when the day closed.
    pub job: FollowJob,
    /// The mirror's partition.
    pub partition: u64,
    /// The mirror's size when the day closed: where the compaction ends.
    pub to: u64,
    /// The first offset not yet compacted.
    pub at: u64,
    /// Records removed so far.
    pub removed: u64,
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
    /// The mirror's size when the job opened, where the day's compaction starts (0092 A7);
    /// `None` for a tenant's source, and for a job opened before followers compacted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_from: Option<u64>,
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

/// Finishes a day's job with its counts, and what the poll read, as its finish record,
/// and saves the jobs as `after` has them in the same unit of work (0092 A10).
#[allow(clippy::too_many_arguments)]
async fn finish_day<S: IngestStore, V: SavePosition<S>>(
    store: &S,
    pipeline: &Pipeline<S>,
    job: &Job,
    open: &FollowJob,
    compacted: Option<u64>,
    now: u64,
    saver: &V,
    after: &FollowState,
) -> Result<(), IngestError> {
    let mut extra = serde_json::Map::new();
    for (k, v) in [
        ("batches", Some(open.batches)),
        ("rows", Some(open.rows)),
        ("late", Some(open.late)),
        ("stale", Some(open.stale)),
        ("compacted", compacted),
    ] {
        if let Some(v) = v {
            extra.insert(k.into(), v.into());
        }
    }
    let mut cx = store.begin().await?;
    job.finish(store, pipeline, &mut cx, &open.counts, extra, now)
        .await?;
    saver.save(&mut cx, None, after).await?;
    store.commit(cx).await?;
    Ok(())
}

/// Closes a day's job (0092 A7, A8): the compaction of the mirror by the keys written since
/// the job opened, up to the mirror's size now, to run beside what follows; `None` when the
/// job has no start to compact from, or there is no mirror, and so is finished at once.
async fn close_day<S: IngestStore>(
    store: &S,
    slug: &str,
    open: FollowJob,
) -> Result<Result<Closing, FollowJob>, IngestError> {
    let Some(from) = open.compact_from else {
        return Ok(Err(open));
    };
    let Some((partition, size)) = mirror_of(store, slug).await? else {
        return Ok(Err(open));
    };
    Ok(Ok(Closing {
        job: open,
        partition,
        to: size,
        at: from.min(size),
        removed: 0,
    }))
}

/// Compacts a closing day beside the batch being written and the one being read (0092
/// A8): a window at a time until `busy`, the count of those still running, falls to 0, and
/// at least one window, so that each batch moves the compaction on; done at once when
/// there is nothing left to compact. A window is its own unit of work, so a crash loses
/// only the progress not yet saved, and the windows run again remove nothing more.
async fn compact_beside<S: IngestStore>(
    store: &S,
    closing: Option<&mut Closing>,
    busy: &Cell<u8>,
) -> Result<(), IngestError> {
    let Some(c) = closing else {
        return Ok(());
    };
    while c.at < c.to {
        let end = c.at.saturating_add(WINDOW).min(c.to);
        c.removed += compact(store, c.partition, c.at, end, WINDOW, |_, _| {}).await?;
        c.at = end;
        if busy.get() == 0 {
            break;
        }
    }
    Ok(())
}

/// Runs `f`, then marks it done in `busy`, for [`compact_beside`].
async fn counted<F: Future>(f: F, busy: &Cell<u8>) -> F::Output {
    let out = f.await;
    busy.set(busy.get().saturating_sub(1));
    out
}

/// Finishes a closing day's job if its compaction is done, taking it out of `closing`,
/// and saves the jobs, `open` and no closing day, with the finish record. Returns whether
/// it finished it.
#[allow(clippy::too_many_arguments)]
async fn finish_closed<S: IngestStore, V: SavePosition<S>>(
    store: &S,
    pipeline: &Pipeline<S>,
    tenant: &str,
    slug: &str,
    attestation: &Attestation,
    closing: &mut Option<Closing>,
    handle: &mut Option<Job>,
    now: u64,
    saver: &V,
    open: Option<&FollowJob>,
) -> Result<bool, IngestError> {
    let Some(c) = closing.as_ref().filter(|c| c.at >= c.to) else {
        return Ok(false);
    };
    let job = handle_of(store, tenant, slug, attestation, &c.job, handle.take()).await?;
    let after = FollowState {
        open: open.cloned(),
        closing: None,
    };
    finish_day(
        store,
        pipeline,
        &job,
        &c.job,
        Some(c.removed),
        now,
        saver,
        &after,
    )
    .await?;
    *closing = None;
    Ok(true)
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
/// (A6), committed in units of work under the load lock; each unit saves the jobs, and the
/// last the position the batch leaves, so that what is saved commits with the records and
/// job records it describes (A10). The day's first batch starts the job, and the first
/// batch of a later day closes it first: a registry provider's mirror is compacted by the
/// day's keys beside the batches that follow, and beside the source's waits between polls,
/// at least a window a batch, and the job is finished when that is done (A7, A8, A9), its
/// finish record committed with the jobs that no longer hold it. A run with `once` leaves
/// what it has not compacted to the next run, as it leaves the day's open job. While a
/// batch is written, the next one is read: the wait on the upstream overlaps the writing,
/// batches are still written one at a time and in order, and each position is saved with
/// its own batch, so a crash replays at most the batch it interrupted. `tenant` is `""` for
/// a registry provider's mirror and the tenant for its entity source `slug` (0078 §4), with
/// `registry` the tenant's table of names.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_follow<S: IngestStore, C: ChangeSource<S>>(
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
    let FollowState {
        mut open,
        mut closing,
    } = source.state().await?;
    let saver = source.saver();
    let mut job: Option<Job> = None;
    let mut closing_job: Option<Job> = None;
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
        let today = now() / DAY_MICROS;
        // A day's job ends with the day, whether or not the new day has changes yet; its
        // compaction runs beside what follows, and a day before it still compacting is
        // finished first.
        if let Some(f) = open.take_if(|f| f.day != today) {
            if let Some(c) = closing.as_mut() {
                c.removed +=
                    Box::pin(compact(store, c.partition, c.at, c.to, WINDOW, |_, _| {})).await?;
                c.at = c.to;
                Box::pin(finish_closed(
                    store,
                    pipeline,
                    tenant,
                    slug,
                    &attestation,
                    &mut closing,
                    &mut closing_job,
                    now(),
                    &saver,
                    Some(&f),
                ))
                .await?;
            }
            let handle = job.take().filter(|j| j.id == f.id);
            match close_day(store, slug, f).await? {
                Ok(c) => {
                    closing = Some(c);
                    closing_job = handle;
                }
                Err(f) => {
                    let handle = handle_of(store, tenant, slug, &attestation, &f, handle).await?;
                    let after = FollowState {
                        open: None,
                        closing: closing.clone(),
                    };
                    Box::pin(finish_day(
                        store,
                        pipeline,
                        &handle,
                        &f,
                        None,
                        now(),
                        &saver,
                        &after,
                    ))
                    .await?;
                }
            }
        }
        let closing_then = closing.clone();
        let writes = !items.is_empty();
        // Boxed: with the read ahead and the compaction beside it, the three would make a
        // large future.
        let write = Box::pin(async {
            if items.is_empty() {
                return Ok::<_, IngestError>(None);
            }
            // A registry provider's new day's job compacts from where the mirror stands now;
            // a mirror not yet created starts at 0.
            let compact_from = if open.is_none() && tenant.is_empty() {
                Some(mirror_of(store, slug).await?.map_or(0, |(_, size)| size))
            } else {
                None
            };
            let handle = match open.as_ref() {
                Some(f) => Some(handle_of(store, tenant, slug, &attestation, f, job.take()).await?),
                None => None,
            };
            let mut save = BatchSave {
                saver: &saver,
                at: &at,
                open: open.clone(),
                closing: closing_then.clone(),
                day: today,
                compact_from,
                rows,
                late,
                stale,
                saved: None,
                store: std::marker::PhantomData,
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
                &mut save,
            )
            .await?;
            job = Some(j);
            let f = save.saved.ok_or_else(|| {
                IngestError::Store("a follower's batch committed without saving".into())
            })?;
            Ok(Some((o, f)))
        });
        let busy = Cell::new(if last { 1 } else { 2 });
        let beside = Box::pin(compact_beside(store, closing.as_mut(), &busy));
        let (written, next, compacted) = if last {
            let (written, compacted) = both(counted(write, &busy), beside).await;
            (written, None, compacted)
        } else {
            let ((written, next), compacted) = both(
                both(counted(write, &busy), counted(source.next(), &busy)),
                beside,
            )
            .await;
            (written, Some(next), compacted)
        };
        if let Some((o, f)) = written? {
            out.counts.add(&o.counts);
            open = Some(f);
        }
        compacted?;
        let finished = Box::pin(finish_closed(
            store,
            pipeline,
            tenant,
            slug,
            &attestation,
            &mut closing,
            &mut closing_job,
            now(),
            &saver,
            open.as_ref(),
        ))
        .await?;
        if !writes && let Some(f) = open.as_mut() {
            f.rows += rows;
            f.late += late;
            f.stale += stale;
        }
        if let Some(f) = open.as_ref() {
            out.job_id = f.id;
        }
        let state = FollowState {
            open: open.clone(),
            closing: closing.clone(),
        };
        // A batch that wrote saved its position with its records; one that did not saves
        // it now, and a compaction that moved on without finishing saves how far it got.
        if !writes {
            save_alone(store, &saver, Some(&at), &state).await?;
        } else if !finished && closing != closing_then {
            save_alone(store, &saver, None, &state).await?;
        }
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
