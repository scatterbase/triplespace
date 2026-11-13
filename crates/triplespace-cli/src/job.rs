//! `triplespace job` (0104 §4, §10; 16 §2.2): the job queue from the command line.
//! `list` prints what waits for the runner and what runs under it, then the tenant's
//! recent jobs; `start import` submits a MediaWiki XML export to the queue, which the
//! server's runner carries (`triplespace import` runs one directly, as before);
//! `cancel` removes a job that waits; `pause` and `resume` set the instance's
//! `jobs.paused`, under which queued jobs wait and a running one finishes.
//!
//! implements 0104 §10

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_actors::key::ActorKey;
use triplespace_api_ingest::import::ImportRequest;
use triplespace_api_ingest::import::issuer_for;
use triplespace_api_ingest::jobs::{self, Kind, Submission};

use crate::common::{connect, farm_of, store};
use crate::settings::{self, Set, Where};

/// The `job` commands.
#[derive(Debug, Subcommand)]
pub enum JobCommand {
    /// What waits for the runner, what runs, and the tenant's recent jobs.
    List(List),
    /// Submit a job to the queue.
    Start {
        /// What to submit.
        #[command(subcommand)]
        kind: StartKind,
    },
    /// Remove a job that waits in the queue.
    Cancel(Cancel),
    /// Pause the runner: queued jobs wait, a running one finishes.
    Pause(Database),
    /// Resume the runner.
    Resume(Database),
}

/// The Postgres URL alone.
#[derive(Debug, Args)]
pub struct Database {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// `job list`.
#[derive(Debug, Args)]
pub struct List {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// How many recent jobs to print.
    #[arg(long, default_value_t = 20)]
    pub limit: i64,
}

/// `job cancel`.
#[derive(Debug, Args)]
pub struct Cancel {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// The job ID.
    pub id: u64,
}

/// What `job start` submits.
#[derive(Debug, Subcommand)]
pub enum StartKind {
    /// A MediaWiki XML export's pages (0008 §9), as `triplespace import` takes them.
    Import(StartImport),
    /// A tenant's dumps (02 §4.1), written by the server's runner into its dumps
    /// directory, as `triplespace dump` writes them here.
    Dump(StartDump),
    /// An adoption (0035) of a dump the server reads: `triplespace adopt`'s arguments,
    /// run by the server's runner.
    Adopt(crate::adopt::Adopt),
    /// A sync of a dump the server reads (0002 §8.4): `triplespace sync`'s arguments.
    Sync(crate::sync::Sync),
    /// A follower (0092 §1), run a pass at a time by the server's runner on its accounts
    /// login: `triplespace follow`'s arguments. Cancel the job to stop following.
    Follow(crate::follow::Follow),
}

/// `job start dump`.
#[derive(Debug, Args)]
pub struct StartDump {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// How many dumps to keep, the new one included; by default the tenant's `dumps.keep`.
    #[arg(long)]
    pub keep: Option<u64>,
    /// The subsidiary running the job, `librarybase:2`; the owner's own key by default.
    #[arg(long)]
    pub subsidiary: Option<String>,
}

/// `job start import`.
#[derive(Debug, Args)]
pub struct StartImport {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// The MediaWiki XML export (`.xml`, `.xml.gz`, `.xml.bz2`).
    #[arg(long)]
    pub file: PathBuf,
    /// Where the file came from: a site alias (`enwiki`) or a name; `imported` by default.
    #[arg(long, default_value = "imported")]
    pub source: String,
    /// The registered issuer the source wiki is (0007 §5).
    #[arg(long)]
    pub issuer: Option<String>,
    /// The job's reason.
    #[arg(long)]
    pub summary: Option<String>,
    /// The subsidiary running the job, `librarybase:2`; the owner's own key by default.
    #[arg(long)]
    pub subsidiary: Option<String>,
    /// Leave the file's talk pages unconverted.
    #[arg(long)]
    pub skip_talk: bool,
}

/// The tenant's owner, or the subsidiary named: the job's actor.
async fn actor_of(
    client: &tokio_postgres::Client,
    tenant: &str,
    subsidiary: Option<&str>,
) -> Result<String> {
    if let Some(s) = subsidiary {
        return Ok(ActorKey::parse(s)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .to_string());
    }
    let owner: Option<String> = client
        .query_opt(
            "SELECT m.actor_key FROM view.membership m WHERE m.tenant = $1 AND m.\"group\" = 'owner' ORDER BY m.actor_key LIMIT 1",
            &[&tenant],
        )
        .await?
        .map(|r| r.get(0));
    owner.with_context(|| format!("no owner found for `{tenant}`; pass --subsidiary"))
}

/// The file's bytes, decompressed where the name says so, as the runner takes them.
fn read_export(path: &std::path::Path) -> Result<Vec<u8>> {
    use std::io::Read as _;
    let mut reader =
        scatter_adapter_wikibase::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}

/// Runs a `job` command.
pub async fn run(command: JobCommand) -> Result<()> {
    match command {
        JobCommand::List(args) => list(args).await,
        JobCommand::Start {
            kind: StartKind::Import(args),
        } => start_import(args).await,
        JobCommand::Start {
            kind: StartKind::Dump(args),
        } => start_dump(args).await,
        JobCommand::Start {
            kind: StartKind::Adopt(mut args),
        } => {
            args.dump = server_path(&args.dump)?;
            let tenant = args.tenant.clone();
            let subsidiary = args.subsidiary.clone();
            start_work(
                &args.database,
                Kind::Adopt,
                &tenant,
                subsidiary.as_deref(),
                &args,
            )
            .await
        }
        JobCommand::Start {
            kind: StartKind::Sync(mut args),
        } => {
            args.dump = server_path(&args.dump)?;
            if let Some(rejects) = &args.rejects {
                args.rejects = Some(std::path::absolute(rejects)?);
            }
            let tenant = args.tenant.clone().unwrap_or_default();
            start_work(&args.database, Kind::Sync, &tenant, None, &args).await
        }
        JobCommand::Start {
            kind: StartKind::Follow(args),
        } => {
            let tenant = args.tenant.clone().unwrap_or_default();
            start_work(&args.database, Kind::Follow, &tenant, None, &args).await
        }
        JobCommand::Cancel(args) => {
            let client = connect(&args.database).await?;
            if jobs::cancel(&client, &args.tenant, args.id).await? {
                println!("job {} removed from the queue", args.id);
            } else {
                bail!(
                    "job {} is not waiting in `{}`'s queue: it is running, finished, or was never queued",
                    args.id,
                    args.tenant
                );
            }
            Ok(())
        }
        JobCommand::Pause(args) => paused(&args.database, true).await,
        JobCommand::Resume(args) => paused(&args.database, false).await,
    }
}

async fn paused(database: &str, paused: bool) -> Result<()> {
    settings::run(settings::Settings::Set(Set {
        at: Where {
            database: database.to_string(),
            tenant: None,
            instance: true,
        },
        key: "jobs.paused".into(),
        value: paused.to_string(),
        default: false,
        lock: false,
        tenancy: false,
        summary: Some(
            if paused {
                "the job runner is paused"
            } else {
                "the job runner resumes"
            }
            .into(),
        ),
    }))
    .await?;
    println!(
        "the job runner is {}",
        if paused { "paused" } else { "running" }
    );
    Ok(())
}

async fn list(args: List) -> Result<()> {
    let client = connect(&args.database).await?;
    let queued = jobs::queued(&client, &args.tenant).await?;
    if queued.is_empty() {
        println!("queue: empty");
    } else {
        println!("queue: {} job(s)", queued.len());
        for q in &queued {
            let state = if q.running {
                match &q.progress {
                    Some(p) => format!("running ({p})"),
                    None => "running".to_string(),
                }
            } else {
                format!("waiting, {} in line", q.position.unwrap_or(0))
            };
            println!(
                "  {:>8}  {:<8}  {}  by {}  {}",
                q.job_id,
                q.kind,
                state,
                q.actor,
                q.params
                    .get("source")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
            );
        }
    }
    let rows = client
        .query(
            "SELECT job_id, status, coalesce(mode, ''), source, actor_key, started::text, coalesce(finished::text, '')
             FROM view.job WHERE tenant = $1 ORDER BY job_id DESC LIMIT $2",
            &[&args.tenant, &args.limit],
        )
        .await?;
    println!("recent: {} job(s)", rows.len());
    for r in &rows {
        println!(
            "  {:>8}  {:<11}  {:<8}  {}  by {}  {} – {}",
            r.get::<_, i64>(0),
            r.get::<_, String>(1),
            r.get::<_, String>(2),
            r.get::<_, String>(3),
            r.get::<_, String>(4),
            r.get::<_, String>(5),
            r.get::<_, String>(6)
        );
    }
    Ok(())
}

/// A dump's path as the server will read it: absolute, and a file that is there now. The
/// server must see the same path; on another machine that is the operator's to arrange.
fn server_path(path: &std::path::Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).with_context(|| format!("the dump {}", path.display()))
}

/// Queues a job whose parameters are a command's own arguments (0104 A9): for `tenant`,
/// or the instance when it is empty, where the job's actor is the farm's operator.
async fn start_work<T: serde::Serialize>(
    database: &str,
    kind: Kind,
    tenant: &str,
    subsidiary: Option<&str>,
    args: &T,
) -> Result<()> {
    let client = connect(database).await?;
    let farm = farm_of(&client).await?;
    let actor = if tenant.is_empty() {
        farm.operator().to_string()
    } else {
        actor_of(&client, tenant, subsidiary).await?
    };
    let (store, _pipeline) = store(database, &farm)?;
    let id = jobs::submit(
        &store,
        &Submission {
            tenant: tenant.to_string(),
            kind,
            params: serde_json::to_value(args)?,
            payload: None,
            actor,
        },
    )
    .await?;
    println!(
        "job {id} queued: {} for {}, which a server's runner runs; Special:Jobs/{id} shows it",
        kind.name(),
        if tenant.is_empty() {
            "the instance".to_string()
        } else {
            format!("`{tenant}`")
        }
    );
    Ok(())
}

async fn start_dump(args: StartDump) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let actor = actor_of(&client, &args.tenant, args.subsidiary.as_deref()).await?;
    let (store, _pipeline) = store(&args.database, &farm)?;
    let mut params = serde_json::Map::new();
    if let Some(keep) = args.keep {
        params.insert("keep".into(), keep.into());
    }
    let id = jobs::submit(
        &store,
        &Submission {
            tenant: args.tenant.clone(),
            kind: Kind::Dump,
            params: serde_json::Value::Object(params),
            payload: None,
            actor,
        },
    )
    .await?;
    println!(
        "job {id} queued: a dump of `{}`, which a server's runner writes into its dumps \
         directory; Special:Jobs/{id} shows it",
        args.tenant
    );
    Ok(())
}

async fn start_import(args: StartImport) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let actor = actor_of(&client, &args.tenant, args.subsidiary.as_deref()).await?;
    let bytes = read_export(&args.file)?;
    let issuer = match &args.issuer {
        Some(code) => Some(issuer_for(code, None).with_context(|| {
            format!("`{code}` is not a registered issuer with numeric accounts (issuers.toml)")
        })?),
        None => issuer_for(&args.source, None),
    };
    let request = ImportRequest {
        source: args.source.clone(),
        version: args
            .file
            .file_name()
            .map(|f| f.to_string_lossy().into_owned()),
        prefix: args.source.clone(),
        issuer,
        summary: args.summary.clone(),
        tags: Vec::new(),
        skip_talk: args.skip_talk,
    };
    let (store, _pipeline) = store(&args.database, &farm)?;
    let id = jobs::submit(
        &store,
        &Submission {
            tenant: args.tenant.clone(),
            kind: Kind::Import,
            params: serde_json::to_value(&request)?,
            payload: Some(bytes),
            actor,
        },
    )
    .await?;
    println!(
        "job {id} queued: {} bytes of {} for `{}`; Special:Jobs/{id} shows it",
        args.file.metadata().map_or(0, |m| m.len()),
        args.file.display(),
        args.tenant
    );
    Ok(())
}
