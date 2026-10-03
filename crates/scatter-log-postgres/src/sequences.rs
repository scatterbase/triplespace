//! The per-tenant ID sequences of 0013 §6: `revision_id`, `log_id` and `page_id`, and
//! one per minted entity type (`item_id`, `property_id`, …), each a Postgres sequence
//! named `log."{tenant}.{kind}"`. They are taken in the appending transaction, like
//! Wikibase's `wb_id_counters`; an adoption sets their floors past what the source
//! consumed (0035 §4).

use scatter_log::store::StoreError;
use tokio_postgres::GenericClient;

use crate::ids::{from_db, to_db};
use crate::storage_error;

/// Which sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sequence {
    /// Header field 7 of a `local` or `pages` record.
    Revision,
    /// Header field 8 of a record that projects as a log event.
    Log,
    /// Header field 9: the first time a key is written.
    Page,
    /// A minted entity type's counter, by the type name (`item`, `property`).
    Entity(String),
}

impl Sequence {
    fn kind(&self) -> String {
        match self {
            Self::Revision => "revision_id".into(),
            Self::Log => "log_id".into(),
            Self::Page => "page_id".into(),
            Self::Entity(t) => format!("{t}_id"),
        }
    }
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The quoted sequence name for a tenant and a kind.
fn sequence_name(tenant: &str, seq: &Sequence) -> Result<String, StoreError> {
    let kind = seq.kind();
    if !is_slug(tenant) || !is_slug(&kind) {
        return Err(StoreError::Storage(format!(
            "`{tenant}` / `{kind}` is not a sequence name"
        )));
    }
    Ok(format!("log.\"{tenant}.{kind}\""))
}

/// Creates the sequences for a tenant, if they do not exist. Entity types are given
/// by name.
pub async fn create<C: GenericClient + Sync>(
    client: &C,
    tenant: &str,
    entity_types: &[&str],
) -> Result<(), StoreError> {
    let mut seqs = vec![Sequence::Revision, Sequence::Log, Sequence::Page];
    seqs.extend(
        entity_types
            .iter()
            .map(|t| Sequence::Entity((*t).to_string())),
    );
    let mut sql = String::new();
    for s in &seqs {
        use std::fmt::Write as _;
        let _ = writeln!(
            sql,
            "CREATE SEQUENCE IF NOT EXISTS {} AS bigint START 1;",
            sequence_name(tenant, s)?
        );
    }
    client
        .batch_execute(&sql)
        .await
        .map_err(|e| storage_error(&e))
}

/// The next value of a sequence.
pub async fn next<C: GenericClient + Sync>(
    client: &C,
    tenant: &str,
    seq: &Sequence,
) -> Result<u64, StoreError> {
    let row = client
        .query_one(
            &format!("SELECT nextval('{}')", sequence_name(tenant, seq)?),
            &[],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    from_db(row.get(0))
}

/// Raises the sequence so that its next value is above `consumed`, never lowering it:
/// an adoption's floor (0035 §4).
pub async fn floor<C: GenericClient + Sync>(
    client: &C,
    tenant: &str,
    seq: &Sequence,
    consumed: u64,
) -> Result<(), StoreError> {
    let name = sequence_name(tenant, seq)?;
    client
        .execute(
            &format!(
                "SELECT setval('{name}', GREATEST($1::bigint, (SELECT last_value FROM {name})), true)"
            ),
            &[&to_db(consumed)?],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(
            sequence_name("librarybase", &Sequence::Revision).unwrap(),
            "log.\"librarybase.revision_id\""
        );
        assert_eq!(
            sequence_name("librarybase", &Sequence::Entity("item".into())).unwrap(),
            "log.\"librarybase.item_id\""
        );
        assert!(sequence_name("Bad Slug", &Sequence::Log).is_err());
        assert!(sequence_name("t", &Sequence::Entity("x\"; DROP".into())).is_err());
    }
}
