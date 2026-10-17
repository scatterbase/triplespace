//! `triplespace status`, `triplespace migrate` and `triplespace rebuild`: the partitions,
//! their heads and every projection's lag, with the migrations the database has not
//! applied; the migrations applied; and a replay of the projections from offset 0 in
//! dependency order (0013 §7, A28).

use anyhow::{Context, Result};
use clap::Args;
use scatter_log_postgres::PgLog;

use crate::common::{connect, farm_of, store};

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

/// A database URL alone.
#[derive(Debug, Args)]
pub struct Database {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// Applies every migration the database has not applied, in build order (0033 §4), and
/// prints each; a database an older binary created gains the tables a newer one reads.
pub async fn migrate(args: Database) -> Result<()> {
    let mut client = connect(&args.database).await?;
    let applied = triplespace_db::migrator()
        .apply(&mut client)
        .await
        .context("migrate")?;
    for (c, name) in &applied.applied {
        println!("applied {c}/{name}");
    }
    println!(
        "{} applied, {} already recorded",
        applied.applied.len(),
        applied.skipped.len()
    );
    if applied
        .applied
        .iter()
        .any(|(_, name)| name.starts_with("0010_"))
    {
        println!(
            "note: view.graph_state and view.entity_revision are filled by writes from now on; \
             `triplespace rebuild` fills them for earlier revisions (0083 A2)"
        );
    }
    Ok(())
}

/// Prints every partition with its head and each projection's lag, and the migrations the
/// database has not applied.
pub async fn status(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    let pending = triplespace_db::migrator()
        .pending(&client)
        .await
        .context("migrations")?;
    if !pending.is_empty() {
        println!(
            "pending migrations (run `triplespace migrate`): {}",
            pending
                .iter()
                .map(|(c, n)| format!("{c}/{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let farm = farm_of(
        &client,
        args.farm_slug.as_deref(),
        args.farm_base.as_deref(),
    )
    .await?;
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

/// Replays every projection from offset 0, in dependency order.
pub async fn rebuild(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(
        &client,
        args.farm_slug.as_deref(),
        args.farm_base.as_deref(),
    )
    .await?;
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
