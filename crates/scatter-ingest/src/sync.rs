//! A mirror sync (0002 §8.4; 0012 §2.2; 0006 §2): an instance job over a provider's
//! mirror partition. Each state the adapter hands over is checked against the version
//! cursor — not newer, or the same content, means `unchanged` — and otherwise written as a
//! `put` carrying `prev_upstream`, `first_seen`, both sizes and the `changes` summary.
//! Upstream snak and reference hashes are recomputed; a differing one is kept and counted
//! (the hash guard). In `snapshot` mode the entities the job did not see are tombstoned
//! once it completes, unless more than the threshold would go.

use std::collections::BTreeSet;

use scatter_log::cbor::Value;
use scatter_projection::{Budget, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{Changes, Graph, JobHeader, Mode, Operation, Upstream};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::EntityId;

use crate::IngestError;
use crate::job::{Counts, Job, Reject};
use crate::store::IngestStore;
use crate::write::{Attestation, Request, write_to};

/// What an adapter hands the sync, with IDs already in stored form.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncItem {
    /// An entity's current upstream state.
    State {
        /// The state.
        entity: Box<Entity>,
        /// Its upstream version.
        upstream: Upstream,
        /// The provider's own item ID, for a key-mapped subject (0009 §9).
        upstream_id: Option<String>,
    },
    /// Upstream deleted the entity.
    Deleted {
        /// The entity.
        id: EntityId,
        /// The deleting version.
        upstream: Upstream,
    },
    /// Upstream merged `from` into `to`.
    Redirected {
        /// The retired ID.
        from: EntityId,
        /// The target.
        to: EntityId,
        /// The merging version.
        upstream: Upstream,
    },
    /// The adapter could not shape an upstream entity as intended and reports it (0009
    /// §9: an invalid or duplicate identity value). Nothing is written for this item;
    /// an adapter that still has a state to write hands that over as a separate `State`.
    Rejected {
        /// The upstream ID.
        upstream_id: String,
        /// Why.
        reason: String,
    },
}

/// What a sync produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// The job ID.
    pub job_id: u64,
    /// The counts on the finish record.
    pub counts: Counts,
    /// Entities the `snapshot` sweep tombstoned.
    pub swept: usize,
}

/// Whether `new` is newer than `old`: by revision ID where both have one, else by
/// inequality of the version strings.
fn is_newer(new: &Upstream, old: &Upstream) -> bool {
    match (new.revid, old.revid) {
        (Some(a), Some(b)) => a > b,
        _ => new.version != old.version || new.revid != old.revid,
    }
}

/// The previous `put` at a cursor, if the record is there and is one.
async fn previous<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    partition: u64,
    offset: u64,
) -> Result<Option<(Entity, Upstream, Option<u64>, Option<u64>)>, IngestError> {
    let Some(record) = store.read(cx, partition, offset).await? else {
        return Ok(None);
    };
    let Some(content) = record.body().content().value().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if content == Value::Null {
        return Ok(None);
    }
    let op: Operation = scatter_log::cbor::from_value(content).map_err(|e| e.to_string())?;
    match op {
        Operation::Put {
            entity,
            upstream,
            first_seen,
            size,
            ..
        } => Ok(Some((entity, upstream, first_seen, size))),
        _ => Ok(None),
    }
}

/// Runs a sync of `header.graph` (a `mirror/{slug}`) in `header.mode` (`upsert` or
/// `snapshot`), `batch` items per unit of work. `threshold` bounds the `snapshot` sweep.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_sync<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    header: JobHeader,
    attestation: Attestation,
    items: impl IntoIterator<Item = SyncItem>,
    threshold: usize,
    registry: &Registry,
    now: u64,
    batch: usize,
) -> Result<SyncOutcome, IngestError> {
    let graph = header.graph();
    let Some(Graph::Mirror(slug)) = &graph else {
        return Err(IngestError::Adapter(format!(
            "a sync writes a mirror graph, not `{}`",
            header.graph
        )));
    };
    let graph = Graph::Mirror(slug.clone());
    let slug = slug.clone();
    let mode = header.mode.unwrap_or(Mode::Upsert);
    if mode == Mode::Adopt {
        return Err(IngestError::Adapter(
            "a sync is `upsert` or `snapshot`".into(),
        ));
    }
    let provider = registry
        .by_slug(&slug)
        .ok_or_else(|| IngestError::UnknownProvider(slug.clone()))?;
    let hasher = Hasher::mirrored_from(&provider.code);
    let graph_name = graph.name();

    let mut cx = store.begin().await?;
    let partition = store
        .partition(&mut cx, "", &graph_name)
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: String::new(),
            graph: graph_name.clone(),
        })?;
    let mut job = Job::start(
        store,
        pipeline,
        &mut cx,
        "",
        header.clone(),
        attestation,
        now,
    )
    .await?;
    let mut request = Request::new("", job.record_attestation(), now);
    request.budget = Budget::NONE;
    let mut counts = Counts::default();
    let mut seen: BTreeSet<EntityId> = BTreeSet::new();
    let mut in_unit = 0usize;
    // Rejects are numbered in the order they arrive; a sync has no batch lines.
    let mut rejected_line = 1usize;

    for item in items {
        if in_unit >= batch.max(1) {
            store.commit(cx).await?;
            cx = store.begin().await?;
            in_unit = 0;
        }
        in_unit += 1;
        let op = match item {
            SyncItem::State {
                entity,
                upstream,
                upstream_id,
            } => {
                let mut entity = *entity;
                let mismatches = hasher.reconcile(&mut entity);
                for (kind, n) in mismatches.by_kind() {
                    job.mismatch(kind, *n);
                }
                seen.insert(entity.id.clone());
                let cursor = store.cursor(&mut cx, "", &entity.id, &graph_name).await?;
                let prev = match &cursor {
                    Some(c) => previous(store, &mut cx, partition, c.offset).await?,
                    None => None,
                };
                if let Some((prev_entity, prev_upstream, _, _)) = &prev
                    && (!is_newer(&upstream, prev_upstream)
                        || prev_entity.to_canonical_json() == entity.to_canonical_json())
                {
                    counts.unchanged += 1;
                    continue;
                }
                let size = entity.to_canonical_json().len() as u64;
                let (prev_upstream, first_seen, prev_size, changes) = match (&prev, &cursor) {
                    (Some((e, u, first, s)), Some(c)) => (
                        Some(u.clone()),
                        Some(first.unwrap_or(c.synced_at)),
                        Some(s.unwrap_or(c.size)),
                        Some(Changes::between(Some(e), &entity)),
                    ),
                    _ => (None, Some(now), None, Some(Changes::between(None, &entity))),
                };
                let id = entity.id.clone();
                Operation::Put {
                    id,
                    entity,
                    upstream,
                    prev_upstream,
                    first_seen,
                    size: Some(size),
                    prev_size,
                    changes,
                    delta: None,
                    upstream_id,
                }
            }
            SyncItem::Deleted { id, upstream } => {
                seen.insert(id.clone());
                if store.cursor(&mut cx, "", &id, &graph_name).await?.is_none() {
                    counts.unchanged += 1;
                    continue;
                }
                Operation::Tombstone { id, upstream }
            }
            SyncItem::Redirected { from, to, upstream } => {
                seen.insert(from.clone());
                Operation::Redirect {
                    from,
                    to,
                    upstream: Some(upstream),
                }
            }
            SyncItem::Rejected {
                upstream_id,
                reason,
            } => {
                counts.rejected += 1;
                job.reject(Reject {
                    line: rejected_line,
                    match_key: None,
                    reason: format!("{upstream_id}: {reason}"),
                });
                rejected_line += 1;
                continue;
            }
        };
        let created = matches!(
            &op,
            Operation::Put {
                prev_upstream: None,
                ..
            }
        );
        write_to(store, pipeline, &mut cx, &request, &op, &graph, registry).await?;
        match &op {
            Operation::Put { .. } if created => counts.created += 1,
            Operation::Put { .. } => counts.merged += 1,
            Operation::Tombstone { .. } => counts.tombstoned += 1,
            _ => counts.other += 1,
        }
    }

    // The snapshot sweep (0002 §8.4): after the job completes, tombstone what it did not
    // see, unless a truncated input would wipe the mirror.
    let mut swept = 0usize;
    let mut extra = serde_json::Map::new();
    if mode == Mode::Snapshot {
        let present = store.entities_of_graph(&mut cx, "", &graph_name).await?;
        let gone: Vec<EntityId> = present
            .into_iter()
            .filter(|id| !seen.contains(id))
            .collect();
        if gone.len() > threshold {
            let err = IngestError::SweepThreshold {
                count: gone.len(),
                threshold,
            };
            job.fail(store, pipeline, &mut cx, &err.to_string(), now)
                .await?;
            store.commit(cx).await?;
            return Err(err);
        }
        let version = Upstream::version(header.source_version().unwrap_or(""));
        for id in gone {
            let op = Operation::Tombstone {
                id,
                upstream: version.clone(),
            };
            write_to(store, pipeline, &mut cx, &request, &op, &graph, registry).await?;
            swept += 1;
            counts.tombstoned += 1;
        }
        extra.insert(
            "sweep".into(),
            serde_json::json!({"count": swept, "threshold": threshold}),
        );
    }
    job.finish(store, pipeline, &mut cx, &counts, extra, now)
        .await?;
    store.commit(cx).await?;
    Ok(SyncOutcome {
        job_id: job.id,
        counts,
        swept,
    })
}
