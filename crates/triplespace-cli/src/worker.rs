//! `worker`: the work queue from the command line (03 §6.1; 0083 §3, A4).
//!
//! `worker run` is the standalone worker: the loop the server's `--workers` tasks run,
//! in a process of its own, for an instance whose servers run with `--workers 0`. It
//! applies every queue (each projection's fan-out, reports, reference counts, the update
//! stream's deltas); the position-based addressing step and the update stream's
//! housekeeping stay with the server. `worker status` prints what waits and what is
//! parked; `worker parked` lists the parked items, those that failed as often as the
//! worker retries; `worker retry` puts them back in the queue and `worker discard`
//! removes them.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use scatter_projection::Backend as _;
use triplespace_api_ingest::worker::{Which, discard, parked, retry, worker};

use crate::common::{connect, farm_of, store_sized};

/// The `worker` commands.
#[derive(Debug, Subcommand)]
pub enum WorkerCommand {
    /// Run the worker over the work queue until stopped.
    Run(Run),
    /// Print each queue's lag: what waits, what is parked, how old the oldest is.
    Status(At),
    /// List the parked items.
    Parked(Parked),
    /// Put parked items back in the queue to be tried again.
    Retry(Named),
    /// Remove parked items from the queue.
    Discard(Named),
}

/// The database.
#[derive(Debug, Args)]
pub struct At {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// `worker run`.
#[derive(Debug, Args)]
pub struct Run {
    /// The database.
    #[command(flatten)]
    pub at: At,
    /// Worker tasks; the queue's per-tenant locks share the tenants between them, and
    /// between this process and any other that runs workers.
    #[arg(long, env = "TRIPLESPACE_WORKERS", default_value_t = 2)]
    pub workers: usize,
    /// Stop once every queue is empty, rather than waiting for more work.
    #[arg(long)]
    pub until_idle: bool,
    /// How long an idle worker waits before looking again, in milliseconds.
    #[arg(long, default_value_t = 1000)]
    pub idle_ms: u64,
}

/// Which parked items.
#[derive(Debug, Args)]
pub struct Filter {
    /// Only this consumer's (`report`, `ref_count`, a projection's name).
    #[arg(long)]
    pub consumer: Option<String>,
    /// Only this tenant's; `--tenant ""` is the instance's.
    #[arg(long)]
    pub tenant: Option<String>,
    /// Only this key's.
    #[arg(long)]
    pub key: Option<String>,
}

impl Filter {
    fn which(&self) -> Which {
        Which {
            consumer: self.consumer.clone(),
            tenant: self.tenant.clone(),
            key: self.key.clone(),
        }
    }
}

/// `worker parked`.
#[derive(Debug, Args)]
pub struct Parked {
    /// The database.
    #[command(flatten)]
    pub at: At,
    /// Which items.
    #[command(flatten)]
    pub filter: Filter,
    /// At most this many, oldest first.
    #[arg(long, default_value_t = 100)]
    pub limit: i64,
}

/// `worker retry` and `worker discard`.
#[derive(Debug, Args)]
pub struct Named {
    /// The database.
    #[command(flatten)]
    pub at: At,
    /// Which items.
    #[command(flatten)]
    pub filter: Filter,
    /// Every parked item, where no filter is given.
    #[arg(long)]
    pub all: bool,
}

impl Named {
    fn which(&self) -> Result<Which> {
        let which = self.filter.which();
        if which == Which::default() && !self.all {
            anyhow::bail!("name the items with --consumer, --tenant or --key, or pass --all");
        }
        Ok(which)
    }
}

/// Runs a `worker` command.
pub async fn run(command: WorkerCommand) -> Result<()> {
    match command {
        WorkerCommand::Run(args) => run_worker(&args).await,
        WorkerCommand::Status(at) => {
            let client = connect(&at.database).await?;
            let farm = farm_of(&client).await?;
            let (store, pipeline) = store_sized(&at.database, &farm, 1)?;
            let max = worker(&pipeline)?.max_attempts;
            let mut cx = store.begin().await?;
            let lag = store.queue_lag(&mut cx, max).await;
            store.rollback(cx).await?;
            let lag = lag?;
            if lag.is_empty() {
                println!("every queue is empty");
            }
            for l in lag {
                println!(
                    "{}\t{}\twaiting {}\tparked {}\toldest {}s",
                    l.consumer,
                    if l.tenant.is_empty() {
                        "(instance)"
                    } else {
                        &l.tenant
                    },
                    l.waiting,
                    l.failed,
                    l.oldest_seconds
                );
            }
            Ok(())
        }
        WorkerCommand::Parked(args) => {
            let (client, max) = client_and_limit(&args.at).await?;
            let items = parked(&client, max, &args.filter.which(), args.limit).await?;
            if items.is_empty() {
                println!("nothing is parked");
            }
            for p in items {
                println!(
                    "{}\t{}\t{}\t{}\tattempts {}\t{}",
                    p.id,
                    p.consumer,
                    if p.tenant.is_empty() {
                        "(instance)"
                    } else {
                        &p.tenant
                    },
                    p.key,
                    p.attempts,
                    p.last_error.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
        WorkerCommand::Retry(args) => {
            let which = args.which()?;
            let (client, max) = client_and_limit(&args.at).await?;
            let n = retry(&client, max, &which).await?;
            println!("{n} parked item(s) put back in the queue");
            Ok(())
        }
        WorkerCommand::Discard(args) => {
            let which = args.which()?;
            let (client, max) = client_and_limit(&args.at).await?;
            let n = discard(&client, max, &which).await?;
            println!("{n} parked item(s) removed from the queue");
            Ok(())
        }
    }
}

/// A connection, and the attempts after which the worker parks an item.
async fn client_and_limit(at: &At) -> Result<(tokio_postgres::Client, u32)> {
    let client = connect(&at.database).await?;
    let farm = farm_of(&client).await?;
    let (_, pipeline) = store_sized(&at.database, &farm, 1)?;
    Ok((client, worker(&pipeline)?.max_attempts))
}

/// The worker tasks, until every one stops: at once with `--until-idle` when the queues
/// are empty, otherwise never, short of a signal.
async fn run_worker(args: &Run) -> Result<()> {
    let client = connect(&args.at.database).await?;
    let farm = farm_of(&client).await?;
    drop(client);
    let tasks = args.workers.max(1);
    let (store, pipeline) = store_sized(&args.at.database, &farm, tasks + 1)?;
    let worker = std::sync::Arc::new(worker(&pipeline).context("the worker's consumers")?);
    let idle = Duration::from_millis(args.idle_ms);
    eprintln!(
        "worker: {tasks} task(s) over {} queue(s)",
        worker.consumers().len()
    );
    let mut handles = Vec::new();
    for n in 0..tasks {
        let (store, worker, until_idle) = (store.clone(), worker.clone(), args.until_idle);
        handles.push(tokio::spawn(async move {
            let mut applied = 0usize;
            loop {
                match worker.run_once(&store).await {
                    Ok(ran) => {
                        for f in &ran.failures {
                            eprintln!(
                                "worker {n}: {} for tenant {:?} failed on {} item(s): {}",
                                f.consumer,
                                f.tenant,
                                f.ids.len(),
                                f.error
                            );
                        }
                        applied += ran.applied;
                        if ran.idle() || (until_idle && ran.applied == 0) {
                            if until_idle {
                                return applied;
                            }
                            tokio::time::sleep(idle).await;
                        }
                    }
                    Err(e) => {
                        eprintln!("worker {n}: {e}");
                        if until_idle {
                            return applied;
                        }
                        tokio::time::sleep(idle).await;
                    }
                }
            }
        }));
    }
    let mut applied = 0usize;
    for h in handles {
        applied += h.await.context("a worker task")?;
    }
    println!("worker: {applied} item(s) applied; the queues are idle");
    Ok(())
}
