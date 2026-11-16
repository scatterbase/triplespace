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
//! thread API and the import jobs; [`import`] the import of a MediaWiki XML export
//! (0008 §9) and [`talk_import`] the talk-page conversion after an adoption or an import
//! (0090 §10), shared by `triplespace-cli` and `action=import`; [`jobs`] the job queue
//! and the runner (0104 §4), which `triplespace-server` runs and the surfaces submit to.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod acl;
pub mod backup_keys;
pub mod batch;
pub mod blocks;
pub mod diff;
pub mod follow;
pub mod groups;
pub mod import;
pub mod jobs;
pub mod membership;
pub mod revert;
pub mod settings;
pub mod sources;
pub mod store;
pub mod talk_import;
pub mod threads;

pub use store::PgIngest;
