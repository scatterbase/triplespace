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
//!
//! [`settings`] is the one writer of settings records (0085 §3), shared by
//! `PATCH /settings` and `triplespace-cli settings`; [`accounts`] the one writer of a
//! registered account (0087 §1, §6), shared by `action=createaccount` and the approval of
//! a request; [`threads`] the one writer of thread records (0019 §1–§4), shared by the
//! thread API and the import jobs.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod acl;
pub mod blocks;
pub mod follow;
pub mod groups;
pub mod membership;
pub mod settings;
pub mod store;
pub mod threads;

pub use store::PgIngest;
