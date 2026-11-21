//! `triplespace fetch`: queues entities to be read from a provider or a tenant's entity
//! source and written to its mirror (0070 §3; 03 §4.12), with the reason `manual`. The
//! follower of the provider or source fetches them with its next poll (0070 A9); one with
//! no position and nothing loaded starts from now when entities wait.
//!
//! An ID is given as the upstream wiki knows it (`Q42`) or in its stored form (`WDQ42`,
//! `lb:Q42`), which is turned back into the upstream's.

use anyhow::{Result, bail};
use clap::Args;
use scatter_wikibase_model::id::EntityId;
use triplespace_api_ingest::fetch::{Reason, enqueue, queued};

use crate::common::connect;
use crate::follow::followed;

/// `fetch`.
#[derive(Debug, Clone, Args)]
pub struct Fetch {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The provider slug (`wikidata`, …).
    #[arg(long, conflicts_with_all = ["tenant", "source"])]
    pub provider: Option<String>,
    /// The tenant whose entity source to fetch into (0078 §4), with `--source`.
    #[arg(long, requires = "source")]
    pub tenant: Option<String>,
    /// The entity source's name, with `--tenant`.
    #[arg(long, requires = "tenant")]
    pub source: Option<String>,
    /// The entities: `Q42`, `WDQ42` or `lb:Q42`.
    #[arg(required = true)]
    pub ids: Vec<String>,
}

/// Queues the entities.
pub async fn run(args: Fetch) -> Result<()> {
    let client = connect(&args.database).await?;
    let (registry, provider, tenant) = followed(
        &client,
        args.provider.as_deref(),
        args.tenant.as_deref(),
        args.source.as_deref(),
    )
    .await?;
    let mut ids = Vec::new();
    for id in &args.ids {
        let upstream = match registry.parse_foreign_id(id) {
            Ok(f) if f.provider().slug == provider.slug => f.upstream_id(),
            Ok(f) => bail!(
                "`{id}` is an ID of `{}`, not of `{}`",
                f.provider().slug,
                provider.slug
            ),
            Err(e) if id.contains(':') => bail!("`{id}`: {e}"),
            Err(_) => id.clone(),
        };
        if EntityId::parse(&upstream).is_err() {
            bail!("`{id}` is not an entity ID");
        }
        ids.push(upstream);
    }
    let added = enqueue(&client, &tenant, &provider.slug, &ids, Reason::Manual).await?;
    let waiting = queued(&client, &tenant, &provider.slug, u64::MAX).await?;
    println!(
        "queued {added} of {} for `{}` ({waiting} waiting); its follower fetches them",
        ids.len(),
        provider.slug
    );
    Ok(())
}
