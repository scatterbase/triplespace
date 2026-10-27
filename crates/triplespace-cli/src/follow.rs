//! `follow`: a registry provider's follower, run once or until stopped (0092 §1–§3, §5),
//! and `upstream-login`: the optional bot password it logs in with (0092 §10).
//!
//! `follow --provider {slug}` continues from the follower's stored position, else from
//! the end of the provider's last dump sync (`sync` records it), else from `--from`.
//! `--once` reads until the wiki has nothing newer and stops; without it the follower
//! polls every `follow.interval` seconds until interrupted.
//!
//! The windows come from the instance settings `follow.settle` and `follow.tail` (0092
//! §6), read from the primary tenant's settings as the server would.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_adapter_mediawiki::Windows;
use scatter_adapter_mediawiki::recentchanges::parse_iso;
use scatter_ingest::follow::run_follow;
use scatter_ingest::write::Attestation;
use scatter_providers::Registry;
use tokio_postgres::Client;
use triplespace_accounts::Secret;
use triplespace_accounts::settings::load;
use triplespace_accounts::upstream_login::{self, Follower};
use triplespace_api_ingest::follow::{ProviderFollower, position};
use triplespace_upstream::Wiki;

use crate::accounts::secret_from;
use crate::common::{connect, farm_of, now, signing_key, store};

/// `follow`.
#[derive(Debug, Args)]
pub struct Follow {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The provider slug (`wikidata`, `librarybase`, …).
    #[arg(long, conflicts_with_all = ["tenant", "source"])]
    pub provider: Option<String>,
    /// The tenant whose entity source to follow (0078 §4), with `--source`.
    #[arg(long, requires = "source")]
    pub tenant: Option<String>,
    /// The entity source's name, with `--tenant`.
    #[arg(long, requires = "tenant")]
    pub source: Option<String>,
    /// Read until the wiki has nothing newer, then stop.
    #[arg(long)]
    pub once: bool,
    /// Where to start when there is no position and no load: an ISO time
    /// (`2026-10-01T00:00:00Z`).
    #[arg(long)]
    pub from: Option<String>,
    /// Seconds between polls when not `--once`; else the `follow.interval` setting.
    #[arg(long)]
    pub interval: Option<u64>,
    /// Entities per unit of work.
    #[arg(long, default_value_t = 200)]
    pub batch: usize,
    /// The instance key file, from which the sealing key derives (for the login).
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: PathBuf,
    /// Follow anonymously even when a login is stored.
    #[arg(long)]
    pub anonymous: bool,
    /// The wiki's `api.php`, when not the registry's (a test wiki, a copy).
    #[arg(long)]
    pub api: Option<String>,
}

/// The `upstream-login` commands.
#[derive(Debug, Subcommand)]
pub enum UpstreamLogin {
    /// Store (or replace) the bot password a provider's follower logs in with.
    Set(SetLogin),
    /// Remove it; the follower goes back to anonymous requests.
    Unset(WhichLogin),
    /// Show whether a login is stored, and its name.
    Show(WhichLogin),
}

/// A provider's follower, or a tenant's source's.
#[derive(Debug, Args)]
pub struct WhichLogin {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The provider slug.
    #[arg(long, conflicts_with_all = ["tenant", "source"])]
    pub provider: Option<String>,
    /// The tenant whose entity source, with `--source`.
    #[arg(long, requires = "source")]
    pub tenant: Option<String>,
    /// The entity source's name, with `--tenant`.
    #[arg(long, requires = "tenant")]
    pub source: Option<String>,
}

impl WhichLogin {
    /// The follower the login is for.
    fn follower(&self) -> Result<Follower> {
        match (&self.provider, &self.tenant, &self.source) {
            (Some(p), None, None) => Ok(Follower::provider(p)),
            (None, Some(t), Some(s)) => Ok(Follower::source(t, s)),
            _ => bail!("name a provider with --provider, or a source with --tenant and --source"),
        }
    }

    /// What to call it.
    fn name(&self) -> String {
        match (&self.provider, &self.tenant, &self.source) {
            (Some(p), _, _) => p.clone(),
            (_, Some(t), Some(s)) => format!("{t}/{s}"),
            _ => String::new(),
        }
    }
}

/// `upstream-login set`.
#[derive(Debug, Args)]
pub struct SetLogin {
    /// The follower.
    #[command(flatten)]
    pub which: WhichLogin,
    /// The bot password's login name, `User@label`.
    #[arg(long)]
    pub username: String,
    /// A file holding the bot password; else `TRIPLESPACE_UPSTREAM_PASSWORD`.
    #[arg(long)]
    pub password_file: Option<PathBuf>,
    /// The instance key file, from which the sealing key derives.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: PathBuf,
}

/// Runs an `upstream-login` command.
pub async fn run_login(cmd: UpstreamLogin) -> Result<()> {
    match cmd {
        UpstreamLogin::Set(args) => {
            let who = args.which.follower()?;
            let password = secret_from(
                args.password_file.as_deref(),
                "TRIPLESPACE_UPSTREAM_PASSWORD",
            )?;
            let secret = Secret::derive(&signing_key(&args.key_file)?.to_bytes());
            let client = connect(&args.which.database).await?;
            upstream_login::set(&client, &secret, &who, &args.username, &password).await?;
            println!(
                "login `{}` stored for `{}`",
                args.username.trim(),
                args.which.name()
            );
        }
        UpstreamLogin::Unset(args) => {
            let who = args.follower()?;
            let client = connect(&args.database).await?;
            if upstream_login::unset(&client, &who).await? {
                println!("login removed for `{}`", args.name());
            } else {
                println!("no login was stored for `{}`", args.name());
            }
        }
        UpstreamLogin::Show(args) => {
            let who = args.follower()?;
            let client = connect(&args.database).await?;
            let row = client
                .query_opt(
                    "SELECT username, updated::text FROM private.upstream_login
                     WHERE tenant = $1 AND kind = $2 AND name = $3",
                    &[&who.tenant, &who.kind, &who.name],
                )
                .await?;
            match row {
                Some(r) => println!(
                    "`{}` logs in as `{}` (set {})",
                    args.name(),
                    r.get::<_, String>(0),
                    r.get::<_, String>(1)
                ),
                None => println!("`{}` follows anonymously", args.name()),
            }
        }
    }
    Ok(())
}

/// The provider or source to follow, in the table of names it is looked up in: the
/// registry for a provider, the tenant's table ([`tenant_registry`]) for a source.
async fn followed(
    client: &Client,
    provider: Option<&str>,
    tenant: Option<&str>,
    source: Option<&str>,
) -> Result<(
    &'static Registry,
    &'static scatter_providers::Provider,
    String,
)> {
    let (registry, slug, tenant) = match (provider, tenant, source) {
        (Some(p), None, None) => (Registry::default_registry(), p, String::new()),
        (None, Some(t), Some(s)) => (tenant_registry(client, t).await?, s, t.to_string()),
        _ => bail!("name a provider with --provider, or a source with --tenant and --source"),
    };
    let provider = registry.by_slug(slug).with_context(|| {
        if tenant.is_empty() {
            format!("provider `{slug}` is not registered")
        } else {
            format!("`{tenant}` declares no source `{slug}`")
        }
    })?;
    if provider.adapter.as_deref() != Some(scatter_adapter_wikibase::NAME) {
        bail!("`{slug}` is not a Wikibase; only Wikibase providers are followed yet");
    }
    if provider.api.is_none() {
        bail!("`{slug}` names no API endpoint");
    }
    Ok((registry, provider, tenant))
}

/// A tenant's table of names (0078 §3), leaked: the adapters borrow a registry for the
/// program's life, and a command builds one.
pub async fn tenant_registry(client: &Client, tenant: &str) -> Result<&'static Registry> {
    let registry = triplespace_projections::entity_sources::registry_for(client, tenant)
        .await
        .map_err(anyhow::Error::msg)?;
    Ok(Box::leak(Box::new(registry)))
}

/// The follower's windows and interval from the instance settings (0092 §6).
async fn windows_of(client: &Client) -> Result<(Windows, u64)> {
    let (snapshot, _) = load(client, None).await.context("settings")?;
    let number = |key: &str, default: u64| {
        snapshot
            .value(key)
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(default)
    };
    let defaults = Windows::default();
    let windows = Windows {
        settle: number("follow.settle", defaults.settle),
        tail: number("follow.tail", defaults.tail),
    };
    Ok((windows, number("follow.interval", 60)))
}

fn seconds() -> u64 {
    now() / 1_000_000
}

/// Runs the follower.
pub async fn run(args: Follow) -> Result<()> {
    let client = connect(&args.database).await?;
    let (registry, provider, tenant) = followed(
        &client,
        args.provider.as_deref(),
        args.tenant.as_deref(),
        args.source.as_deref(),
    )
    .await?;
    let who = if tenant.is_empty() {
        Follower::provider(&provider.slug)
    } else {
        Follower::source(&tenant, &provider.slug)
    };
    let api = args
        .api
        .as_deref()
        .or(provider.api.as_deref())
        .unwrap_or_default();
    let farm = farm_of(&client).await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let (windows, setting_interval) = windows_of(&client).await?;
    let interval = args.interval.unwrap_or(setting_interval).max(1);

    let agent = format!(
        "Triplespace/{} ({}; follow {})",
        env!("CARGO_PKG_VERSION"),
        farm.base,
        provider.slug
    );
    let mut wiki = Wiki::new(api, &agent).context("upstream client")?;
    if !args.anonymous {
        let secret = Secret::derive(&signing_key(&args.key_file)?.to_bytes());
        match upstream_login::get(&client, &secret, &who).await {
            Ok(Some(login)) => match wiki.login(&login.username, &login.password).await {
                Ok(l) => eprintln!(
                    "logged in as {}{}",
                    l.user,
                    if l.high_limits { " (high limits)" } else { "" }
                ),
                Err(e) => eprintln!(
                    "login as `{}` failed, following anonymously: {e}",
                    login.username
                ),
            },
            Ok(None) => {}
            Err(e) => eprintln!("stored login could not be opened, following anonymously: {e}"),
        }
    }

    let from = args
        .from
        .as_deref()
        .map(|t| parse_iso(t).with_context(|| format!("`{t}` is not an ISO time")))
        .transpose()?;
    let mut source = ProviderFollower::start(
        &client, &wiki, provider, registry, &tenant, windows, from, seconds,
    )
    .await?;
    eprintln!(
        "following {} from {}",
        provider.slug,
        scatter_adapter_mediawiki::recentchanges::iso(source.poller().time)
    );

    loop {
        let outcome = run_follow(
            &store,
            &pipeline,
            &tenant,
            &provider.slug,
            Attestation::by(&farm.operator().to_string()),
            &mut source,
            registry,
            now,
            args.batch.max(1),
            true,
        )
        .await
        .context("follow")?;
        let pos = position(&client, &who.tenant, &who.kind, &provider.slug).await?;
        println!(
            "{} rows in {} batch(es): {} created, {} updated, {} unchanged, {} rejected, {} tombstoned, {} redirected; {} late, {} stale; at {}",
            outcome.rows,
            outcome.batches,
            outcome.counts.created,
            outcome.counts.merged,
            outcome.counts.unchanged,
            outcome.counts.rejected,
            outcome.counts.tombstoned,
            outcome.counts.other,
            outcome.late,
            outcome.stale,
            pos.rc_time
                .map(scatter_adapter_mediawiki::recentchanges::iso)
                .unwrap_or_default()
        );
        if args.once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}
