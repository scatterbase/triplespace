//! The bulk-job side of the server (0002 §8.1, entry point 2): [`PgIngest`],
//! `scatter-ingest`'s [`IngestStore`](scatter_ingest::IngestStore) over Postgres, which
//! the job endpoint and `triplespace-cli` both run jobs through. The HTTP routes follow
//! with the server.
//!
//! The store is the projections' [`PgBackend`](triplespace_projections::PgBackend) with
//! the ingest reads added: the log tables for appending and the key's newest record, the
//! per-tenant and instance
//! sequences of 0013 §6 (A29), and the `view` tables for cursors, match keys, surrogates,
//! property types and the adoption preconditions. Everything runs inside the backend's
//! unit of work, so a job's append and its projections commit together (0013 §7).

#![forbid(unsafe_code)]

pub mod store;

pub use store::PgIngest;
