//! `LogStore` and `CheckpointStore` over the Postgres `log` schema (0013 §2–3, §6;
//! 0005 §4.2 as amended by A56).
//!
//! - [`migrate`] applies the schema; [`MIGRATIONS`] is what `triplespace-db`'s runner
//!   applies in build order (0005 rule 9).
//! - [`PgLog`] implements `LogStore` on an owned client, opening a transaction per
//!   mutating call, or on a borrowed transaction, joining the caller's — the form the
//!   write path uses, since an interactive write appends and applies projections in one
//!   transaction (0013 §7).
//! - [`merkle`] keeps every complete tree node in `log.merkle_node`, so the head root and
//!   inclusion proofs read O(log n) rows, and so a compacted offset's leaf survives the
//!   deletion of its row (0006 A14).
//! - [`PgCheckpoints`] implements `CheckpointStore` over `log.checkpoint` and
//!   `log.segment_manifest`.
//! - [`sequences`] are the per-tenant revision, log, page and entity-ID sequences of
//!   0013 §6, with the floors an adoption sets (0035 §4).
//!
//! Partition IDs are 64 random bits stored in `bigint` by reinterpretation; the
//! conversions are in [`ids`]. Tests run against a real database named by
//! `TRIPLESPACE_TEST_DATABASE_URL` (0033 §4, §15).

pub mod checkpoints;
pub mod client;
pub mod ids;
pub mod log;
pub mod merkle;
pub mod partition;
pub mod sequences;

pub use checkpoints::PgCheckpoints;
pub use client::PgClient;
pub use log::Frontiers;
pub use log::PgLog;
pub use partition::PartitionInfo;

/// One migration: a name and idempotent SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// `0001_log`.
    pub name: &'static str,
    /// The statements.
    pub sql: &'static str,
}

/// The `log` schema's migrations, in order.
pub const MIGRATIONS: &[Migration] = &[Migration {
    name: "0001_log",
    sql: include_str!("../migrations/0001_log.sql"),
}];

/// Applies every migration, in order. Each is idempotent, so this can run at every start.
pub async fn migrate<C: tokio_postgres::GenericClient>(
    client: &C,
) -> Result<(), tokio_postgres::Error> {
    for m in MIGRATIONS {
        client.batch_execute(m.sql).await?;
    }
    Ok(())
}

/// Turns a driver error into the store's.
#[must_use]
pub fn storage_error(e: &tokio_postgres::Error) -> scatter_log::store::StoreError {
    scatter_log::store::StoreError::Storage(e.to_string())
}
