//! The `triplespace` binary.

#![forbid(unsafe_code)]

use anyhow::Result;
use clap::{Parser, Subcommand};
use triplespace_cli::{accounts, adopt, forwarder, instance, status, sync};

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
    /// Partitions, heads and projection lags.
    Status(status::Target),
    /// Replay every projection from the log.
    Rebuild(status::Target),
}

#[derive(Debug, Subcommand)]
enum PasswordCommand {
    /// Set or replace an account's password from a file or `TRIPLESPACE_PASSWORD`.
    Set(accounts::PasswordSet),
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
        Command::Status(args) => status::status(args).await,
        Command::Rebuild(args) => status::rebuild(args).await,
    }
}
