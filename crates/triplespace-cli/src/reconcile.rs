//! `reconcile`: the `reconcile:default` record (0004 §9, A29; 23 §2.3). The instance's is
//! the policy record shared rows are computed under: the provider order, the tier-2 link
//! properties and the inference properties. A tenant's narrows it and configures the
//! tenant's own entity sources. `reconcile show` prints the record, `reconcile set` writes
//! one from a JSON file as an instance act, and `reconcile unset` retires it.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use scatter_projection::Backend as _;
use triplespace_api_ingest::reconcile::write;

use crate::common::{connect, farm_of, now, store};
use crate::rights::operator;

/// The `reconcile` commands.
#[derive(Debug, Subcommand)]
pub enum ReconcileCommand {
    /// Print the record.
    Show(At),
    /// Write the record from a JSON file.
    Set(Set),
    /// Retire the record: the defaults apply again.
    Unset(Unset),
}

/// The database and the scope.
#[derive(Debug, Args)]
pub struct At {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug; the instance's record without it.
    #[arg(long)]
    pub tenant: Option<String>,
}

/// `reconcile set`.
#[derive(Debug, Args)]
pub struct Set {
    /// The database and the scope.
    #[command(flatten)]
    pub at: At,
    /// The record, a JSON object: for the instance `order`, `order_by_type`, `links` and
    /// `infer`; for a tenant `links` and `order` for its own entity sources, and `infer`
    /// (`false` declines inferred links).
    #[arg(long)]
    pub file: PathBuf,
    /// The reason, kept with the record.
    #[arg(long, default_value = "")]
    pub reason: String,
}

/// `reconcile unset`.
#[derive(Debug, Args)]
pub struct Unset {
    /// The database and the scope.
    #[command(flatten)]
    pub at: At,
    /// The reason, kept with the record.
    #[arg(long, default_value = "")]
    pub reason: String,
}

/// Runs a `reconcile` command.
pub async fn run(command: ReconcileCommand) -> Result<()> {
    match command {
        ReconcileCommand::Show(at) => {
            let client = connect(&at.database).await?;
            let tenant = at.tenant.as_deref().unwrap_or_default();
            let row = client
                .query_opt(
                    "SELECT config FROM view.registry
                      WHERE tenant = $1 AND kind = 'reconcile' AND code = 'default'",
                    &[&tenant],
                )
                .await?;
            match row {
                Some(r) => println!(
                    "{}",
                    serde_json::to_string_pretty(&r.get::<_, serde_json::Value>(0))?
                ),
                None => println!("no `reconcile:default` record: the defaults apply"),
            }
        }
        ReconcileCommand::Set(set) => {
            let text = std::fs::read_to_string(&set.file)
                .with_context(|| format!("reading {}", set.file.display()))?;
            let record: serde_json::Value = serde_json::from_str(&text)
                .with_context(|| format!("{} is not JSON", set.file.display()))?;
            write_record(&set.at, Some(record), &set.reason).await?;
            println!("`reconcile:default` written for {}", scope(&set.at));
        }
        ReconcileCommand::Unset(unset) => {
            write_record(&unset.at, None, &unset.reason).await?;
            println!("`reconcile:default` retired for {}", scope(&unset.at));
        }
    }
    Ok(())
}

fn scope(at: &At) -> String {
    at.tenant
        .as_ref()
        .map_or_else(|| "the instance".to_string(), |t| format!("`{t}`"))
}

/// Writes or retires the record as an instance act, in one unit of work.
async fn write_record(at: &At, record: Option<serde_json::Value>, reason: &str) -> Result<()> {
    let client = connect(&at.database).await?;
    let farm = farm_of(&client).await?;
    let (store, pipeline) = store(&at.database, &farm)?;
    let mut cx = store.begin().await?;
    let out = write(
        &store,
        &pipeline,
        &mut cx,
        at.tenant.as_deref().unwrap_or_default(),
        &farm.operator().to_string(),
        &operator([]),
        record,
        reason,
        now(),
    )
    .await;
    match out {
        Ok(_) => {
            store.commit(cx).await?;
            Ok(())
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(e.into())
        }
    }
}
