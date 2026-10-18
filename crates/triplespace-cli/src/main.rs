//! The `triplespace` binary.

#![forbid(unsafe_code)]

use anyhow::Result;
use clap::{Parser, Subcommand};
use triplespace_cli::{accounts, adopt, challenge, forwarder, instance, settings, status, sync};

/// Triplespace: a Wikibase as an append-only log.
#[derive(Debug, Parser)]
#[command(name = "triplespace", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// The instance: create it; its forwarder keys.
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },
    /// Adopt a frozen Wikibase from its XML dump into a tenant (0035).
    Adopt(adopt::Adopt),
    /// Sync a provider's dump into its mirror (0002 §8.4).
    Sync(sync::Sync),
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
    /// Settings: a tenant's, the instance's, its defaults and locks (0085 §3).
    Settings {
        #[command(subcommand)]
        command: settings::Settings,
    },
    /// Partitions, heads and projection lags, and any migration not yet applied.
    Status(status::Target),
    /// Apply the migrations this binary carries that the database has not applied.
    Migrate(status::Database),
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
enum InstanceCommand {
    /// Create the instance and its first tenant.
    Create(instance::Create),
    /// Forwarder keys, which let a proxy vouch for the client address (0057 §10).
    Forwarder {
        #[command(subcommand)]
        command: forwarder::Forwarder,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Instance {
            command: InstanceCommand::Create(args),
        } => instance::run(args).await,
        Command::Instance {
            command: InstanceCommand::Forwarder { command },
        } => forwarder::run(command).await,
        Command::Adopt(args) => adopt::run(args).await,
        Command::Sync(args) => sync::run(args).await,
        Command::Password {
            command: PasswordCommand::Set(args),
        } => accounts::run_password_set(args).await,
        Command::Subsidiary { command } => accounts::run_subsidiary(command).await,
        Command::Challenge {
            command: ChallengeCommand::Solve(args),
        } => challenge::run(&args),
        Command::Settings { command } => settings::run(command).await,
        Command::Status(args) => status::status(args).await,
        Command::Migrate(args) => status::migrate(args).await,
        Command::Rebuild(args) => status::rebuild(args).await,
    }
}
