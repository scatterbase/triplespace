//! Bootstrap mode (0013 §9; 05 §3.7–3.8): the initial load of an empty mirror partition.
//!
//! An initial load has no previous state to read, so every record it writes is known from
//! the dump alone: a new entity's `put` (prev_upstream, prev_size and delta absent,
//! `first_seen` now, the changes against nothing) as [`crate::sync::run_sync_in`] writes for
//! a subject with no cursor. That is what lets the work be split. [`Prepare::item`] turns
//! one sync item into a draft and its body's commitment, touching no store: the hash check,
//! the operation, its canonical encoding and the part hashes, which a caller runs on worker
//! threads. [`run_bootstrap`] takes the prepared records in dump order and, a block at a
//! time, gives them page IDs from one reservation, appends them (binary `COPY` in Postgres)
//! and commits.
//!
//! The projections run during the load, over each block at once
//! ([`scatter_projection::Pipeline::apply_inline_many`]; 0013 A71): set-based where a
//! projection has a form for it, record by record otherwise, so the record table keeps its
//! key index for that path. A load runs in units of `batch`
//! records, each committed: a load that fails part way leaves a partition without holes,
//! which a rerun (no longer empty) continues through the ordinary sync, whose version
//! cursor skips what is already there.
//!
//! Several blocks are written at once where the store allows it (0013 A82): the coordinator
//! gives each block, in dump order, its page IDs, offsets and tree nodes, and each block's
//! statements run in a unit of work of its own, up to `writers` at a time. A block moves
//! the partition's next offset and commits only after every earlier block has committed, so
//! what has committed is always a prefix of whole blocks, as with one block at a time.
//!
//! A block's commit does not wait for the disk ([`IngestStore::commit_waits_for_disk`];
//! 0013 A72): a crash loses at most the last blocks committed, which the rerun loads
//! again; the last unit, which finishes the job, waits, and so makes the whole load
//! durable once it returns.
//!
//! The tables a load writes grow from nothing faster than the database's own statistics
//! follow, so the load refreshes them ([`IngestStore::refresh_statistics`]) each time the
//! number of blocks written doubles (0013 A73): a plan chosen for an empty table scans every row
//! written so far on every block, which makes a load quadratic.
//!
//! A [`Prepare`] may be given the projections' per-record work ([`Warm`]; 0013 A74), which
//! then runs on the threads that prepare the records, ahead of the coordinator, so that
//! applying a block is little more than its statements.
//!
//! Only an instance mirror whose subjects are their own keys is bootstrapped: a keyed
//! subject's surrogate is minted on the write path (0009 §7), so a key-mapped provider
//! loads through the ordinary sync.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};

use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::hash::Hash;
use scatter_log::record::{Record, global_id};
use scatter_log::store::Draft;
use scatter_log::tree::Frontier;
use scatter_projection::{Budget, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{Changes, Graph, JobHeader, Mode, Operation};
use scatter_wikibase_model::hash::{HashMismatches, Hasher};
use scatter_wikibase_model::id::{EntityId, IdForm};

use crate::IngestError;
use crate::job::{Counts, Job, Reject};
use crate::store::{IngestStore, Sequence};
use crate::sync::{SyncItem, SyncOutcome};
use crate::write::{Attestation, PAYLOAD_CHANGESET};

/// What preparing a record needs, fixed for the whole load: shared by the worker threads.
#[derive(Clone)]
pub struct Prepare {
    hasher: Hasher,
    attestation: Value,
    now: u64,
    provider_number: u64,
    graph: String,
    warm: Option<Warm>,
}

/// Work the projections do per record, run where the record is prepared (0013 A74): given
/// the graph, the record's body and the operation it carries, a projection can compute
/// ahead, off the coordinator, what it would otherwise compute from the record when its
/// block is applied. The records are appended as prepared, so the content bytes it sees
/// are the record's.
pub type Warm = std::sync::Arc<dyn Fn(&str, &Body, Operation) + Send + Sync>;

impl std::fmt::Debug for Prepare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepare")
            .field("hasher", &self.hasher)
            .field("now", &self.now)
            .field("provider_number", &self.provider_number)
            .field("graph", &self.graph)
            .field("warm", &self.warm.is_some())
            .finish_non_exhaustive()
    }
}

/// One sync item, prepared.
#[derive(Debug)]
pub enum Prepared {
    /// A record to append, its body's commitment computed. The draft has no page ID yet:
    /// [`run_bootstrap`] assigns them in order.
    Record {
        /// The draft.
        draft: Draft,
        /// `draft.body.commitment()`.
        commitment: Hash,
        /// What the hash check found, for the job's counts.
        mismatches: HashMismatches,
        /// Whether it is a new subject's `put` (`created`), else a redirect (`other`).
        created: bool,
    },
    /// The adapter's reject, for the job's rejects (its state, if any, follows separately).
    Rejected {
        /// The upstream ID.
        upstream_id: String,
        /// Why.
        reason: String,
    },
    /// Nothing to write: an upstream deletion of an entity the partition never held.
    Unchanged,
}

impl Prepare {
    /// The same, with `warm` run on each record prepared ([`Warm`]).
    #[must_use]
    pub fn warmed_by(mut self, warm: Warm) -> Self {
        self.warm = Some(warm);
        self
    }

    /// The record a sync item becomes in an empty partition. Touches no store.
    pub fn item(&self, item: SyncItem) -> Result<Prepared, IngestError> {
        match item {
            SyncItem::State {
                entity,
                upstream,
                upstream_id,
                mut upstream_aliases,
            } => {
                let mut entity = *entity;
                let mismatches = self.hasher.reconcile(&mut entity);
                upstream_aliases.sort();
                upstream_aliases.dedup();
                let size = entity.to_canonical_json().len() as u64;
                let changes = Changes::between(None, &entity);
                let id = entity.id.clone();
                let revid = upstream.revid;
                let op = Operation::Put {
                    id: id.clone(),
                    entity,
                    upstream,
                    prev_upstream: None,
                    first_seen: Some(self.now),
                    size: Some(size),
                    prev_size: None,
                    changes: Some(changes),
                    delta: None,
                    upstream_id,
                    upstream_aliases,
                };
                let (draft, commitment) = self.draft(&id, revid, op)?;
                Ok(Prepared::Record {
                    draft,
                    commitment,
                    mismatches,
                    created: true,
                })
            }
            SyncItem::Redirected { from, to, upstream } => {
                let revid = upstream.revid;
                let op = Operation::Redirect {
                    from: from.clone(),
                    to,
                    upstream: Some(upstream),
                };
                let (draft, commitment) = self.draft(&from, revid, op)?;
                Ok(Prepared::Record {
                    draft,
                    commitment,
                    mismatches: HashMismatches::default(),
                    created: false,
                })
            }
            SyncItem::Deleted { .. } => Ok(Prepared::Unchanged),
            SyncItem::Rejected {
                upstream_id,
                reason,
            } => Ok(Prepared::Rejected {
                upstream_id,
                reason,
            }),
        }
    }

    /// The draft of a mirror operation on `subject`, as [`crate::write::write_to`] builds it
    /// (the subject its own key, the provider-ranged revision ID, no comment), and its
    /// body's commitment, the operation then handed to [`Warm`] if there is one. Mirror
    /// operations carry their adapter's statement IDs, so none are filled in.
    fn draft(
        &self,
        subject: &EntityId,
        upstream_revid: Option<u64>,
        op: Operation,
    ) -> Result<(Draft, Hash), IngestError> {
        if subject.form() == IdForm::Keyed {
            return Err(IngestError::Adapter(format!(
                "`{subject}` is keyed: its surrogate is minted on the write path, so its provider loads through the ordinary sync"
            )));
        }
        let revid = upstream_revid
            .map(|n| {
                global_id(self.provider_number, n).ok_or_else(|| {
                    IngestError::Adapter(format!("upstream revision {n} is out of range"))
                })
            })
            .transpose()?;
        let content = serde_json::to_value(&op).map_err(|e| e.to_string())?;
        let body = Body::core(&Value::from_json(&content), &Value::Null, &self.attestation)
            .map_err(|e| e.to_string())?;
        drop(content);
        let commitment = body.commitment();
        if let Some(warm) = &self.warm {
            warm(&self.graph, &body, op);
        }
        Ok((
            Draft {
                appended_at: self.now,
                payload_type: PAYLOAD_CHANGESET.into(),
                key: Some(subject.as_str().to_string()),
                revid,
                logid: None,
                page_id: None,
                body,
            },
            commitment,
        ))
    }
}

/// Loads an empty instance mirror partition from prepared records (see the module).
/// `header` names the `mirror/{slug}` graph and the mode, as for [`crate::sync::run_sync`];
/// `items` is given the [`Prepare`] for this job (its attestation names the job) and returns
/// the prepared records in dump order, typically from worker threads. Up to `writers`
/// blocks are written at once when the store allows it ([`IngestStore::concurrent_blocks`];
/// 0013 A82), committing in dump order; otherwise, or with `writers` 1, one at a time. A
/// non-empty partition is refused: an incremental load is the ordinary sync's.
///
/// # Errors
///
/// When the graph is not an instance mirror, the partition holds records, an item cannot
/// be prepared or the store fails.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_bootstrap<S, F, I>(
    store: &S,
    pipeline: &Pipeline<S>,
    header: JobHeader,
    attestation: Attestation,
    registry: &Registry,
    now: u64,
    batch: usize,
    writers: usize,
    items: F,
) -> Result<SyncOutcome, IngestError>
where
    S: IngestStore + Sync,
    S::Cx: Send,
    Pipeline<S>: Sync,
    F: FnOnce(Prepare) -> I,
    I: IntoIterator<Item = Result<Prepared, IngestError>>,
{
    let graph = header.graph();
    let Some(Graph::Mirror(slug)) = &graph else {
        return Err(IngestError::Adapter(format!(
            "bootstrap mode loads an instance mirror, not `{}`",
            header.graph
        )));
    };
    if header.mode.unwrap_or(Mode::Upsert) == Mode::Adopt {
        return Err(IngestError::Adapter(
            "a sync is `upsert` or `snapshot`".into(),
        ));
    }
    let provider = registry
        .by_slug(slug)
        .ok_or_else(|| IngestError::UnknownProvider(slug.clone()))?;
    let graph_name = format!("mirror/{slug}");

    // One load of a source and graph at a time (0011 A37).
    let _lock = store
        .lock_load(
            "",
            &header.source,
            &header.graph,
            Some(crate::job::LOAD_LOCK_WAIT),
        )
        .await?;
    let mut cx = store.begin().await?;
    let partition = store
        .partition(&mut cx, "", &graph_name)
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: String::new(),
            graph: graph_name.clone(),
        })?;
    let size = store.partition_size(&mut cx, partition).await?;
    if size != 0 {
        return Err(IngestError::Adapter(format!(
            "bootstrap mode needs an empty partition; `{graph_name}` holds {size} records"
        )));
    }
    let mut job = Job::start(store, pipeline, &mut cx, "", header, attestation, now).await?;
    // A load of the same source and graph whose process died is marked so (0011 A36).
    job.interrupt_earlier(store, pipeline, &mut cx, now).await?;
    let prepare = Prepare {
        hasher: Hasher::mirrored_from(&provider.code),
        attestation: job.record_attestation().to_value(),
        now,
        provider_number: u64::from(provider.number),
        graph: graph_name.clone(),
        warm: None,
    };
    let mut tally = Tally::default();
    let batch = batch.max(1);
    let cx = if writers > 1 && store.concurrent_blocks() {
        // The job's start commits on its own, so that no block waits for it.
        store.commit(cx).await?;
        let mut writer = Writers::new(store, pipeline, partition, writers);
        let mut block = Vec::with_capacity(batch);
        for prepared in items(prepare) {
            writer.progress().await?;
            if let Some(entry) = tally.take(&mut job, prepared?) {
                block.push(entry);
                if block.len() >= batch {
                    writer.dispatch(std::mem::take(&mut block)).await?;
                }
            }
        }
        writer.dispatch(block).await?;
        writer.finish().await?;
        store.begin().await?
    } else {
        let mut cx = cx;
        let mut block = Vec::with_capacity(batch);
        let mut blocks = 0u64;
        for prepared in items(prepare) {
            if let Some(entry) = tally.take(&mut job, prepared?) {
                block.push(entry);
                if block.len() >= batch {
                    write_block(
                        store,
                        pipeline,
                        &mut cx,
                        partition,
                        std::mem::take(&mut block),
                    )
                    .await?;
                    store.commit(cx).await?;
                    cx = store.begin().await?;
                    store.commit_waits_for_disk(&mut cx, false).await?;
                    blocks += 1;
                    // The tables have doubled since the last refresh (0013 A73): about 20 for a
                    // full Wikidata load.
                    if blocks.is_power_of_two() {
                        store.refresh_statistics(&mut cx).await?;
                        store.commit(cx).await?;
                        cx = store.begin().await?;
                        store.commit_waits_for_disk(&mut cx, false).await?;
                    }
                }
            }
        }
        write_block(store, pipeline, &mut cx, partition, block).await?;
        cx
    };
    // The last unit waits for the disk, and with it every block committed before it.
    let mut cx = cx;
    store.commit_waits_for_disk(&mut cx, true).await?;
    job.finish(
        store,
        pipeline,
        &mut cx,
        &tally.counts,
        serde_json::Map::new(),
        now,
    )
    .await?;
    store.commit(cx).await?;
    Ok(SyncOutcome {
        job_id: job.id,
        counts: tally.counts,
        swept: 0,
    })
}

/// A load's counts and rejects, kept as the prepared items arrive in dump order.
#[derive(Default)]
struct Tally {
    counts: Counts,
    rejected_line: usize,
}

/// A record to write: its draft, its body's commitment, and whether it is a `put`.
type Entry = (Draft, Hash, bool);

impl Tally {
    /// Counts one prepared item into the job, returning the record to write, if any.
    fn take(&mut self, job: &mut Job, prepared: Prepared) -> Option<Entry> {
        match prepared {
            Prepared::Record {
                draft,
                commitment,
                mismatches,
                created,
            } => {
                for (kind, n) in mismatches.by_kind() {
                    job.mismatch(kind, *n);
                }
                if created {
                    self.counts.created += 1;
                } else {
                    self.counts.other += 1;
                }
                Some((draft, commitment, created))
            }
            Prepared::Rejected {
                upstream_id,
                reason,
            } => {
                self.counts.rejected += 1;
                self.rejected_line += 1;
                job.reject(Reject {
                    line: self.rejected_line,
                    match_key: None,
                    reason: format!("{upstream_id}: {reason}"),
                });
                None
            }
            Prepared::Unchanged => {
                self.counts.unchanged += 1;
                None
            }
        }
    }
}

/// Gives a block its page IDs from one reservation, appends it, and runs the synchronous
/// projections over the block ([`Pipeline::apply_inline_many`]): set-based where a
/// projection has a form for it, record by record otherwise.
async fn write_block<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    partition: u64,
    block: Vec<Entry>,
) -> Result<(), IngestError> {
    if block.is_empty() {
        return Ok(());
    }
    let pages = store.next_ids(cx, "", &Sequence::Page, block.len()).await?;
    let block = block
        .into_iter()
        .zip(pages)
        .map(|((mut draft, commitment, _), page)| {
            draft.page_id = Some(page);
            (draft, commitment)
        })
        .collect();
    crate::write::append_projected_many(store, pipeline, cx, partition, block, Budget::NONE)
        .await?;
    Ok(())
}

/// Whether a block must wait for every earlier block to commit before its unit of work
/// starts (0013 A82): when it holds anything but `put`s, names a subject twice or has a
/// record with no key, or names a subject a block in flight names.
fn waits_for_earlier<'s>(
    block: &[Entry],
    subjects: &BTreeSet<String>,
    mut inflight: impl Iterator<Item = &'s BTreeSet<String>>,
) -> bool {
    block.iter().any(|(_, _, put)| !put)
        || subjects.len() < block.len()
        || inflight.any(|s| !s.is_disjoint(subjects))
}

/// A block in flight: its unit of work's future, which resolves when it has committed,
/// rolled back after an earlier block failed, or failed.
type Flight<'a> = Pin<Box<dyn Future<Output = Result<(), IngestError>> + Send + 'a>>;

/// Several blocks in flight, committed in dump order (0013 §9, A82). The coordinator, on
/// the caller's task, gives each block in turn its page IDs, offsets and tree nodes, the
/// tree's frontier kept in memory; each block's statements then run in a unit of work of
/// its own, and its last statement, which moves the partition's next offset, runs once
/// every earlier block has committed, followed by its commit (in Postgres, the
/// projections' positions are written at commit too). So every state a reader or a crash
/// can see is a prefix of whole blocks. The blocks in flight are driven by the
/// coordinator's own task, polled between the items it takes, so no runtime is named.
///
/// Two blocks in flight never name the same subject, and a block with anything but a
/// `put` waits for every earlier block: a block's projections read what earlier blocks
/// wrote about its subjects, which they could not see before those commit, and a later
/// block's write to a row an earlier one holds would wait on it while it waits, in order,
/// on the later one.
struct Writers<'a, S: IngestStore> {
    store: &'a S,
    pipeline: &'a Pipeline<S>,
    partition: u64,
    /// The most blocks in flight at once.
    limit: usize,
    frontier: Frontier,
    next_offset: u64,
    /// Blocks dispatched so far, the next block's index.
    dispatched: u64,
    /// How many blocks, in order, have committed; and whether one has failed.
    order: Arc<Order>,
    inflight: Vec<(Flight<'a>, BTreeSet<String>)>,
    /// The first failure, kept while the blocks still in flight roll back.
    failed: Option<IngestError>,
}

#[derive(Default)]
struct Order {
    committed: AtomicU64,
    failed: AtomicBool,
}

impl<'a, S> Writers<'a, S>
where
    S: IngestStore + Sync,
    S::Cx: Send,
    Pipeline<S>: Sync,
{
    fn new(store: &'a S, pipeline: &'a Pipeline<S>, partition: u64, writers: usize) -> Self {
        Self {
            store,
            pipeline,
            partition,
            limit: writers,
            frontier: Frontier::new(),
            next_offset: 0,
            dispatched: 0,
            order: Arc::default(),
            inflight: Vec::new(),
            failed: None,
        }
    }

    /// Polls every block in flight until a pass completes none, without waiting; returns
    /// how many completed, keeping the first failure.
    fn poll_all(&mut self, cx: &mut Context<'_>) -> usize {
        let mut done = 0;
        loop {
            let mut progressed = false;
            let mut i = 0;
            while i < self.inflight.len() {
                if let Poll::Ready(r) = self.inflight[i].0.as_mut().poll(cx) {
                    drop(self.inflight.swap_remove(i));
                    if let Err(e) = r {
                        self.order.failed.store(true, Ordering::SeqCst);
                        self.failed.get_or_insert(e);
                    }
                    done += 1;
                    progressed = true;
                } else {
                    i += 1;
                }
            }
            if !progressed {
                return done;
            }
        }
    }

    /// Moves the blocks in flight as far as they go now; fails once one has failed and
    /// every other has rolled back.
    async fn progress(&mut self) -> Result<(), IngestError> {
        std::future::poll_fn(|cx| {
            self.poll_all(cx);
            Poll::Ready(())
        })
        .await;
        if self.failed.is_some() {
            self.drain(0).await?;
        }
        Ok(())
    }

    /// Waits until at most `left` blocks are in flight; fails, once none is, if one failed.
    async fn drain(&mut self, left: usize) -> Result<(), IngestError> {
        std::future::poll_fn(|cx| {
            self.poll_all(cx);
            let left = if self.failed.is_some() { 0 } else { left };
            if self.inflight.len() > left {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        match self.failed.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Runs `fut` on the coordinator while the blocks in flight keep moving.
    async fn alongside<T>(
        &mut self,
        fut: impl Future<Output = Result<T, IngestError>>,
    ) -> Result<T, IngestError> {
        let mut fut = std::pin::pin!(fut);
        let out = std::future::poll_fn(|cx| {
            self.poll_all(cx);
            fut.as_mut().poll(cx)
        })
        .await;
        if self.failed.is_some() {
            self.drain(0).await?;
        }
        out
    }

    /// Seals a block at the next offsets, folds its leaves into the frontier, and sets its
    /// unit of work going once fewer than `limit` blocks are in flight.
    async fn dispatch(&mut self, block: Vec<Entry>) -> Result<(), IngestError> {
        if block.is_empty() {
            return Ok(());
        }
        let subjects: BTreeSet<String> =
            block.iter().filter_map(|(d, _, _)| d.key.clone()).collect();
        let alone = waits_for_earlier(&block, &subjects, self.inflight.iter().map(|(_, s)| s));
        self.drain(if alone { 0 } else { self.limit - 1 }).await?;
        let store = self.store;
        let n = block.len();
        let pages = self
            .alongside(async move {
                let mut c = store.begin().await?;
                let pages = store.next_ids(&mut c, "", &Sequence::Page, n).await?;
                store.commit(c).await?;
                Ok(pages)
            })
            .await?;
        let partition = self.partition;
        let start = self.next_offset;
        let records: Vec<Record> = block
            .into_iter()
            .zip(pages)
            .zip(start..)
            .map(|(((mut draft, commitment, _), page), offset)| {
                draft.page_id = Some(page);
                draft.seal_committed(partition, offset, commitment)
            })
            .collect();
        let mut nodes = Vec::with_capacity(records.len() * 2);
        for r in &records {
            nodes.extend(self.frontier.push_leaf_completing(r.leaf()));
        }
        let end = start + records.len() as u64;
        self.next_offset = end;
        let index = self.dispatched;
        self.dispatched += 1;
        let pipeline = self.pipeline;
        let order = Arc::clone(&self.order);
        let flight: Flight<'a> = Box::pin(async move {
            let mut c = store.begin().await?;
            let written = async {
                store.commit_waits_for_disk(&mut c, false).await?;
                store
                    .write_sealed(&mut c, partition, &records, &nodes)
                    .await?;
                pipeline
                    .apply_inline_many(store, &mut c, &records, Budget::NONE)
                    .await?;
                // The blocks before this one commit first, or one of them fails.
                std::future::poll_fn(|_| {
                    if order.failed.load(Ordering::SeqCst)
                        || order.committed.load(Ordering::SeqCst) >= index
                    {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                if order.failed.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                store
                    .advance_partition(&mut c, partition, start, end)
                    .await?;
                Ok::<_, IngestError>(true)
            }
            .await;
            match written {
                Ok(true) => {
                    store.commit(c).await?;
                    order.committed.store(index + 1, Ordering::SeqCst);
                    Ok(())
                }
                Ok(false) => {
                    store.rollback(c).await?;
                    Ok(())
                }
                Err(e) => {
                    order.failed.store(true, Ordering::SeqCst);
                    let _ = store.rollback(c).await;
                    Err(e)
                }
            }
        });
        self.inflight.push((flight, subjects));
        // The tables have doubled since the last refresh (0013 A73).
        if self.dispatched.is_power_of_two() {
            self.alongside(async move {
                let mut c = store.begin().await?;
                store.refresh_statistics(&mut c).await?;
                store.commit(c).await?;
                Ok(())
            })
            .await?;
        }
        Ok(())
    }

    /// Waits for every block in flight to commit.
    async fn finish(mut self) -> Result<(), IngestError> {
        self.drain(0).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, put: bool) -> Entry {
        let body = Body::core(&Value::Null, &Value::Null, &Value::Null).unwrap();
        let commitment = body.commitment();
        (
            Draft {
                appended_at: 0,
                payload_type: PAYLOAD_CHANGESET.into(),
                key: Some(key.into()),
                revid: None,
                logid: None,
                page_id: None,
                body,
            },
            commitment,
            put,
        )
    }

    fn keys(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| (*k).to_string()).collect()
    }

    /// The blocks that may run beside earlier ones, and those that wait (0013 §9, A82).
    #[test]
    fn a_block_waits_when_it_could_meet_an_earlier_one() {
        let block = [entry("Q1", true), entry("Q2", true)];
        let subjects = keys(&["Q1", "Q2"]);
        let inflight = [keys(&["Q3"]), keys(&["Q4", "Q5"])];
        assert!(!waits_for_earlier(&block, &subjects, inflight.iter()));
        // A subject a block in flight names.
        let inflight = [keys(&["Q3"]), keys(&["Q2"])];
        assert!(waits_for_earlier(&block, &subjects, inflight.iter()));
        // A redirect, or anything but a `put`.
        let block = [entry("Q1", true), entry("Q2", false)];
        assert!(waits_for_earlier(&block, &subjects, std::iter::empty()));
        // A subject named twice in the block.
        let block = [entry("Q1", true), entry("Q1", true)];
        assert!(waits_for_earlier(
            &block,
            &keys(&["Q1"]),
            std::iter::empty()
        ));
    }
}
