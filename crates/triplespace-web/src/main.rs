//! `triplespace-web`: the stateless web tier (0057 §1–2).
//!
//! It serves the site ([`triplespace_ui`]) from the instance's public API, which it
//! reaches over HTTP ([`triplespace_client::HttpTransport`]). It keeps nothing between
//! requests, so any number of replicas can stand behind the edge, which sends the API's
//! paths to the API and the rest here (`triplespace-web routes`, 0057 §3).
//!
//! ```text
//! triplespace-web --api http://api.svc:8080
//! triplespace-web routes --format caddy
//! ```

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use clap::{Parser, Subcommand};
use triplespace_client::{Client, HttpTransport, Incoming};
use triplespace_ui::routes::{Format, print};

/// The Triplespace web tier.
#[derive(Debug, Parser)]
#[command(
    name = "triplespace-web",
    version,
    about,
    args_conflicts_with_subcommands = true
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    serve: Serve,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Prints the edge proxy's routing: which paths go to the API, which to the web tier.
    Routes {
        /// `caddy`, `nginx`, `haproxy`, or `json` for tools.
        #[arg(long, default_value = "caddy")]
        format: String,
    },
}

#[derive(Debug, clap::Args)]
struct Serve {
    /// The API's base URL (`web.api`): `https://…`, or plain `http://…` to a host that
    /// resolves only to internal addresses (0057 §4).
    #[arg(long, env = "TRIPLESPACE_WEB_API")]
    api: Option<String>,
    /// Where to listen (`web.listen`).
    #[arg(long, env = "TRIPLESPACE_WEB_LISTEN", default_value = "127.0.0.1:8081")]
    listen: String,
    /// A separate listener for `/healthz` and `/readyz` (`web.admin_listen`).
    #[arg(long, env = "TRIPLESPACE_WEB_ADMIN_LISTEN")]
    admin_listen: Option<String>,
    /// A file holding this tier's forwarder key, sent to the API in
    /// `Triplespace-Forwarder` (0057 §10). Read from a file so that it never appears in
    /// a process listing.
    #[arg(long, env = "TRIPLESPACE_WEB_FORWARDER_KEY_FILE")]
    forwarder_key_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(Command::Routes { format }) = args.command {
        let Some(f) = Format::parse(&format) else {
            bail!("--format is caddy, nginx, haproxy or json, not `{format}`");
        };
        print!("{}", print(f));
        return Ok(());
    }
    serve(args.serve).await
}

async fn serve(args: Serve) -> Result<()> {
    let Some(api) = args.api else {
        bail!("--api (or TRIPLESPACE_WEB_API) names the API's base URL");
    };
    let key = match &args.forwarder_key_file {
        Some(path) => {
            let k = std::fs::read_to_string(path)
                .with_context(|| format!("read {}", path.display()))?
                .trim()
                .to_string();
            if k.is_empty() {
                bail!("{} is empty", path.display());
            }
            Some(k)
        }
        None => None,
    };
    let transport = HttpTransport::new(&api, key)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let client = Client::new(transport);
    if let Some(admin) = &args.admin_listen {
        let client = client.clone();
        let admin_router = axum::Router::new()
            .route("/healthz", axum::routing::get(|| async { "ok" }))
            .route(
                "/readyz",
                axum::routing::get(move || {
                    let client = client.clone();
                    async move { ready(&client).await }
                }),
            );
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
        "triplespace-web {} on {}, API at {api}",
        env!("CARGO_PKG_VERSION"),
        args.listen
    );
    axum::serve(
        listener,
        triplespace_ui::router(client).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .context("serve")?;
    Ok(())
}

/// Ready when the API answers its health check. The API's version is checked on every
/// page instead, since `siteinfo` needs a tenant's host (0057 §9).
async fn ready(client: &Client) -> axum::response::Response {
    match client.get(&Incoming::default(), "/healthz").await {
        Ok(r) if r.status == StatusCode::OK => "ok".into_response(),
        Ok(r) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("the API answered {}\n", r.status),
        )
            .into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("{e}\n")).into_response(),
    }
}
