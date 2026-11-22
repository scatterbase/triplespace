//! Queueing the RDF delta consumer's work (02 §6.2; 03 §4.11, §6.1).
//!
//! The delta consumer is a tier-3 consumer of composition: wherever composition writes or
//! removes an entity's row, or a read restriction changes what the public form holds, the
//! entity is queued for it here. The consumer itself renders, and so lives above this
//! crate, in `triplespace-rdf`.
//!
//! Work is queued only for a tenant whose stream is on, which is a row in
//! `ops.tenant_epoch` (0032 A15): an instance that serves no stream pays one index probe
//! per composition statement and nothing else. A shared row's work is queued under the
//! empty-string tenant, whose epoch row exists while any tenant's stream is on.
//!
//! The work key carries what the consumer needs beyond the entity: the transaction that
//! composed the change, which becomes `rdf_delta.txn`, so that the deltas of one edit are
//! one event (0032 A15), and the record that caused it.

use scatter_log_postgres::PgClient;

use crate::backend::PgCx;

/// The consumer's name in `ops.projection_work`.
pub const CONSUMER: &str = "rdf_delta";

/// One queued change, as its work key names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The composing transaction.
    pub txn: i64,
    /// The entity, in source form.
    pub entity_id: String,
}

impl Change {
    /// Reads a work key, `{txn}:{entity ID}`. An entity ID may itself hold a colon (a
    /// keyed ID), so the key splits at the first.
    #[must_use]
    pub fn parse(key: &str) -> Option<Self> {
        let (txn, id) = key.split_once(':')?;
        Some(Self {
            txn: txn.parse().ok()?,
            entity_id: id.to_string(),
        })
    }
}

/// Queues `ids` on `tenant` for the delta consumer, if the tenant's stream is on.
///
/// # Errors
///
/// The database's.
pub async fn queue(cx: &PgCx, tenant: &str, ids: &[&str]) -> Result<u64, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    cx.execute(
        "INSERT INTO ops.projection_work (projection, tenant, partition, \"offset\", key)
         SELECT $1, $2, 0, 0, (pg_current_xact_id()::text::bigint)::text || ':' || id
           FROM unnest($3::text[]) id
          WHERE EXISTS (SELECT 1 FROM ops.tenant_epoch WHERE tenant = $2 AND enabled)",
        &[&CONSUMER, &tenant, &ids.to_vec()],
    )
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 03 §4.11: the work key carries the composing transaction and the entity.
    #[test]
    fn a_work_key_names_the_transaction_and_the_entity() {
        assert_eq!(
            Change::parse("731:WDQ42"),
            Some(Change {
                txn: 731,
                entity_id: "WDQ42".into()
            })
        );
        assert_eq!(
            Change::parse("9:domain:example.org").unwrap().entity_id,
            "domain:example.org"
        );
        assert_eq!(Change::parse("WDQ42"), None);
    }
}
