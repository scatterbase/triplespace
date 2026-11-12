//! `triplespace-server`: the Action API and the `triplespace/v0` REST API over axum
//! (0033 §3; 0056 §10), and the site embedded unless `--ui off` (0057 §1.4).

#![forbid(unsafe_code)]

// The process allocator is mimalloc (0033 §2, A24): glibc's `malloc` and `free` were a
// third of a Wikidata sync's instructions (docs/testing/performance.md §8). Built without the
// default `mimalloc` feature, the binary uses the system allocator, for heap tools.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
    /// The Postgres URL the public API and the write path connect with: a login granted
    /// `ts_server` (0013 §4). Without the two below, every part connects with it.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    database: String,
    /// The Postgres URL the accounts service connects with: a login granted `ts_accounts`,
    /// the role that reads and writes `private` (0013 §4, A99).
    #[arg(long, env = "TRIPLESPACE_ACCOUNTS_DATABASE_URL")]
    accounts_database: Option<String>,
    /// The Postgres URL the notifier connects with: a login granted `ts_notify`.
    #[arg(long, env = "TRIPLESPACE_NOTIFY_DATABASE_URL")]
    notify_database: Option<String>,
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
    /// Worker tasks over the work queue (03 §6.1): fan-out past the synchronous budget
    /// and the tier-3 consumers (reports). `0` runs none, for a process that only serves
    /// while another runs the workers.
    #[arg(long, env = "TRIPLESPACE_WORKERS", default_value_t = 2)]
    workers: usize,
    /// Job runner tasks over `ops.job_queue` (0104 §4; 16 §2.2): each runs one queued job
    /// at a time, one running job per partition between them. `0` runs none, for a
    /// process that only serves while another runs the jobs.
    #[arg(long, env = "TRIPLESPACE_JOB_RUNNERS", default_value_t = 1)]
    job_runners: usize,
    /// The auth host, `server.auth_base` (0089 §1): where a person authenticates to the
    /// instance; required once the instance has a second tenant.
    #[arg(long, env = "TRIPLESPACE_AUTH_BASE")]
    auth_base: Option<String>,
    /// `embedded` to serve the site from this process, over the API's own router;
    /// `off` when a separate web tier serves it (`server.ui`, 0057 §1.4).
    #[arg(long, env = "TRIPLESPACE_UI", default_value = "embedded")]
    ui: String,
    /// A directory of installed skins beside the shipped ones (`server.skins`, 0097 §8).
    #[arg(long, env = "TRIPLESPACE_SKINS")]
    skins: Option<std::path::PathBuf>,
    /// How the instance sends mail (`mail.transport`, 0100 §5): `none`, `log`, `smtp` or
    /// `postmark`. `log` is refused in production.
    #[arg(long, env = "TRIPLESPACE_MAIL_TRANSPORT", default_value = "none")]
    mail_transport: String,
    /// The instance's one sender address (`mail.from`), required unless the transport is
    /// `none`.
    #[arg(long, env = "TRIPLESPACE_MAIL_FROM")]
    mail_from: Option<String>,
    /// Where the `log` transport writes each message as an `.eml` file (`mail.log_dir`).
    #[arg(long, env = "TRIPLESPACE_MAIL_LOG_DIR", default_value = "data/mail")]
    mail_log_dir: PathBuf,
    /// Where `triplespace dump` writes each tenant's dumps, which `{base}/dumps/` serves
    /// (`dumps.dir`, 0013 A78).
    #[arg(long, env = "TRIPLESPACE_DUMPS_DIR", default_value = "data/dumps")]
    dumps_dir: PathBuf,
    /// The SMTP relay (`mail.smtp.url`): `smtps://user@host` for implicit TLS,
    /// `smtp://user@host` with STARTTLS required, plaintext only to a loopback host.
    #[arg(long, env = "TRIPLESPACE_MAIL_SMTP_URL")]
    mail_smtp_url: Option<String>,
    /// The relay's password, read from a file (`mail.smtp.password_file`).
    #[arg(long, env = "TRIPLESPACE_MAIL_SMTP_PASSWORD_FILE")]
    mail_smtp_password_file: Option<PathBuf>,
    /// Postmark's server token, read from a file (`mail.postmark.token_file`).
    #[arg(long, env = "TRIPLESPACE_MAIL_POSTMARK_TOKEN_FILE")]
    mail_postmark_token_file: Option<PathBuf>,
    /// The HTTP Basic password Postmark's bounce and complaint webhook presents, read
    /// from a file (`mail.postmark.webhook_secret_file`); without it the webhook is 404.
    #[arg(long, env = "TRIPLESPACE_MAIL_POSTMARK_WEBHOOK_SECRET_FILE")]
    mail_postmark_webhook_secret_file: Option<PathBuf>,
    /// Postmark's stream for codes and security notices (`mail.postmark.stream`).
    #[arg(
        long,
        env = "TRIPLESPACE_MAIL_POSTMARK_STREAM",
        default_value = "outbound"
    )]
    mail_postmark_stream: String,
    /// Postmark's stream for notifications and digests
    /// (`mail.postmark.notification_stream`), a broadcast stream the operator creates.
    #[arg(
        long,
        env = "TRIPLESPACE_MAIL_POSTMARK_NOTIFICATION_STREAM",
        default_value = "broadcast"
    )]
    mail_postmark_notification_stream: String,
    /// Report what the server uses from the catalogues (permissions, settings, log events)
    /// to this file, one `{kind} {name}` line each, for `tools/probe_report.py` (0107 §1).
    /// For test runs; off unless set.
    #[arg(long, env = "TRIPLESPACE_PROBE")]
    probe: Option<PathBuf>,
}

/// The mail transport the settings choose (0100 §5; 16 §5.7), and what the handlers are
/// told of it.
fn mail_transport(
    args: &Args,
    mode: Mode,
) -> Result<(
    Option<std::sync::Arc<dyn scatter_mail::MailTransport>>,
    triplespace_api_action::MailSettings,
)> {
    use std::sync::Arc;
    let read_secret = |path: &Option<PathBuf>, what: &str| -> Result<Option<String>> {
        path.as_ref()
            .map(|p| {
                std::fs::read_to_string(p)
                    .map(|s| s.trim().to_string())
                    .with_context(|| format!("{what} {}", p.display()))
            })
            .transpose()
    };
    let transport: Option<Arc<dyn scatter_mail::MailTransport>> = match args.mail_transport.as_str()
    {
        "none" => None,
        "log" => {
            if mode == Mode::Production {
                bail!(
                    "--mail-transport log is a development sink and is refused in production (0100 §5)"
                );
            }
            Some(Arc::new(scatter_mail::log::LogTransport::new(
                &args.mail_log_dir,
            )?))
        }
        "smtp" => {
            let url = args
                .mail_smtp_url
                .as_deref()
                .filter(|u| !u.is_empty())
                .ok_or_else(|| anyhow::anyhow!("--mail-transport smtp needs --mail-smtp-url"))?;
            let password = read_secret(&args.mail_smtp_password_file, "--mail-smtp-password-file")?;
            Some(Arc::new(scatter_mail::smtp::SmtpTransport::new(
                url,
                password.as_deref(),
            )?))
        }
        "postmark" => {
            let token = read_secret(&args.mail_postmark_token_file, "--mail-postmark-token-file")?
                .ok_or_else(|| {
                    anyhow::anyhow!("--mail-transport postmark needs --mail-postmark-token-file")
                })?;
            Some(Arc::new(scatter_mail::postmark::PostmarkTransport::new(
                &token,
                &args.mail_postmark_stream,
                &args.mail_postmark_notification_stream,
            )?))
        }
        other => bail!("--mail-transport is none, log, smtp or postmark, not `{other}`"),
    };
    let from = args.mail_from.clone().filter(|f| !f.is_empty());
    if transport.is_some() {
        let f = from.as_deref().ok_or_else(|| {
            anyhow::anyhow!("--mail-from is required unless --mail-transport is none (0100 §5)")
        })?;
        triplespace_accounts::email::parse(f)
            .map_err(|_| anyhow::anyhow!("--mail-from `{f}` is not an email address"))?;
    }
    let settings = triplespace_api_action::MailSettings {
        transport: args.mail_transport.clone(),
        from,
        postmark_webhook_secret: read_secret(
            &args.mail_postmark_webhook_secret_file,
            "--mail-postmark-webhook-secret-file",
        )?,
    };
    Ok((transport, settings))
}

/// The delivery worker (16 §5.7): a pass over due rows whenever a row is staged
/// (`LISTEN ts_delivery`), when the next retry is due, and at least every minute.
async fn deliver(
    app: triplespace_api_action::App,
    database: String,
    transport: std::sync::Arc<dyn scatter_mail::MailTransport>,
) {
    use std::sync::Arc;
    let wake = Arc::new(tokio::sync::Notify::new());
    {
        // A connection of its own listens; its notifications wake the worker.
        let wake = wake.clone();
        let database = database.clone();
        tokio::spawn(async move {
            loop {
                match listen(&database, &wake).await {
                    Ok(()) => {}
                    Err(e) => eprintln!("mail: listener: {e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    }
    loop {
        let pass = match app.notify_pool().get().await {
            Ok(mut client) => {
                triplespace_notify::mail::deliver_once(&mut client, transport.as_ref())
                    .await
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        };
        let wait = match pass {
            Ok(p) if !p.idle() => continue,
            Ok(_) => match app.pool().get().await {
                Ok(client) => triplespace_notify::mail::next_due(&**client)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|t| t.duration_since(std::time::SystemTime::now()).ok())
                    .unwrap_or(DELIVERY_IDLE)
                    .min(DELIVERY_IDLE),
                Err(_) => DELIVERY_IDLE,
            },
            Err(e) => {
                eprintln!("mail: delivery: {e}");
                WORKER_IDLE
            }
        };
        tokio::select! {
            () = wake.notified() => {}
            () = tokio::time::sleep(wait.max(std::time::Duration::from_millis(100))) => {}
        }
    }
}

/// How long the delivery worker sleeps with nothing due.
const DELIVERY_IDLE: std::time::Duration = std::time::Duration::from_mins(1);

/// Holds a connection on `LISTEN ts_delivery` and notifies the worker of each message.
async fn listen(database: &str, wake: &tokio::sync::Notify) -> Result<()> {
    let config: tokio_postgres::Config = database.parse().context("database URL")?;
    let (client, mut connection) = config.connect(tokio_postgres::NoTls).await?;
    let channel = triplespace_notify::mail::CHANNEL;
    // The LISTEN must be driven while the connection is polled, so the two are joined.
    let statement = format!("LISTEN {channel}");
    let listen = client.batch_execute(&statement);
    let drive = async {
        loop {
            match std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                None => return Ok::<(), anyhow::Error>(()),
                Some(Ok(tokio_postgres::AsyncMessage::Notification(_))) => wake.notify_one(),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
            }
        }
    };
    tokio::pin!(drive);
    tokio::select! {
        r = &mut drive => r,
        r = listen => {
            r?;
            drive.await
        }
    }
}

/// How long an idle worker waits before looking again.
const WORKER_IDLE: std::time::Duration = std::time::Duration::from_secs(1);

/// The background work of a running server: the services' states for Special:Version,
/// the asynchronous projection steps, and the workers.
async fn background(app: &triplespace_api_action::App, workers: usize) {
    // The services' states and versions for Special:Version and siteinfo: read now and
    // every minute, never while answering a request (0077 §4).
    triplespace_api_action::version::refresh(app).await;
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(PROBE_EVERY);
            every.tick().await;
            loop {
                every.tick().await;
                triplespace_api_action::version::refresh(&app).await;
            }
        });
    }
    // The position-based asynchronous step (addressing), behind every write by about this
    // much (0013 §7).
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(WORKER_IDLE);
            loop {
                every.tick().await;
                if let Err(e) = app.catch_up_async().await {
                    eprintln!("asynchronous projections: {e}");
                }
            }
        });
    }
    // The workers over the work queue (03 §6.1). Each runs a pass, then goes again at once
    // while there is work and waits when there is none; the queue's per-tenant locks share
    // the tenants between them. A consumer's failure is logged once per batch and recorded
    // on the items, which are retried a bounded number of times.
    for n in 0..workers {
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                match app.work_once().await {
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
                        if ran.idle() {
                            tokio::time::sleep(WORKER_IDLE).await;
                        }
                    }
                    Err(e) => {
                        eprintln!("worker {n}: {e}");
                        tokio::time::sleep(WORKER_IDLE).await;
                    }
                }
            }
        });
    }
}

/// Starts the probe (0107 §1), with the API's observers of the modules and routes served,
/// which the CLI library cannot install (0108 §1).
fn start_probe(path: &std::path::Path) -> Result<()> {
    triplespace_cli::probe::start(path)?;
    triplespace_api_action::observe_modules(triplespace_cli::probe::module);
    triplespace_api_rest::observe_routes(triplespace_cli::probe::route);
    eprintln!("probe: reporting to {}", path.display());
    Ok(())
}

fn main() -> Result<()> {
    // Before the runtime starts: the `.env` file is read into the environment (23 §5.1).
    if let Some(path) = triplespace_cli::env_file::load()? {
        eprintln!("configuration from {}", path.display());
    }
    run()
}

#[tokio::main]
async fn run() -> Result<()> {
    let args = Args::parse();
    if let Some(path) = &args.probe {
        start_probe(path)?;
    }
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
    // A database behind this binary's migrations would answer every read that touches a
    // missing table with an internal error; refuse up front and name the fix.
    let pending = triplespace_db::migrator()
        .pending(&client)
        .await
        .context("migrations")?;
    if !pending.is_empty() {
        bail!(
            "the database has not applied {}; run `triplespace migrate` first",
            pending
                .iter()
                .map(|(c, n)| format!("{c}/{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let farm = triplespace_cli::common::farm_of(&client).await?;
    let auth_base = auth_base(&client, &farm, args.auth_base.as_deref()).await?;
    drop(client);
    let (transport, mail) = mail_transport(&args, mode)?;
    let pools = pools(&args).await?;
    let app = App::new(
        pools,
        secret,
        Config {
            mode,
            trusted_proxies: args.trusted_proxies.clone(),
            dev_tenant: args.dev_tenant.clone(),
            farm,
            auth_base,
            // Clients parse a MediaWiki version out of this (Pywikibot refuses anything
            // under 1.31); the reference install this API is measured against is 1.43.9
            // (mediawiki-compat.md), so that is the version claimed.
            generator: format!(
                "MediaWiki 1.43.9 (Triplespace {})",
                env!("CARGO_PKG_VERSION")
            ),
            version: Some(VERSION_JSON),
        },
    )
    .map_err(|e| anyhow::anyhow!(e))?;
    app.set_mail(mail);
    app.set_dumps_dir(args.dumps_dir.clone());
    if let Some(transport) = transport {
        eprintln!("mail: transport {}", transport.name());
        let notify = args.notify_database.as_ref().unwrap_or(&args.database);
        tokio::spawn(deliver(app.clone(), notify.clone(), transport));
    }
    background(&app, args.workers).await;
    // The queued kinds, import and revert, write public partitions only: the runners
    // connect as the public API (0013 §4, A101). An adoption, which appends to accounts
    // partitions, is run by `triplespace adopt` as the owner until it joins the queue.
    job_runners(&app, &args.database, args.job_runners, &args.dumps_dir)?;
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
    let router = if embedded {
        with_site(api, args.skins.as_deref())
    } else {
        api
    };
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
fn with_site(api: axum::Router, skins: Option<&std::path::Path>) -> axum::Router {
    let client = Client::new(ServiceTransport::new(api.clone()));
    let site = triplespace_ui::Site::new(client).with_skins_dir(skins);
    api.fallback_service(triplespace_ui::router_for(site))
}

/// The pools of each part, from its URL (0013 §4, A99), reported as they connect.
async fn pools(args: &Args) -> Result<triplespace_db::pool::Pools> {
    let pools = triplespace_db::pool::Pools::new(
        &args.database,
        args.accounts_database.as_deref(),
        args.notify_database.as_deref(),
        args.pool,
    )
    .context("pool")?;
    report_privacy(&pools).await;
    Ok(pools)
}

/// Says, for each pool, whether its login is held to its role (0013 §4, A99): a public
/// pool that can read `private`, as the owner can, means the database does not enforce
/// the privacy of `private` and the accounts partition, and the server says so.
async fn report_privacy(pools: &triplespace_db::pool::Pools) {
    use triplespace_db::grants::role;
    let parts = [
        ("public API", &pools.server, role::SERVER),
        ("accounts service", &pools.accounts, role::ACCOUNTS),
        ("notifier", &pools.notify, role::NOTIFY),
    ];
    for (part, pool, want) in parts {
        let client = match pool.get().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("database: the {part} cannot connect: {e}");
                continue;
            }
        };
        let row = client
            .query_one(
                "SELECT current_user::text, has_schema_privilege('private', 'USAGE')",
                &[],
            )
            .await;
        match row {
            Ok(r) => {
                let (user, reads_private): (String, bool) = (r.get(0), r.get(1));
                if want == role::SERVER && reads_private {
                    eprintln!(
                        "database: warning: the {part} connects as `{user}`, which can read \
                         `private`; the database does not enforce its privacy. Connect it as a \
                         login granted {want}, and the accounts service and the notifier with \
                         --accounts-database and --notify-database (0013 §4)"
                    );
                } else {
                    eprintln!("database: the {part} connects as `{user}`");
                }
            }
            Err(e) => eprintln!("database: the {part}: {e}"),
        }
    }
}

/// The job runners (0104 §4): background tasks claiming `ops.job_queue`, each with a
/// store of its own, since a job's units of work are its own transactions. Each runs
/// imports and reverts, and dumps into `dumps_dir` (0104 A6).
fn job_runners(
    app: &triplespace_api_action::App,
    database: &str,
    n: usize,
    dumps_dir: &std::path::Path,
) -> Result<()> {
    for i in 0..n {
        let (store, pipeline) =
            triplespace_cli::common::store(database, &app.config().farm).context("job runner")?;
        tokio::spawn(triplespace_api_ingest::jobs::run_forever(
            store,
            pipeline,
            triplespace_api_ingest::jobs::Runner::new(format!("runner {i}")).with_dumps(dumps_dir),
            WORKER_IDLE,
            triplespace_cli::common::now,
        ));
    }
    Ok(())
}

/// How often the services are probed for Special:Version (0077 §4), in seconds.
const PROBE_SECS: u64 = 60;
const PROBE_EVERY: std::time::Duration = std::time::Duration::from_secs(PROBE_SECS);

/// This binary's build and its part of the component manifest (0077 §15), written by
/// `build.rs`.
const VERSION_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/version.json"));

fn triplespace_accounts_secret(key: &ed25519_dalek::SigningKey) -> triplespace_accounts::Secret {
    triplespace_accounts::Secret::derive(&key.to_bytes())
}

/// The auth base (0089 §1): required once there is a second tenant, and neither a tenant
/// base nor the farm base.
async fn auth_base(
    client: &tokio_postgres::Client,
    farm: &triplespace_projections::Farm,
    given: Option<&str>,
) -> Result<Option<String>> {
    let tenants = triplespace_api_action::tenant::registered(client).await?;
    let auth_base = given.filter(|a| !a.is_empty()).map(str::to_owned);
    match &auth_base {
        None if tenants.len() > 1 => bail!(
            "ts-auth-base-required: this instance has {} tenants and needs an auth host; set --auth-base (server.auth_base, 0089 §1)",
            tenants.len()
        ),
        Some(a) => {
            let host = triplespace_api_action::tenant::host_of(a)
                .with_context(|| format!("--auth-base `{a}` is not a URL"))?;
            let clash = |base: &str| {
                triplespace_api_action::tenant::host_of(base)
                    .is_some_and(|h| h.eq_ignore_ascii_case(&host))
            };
            if clash(&farm.base) || tenants.iter().any(|t| clash(&t.base)) {
                bail!("--auth-base `{a}` is the farm base or a tenant's base (0089 §1)");
            }
        }
        None => {}
    }
    Ok(auth_base)
}
