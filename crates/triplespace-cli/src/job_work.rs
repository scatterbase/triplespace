//! The work of the queue's `adopt`, `sync` and `follow` jobs (0104 §4, A9): what
//! `triplespace adopt`, `sync` and `follow --once` do, run by a server's job runner from a
//! queue row whose parameters are the command's own arguments. The runner's crate cannot
//! reach this one, so the server hands these to its runners
//! ([`triplespace_api_ingest::jobs::Runner::with_work`]).
//!
//! Each runs on a thread of its own, since a load's future is not `Send`, and is stopped
//! there when its claim is lost. A dump is named by a path the server process reads: an
//! operator queues these jobs, with `triplespace job start`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use scatter_ingest::IngestError;
use triplespace_api_ingest::jobs::{Claimed, Kind, Outcome, Runner, Work, WorkFuture, on_thread};

use crate::{adopt, follow, sync};

fn failed(e: &anyhow::Error) -> IngestError {
    IngestError::Store(format!("{e:#}"))
}

fn params<T: serde::de::DeserializeOwned>(job: &Claimed) -> Result<T, IngestError> {
    serde_json::from_value(job.params.clone()).map_err(|e| {
        IngestError::Store(format!(
            "the `{}` job's parameters do not read: {e}",
            job.kind.name()
        ))
    })
}

/// An `adopt` job: `triplespace adopt` under the job's ID.
#[derive(Debug, Clone)]
pub struct AdoptWork {
    /// The database the adoption writes, as the runner's login.
    pub database: String,
}

impl Work for AdoptWork {
    fn run<'a>(&'a self, job: &'a Claimed, _now: fn() -> u64) -> WorkFuture<'a> {
        Box::pin(async move {
            let mut args: adopt::Adopt = params(job)?;
            args.database.clone_from(&self.database);
            args.job_id = Some(job.job_id);
            on_thread(move || adopt::run(args))
                .await?
                .map_err(|e| failed(&e))?;
            Ok(Outcome::Done)
        })
    }
}

/// A `sync` job: `triplespace sync` under the job's ID.
#[derive(Debug, Clone)]
pub struct SyncWork {
    /// The database the sync writes, as the runner's login.
    pub database: String,
}

impl Work for SyncWork {
    fn run<'a>(&'a self, job: &'a Claimed, _now: fn() -> u64) -> WorkFuture<'a> {
        Box::pin(async move {
            let mut args: sync::Sync = params(job)?;
            args.database.clone_from(&self.database);
            args.job_id = Some(job.job_id);
            on_thread(move || sync::run(args))
                .await?
                .map_err(|e| failed(&e))?;
            Ok(Outcome::Done)
        })
    }
}

/// A `follow` job: one pass of `triplespace follow`, after which the row waits the
/// follower's interval and is claimed again. The follower's own job, one a day, is the
/// one its records name (0092 A6); the queue row's ID is the row's alone.
#[derive(Debug, Clone)]
pub struct FollowWork {
    /// The database, as the accounts login: the follower reads its upstream password
    /// from `private` (0092 §10).
    pub database: String,
    /// The instance's key file, which opens the stored password.
    pub key_file: PathBuf,
}

impl Work for FollowWork {
    fn run<'a>(&'a self, job: &'a Claimed, _now: fn() -> u64) -> WorkFuture<'a> {
        Box::pin(async move {
            let mut args: follow::Follow = params(job)?;
            args.database.clone_from(&self.database);
            args.key_file.clone_from(&self.key_file);
            args.once = true;
            let wait = follow::interval(&args).await.map_err(|e| failed(&e))?;
            on_thread(move || follow::run(args))
                .await?
                .map_err(|e| failed(&e))?;
            Ok(Outcome::Again(Duration::from_secs(wait)))
        })
    }
}

/// Gives `runner` the adoption and sync work, over `database`.
#[must_use]
pub fn with_loads(runner: Runner, database: &str) -> Runner {
    runner
        .with_work(
            Kind::Adopt,
            Arc::new(AdoptWork {
                database: database.to_string(),
            }),
        )
        .with_work(
            Kind::Sync,
            Arc::new(SyncWork {
                database: database.to_string(),
            }),
        )
}

/// Gives `runner` the followers' work, over `database`, the accounts login's.
#[must_use]
pub fn with_followers(runner: Runner, database: &str, key_file: &std::path::Path) -> Runner {
    runner.with_work(
        Kind::Follow,
        Arc::new(FollowWork {
            database: database.to_string(),
            key_file: key_file.to_path_buf(),
        }),
    )
}
