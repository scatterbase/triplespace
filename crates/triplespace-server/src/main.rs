//! `triplespace-server`: the Action API over axum (0033 §3; 0056 §10).

#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use triplespace_api_action::{App, Config, Mode, router};

/// The Triplespace server.
#[derive(Debug, Parser)]
#[command(name = "triplespace-server", version, about)]
struct Args {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    database: String,
    /// Where to listen.
    #[arg(long, env = "TRIPLESPACE_LISTEN", default_value = "127.0.0.1:8080")]
    listen: String,
    /// A separate listener for health and debug routes (`server.admin_listen`).
    #[arg(long, env = "TRIPLESPACE_ADMIN_LISTEN")]
    admin_listen: Option<String>,
    /// `production` or `development` (`server.mode`).
    #[arg(long, env = "TRIPLESPACE_MODE", default_value = "production")]
    mode: String,
    /// Proxies whose X-Forwarded-* headers are trusted (`server.trusted_proxies`).
    #[arg(
        long = "trusted-proxy",
        env = "TRIPLESPACE_TRUSTED_PROXIES",
        value_delimiter = ','
    )]
    trusted_proxies: Vec<String>,
    /// In development mode, the tenant an unregistered host (localhost) is served as.
    #[arg(long, env = "TRIPLESPACE_DEV_TENANT")]
    dev_tenant: Option<String>,
    /// The instance key file; sessions' tokens derive from it.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    key_file: PathBuf,
    /// Connections in the pool.
    #[arg(long, default_value_t = 16)]
    pool: usize,
    /// The farm slug and base; the first tenant's by default.
    #[arg(long)]
    farm_slug: Option<String>,
    /// The farm base.
    #[arg(long)]
    farm_base: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mode = Mode::parse(&args.mode)
        .with_context(|| format!("--mode is production or development, not `{}`", args.mode))?;
    if mode == Mode::Production && args.dev_tenant.is_some() {
        bail!("--dev-tenant is a development-mode setting (0056 §10)");
    }
    let key = triplespace_cli::common::signing_key(&args.key_file)?;
    let secret = triplespace_accounts_secret(&key);
    let client = triplespace_cli::common::connect(&args.database).await?;
    let farm = triplespace_cli::common::farm_of(
        &client,
        args.farm_slug.as_deref(),
        args.farm_base.as_deref(),
    )
    .await?;
    drop(client);
    let pool = triplespace_db::pool::pool(&args.database, args.pool).context("pool")?;
    let app = App::new(
        pool,
        secret,
        Config {
            mode,
            trusted_proxies: args.trusted_proxies.clone(),
            dev_tenant: args.dev_tenant.clone(),
            farm,
            generator: format!("Triplespace {}", env!("CARGO_PKG_VERSION")),
        },
    )
    .map_err(|e| anyhow::anyhow!(e))?;
    if let Some(admin) = &args.admin_listen {
        let admin_router =
            axum::Router::new().route("/healthz", axum::routing::get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind(admin)
            .await
            .with_context(|| format!("bind {admin}"))?;
        eprintln!("admin on {admin}");
        tokio::spawn(async move {
            let _ = axum::serve(listener, admin_router).await;
        });
    }
    let listener = tokio::net::TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("bind {}", args.listen))?;
    eprintln!(
        "triplespace-server {} on {} ({})",
        env!("CARGO_PKG_VERSION"),
        args.listen,
        args.mode
    );
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("serve")?;
    Ok(())
}

fn triplespace_accounts_secret(key: &ed25519_dalek::SigningKey) -> triplespace_accounts::Secret {
    triplespace_accounts::Secret::derive(&key.to_bytes())
}
