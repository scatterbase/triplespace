//! `triplespace`: the command line (0005 §2; 0033 §13).

#![forbid(unsafe_code)]

mod adopt;
mod common;
mod instance;
mod status;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Triplespace: a Wikibase as an append-only log.
#[derive(Debug, Parser)]
#[command(name = "triplespace", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// The instance: create it.
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },
    /// Adopt a frozen Wikibase from its XML dump into a tenant (0035).
    Adopt(adopt::Adopt),
    /// Partitions, heads and projection lags.
    Status(status::Target),
    /// Replay every projection from the log.
    Rebuild(status::Target),
}

#[derive(Debug, Subcommand)]
enum InstanceCommand {
    /// Create the instance and its first tenant.
    Create(instance::Create),
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Instance {
            command: InstanceCommand::Create(args),
        } => instance::run(args).await,
        Command::Adopt(args) => adopt::run(args).await,
        Command::Status(args) => status::status(args).await,
        Command::Rebuild(args) => status::rebuild(args).await,
    }
}
