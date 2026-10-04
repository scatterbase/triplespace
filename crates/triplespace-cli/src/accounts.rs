//! `triplespace password set`, `triplespace subsidiary create|key|keys|revoke` (0007 §3;
//! 0024 §1–2, §4): the owner's password and the bot accounts a client logs in as.
//! Secrets come from files or the environment, never the command line (0033 §12).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_actors::actor::ActorRecord;
use scatter_actors::key::ActorKey;
use scatter_actors::membership::Membership;
use scatter_ingest::store::{IngestStore, Sequence};
use scatter_ingest::write::append_projected;
use scatter_projection::{Backend, Budget, Pipeline};
use tokio_postgres::GenericClient;
use triplespace_accounts::{keys, password};
use triplespace_api_ingest::PgIngest;
use triplespace_projections::PgCx;
use triplespace_projections::actors::payload;

use crate::common::{connect, draft, farm_of, now, store};

/// Reads a secret: the file named, else the environment variable, else an error.
pub fn secret_from(file: Option<&Path>, env_var: &str) -> Result<String> {
    if let Some(p) = file {
        let text = std::fs::read_to_string(p)
            .with_context(|| format!("read secret file {}", p.display()))?;
        return Ok(text.trim_end_matches(['\r', '\n']).to_string());
    }
    if let Ok(v) = std::env::var(env_var) {
        return Ok(v);
    }
    bail!("no secret: pass a file, or set {env_var}")
}

/// An account on a tenant, by name or by number.
async fn actor_key<C: GenericClient>(
    client: &C,
    tenant: &str,
    name: Option<&str>,
    id: Option<u64>,
) -> Result<(String, Option<String>)> {
    match (name, id) {
        (_, Some(id)) => {
            let key = ActorKey::numeric(tenant, id).to_string();
            let row = client
                .query_opt(
                    "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                    &[&tenant, &key],
                )
                .await?
                .with_context(|| format!("no account `{key}` on `{tenant}`"))?;
            Ok((key, row.get(0)))
        }
        (Some(name), None) => {
            let name = scatter_actors::actor::normalize_name(name);
            let row = client
                .query_opt(
                    "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = $2",
                    &[&tenant, &name],
                )
                .await?
                .with_context(|| format!("no account named `{name}` on `{tenant}`"))?;
            Ok((row.get(0), Some(name)))
        }
        (None, None) => bail!("name the account with --name or --id"),
    }
}

/// `password set`.
#[derive(Debug, Args)]
pub struct PasswordSet {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// The account's name.
    #[arg(long)]
    pub name: Option<String>,
    /// The account's user ID.
    #[arg(long)]
    pub id: Option<u64>,
    /// A file holding the password (else `TRIPLESPACE_PASSWORD`).
    #[arg(long)]
    pub password_file: Option<PathBuf>,
    /// The farm slug and base, for the pipeline.
    #[arg(long)]
    pub farm_slug: Option<String>,
    /// The farm base.
    #[arg(long)]
    pub farm_base: Option<String>,
}

/// Sets an account's password: the hash in `private.password`, and the binding record in
/// the tenant's `accounts` partition if the account has none yet.
#[allow(clippy::too_many_arguments)]
pub async fn set_password(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor_key: &str,
    user_id: u64,
    password_text: &str,
    attested_by: &str,
    now: u64,
) -> Result<bool> {
    let had = password::has(cx.conn(), actor_key).await?;
    password::set(cx.conn(), actor_key, password_text).await?;
    if !had {
        let accounts = store
            .partition(cx, tenant, "accounts")
            .await?
            .with_context(|| format!("`{tenant}` has no accounts partition"))?;
        let d = password::binding_draft(actor_key, user_id, attested_by, now)?;
        append_projected(store, pipeline, cx, accounts, d, Budget::NONE).await?;
    }
    Ok(!had)
}

/// Runs `password set`.
pub async fn run_password_set(args: PasswordSet) -> Result<()> {
    let text = secret_from(args.password_file.as_deref(), "TRIPLESPACE_PASSWORD")?;
    let client = connect(&args.database).await?;
    let (key, name) = actor_key(&client, &args.tenant, args.name.as_deref(), args.id).await?;
    let user_id = ActorKey::parse(&key)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .numeric_subject()
        .context("the account has no numeric ID")?;
    let farm = farm_of(
        &client,
        args.farm_slug.as_deref(),
        args.farm_base.as_deref(),
    )
    .await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let mut cx = store.begin().await?;
    let fresh = set_password(
        &store,
        &pipeline,
        &mut cx,
        &args.tenant,
        &key,
        user_id,
        &text,
        &ActorKey::instance(&farm.slug).to_string(),
        now(),
    )
    .await?;
    store.commit(cx).await?;
    println!(
        "password {} for {key} ({})",
        if fresh { "set" } else { "replaced" },
        name.unwrap_or_default()
    );
    Ok(())
}

/// `subsidiary …`.
#[derive(Debug, Subcommand)]
pub enum Subsidiary {
    /// Create a subsidiary account for an operator (0024 §2).
    Create(SubsidiaryCreate),
    /// Issue an API key for a subsidiary; the secret is printed once (0024 §4).
    Key(SubsidiaryKey),
    /// List a subsidiary's keys.
    Keys(SubsidiaryTarget),
    /// Revoke a key and every session it opened.
    Revoke(SubsidiaryRevoke),
}

/// Common arguments naming a subsidiary.
#[derive(Debug, Args)]
pub struct SubsidiaryTarget {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// The subsidiary's name.
    #[arg(long)]
    pub name: String,
}

/// `subsidiary create`.
#[derive(Debug, Args)]
pub struct SubsidiaryCreate {
    /// The subsidiary.
    #[command(flatten)]
    pub target: SubsidiaryTarget,
    /// The operator's name (a primary account of the tenant).
    #[arg(long)]
    pub operator: Option<String>,
    /// The operator's user ID.
    #[arg(long)]
    pub operator_id: Option<u64>,
    /// Groups to add the new account to (`bot`).
    #[arg(long = "group")]
    pub groups: Vec<String>,
    /// The farm slug and base, for the pipeline.
    #[arg(long)]
    pub farm_slug: Option<String>,
    /// The farm base.
    #[arg(long)]
    pub farm_base: Option<String>,
}

/// `subsidiary key`.
#[derive(Debug, Args)]
pub struct SubsidiaryKey {
    /// The subsidiary.
    #[command(flatten)]
    pub target: SubsidiaryTarget,
    /// The key's label; `{name}@{label}` is the bot-password login name.
    #[arg(long)]
    pub label: String,
    /// Grants (`editentity`, `highvolume`, …); `basic` is always included.
    #[arg(long = "grant")]
    pub grants: Vec<String>,
    /// Days until the key expires; never by default.
    #[arg(long)]
    pub expires_days: Option<u64>,
}

/// `subsidiary revoke`.
#[derive(Debug, Args)]
pub struct SubsidiaryRevoke {
    /// The subsidiary.
    #[command(flatten)]
    pub target: SubsidiaryTarget,
    /// The key ID.
    #[arg(long)]
    pub key_id: String,
}

/// Creates a subsidiary: the actor record (kind `bot`, with its operator) in the tenant's
/// `actors` partition, and its memberships. Returns the new actor key.
#[allow(clippy::too_many_arguments)]
pub async fn create_subsidiary(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    name: &str,
    operator: &str,
    groups: &[String],
    attested_by: &str,
    now: u64,
) -> Result<String> {
    let operator_key = ActorKey::parse(operator).map_err(|e| anyhow::anyhow!("{e}"))?;
    let record = ActorRecord::subsidiary(name, operator_key);
    let taken = cx
        .conn()
        .query_opt(
            "SELECT 1 FROM view.actor WHERE tenant = $1 AND name = $2",
            &[&tenant, &scatter_actors::actor::normalize_name(name)],
        )
        .await?;
    if taken.is_some() {
        bail!("`{name}` is already an account on `{tenant}`");
    }
    let id = store.next_id(cx, tenant, &Sequence::User).await?;
    let key = ActorKey::numeric(tenant, id);
    record
        .validate(&key)
        .map_err(|e| anyhow::anyhow!("subsidiary record: {e}"))?;
    let actors = store
        .partition(cx, tenant, "actors")
        .await?
        .with_context(|| format!("`{tenant}` has no actors partition"))?;
    let mut d = draft(
        payload::ACTOR,
        &key.to_string(),
        &serde_json::to_value(&record)?,
        attested_by,
        Some("subsidiary create"),
        now,
    )?;
    d.logid = Some(store.next_id(cx, tenant, &Sequence::Log).await?);
    append_projected(store, pipeline, cx, actors, d, Budget::NONE).await?;
    for g in groups {
        let mut m = draft(
            payload::MEMBERSHIP,
            &key.to_string(),
            &serde_json::to_value(Membership::add(g, None))?,
            attested_by,
            Some("subsidiary create"),
            now,
        )?;
        m.logid = Some(store.next_id(cx, tenant, &Sequence::Log).await?);
        append_projected(store, pipeline, cx, actors, m, Budget::NONE).await?;
    }
    Ok(key.to_string())
}

/// Runs a `subsidiary` command.
pub async fn run_subsidiary(cmd: Subsidiary) -> Result<()> {
    match cmd {
        Subsidiary::Create(args) => {
            let client = connect(&args.target.database).await?;
            let (operator, _) = actor_key(
                &client,
                &args.target.tenant,
                args.operator.as_deref(),
                args.operator_id,
            )
            .await?;
            let farm = farm_of(
                &client,
                args.farm_slug.as_deref(),
                args.farm_base.as_deref(),
            )
            .await?;
            let (store, pipeline) = store(&args.target.database, &farm)?;
            let mut cx = store.begin().await?;
            let key = create_subsidiary(
                &store,
                &pipeline,
                &mut cx,
                &args.target.tenant,
                &args.target.name,
                &operator,
                &args.groups,
                &operator,
                now(),
            )
            .await?;
            store.commit(cx).await?;
            println!("subsidiary {key} \"{}\" of {operator}", args.target.name);
            println!(
                "  next   triplespace subsidiary key --tenant {} --name \"{}\" --label <label> --grant editentity",
                args.target.tenant, args.target.name
            );
            Ok(())
        }
        Subsidiary::Key(args) => {
            let client = connect(&args.target.database).await?;
            let (key, _) =
                actor_key(&client, &args.target.tenant, Some(&args.target.name), None).await?;
            let expires = args
                .expires_days
                .map(|d| SystemTime::now() + Duration::from_secs(d * 24 * 60 * 60));
            let issued = keys::issue(&client, &key, &args.label, &args.grants, expires).await?;
            println!("key {} issued for {key}", issued.key_id);
            println!("  login   lgname={}@{}", args.target.name, args.label);
            println!(
                "  secret  {}",
                issued.bearer.split_once('.').map_or("", |(_, s)| s)
            );
            println!("  bearer  Authorization: Bearer {}", issued.bearer);
            println!("  shown once; store it now");
            Ok(())
        }
        Subsidiary::Keys(args) => {
            let client = connect(&args.database).await?;
            let (key, _) = actor_key(&client, &args.tenant, Some(&args.name), None).await?;
            for k in keys::list(&client, &key).await? {
                println!(
                    "{:<24} {:<16} {:<32} {}",
                    k.key_id,
                    k.label,
                    k.grants.join(","),
                    if k.revoked_at.is_some() {
                        "revoked"
                    } else {
                        "live"
                    }
                );
            }
            Ok(())
        }
        Subsidiary::Revoke(args) => {
            let client = connect(&args.target.database).await?;
            let (key, _) =
                actor_key(&client, &args.target.tenant, Some(&args.target.name), None).await?;
            if keys::revoke(&client, &key, &args.key_id).await? {
                println!("key {} revoked", args.key_id);
            } else {
                println!("key {} was not live", args.key_id);
            }
            Ok(())
        }
    }
}
