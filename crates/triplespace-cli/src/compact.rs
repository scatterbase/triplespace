//! `compact`: compacts a registry provider's mirror by the keys of its records in a range
//! of offsets (0013 §3, A96), the compaction a follower runs as its day's job closes
//! (0092 A7), by hand: for what runs before followers compacted left, or after a load.
//!
//! Postgres keeps the space the removed rows held for the partition's later rows; `VACUUM`
//! (autovacuum, or by hand) is what makes it reusable.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use clap::Args;
use scatter_ingest::compact::{WINDOW, compact, mirror_of};

use crate::common::{connect, farm_of, store};

/// `compact`.
#[derive(Debug, Args)]
pub struct Compact {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The provider slug (`wikidata`, `librarybase`, …).
    #[arg(long)]
    pub provider: String,
    /// The first offset whose record's key is compacted.
    #[arg(long, default_value_t = 0)]
    pub from: u64,
    /// The offset to stop before; else the mirror's size.
    #[arg(long)]
    pub to: Option<u64>,
    /// Offsets per unit of work.
    #[arg(long, default_value_t = WINDOW)]
    pub window: u64,
}

/// Runs `compact`.
pub async fn run(args: Compact) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let (store, _) = store(&args.database, &farm)?;
    let Some((partition, size)) = mirror_of(&store, &args.provider).await? else {
        bail!("`{}` has no mirror here", args.provider);
    };
    let to = args.to.map_or(size, |t| t.min(size));
    let started = Instant::now();
    let mut shown = Instant::now();
    let removed = compact(
        &store,
        partition,
        args.from,
        to,
        args.window,
        |at, removed| {
            if shown.elapsed() >= Duration::from_secs(10) {
                eprintln!("at {at} of {to}: {removed} removed");
                shown = Instant::now();
            }
        },
    )
    .await?;
    println!(
        "{removed} records removed from mirror/{} by the keys at offsets {}..{to}, in {}s",
        args.provider,
        args.from,
        started.elapsed().as_secs()
    );
    Ok(())
}
