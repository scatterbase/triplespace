//! Building a `records` export from the live store (21 §4.2; 0047 §8; 0006 §9): the
//! records of the selected pages and local entities, from the tenant's `pages` and `local`
//! partitions, each shown as the export scope shows it to the reader and proved against
//! its partition's latest checkpoint, with that checkpoint and the instance's `key:`
//! records. A record past the latest checkpoint is left out and listed as pending; the
//! `log` partition is never read. The document and its verification are
//! [`scatter_integrity::records`].

use std::collections::BTreeMap;

use scatter_integrity::bundle::Withhold;
use scatter_integrity::records::{Entry, RecordsExport, RecordsPartition};
use scatter_log_postgres::{PgClient, checkpoints, ids, log, merkle};
use tokio_postgres::GenericClient;
use triplespace_projections::common::target_kind;

pub use scatter_integrity::records::{MEDIA_TYPE, RecordsExport as Export};

use crate::bundle::{Offsets, PartitionRef, Reader, key_records, origin_of, published, redact};

/// What to export: pages and local entities, each with the text it was asked by.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Page IDs, each with its title as asked.
    pub pages: Vec<(u64, String)>,
    /// Local entity IDs, each as asked.
    pub entities: Vec<(String, String)>,
    /// Every revision, up to the caps, rather than the current one.
    pub history: bool,
    /// Records per page, at most (`export.max_history`).
    pub max_page_history: usize,
    /// Records per entity, at most (`export.max_entity_history`).
    pub max_entity_history: usize,
}

/// Why an export was not built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// More records than one request may stream (`export.sync_revisions`).
    #[error("the selection holds {records} records, more than the {limit} an export streams")]
    TooLarge {
        /// The records selected.
        records: usize,
        /// The limit.
        limit: usize,
    },
    /// The database failed.
    #[error("{0}")]
    Database(String),
    /// Writing the document failed.
    #[error("writing the export: {0}")]
    Io(String),
}

fn db(e: impl std::fmt::Display) -> BuildError {
    BuildError::Database(e.to_string())
}

/// Builds the export of `selection` on `tenant` for `reader`, refusing more than `limit`
/// records. A target with no records the reader may see, or only records of a target a
/// confidential restriction hides, is listed as missing, so that the export says no more
/// of it than an absent one (0056 §5).
///
/// # Errors
///
/// A selection over `limit`, or a database failure.
#[allow(clippy::too_many_lines)]
pub async fn build<C: PgClient + GenericClient>(
    client: &C,
    tenant: &str,
    reader: &Reader,
    selection: &Selection,
    limit: usize,
    created: u64,
) -> Result<RecordsExport, BuildError> {
    let mut rows = Vec::new();
    for name in ["pages", "local"] {
        if let Some(r) = published(client, tenant, reader, PartitionRef::Name(name))
            .await
            .map_err(db)?
        {
            rows.push(r);
        }
    }
    let ids: Vec<i64> = rows
        .iter()
        .map(|r| ids::partition_to_db(r.partition))
        .collect();
    let page_ids: Vec<String> = selection
        .pages
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();
    let entity_ids: Vec<String> = selection
        .entities
        .iter()
        .map(|(id, _)| id.clone())
        .collect();
    let found = PgClient::query(
        client,
        "SELECT partition, \"offset\", target_kind, target_id FROM view.activity
         WHERE tenant = $1 AND partition = ANY($2)
           AND ((target_kind = $3 AND target_id = ANY($4))
             OR (target_kind = $5 AND target_id = ANY($6)))
         ORDER BY partition, \"offset\"",
        &[
            &tenant,
            &ids,
            &target_kind::PAGE,
            &page_ids,
            &target_kind::ENTITY,
            &entity_ids,
        ],
    )
    .await
    .map_err(db)?;
    // Each target's records, oldest first, cut to the newest its cap allows.
    let mut by_target: BTreeMap<(i16, String), Vec<(u64, u64)>> = BTreeMap::new();
    for r in &found {
        let at = (
            ids::partition_from_db(r.get(0)),
            u64::try_from(r.get::<_, i64>(1)).unwrap_or_default(),
        );
        by_target.entry((r.get(2), r.get(3))).or_default().push(at);
    }
    for ((kind, _), records) in &mut by_target {
        let cap = match (selection.history, *kind) {
            (false, _) => 1,
            (true, target_kind::PAGE) => selection.max_page_history,
            (true, _) => selection.max_entity_history,
        };
        let cut = records.len().saturating_sub(cap.max(1));
        records.drain(..cut);
    }
    let total: usize = by_target.values().map(Vec::len).sum();
    if total > limit {
        return Err(BuildError::TooLarge {
            records: total,
            limit,
        });
    }
    let offsets: Vec<u64> = by_target.values().flatten().map(|&(_, o)| o).collect();
    let withhold = reader
        .decide(client, tenant, &ids, Offsets::At(&offsets))
        .await
        .map_err(db)?;
    // A target shown only as leaves is one the reader may not know of.
    let mut missing = Vec::new();
    let mut chosen: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let asked = |kind: i16, id: &str| -> String {
        if kind == target_kind::PAGE {
            selection
                .pages
                .iter()
                .find(|(p, _)| p.to_string() == id)
                .map_or_else(|| id.to_string(), |(_, t)| t.clone())
        } else {
            selection
                .entities
                .iter()
                .find(|(e, _)| e == id)
                .map_or_else(|| id.to_string(), |(_, t)| t.clone())
        }
    };
    for (t, title) in &selection.pages {
        if !by_target.contains_key(&(target_kind::PAGE, t.to_string())) {
            missing.push(title.clone());
        }
    }
    for (e, as_asked) in &selection.entities {
        if !by_target.contains_key(&(target_kind::ENTITY, e.clone())) {
            missing.push(as_asked.clone());
        }
    }
    for ((kind, id), records) in &by_target {
        if records
            .iter()
            .all(|at| matches!(withhold.get(at), Some(Withhold::Record)))
        {
            missing.push(asked(*kind, id));
            continue;
        }
        for &(p, o) in records {
            chosen.entry(p).or_default().push(o);
        }
    }
    let mut partitions = Vec::new();
    for row in &rows {
        let Some(mut offsets) = chosen.remove(&row.partition) else {
            continue;
        };
        offsets.sort_unstable();
        offsets.dedup();
        let checkpoint = checkpoints::list(client, row.partition)
            .await
            .map_err(db)?
            .pop();
        let size = checkpoint.as_ref().map_or(0, |c| c.checkpoint.size);
        let (covered, pending): (Vec<u64>, Vec<u64>) = offsets.into_iter().partition(|&o| o < size);
        // The recorded origin (01 §4.2; 0006 A29), which the checkpoint, if any, carries.
        let origin = origin_of(client, row, "").await.map_err(db)?;
        let (nodes, paths) = if covered.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            merkle::proofs(client, row.partition, &covered, size)
                .await
                .map_err(db)?
        };
        let mut entries = Vec::with_capacity(covered.len());
        for (&offset, path) in covered.iter().zip(paths) {
            let slot = log::read(client, row.partition, offset).await.map_err(db)?;
            entries.push(Entry {
                offset,
                shown: redact(slot, withhold.get(&(row.partition, offset))).map_err(db)?,
                path: path.into_iter().map(|i| i as u64).collect(),
            });
        }
        partitions.push(RecordsPartition {
            partition: row.partition,
            name: row.name.clone(),
            origin,
            checkpoint,
            nodes: nodes.into_iter().map(|(_, _, h)| h).collect(),
            entries,
            pending,
        });
    }
    Ok(RecordsExport {
        created,
        keys: key_records(client).await.map_err(db)?,
        missing,
        partitions,
    })
}
