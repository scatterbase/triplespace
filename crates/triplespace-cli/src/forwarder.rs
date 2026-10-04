//! `triplespace instance forwarder create|list|revoke` (0057 §10): the keys a proxy sends
//! in `Triplespace-Forwarder` so that the server believes the client address it appends
//! to `X-Forwarded-For`, where trusting it by address (`--trusted-proxy`) is not enough.
//!
//! Issuing and revoking a key is two writes in one transaction: the hash in
//! `private.forwarder_key`, and a `forwarder:{key ID}` record in the instance `config`
//! partition that puts the act in the log without the secret.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use scatter_actors::key::ActorKey;
use scatter_ingest::write::append_projected;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{PAYLOAD_CONFIG, config_key};
use scatter_projection::{Backend, Budget};
use triplespace_accounts::forwarder;

use crate::common::{connect, draft, farm_of, now, store};

/// The forwarder commands.
#[derive(Debug, Subcommand)]
pub enum Forwarder {
    /// Issue a key; it is printed once.
    Create(Create),
    /// List the keys, live and revoked.
    List(Target),
    /// Revoke a key.
    Revoke(Revoke),
}

/// The database.
#[derive(Debug, Args)]
pub struct Target {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The farm slug, for the instance actor; the first tenant's by default.
    #[arg(long)]
    pub farm_slug: Option<String>,
    /// The farm base.
    #[arg(long)]
    pub farm_base: Option<String>,
}

/// `forwarder create`.
#[derive(Debug, Args)]
pub struct Create {
    /// The database.
    #[command(flatten)]
    pub target: Target,
    /// What the key is for: `edge`, `web`. Unique among live keys.
    #[arg(long)]
    pub label: String,
}

/// `forwarder revoke`.
#[derive(Debug, Args)]
pub struct Revoke {
    /// The database.
    #[command(flatten)]
    pub target: Target,
    /// The key ID, as `forwarder list` shows it.
    #[arg(long)]
    pub key_id: String,
}

/// Runs a forwarder command.
pub async fn run(cmd: Forwarder) -> Result<()> {
    match cmd {
        Forwarder::Create(args) => create(args).await,
        Forwarder::List(args) => list(args).await,
        Forwarder::Revoke(args) => revoke(args).await,
    }
}

/// The store, the pipeline and the instance actor for a target.
async fn open(
    target: &Target,
) -> Result<(
    triplespace_api_ingest::PgIngest,
    scatter_projection::Pipeline<triplespace_api_ingest::PgIngest>,
    String,
)> {
    let client = connect(&target.database).await?;
    let farm = farm_of(
        &client,
        target.farm_slug.as_deref(),
        target.farm_base.as_deref(),
    )
    .await?;
    let actor = ActorKey::instance(&farm.slug).to_string();
    let (store, pipeline) = store(&target.database, &farm)?;
    Ok((store, pipeline, actor))
}

async fn create(args: Create) -> Result<()> {
    let (store, pipeline, actor) = open(&args.target).await?;
    let mut cx = store.begin().await?;
    let issued = forwarder::issue(cx.conn(), &args.label).await?;
    let t = now();
    let d = draft(
        PAYLOAD_CONFIG,
        &config_key(forwarder::KIND, &issued.key_id),
        &forwarder::record_content(&args.label, t),
        &actor,
        Some("forwarder create"),
        t,
    )?;
    append_projected(
        &store,
        &pipeline,
        &mut cx,
        CONFIG_PARTITION,
        d,
        Budget::NONE,
    )
    .await
    .context("record the key in the instance config")?;
    store.commit(cx).await?;
    println!(
        "forwarder key {} issued, labelled {}",
        issued.key_id, args.label
    );
    println!("  header  Triplespace-Forwarder: {}", issued.key);
    println!("  shown once; store it now, in a file the proxy alone can read");
    Ok(())
}

async fn list(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    for k in forwarder::list(&client).await? {
        let state = if k.revoked_at.is_some() {
            "revoked"
        } else {
            "live"
        };
        println!("{:<24} {:<16} {state}", k.key_id, k.label);
    }
    Ok(())
}

async fn revoke(args: Revoke) -> Result<()> {
    let (store, pipeline, actor) = open(&args.target).await?;
    let mut cx = store.begin().await?;
    if !forwarder::revoke(cx.conn(), &args.key_id).await? {
        println!("forwarder key {} was not live", args.key_id);
        return Ok(());
    }
    let t = now();
    let d = draft(
        PAYLOAD_CONFIG,
        &config_key(forwarder::KIND, &args.key_id),
        &serde_json::Value::Null,
        &actor,
        Some("forwarder revoke"),
        t,
    )?;
    append_projected(
        &store,
        &pipeline,
        &mut cx,
        CONFIG_PARTITION,
        d,
        Budget::NONE,
    )
    .await
    .context("record the revocation in the instance config")?;
    store.commit(cx).await?;
    println!(
        "forwarder key {} revoked; servers stop believing it within 30 seconds",
        args.key_id
    );
    Ok(())
}
