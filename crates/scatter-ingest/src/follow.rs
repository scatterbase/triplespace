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
//! One run is one job, with the job's counts summed over its batches, so that
//! `follow --once` and a server's loop are the same code with a different stop.

use std::future::Future;

use scatter_projection::Pipeline;
use scatter_providers::Registry;
use scatter_wikibase_changeset::{JobHeader, Mode};

use crate::IngestError;
use crate::job::Counts;
use crate::store::IngestStore;
use crate::sync::{SyncItem, run_sync_in};
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
pub trait ChangeSource {
    /// The next batch. An empty batch with `caught_up` set means "nothing new".
    fn next(&mut self) -> impl Future<Output = Result<ChangeBatch, IngestError>> + Send;

    /// Saves the position the last batch left: called after that batch's records are
    /// committed, never before, so that a crash between the two replays rather than
    /// skips (0092 §3).
    fn save(&mut self) -> impl Future<Output = Result<(), IngestError>> + Send;
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FollowOutcome {
    /// The job ID.
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

/// Runs a follower until the source is caught up, or forever when `once` is false, as
/// one sync job per batch under one follower run.
///
/// Each batch is a complete sync in `upsert` mode (0092 §2), so its records, counts and
/// rejects are the sync's; the position is saved once the batch's job has finished.
/// `tenant` is `""` for a registry provider's mirror and the tenant for its entity source
/// `slug` (0078 §4), with `registry` the tenant's table of names.
#[allow(clippy::too_many_arguments)]
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
    loop {
        let b = source.next().await?;
        out.batches += 1;
        out.rows += b.rows;
        out.late += b.late;
        out.stale += b.stale;
        if !b.items.is_empty() {
            let version = format!("follow-{}", now());
            let mut header = if tenant.is_empty() {
                JobHeader::mirror(slug, &version, Mode::Upsert)
            } else {
                JobHeader::source(slug, &version, Mode::Upsert)
            };
            header
                .params
                .insert("follow".into(), serde_json::Value::Bool(true));
            let o = run_sync_in(
                store,
                pipeline,
                tenant,
                header,
                attestation.clone(),
                b.items,
                0,
                registry,
                now(),
                batch,
            )
            .await?;
            out.job_id = o.job_id;
            out.counts.created += o.counts.created;
            out.counts.merged += o.counts.merged;
            out.counts.unchanged += o.counts.unchanged;
            out.counts.rejected += o.counts.rejected;
            out.counts.tombstoned += o.counts.tombstoned;
            out.counts.cleared += o.counts.cleared;
            out.counts.other += o.counts.other;
        }
        source.save().await?;
        if b.caught_up && once {
            return Ok(out);
        }
    }
}
