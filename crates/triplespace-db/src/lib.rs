//! The database layer of a Triplespace instance (0013 §4–7; 0005 §2): the `view`,
//! `private` and `ops` schemas and their migrations, the runner that applies every
//! crate's migrations in build order and records them in `ops.migration`, the roles and
//! grants that enforce `private`'s privacy, connection pools, and replica routing by
//! WAL position.
//!
//! - [`migrate`]: [`Migrator`], which applies `scatter-log-postgres`'s migrations and then
//!   this crate's ([`MIGRATIONS`]), checksummed, idempotent, recorded;
//! - [`private`]: the portability class of every `private` table (0027 §2), checked
//!   against the live schema;
//! - [`grants`]: per-partition grants, including the accounts partition's exclusion;
//! - [`pool`]: `deadpool-postgres` pools from a URL;
//! - [`routing`]: read-your-writes across replicas by LSN (0013 §7);
//! - [`projection_state`]: `ops.projection_state`, how far each projection has replayed.
//!
//! Tests run against a real database named by `TRIPLESPACE_TEST_DATABASE_URL`.

pub mod grants;
pub mod migrate;
pub mod pool;
pub mod private;
pub mod projection_state;
pub mod routing;

pub use migrate::{Applied, MigrateError, Migrator};
pub use scatter_log_postgres::Migration;

/// This crate's migrations, in order, after `scatter-log-postgres`'s.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "0001_ops",
        sql: include_str!("../migrations/0001_ops.sql"),
    },
    Migration {
        name: "0002_view",
        sql: include_str!("../migrations/0002_view.sql"),
    },
    Migration {
        name: "0003_private",
        sql: include_str!("../migrations/0003_private.sql"),
    },
    Migration {
        name: "0004_roles",
        sql: include_str!("../migrations/0004_roles.sql"),
    },
    Migration {
        name: "0005_resolved_compression",
        sql: include_str!("../migrations/0005_resolved_compression.sql"),
    },
    Migration {
        name: "0006_private_session",
        sql: include_str!("../migrations/0006_private_session.sql"),
    },
];

/// The crate name migrations are recorded under.
pub const CRATE: &str = "triplespace-db";

/// A migrator loaded with every schema in 0005 §7's build order: the log, then this
/// crate. Crates that own later tables add theirs.
#[must_use]
pub fn migrator() -> Migrator {
    Migrator::new()
        .with("scatter-log-postgres", scatter_log_postgres::MIGRATIONS)
        .with(CRATE, MIGRATIONS)
}
