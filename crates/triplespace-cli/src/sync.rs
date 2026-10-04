//! `triplespace sync`: a provider's dump into its mirror partition (0002 §8.4), through the
//! provider's adapter. Only `internetdomains` has one so far (0009 §9): two passes over the
//! dump, the key index and then the entities.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_actors::key::ActorKey;
use scatter_adapter_internetdomains::{DomainKeyMap, Index, InternetDomainsAdapter, Outcome};
use scatter_adapter_wikidata::Entities;
use scatter_ingest::sync::{SyncItem, run_sync};
use scatter_ingest::write::Attestation;
use scatter_providers::Registry;
use scatter_wikibase_changeset::{JobHeader, Mode};

use crate::common::{connect, farm_of, now, store};

/// `sync`.
#[derive(Debug, Args)]
pub struct Sync {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The provider slug (`internetdomains`).
    #[arg(long)]
    pub provider: String,
    /// The dump: a Wikibase JSON dump or a MediaWiki XML dump (`.json`, `.xml`, `.gz`, `.bz2`).
    #[arg(long)]
    pub dump: PathBuf,
    /// The dump's identity, the source version (`20261001`); the file name by default.
    #[arg(long)]
    pub version: Option<String>,
    /// The dump is complete: entities it does not carry are tombstoned afterwards.
    #[arg(long)]
    pub snapshot: bool,
    /// The most entities a snapshot sweep may tombstone before the job fails instead.
    #[arg(long, default_value_t = 1000)]
    pub threshold: usize,
    /// Entities per unit of work.
    #[arg(long, default_value_t = 200)]
    pub batch: usize,
    /// The farm slug and base, for the pipeline and the acting instance key.
    #[arg(long)]
    pub farm_slug: Option<String>,
    /// The farm base.
    #[arg(long)]
    pub farm_base: Option<String>,
}

/// What a dump yields for the sync.
pub struct Prepared {
    /// The items, in dump order.
    pub items: Vec<SyncItem>,
    /// Entities the dump reader could not parse.
    pub unreadable: u64,
}

/// Reads the dump twice through the internetdomains adapter.
pub fn prepare_internetdomains(dump: &std::path::Path) -> Result<Prepared> {
    let registry = Registry::default_registry();
    let provider = registry
        .by_slug(scatter_adapter_internetdomains::SLUG)
        .context("internetdomains is not registered")?;
    let key_map = DomainKeyMap::from_provider(provider)?;
    let mut index = Index::default();
    let mut unreadable = 0u64;
    let mut seen = 0u64;
    eprintln!("indexing {}", dump.display());
    for parsed in Entities::open(dump).context("open dump")? {
        match parsed {
            Ok(p) => {
                seen += 1;
                // A reject here is reported on the second pass.
                let _ = index.observe(&p.entity, &key_map);
            }
            Err(e) => {
                unreadable += 1;
                eprintln!("dump error: {e}");
            }
        }
    }
    index.finish();
    eprintln!(
        "  {seen} entities, {} mapped to domains, {} keys in conflict",
        index.mapped(),
        index.conflicts().count()
    );
    let adapter = InternetDomainsAdapter::new(index)?;
    let mut items = Vec::new();
    for parsed in Entities::open(dump).context("open dump")? {
        let Ok(parsed) = parsed else {
            continue;
        };
        match adapter.sync_item(parsed) {
            Ok(Outcome::State {
                entity,
                upstream,
                upstream_id,
            }) => items.push(SyncItem::State {
                entity,
                upstream,
                upstream_id,
            }),
            Ok(Outcome::Rejected {
                entity,
                upstream,
                upstream_id,
                reason,
            }) => {
                items.push(SyncItem::Rejected {
                    upstream_id,
                    reason,
                });
                items.push(SyncItem::State {
                    entity,
                    upstream,
                    upstream_id: None,
                });
            }
            Err(e) => {
                unreadable += 1;
                eprintln!("adapter error: {e}");
            }
        }
    }
    Ok(Prepared { items, unreadable })
}

/// Runs the sync.
pub async fn run(args: Sync) -> Result<()> {
    if args.provider != scatter_adapter_internetdomains::SLUG {
        bail!(
            "no adapter for provider `{}` yet; `internetdomains` is the one this build syncs",
            args.provider
        );
    }
    let prepared = prepare_internetdomains(&args.dump)?;
    let client = connect(&args.database).await?;
    let farm = farm_of(
        &client,
        args.farm_slug.as_deref(),
        args.farm_base.as_deref(),
    )
    .await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let version = args.version.clone().unwrap_or_else(|| {
        args.dump
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let mode = if args.snapshot {
        Mode::Snapshot
    } else {
        Mode::Upsert
    };
    let mut header = JobHeader::mirror(&args.provider, &version, mode);
    header.adapter_version = Some(format!(
        "scatter-adapter-internetdomains {}",
        env!("CARGO_PKG_VERSION")
    ));
    let outcome = run_sync(
        &store,
        &pipeline,
        header,
        Attestation::by(&ActorKey::instance(&farm.slug).to_string()),
        prepared.items,
        args.threshold,
        Registry::default_registry(),
        now(),
        args.batch.max(1),
    )
    .await
    .context("sync")?;
    println!("sync job {} finished", outcome.job_id);
    println!("  created    {}", outcome.counts.created);
    println!("  updated    {}", outcome.counts.merged);
    println!("  unchanged  {}", outcome.counts.unchanged);
    println!("  rejected   {}", outcome.counts.rejected);
    println!("  tombstoned {}", outcome.counts.tombstoned);
    if outcome.swept > 0 {
        println!("  swept      {}", outcome.swept);
    }
    if prepared.unreadable > 0 {
        println!("  unreadable {} (see stderr)", prepared.unreadable);
    }
    Ok(())
}
