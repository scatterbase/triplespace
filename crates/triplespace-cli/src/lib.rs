//! `triplespace`: the command line (0005 §2; 0033 §13), as a library so the acceptance
//! tests drive the same code the binary runs.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod adopt;
pub mod adopt_pages;
pub mod backup_key;
pub mod bootstrap;
pub mod carried;
pub mod challenge;
pub mod common;
pub mod compact;
pub mod dump;
pub mod env_file;
pub mod follow;
pub mod forwarder;
pub mod import_pages;
pub mod instance;
pub mod job;
pub mod job_work;
pub mod log_bundle;
pub mod logins;
pub mod oauth_client;
pub mod probe;
pub mod restore;
pub mod rights;
pub mod sealed;
pub mod settings;
pub mod setup;
pub mod source;
pub mod status;
pub mod sync;
pub mod tenant;
