//! `triplespace registry check` (0078 §3, §9; 05 §6.3, §6.8): what the registry this
//! binary embeds means for each tenant's entity sources. After an update it lists every
//! source whose name the registry now has (a provider slug, keyed type, resolver or
//! notation scheme), which the source shadows on its tenant, and every unpromoted source a
//! registry provider now covers, which the tenant can promote. It changes nothing.

use anyhow::Result;
use clap::{Args, Subcommand};
use scatter_providers::Registry;
use triplespace_api_ingest::sources::findings;

use crate::common::connect;

/// The registry commands.
#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// List the sources a registry update shadows or makes promotable.
    Check(Check),
}

/// `registry check`.
#[derive(Debug, Args)]
pub struct Check {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// One tenant; every tenant by default.
    #[arg(long)]
    pub tenant: Option<String>,
}

/// Runs a registry command.
pub async fn run(cmd: RegistryCommand) -> Result<()> {
    match cmd {
        RegistryCommand::Check(args) => check(args).await,
    }
}

async fn check(args: Check) -> Result<()> {
    let client = connect(&args.database).await?;
    let tenants: Vec<String> = match args.tenant {
        Some(t) => vec![t],
        None => triplespace_projections::tenants::tenants(&client)
            .await?
            .into_iter()
            .map(|t| t.slug)
            .collect(),
    };
    let registry = Registry::default_registry();
    let mut reported = 0;
    for tenant in &tenants {
        let sources = triplespace_projections::entity_sources::declared(&client, tenant)
            .await
            .map_err(anyhow::Error::msg)?;
        for source in &sources {
            for line in findings(source, registry) {
                println!("{tenant}: {line}");
                reported += 1;
            }
        }
    }
    if reported == 0 {
        println!(
            "nothing to report: no source on {} is shadowed by the registry or promotable",
            if tenants.len() == 1 {
                format!("`{}`", tenants[0])
            } else {
                format!("{} tenants", tenants.len())
            }
        );
    }
    Ok(())
}
