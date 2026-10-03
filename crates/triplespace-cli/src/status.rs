//! `triplespace status` and `triplespace rebuild`: the partitions, their heads and every
//! projection's lag; and a replay of the projections from offset 0 in dependency order
//! (0013 §7, A28).

use anyhow::{Context, Result};
use clap::Args;
use scatter_log_postgres::PgLog;
use triplespace_projections::Farm;

use crate::common::{connect, store};

/// `status` / `rebuild`.
#[derive(Debug, Args)]
pub struct Target {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The farm slug and base, for the pipeline; the primary tenant's by default.
    #[arg(long)]
    pub farm_slug: Option<String>,
    /// The farm base.
    #[arg(long)]
    pub farm_base: Option<String>,
}

/// A partition as `log.partition` describes it.
struct Partition {
    id: u64,
    tenant: Option<String>,
    name: String,
    size: u64,
}

async fn partitions(client: &tokio_postgres::Client) -> Result<Vec<Partition>> {
    let rows = client
        .query(
            "SELECT partition, tenant, name, next_offset FROM log.partition ORDER BY tenant NULLS FIRST, name",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| Partition {
            id: scatter_log_postgres::ids::partition_from_db(r.get(0)),
            tenant: r.get(1),
            name: r.get(2),
            size: u64::try_from(r.get::<_, i64>(3)).unwrap_or(0),
        })
        .collect())
}

/// The dependency order of a rebuild: config partitions, the instance log, mirrors, then
/// the rest.
fn ordered(partitions: &[Partition]) -> Vec<u64> {
    let rank = |p: &Partition| match (p.tenant.is_none(), p.name.as_str()) {
        (_, "config") => 0,
        (true, "log") => 1,
        (true, n) if n.starts_with("mirror/") => 2,
        (true, _) => 3,
        (false, "actors") => 4,
        (false, "log") => 5,
        _ => 6,
    };
    let mut sorted: Vec<&Partition> = partitions.iter().collect();
    sorted.sort_by_key(|p| (rank(p), p.id));
    sorted.into_iter().map(|p| p.id).collect()
}

async fn farm_of(client: &tokio_postgres::Client, args: &Target) -> Result<Farm> {
    let row = client
        .query_opt(
            "SELECT code, config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' ORDER BY code LIMIT 1",
            &[],
        )
        .await?;
    let (slug, base): (String, Option<String>) = match row {
        Some(r) => (r.get(0), r.get(1)),
        None => (String::from("scatter"), None),
    };
    Ok(Farm {
        slug: args.farm_slug.clone().unwrap_or(slug),
        base: args
            .farm_base
            .clone()
            .or(base)
            .unwrap_or_else(|| "https://scatter.example".into()),
    })
}

pub async fn status(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client, &args).await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let parts = partitions(&client).await?;
    let log_store = PgLog::new(client);
    println!(
        "{:<18} {:<14} {:<24} {:>10}  lags",
        "partition", "tenant", "graph", "head"
    );
    for p in &parts {
        let lags = pipeline.lags(&store, &log_store, p.id).await?;
        let behind: Vec<String> = lags
            .iter()
            .filter(|l| l.behind() > 0)
            .map(|l| format!("{} -{}", l.projection, l.behind()))
            .collect();
        println!(
            "{:#018x} {:<14} {:<24} {:>10}  {}",
            p.id,
            p.tenant.as_deref().unwrap_or("(instance)"),
            p.name,
            p.size,
            if behind.is_empty() {
                "current".to_string()
            } else {
                behind.join(", ")
            }
        );
    }
    Ok(())
}

pub async fn rebuild(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client, &args).await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let parts = partitions(&client).await?;
    let order = ordered(&parts);
    let log_store = PgLog::new(client);
    let progress = pipeline
        .rebuild(&store, &log_store, &order, 500)
        .await
        .context("rebuild")?;
    for (id, p) in order.iter().zip(progress) {
        println!("{id:#018x}: {} records, {} rows", p.records, p.rows);
    }
    Ok(())
}
