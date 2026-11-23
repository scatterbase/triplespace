//! `view.same_as`: the `same-as` links a tenant's own graphs have written, until identity
//! clusters are projected (0078 A10). The resolution of an entity source's ID reads it, so
//! that `MHC:Q1` reaches the local item minted for it (05 §6.5, §6.6); nothing else does.
//!
//! A `same-as` in a change set of a tenant graph that is not a mirror (`local`, `pages`, a
//! derived graph) adds a row in each direction; a `remove` of the link deletes them. An
//! erased record deletes the links keyed by its key, since its content is gone.

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_wikibase_changeset::{Link, Operation};

use crate::backend::PgCx;
use crate::common::{PAYLOAD_CHANGESET, offset_db, operation, partition_info};
use scatter_log_postgres::PgClient;

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// The projection.
#[derive(Debug, Default, Clone, Copy)]
pub struct SameAsProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for SameAsProjection {
    fn name(&self) -> &'static str {
        "same_as"
    }

    fn step(&self) -> Step {
        Step::Activity
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CHANGESET
    }

    /// Each record's rows depend on no other record's, applied in order.
    fn repeats_in_block(&self, _earlier: &[&Record], _later: &Record) -> bool {
        true
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let info = partition_info(cx, h.partition).await?;
            if info.tenant.is_empty() || info.is_mirror() {
                return Ok(Applied::rows(0));
            }
            let partition = scatter_log_postgres::ids::partition_to_db(h.partition);
            let offset = offset_db(record)?;
            let Some(op) = operation(record)? else {
                // Erased: the links this record keyed go with it.
                let Some(key) = h.key.as_deref() else {
                    return Ok(Applied::rows(0));
                };
                let n = cx
                    .execute(
                        "DELETE FROM view.same_as WHERE tenant = $1 AND (id = $2 OR other = $2)
                           AND \"partition\" = $3",
                        &[&info.tenant, &key, &partition],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
                return Ok(Applied::rows(usize::try_from(n).unwrap_or(0)));
            };
            let (ids, add) = match op.as_ref() {
                Operation::SameAs { ids, .. } => (ids, true),
                Operation::Remove {
                    link: Some(Link::SameAs { ids }),
                    ..
                } => (ids, false),
                _ => return Ok(Applied::rows(0)),
            };
            let [a, b] = ids.as_slice() else {
                return Ok(Applied::rows(0));
            };
            let (a, b) = (a.as_str(), b.as_str());
            let n = if add {
                cx.execute(
                    "INSERT INTO view.same_as (tenant, id, other, \"partition\", \"offset\")
                     VALUES ($1, $2, $3, $4, $5), ($1, $3, $2, $4, $5)
                     ON CONFLICT (tenant, id, other) DO UPDATE
                       SET \"partition\" = EXCLUDED.\"partition\", \"offset\" = EXCLUDED.\"offset\"",
                    &[&info.tenant, &a, &b, &partition, &offset],
                )
                .await
            } else {
                cx.execute(
                    "DELETE FROM view.same_as
                     WHERE tenant = $1 AND ((id = $2 AND other = $3) OR (id = $3 AND other = $2))",
                    &[&info.tenant, &a, &b],
                )
                .await
            }
            .map_err(|e| sql(&e))?;
            Ok(Applied::rows(usize::try_from(n).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let partition = scatter_log_postgres::ids::partition_to_db(partition);
            cx.batch_execute(&format!(
                "DELETE FROM view.same_as WHERE \"partition\" = {partition}"
            ))
            .await
            .map_err(|e| sql(&e))
        })
    }
}

/// The IDs the tenant's own graphs link to `id` as the same item, in text order.
///
/// # Errors
///
/// The database's.
pub async fn linked<C: PgClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<Vec<String>, String> {
    let rows = client
        .query(
            "SELECT other FROM view.same_as WHERE tenant = $1 AND id = $2 ORDER BY other",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}
