//! `triplespace instance provider add` (0018 A40): opens a registered provider's mirror
//! partition on a running instance, as `instance create --provider` does at creation, and
//! queues its follower.
//!
//! The partition and its `graph:` record in the instance `config` are written in one
//! transaction, the record attested by the instance citing its founding record (0079 §4),
//! as at creation. No dump is loaded: the mirror starts empty, and so shallow (0070 A11),
//! holding what the tenants link and its closure. The follower is a `follow` job for the
//! instance, which a server's follower runner runs and queues again after each pass.

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::GraphRegistry;
use std::path::PathBuf;

use crate::common::{connect, farm_of, now};
use crate::instance::open_graph;
use crate::tenant::{self, current_key, instance_founding, provision};

/// The provider commands.
#[derive(Debug, Subcommand)]
pub enum Provider {
    /// Open a registered provider's mirror partition and queue its follower.
    Add(Add),
}

/// `provider add`.
#[derive(Debug, Args)]
pub struct Add {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The instance key file, which must hold the instance's current key.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: PathBuf,
    /// The provider's slug (`wikidata`).
    pub provider: String,
    /// Open the mirror without queueing its follower; `job start follow --provider` queues
    /// it later.
    #[arg(long)]
    pub no_follow: bool,
}

/// Runs a provider command.
pub async fn run(cmd: Provider) -> Result<()> {
    match cmd {
        Provider::Add(args) => add(args).await,
    }
}

async fn add(args: Add) -> Result<()> {
    let provider = scatter_providers::Registry::default_registry()
        .by_slug(&args.provider)
        .with_context(|| format!("`{}` is not a registered provider", args.provider))?;
    if provider.source {
        bail!(
            "`{}` is an entity source, which a tenant declares (`source add`), not an \
             instance mirror",
            args.provider
        );
    }
    let name = provider.mirror_graph();
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let held = client
        .query_opt(
            "SELECT 1 FROM log.partition WHERE tenant IS NULL AND name = $1",
            &[&name],
        )
        .await?;
    if held.is_some() {
        bail!("this instance already has `{name}`");
    }
    let key = current_key(&client, &args.key_file).await?;
    let founding = instance_founding(&client).await?;
    let attest = provision(&key, &farm, &founding);
    let registry = GraphRegistry::embedded();
    let partition = tenant::transaction(
        &client,
        open_graph(&client, &registry, &name, &farm.base, now(), &attest),
    )
    .await?;
    tenant::project(
        &args.database,
        &farm,
        client,
        &[CONFIG_PARTITION, partition],
    )
    .await?;
    println!("`{name}` opened, partition {partition:#x}");
    if args.no_follow {
        println!(
            "  no follower queued; `triplespace job start follow --provider {}` queues one",
            args.provider
        );
        return Ok(());
    }
    crate::job::start_follow(&args.database, &args.provider).await
}
