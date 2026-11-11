//! The `triplespace` binary.

#![forbid(unsafe_code)]

// The process allocator is mimalloc (0033 §2, A24): glibc's `malloc` and `free` were a
// third of a Wikidata sync's instructions (docs/testing/performance.md §8). Built without the
// default `mimalloc` feature, the binary uses the system allocator, for heap tools.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use triplespace_cli::{
    accounts, adopt, challenge, compact, dump, env_file, follow, forwarder, import_pages, instance,
    job, log_bundle, logins, oauth_client, rights, settings, source, status, sync, tenant,
};

/// Triplespace: a Wikibase as an append-only log.
#[derive(Debug, Parser)]
#[command(name = "triplespace", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Report what this run uses from the catalogues (permissions, settings, log events) to
    /// this file, one `{kind} {name}` line each, for `tools/probe_report.py` (0107 §1). For
    /// test runs; off unless set.
    #[arg(long, global = true, env = "TRIPLESPACE_PROBE")]
    probe: Option<std::path::PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// The instance: create it; its forwarder keys.
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },
    /// Tenants: create one on an existing instance, list them (0018; 0079).
    Tenant {
        #[command(subcommand)]
        command: tenant::TenantCommand,
    },
    /// Adopt a frozen Wikibase from its XML dump into a tenant (0035).
    Adopt(adopt::Adopt),
    /// Import pages from any wiki's MediaWiki XML export into a tenant (0008 §9).
    Import(import_pages::Import),
    /// The job queue: list, start, cancel, pause and resume (0104 §4).
    Job {
        #[command(subcommand)]
        command: job::JobCommand,
    },
    /// Sync a provider's dump into its mirror (0002 §8.4).
    Sync(sync::Sync),
    /// Follow a provider's wiki by its recent changes (0092 §1).
    Follow(follow::Follow),
    /// Compact a provider's mirror by the keys of a range of its records (0013 §3, A96).
    Compact(compact::Compact),
    /// The bot password a provider's follower logs in with (0092 §10).
    UpstreamLogin {
        #[command(subcommand)]
        command: follow::UpstreamLogin,
    },
    /// A tenant's entity sources (0078 §1).
    Source {
        #[command(subcommand)]
        command: source::SourceCommand,
    },
    /// Passwords of primary accounts (0007 §3).
    Password {
        #[command(subcommand)]
        command: PasswordCommand,
    },
    /// Subsidiary accounts and their API keys (0024).
    Subsidiary {
        #[command(subcommand)]
        command: accounts::Subsidiary,
    },
    /// Registration and login challenges (0087 §9).
    Challenge {
        #[command(subcommand)]
        command: ChallengeCommand,
    },
    /// Client credentials for logging in through external issuers (0088 §3).
    OauthClient {
        #[command(subcommand)]
        command: oauth_client::OAuthClient,
    },
    /// Settings: a tenant's, the instance's, its defaults and locks (0085 §3).
    Settings {
        #[command(subcommand)]
        command: settings::Settings,
    },
    /// Memberships, as instance acts: appointing owners among them (09 §3.6).
    Member {
        #[command(subcommand)]
        command: rights::Member,
    },
    /// Group definitions (09 §3.2).
    Group {
        #[command(subcommand)]
        command: rights::GroupCommand,
    },
    /// ACLs on pages, namespaces, sets and the tenant (09 §4).
    Acl {
        #[command(subcommand)]
        command: rights::AclCommand,
    },
    /// Write a tenant's dumps: the resolved view and the local graph (02 §4.1).
    Dump(dump::Dump),
    /// Sign every partition's new head and unsigned full segments (0006 §6).
    Checkpoint(log_bundle::Checkpoint),
    /// Write a bundle of the log (0006 §9.1, §9.2; 0103 §1).
    Export(log_bundle::Export),
    /// Verify a bundle (0006 §9.3).
    Verify(log_bundle::Verify),
    /// Partitions, heads and projection lags, and any migration not yet applied.
    Status(status::Target),
    /// Apply the migrations this binary carries that the database has not applied.
    Migrate(status::Database),
    /// Instance logins: the migration of tenant-account credentials (0089 §15).
    Logins {
        #[command(subcommand)]
        command: logins::LoginsCommand,
    },
    /// Replay every projection from the log.
    Rebuild(status::Target),
}

#[derive(Debug, Subcommand)]
enum PasswordCommand {
    /// Set or replace an account's password from a file or `TRIPLESPACE_PASSWORD`.
    Set(accounts::PasswordSet),
}

#[derive(Debug, Subcommand)]
enum ChallengeCommand {
    /// Print a solution of a challenge.
    Solve(challenge::Solve),
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // parsed once from the command line
enum InstanceCommand {
    /// Create the instance and its primary tenant.
    Create(instance::Create),
    /// Forwarder keys, which let a proxy vouch for the client address (0057 §10).
    Forwarder {
        #[command(subcommand)]
        command: forwarder::Forwarder,
    },
}

fn main() -> Result<()> {
    // Before the runtime starts: the `.env` file is read into the environment (23 §5.1).
    env_file::load()?;
    run()
}

#[tokio::main]
async fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(path) = &cli.probe {
        triplespace_cli::probe::start(path)?;
    }
    match cli.command {
        Command::Instance {
            command: InstanceCommand::Create(args),
        } => instance::run(args).await,
        Command::Instance {
            command: InstanceCommand::Forwarder { command },
        } => forwarder::run(command).await,
        Command::Tenant { command } => tenant::run(command).await,
        Command::Adopt(args) => adopt::run(args).await,
        Command::Import(args) => import_pages::run(args).await,
        Command::Job { command } => job::run(command).await,
        Command::Sync(args) => sync::run(args).await,
        Command::Follow(args) => follow::run(args).await,
        Command::Compact(args) => compact::run(args).await,
        Command::UpstreamLogin { command } => follow::run_login(command).await,
        Command::Source { command } => source::run(command).await,
        Command::Password {
            command: PasswordCommand::Set(args),
        } => accounts::run_password_set(args).await,
        Command::Subsidiary { command } => accounts::run_subsidiary(command).await,
        Command::Challenge {
            command: ChallengeCommand::Solve(args),
        } => challenge::run(&args),
        Command::OauthClient { command } => oauth_client::run(command).await,
        Command::Settings { command } => settings::run(command).await,
        Command::Member { command } => rights::run_member(command).await,
        Command::Group { command } => rights::run_group(command).await,
        Command::Acl { command } => rights::run_acl(command).await,
        Command::Dump(args) => dump::run(args).await,
        Command::Checkpoint(args) => log_bundle::run_checkpoint(args).await,
        Command::Export(args) => log_bundle::run_export(args).await,
        Command::Verify(args) => log_bundle::run_verify(args).await,
        Command::Status(args) => status::status(args).await,
        Command::Migrate(args) => status::migrate(args).await,
        Command::Logins { command } => logins::run(command).await,
        Command::Rebuild(args) => status::rebuild(args).await,
    }
}
