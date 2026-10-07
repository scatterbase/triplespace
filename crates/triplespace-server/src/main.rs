//! `triplespace-server`: the Action API and the `triplespace/v0` REST API over axum
//! (0033 §3; 0056 §10), and the site embedded unless `--ui off` (0057 §1.4).

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use triplespace_api_action::{App, Config, Mode, router_with};
use triplespace_client::{Client, ServiceTransport};

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
    /// Proxies whose X-Forwarded-* headers are trusted by address: addresses and CIDR
    /// ranges (`server.trusted_proxies`, 0057 §10). Forwarder keys need no setting here.
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
    /// `embedded` to serve the site from this process, over the API's own router;
    /// `off` when a separate web tier serves it (`server.ui`, 0057 §1.4).
    #[arg(long, env = "TRIPLESPACE_UI", default_value = "embedded")]
    ui: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let embedded = match args.ui.as_str() {
        "embedded" => true,
        "off" => false,
        other => bail!("--ui is embedded or off, not `{other}`"),
    };
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
            // Clients parse a MediaWiki version out of this (Pywikibot refuses anything
            // under 1.31); the reference install this API is measured against is 1.43.9
            // (mediawiki-compat.md), so that is the version claimed.
            generator: format!(
                "MediaWiki 1.43.9 (Triplespace {})",
                env!("CARGO_PKG_VERSION")
            ),
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
        "triplespace-server {} on {} ({}, site {})",
        env!("CARGO_PKG_VERSION"),
        args.listen,
        args.mode,
        args.ui
    );
    let api = router_with(app, triplespace_api_rest::routes());
    let router = if embedded { with_site(api) } else { api };
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .context("serve")?;
    Ok(())
}

/// The API's router with the site as its fallback, calling the API in process (0057 §1.4,
/// §2): whatever the API does not route, the site serves, except the API's own paths,
/// which the site refuses.
fn with_site(api: axum::Router) -> axum::Router {
    let client = Client::new(ServiceTransport::new(api.clone()));
    api.fallback_service(triplespace_ui::router(client))
}

fn triplespace_accounts_secret(key: &ed25519_dalek::SigningKey) -> triplespace_accounts::Secret {
    triplespace_accounts::Secret::derive(&key.to_bytes())
}
