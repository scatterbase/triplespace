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
    e.to_string()
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

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
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
