//! `view.graph_state` and `view.entity_revision` (0083 §4, A1): the tier-1 state of the
//! local graph, written in the appending transaction beside the record.
//!
//! For every change-set record of a tenant's `local` partition about an entity, the
//! projection reads the graph's current state for the key from `view.graph_state`,
//! applies the record's operation, writes the new state back (`version` bumped, the
//! record's offset as the graph's latest) and inserts the state after the revision into
//! `view.entity_revision` under the record's revision ID. A past state is then one row,
//! never a replay; [`crate::revisions`] reads it and keeps the fold only for rows the
//! table does not have (a database migrated before a rebuild).
//!
//! An erased record, whose content part is gone, leaves the state as it was: the fold
//! skips it too. A key with no row yet is seeded from its earlier records, so a database
//! that gains the table after the key had records (0083 A1) writes a whole state on the
//! next edit rather than one that starts at that edit; a rebuild fills the rest. The `pages` partition joins when the pages stack writes to it; the
//! `derived/*` and `source/*` graphs join with their writers (0083 §4).
//!
//! The stored form is `LocalState::to_storage_json`, as bytes. It is a serving table's
//! cell, not a record: nothing hashes it, and a rebuild re-derives it from the log.

use scatter_log::header::{CONFIG_PARTITION, Header};
use scatter_log::record::Record;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_wikibase_changeset::apply::LocalState;

use crate::backend::PgCx;
use crate::common::{PAYLOAD_CHANGESET, operation, partition_info};
use crate::sources::entity_subject;
use scatter_log_postgres::PgClient;

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// The graph a partition's records are deltas of, for the state tables: `local` for the
/// tenant's local partition; nothing else yet.
fn delta_graph(name: &str) -> Option<&'static str> {
    match name {
        "local" => Some("local"),
        _ => None,
    }
}

/// `view.graph_state` and `view.entity_revision`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RevisionStateProjection;

/// Reads a graph's current state for a key: the stored row, or an empty state.
pub async fn graph_state<C: PgClient>(
    client: &C,
    tenant: &str,
    entity_id: &str,
    graph: &str,
) -> Result<Option<(LocalState, u64, i64)>, String> {
    let row = client
        .query_opt(
            "SELECT state, \"offset\", version FROM view.graph_state
             WHERE tenant = $1 AND entity_id = $2 AND graph = $3",
            &[&tenant, &entity_id, &graph],
        )
        .await
        .map_err(|e| sql(&e))?;
    row.map(|r| {
        let bytes: Vec<u8> = r.get(0);
        let state = decode(&bytes)?;
        let offset = u64::try_from(r.get::<_, i64>(1)).unwrap_or(0);
        Ok((state, offset, r.get::<_, i64>(2)))
    })
    .transpose()
}

/// Reads the local-graph state after a revision, by revision ID.
pub async fn revision_state<C: PgClient>(
    client: &C,
    tenant: &str,
    revid: u64,
) -> Result<Option<(LocalState, u64, u64)>, String> {
    let row = client
        .query_opt(
            "SELECT state, partition, \"offset\" FROM view.entity_revision
             WHERE tenant = $1 AND revid = $2",
            &[&tenant, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await
        .map_err(|e| sql(&e))?;
    row.map(|r| {
        let bytes: Vec<u8> = r.get(0);
        Ok((
            decode(&bytes)?,
            scatter_log_postgres::ids::partition_from_db(r.get(1)),
            u64::try_from(r.get::<_, i64>(2)).unwrap_or(0),
        ))
    })
    .transpose()
}

/// Reads the local-graph state after the record at an offset.
pub async fn state_at_record<C: PgClient>(
    client: &C,
    partition: u64,
    offset: u64,
) -> Result<Option<(LocalState, u64)>, String> {
    let row = client
        .query_opt(
            "SELECT state, revid FROM view.entity_revision WHERE partition = $1 AND \"offset\" = $2",
            &[
                &scatter_log_postgres::ids::partition_to_db(partition),
                &i64::try_from(offset).unwrap_or(i64::MAX),
            ],
        )
        .await
        .map_err(|e| sql(&e))?;
    row.map(|r| {
        let bytes: Vec<u8> = r.get(0);
        Ok((
            decode(&bytes)?,
            u64::try_from(r.get::<_, i64>(1)).unwrap_or(0),
        ))
    })
    .transpose()
}

/// The stored form.
#[must_use]
pub fn encode(state: &LocalState) -> Vec<u8> {
    state.to_storage_json().to_string().into_bytes()
}

/// From the stored form.
pub fn decode(bytes: &[u8]) -> Result<LocalState, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("stored state: {e}"))?;
    LocalState::from_storage_json(&v)
}

/// Applies one record: the key's state after it, and the revision's row.
async fn apply_record(cx: &PgCx, record: &Record) -> Result<Applied, String> {
    let h = record.header();
    let info = partition_info(cx, h.partition).await?;
    let Some(graph) = delta_graph(&info.name) else {
        return Ok(Applied::default());
    };
    let Some(op) = operation(record)? else {
        return Ok(Applied::default());
    };
    let Some(id) = entity_subject(&op) else {
        return Ok(Applied::default());
    };
    let entity_id = id.as_str().to_string();
    let (mut state, version) =
        if let Some((s, _, v)) = graph_state(cx, &info.tenant, &entity_id, graph).await? {
            (s, v)
        } else {
            // No row yet: either the key's first local record, or a database that
            // gained the table after the key had records (0083 A1). Seed from the
            // records before this one, so the state is whole without a rebuild.
            let earlier = crate::revisions::local_revisions(cx, &info.tenant, &id).await?;
            let mut seed = LocalState::new();
            for r in earlier.iter().filter(|r| r.offset() < h.offset) {
                seed.apply(&r.operation, Some(r.offset()));
            }
            (seed, 0)
        };
    state.apply(&op, Some(h.offset));
    let bytes = encode(&state);
    let offset = i64::try_from(h.offset).map_err(|e| e.to_string())?;
    let mut rows = cx
        .execute(
            "INSERT INTO view.graph_state (tenant, entity_id, graph, state, \"offset\", version)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (tenant, entity_id, graph) DO UPDATE SET
               state = EXCLUDED.state, \"offset\" = EXCLUDED.\"offset\", version = EXCLUDED.version",
            &[&info.tenant, &entity_id, &graph, &bytes, &offset, &(version + 1)],
        )
        .await
        .map_err(|e| sql(&e))?;
    if let Some(revid) = h.revid {
        rows += cx
            .execute(
                "INSERT INTO view.entity_revision (tenant, revid, entity_id, partition, \"offset\", state)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (tenant, revid) DO UPDATE SET
                   entity_id = EXCLUDED.entity_id, partition = EXCLUDED.partition,
                   \"offset\" = EXCLUDED.\"offset\", state = EXCLUDED.state",
                &[
                    &info.tenant,
                    &i64::try_from(revid).map_err(|e| e.to_string())?,
                    &entity_id,
                    &scatter_log_postgres::ids::partition_to_db(h.partition),
                    &offset,
                    &bytes,
                ],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
}

/// [`apply_record`] for a block (0013 A95): the keys' states read in one statement, each
/// key's records folded onto its state in order, and the states and the revisions' rows
/// written in one statement a table. A key with no state yet is seeded from its earlier
/// records as [`apply_record`] seeds it, which one statement finds the need for: none,
/// for a key the block is the first to write. `None` when the block names a revision ID
/// twice, which one statement cannot write.
#[allow(clippy::too_many_lines)]
async fn apply_block(cx: &PgCx, records: &[Record]) -> Result<Option<Applied>, String> {
    let Some(first) = records.first() else {
        return Ok(Some(Applied::default()));
    };
    let info = partition_info(cx, first.header().partition).await?;
    let Some(graph) = delta_graph(&info.name) else {
        return Ok(Some(Applied::default()));
    };
    // The records that say something about an entity, each with its operation.
    let mut entries = Vec::with_capacity(records.len());
    let mut named = std::collections::BTreeSet::new();
    for record in records {
        let Some(op) = operation(record)? else {
            continue;
        };
        let Some(id) = entity_subject(&op) else {
            continue;
        };
        if let Some(revid) = record.header().revid
            && !named.insert(revid)
        {
            return Ok(None);
        }
        entries.push((record, op, id));
    }
    if entries.is_empty() {
        return Ok(Some(Applied::default()));
    }
    let mut ids: Vec<&str> = entries.iter().map(|(_, _, id)| id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut states: std::collections::BTreeMap<String, (LocalState, i64)> = cx
        .query(
            "SELECT entity_id, state, version FROM view.graph_state
             WHERE tenant = $1 AND graph = $2 AND entity_id = ANY($3)",
            &[&info.tenant, &graph, &ids],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| {
            let bytes: Vec<u8> = r.get(1);
            Ok((r.get::<_, String>(0), (decode(&bytes)?, r.get::<_, i64>(2))))
        })
        .collect::<Result<_, String>>()?;
    // A key with no row: its first record here, and whether records came before it.
    let mut unseeded: Vec<(&Record, &scatter_wikibase_model::id::EntityId)> = Vec::new();
    for (record, _, id) in &entries {
        if !states.contains_key(id.as_str()) && !unseeded.iter().any(|(_, u)| *u == id) {
            unseeded.push((record, id));
        }
    }
    if !unseeded.is_empty() {
        let keys: Vec<&str> = unseeded
            .iter()
            .filter_map(|(r, _)| r.header().key.as_deref())
            .collect();
        let earlier: std::collections::BTreeSet<String> = cx
            .query(
                "SELECT DISTINCT key FROM log.record
                 WHERE partition = $1 AND key = ANY($2) AND \"offset\" < $3",
                &[
                    &scatter_log_postgres::ids::partition_to_db(first.header().partition),
                    &keys,
                    &i64::try_from(first.header().offset).map_err(|e| e.to_string())?,
                ],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| r.get(0))
            .collect();
        for (record, id) in unseeded {
            let mut seed = LocalState::new();
            let has_earlier = record
                .header()
                .key
                .as_deref()
                .is_none_or(|k| earlier.contains(k));
            if has_earlier {
                // A database that gained the table after the key had records (0083 A1).
                let before = crate::revisions::local_revisions(cx, &info.tenant, id).await?;
                for r in before
                    .iter()
                    .filter(|r| r.offset() < record.header().offset)
                {
                    seed.apply(&r.operation, Some(r.offset()));
                }
            }
            states.insert(id.as_str().to_string(), (seed, 0));
        }
    }
    // Each record onto its key's state, in order: the state after it is its revision's.
    let partition = scatter_log_postgres::ids::partition_to_db(first.header().partition);
    let (mut rev_ids, mut rev_entities, mut rev_offsets, mut rev_states) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut latest: std::collections::BTreeMap<&str, (Vec<u8>, i64)> =
        std::collections::BTreeMap::new();
    for (record, op, id) in &entries {
        let h = record.header();
        let (state, version) = states
            .get_mut(id.as_str())
            .ok_or("every key of the block has a state")?;
        state.apply(op, Some(h.offset));
        *version += 1;
        let bytes = encode(state);
        let offset = i64::try_from(h.offset).map_err(|e| e.to_string())?;
        if let Some(revid) = h.revid {
            rev_ids.push(i64::try_from(revid).map_err(|e| e.to_string())?);
            rev_entities.push(id.as_str());
            rev_offsets.push(offset);
            rev_states.push(bytes.clone());
        }
        latest.insert(id.as_str(), (bytes, offset));
    }
    let (mut g_ids, mut g_states, mut g_offsets, mut g_versions) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (id, (bytes, offset)) in latest {
        g_versions.push(states[id].1);
        g_ids.push(id);
        g_states.push(bytes);
        g_offsets.push(offset);
    }
    let graph_rows = async {
        cx.execute(
        "INSERT INTO view.graph_state (tenant, entity_id, graph, state, \"offset\", version)
         SELECT $1, t.entity_id, $2, t.state, t.o, t.version
         FROM unnest($3::text[], $4::bytea[], $5::bigint[], $6::bigint[]) AS t(entity_id, state, o, version)
         ON CONFLICT (tenant, entity_id, graph) DO UPDATE SET
           state = EXCLUDED.state, \"offset\" = EXCLUDED.\"offset\", version = EXCLUDED.version",
        &[&info.tenant, &graph, &g_ids, &g_states, &g_offsets, &g_versions],
    )
        .await
    };
    let revision_rows = async {
        if rev_ids.is_empty() {
            return Ok(0);
        }
        cx.execute(
            "INSERT INTO view.entity_revision (tenant, revid, entity_id, partition, \"offset\", state)
             SELECT $1, t.revid, t.entity_id, $2, t.o, t.state
             FROM unnest($3::bigint[], $4::text[], $5::bigint[], $6::bytea[]) AS t(revid, entity_id, o, state)
             ON CONFLICT (tenant, revid) DO UPDATE SET
               entity_id = EXCLUDED.entity_id, partition = EXCLUDED.partition,
               \"offset\" = EXCLUDED.\"offset\", state = EXCLUDED.state",
            &[&info.tenant, &partition, &rev_ids, &rev_entities, &rev_offsets, &rev_states],
        )
        .await
    };
    let (a, b) = tokio::try_join!(graph_rows, revision_rows).map_err(|e| sql(&e))?;
    Ok(Some(Applied::rows(usize::try_from(a + b).unwrap_or(0))))
}

impl<B: Backend<Cx = PgCx>> Projection<B> for RevisionStateProjection {
    fn name(&self) -> &'static str {
        "revision_state"
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.partition != CONFIG_PARTITION && header.payload_type == PAYLOAD_CHANGESET
    }

    /// Reads no other projection's rows (0013 A89): the key's state is folded from its own
    /// table and the log, record by record, in a block as one at a time.
    fn repeats_in_block(&self, _earlier: &[&Record], _later: &Record) -> bool {
        true
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move { apply_record(cx, record).await })
    }

    /// A block's states are read in one statement, folded in memory and written in one
    /// statement a table ([`apply_block`]; 0013 A95); a block that names a revision ID
    /// twice is applied a key at a time ([`crate::common::by_key`]; A93).
    fn apply_many<'a>(
        &'a self,
        cx: &'a mut PgCx,
        records: &'a [Record],
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let cx: &PgCx = cx;
            if let Some(applied) = apply_block(cx, records).await? {
                return Ok(applied);
            }
            crate::common::by_key(records, &|record| Box::pin(apply_record(cx, record))).await
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            let Some(graph) = delta_graph(&info.name) else {
                return Ok(());
            };
            cx.execute(
                "DELETE FROM view.graph_state WHERE tenant = $1 AND graph = $2",
                &[&info.tenant, &graph],
            )
            .await
            .map_err(|e| sql(&e))?;
            cx.execute(
                "DELETE FROM view.entity_revision WHERE partition = $1",
                &[&scatter_log_postgres::ids::partition_to_db(partition)],
            )
            .await
            .map(|_| ())
            .map_err(|e| sql(&e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stored_form_round_trips() {
        let mut s = LocalState::new();
        let op: scatter_wikibase_changeset::Operation = serde_json::from_value(serde_json::json!({
            "op": "add", "id": "Q6", "labels": {"fr": "Six"}
        }))
        .unwrap();
        s.apply(&op, Some(3));
        let back = decode(&encode(&s)).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.offsets, vec![3]);
        assert!(decode(b"nope").is_err());
    }

    #[test]
    fn only_the_local_graph_for_now() {
        assert_eq!(delta_graph("local"), Some("local"));
        assert_eq!(delta_graph("mirror/wikidata"), None);
        assert_eq!(delta_graph("pages"), None);
    }
}
