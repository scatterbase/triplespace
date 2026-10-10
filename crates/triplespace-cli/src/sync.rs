//! `triplespace sync`: a provider's dump into its mirror partition (0002 §8.4), through the
//! provider's adapter. Only `internetdomains` has one so far (0009 §9): two passes over the
//! dump, the key index and then the entities. The first pass also collects the dump's
//! property types, so that the snaks of an XML dump, whose stored JSON has no `datatype`,
//! are typed before the adapter sees them (as `adopt` does, 0035 §3).

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_actors::key::ActorKey;
use scatter_adapter_internetdomains::{DomainKeyMap, Index, InternetDomainsAdapter, Outcome};
use scatter_adapter_wikidata::{DumpPropertyTypes, Entities};
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
    let mut types = DumpPropertyTypes::default();
    let mut unreadable = 0u64;
    let mut seen = 0u64;
    eprintln!("indexing {}", dump.display());
    for parsed in Entities::open(dump).context("open dump")? {
        match parsed {
            Ok(p) => {
                seen += 1;
                // A reject here is reported on the second pass.
                let _ = index.observe(&p.entity, &key_map);
                types.observe(&p.entity);
            }
            Err(e) => {
                unreadable += 1;
                eprintln!("dump error: {e}");
            }
        }
    }
    index.finish();
    eprintln!(
        "  {seen} entities, {} mapped to domains, {} keys in conflict, {} property types",
        index.mapped(),
        index.conflicts().count(),
        types.len()
    );
    let adapter = InternetDomainsAdapter::new(index)?;
    let mut items = Vec::new();
    for parsed in Entities::open(dump).context("open dump")? {
        let Ok(mut parsed) = parsed else {
            continue;
        };
        types.apply(&mut parsed.entity);
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
    if let Some(line) = types.untyped_summary() {
        eprintln!("  {line}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_wikibase_model::entity::EntityType;
    use scatter_wikibase_model::id::EntityId;
    use scatter_wikibase_model::statement::SnakKind;
    use scatter_wikibase_model::value::{DataType, DataValue};

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../triplespace-server/tests/fixtures")
            .join(name)
    }

    /// The XML fixture is the JSON one as Wikibase stores it: no `datatype` on any snak.
    /// Typed from the dump's own properties, it reads to the same mirror states.
    #[test]
    fn an_xml_dump_syncs_like_the_json_dump_of_the_same_wiki() {
        let json = prepare_internetdomains(&fixture("internetdomains.json")).unwrap();
        let xml = prepare_internetdomains(&fixture("internetdomains.xml")).unwrap();
        assert_eq!((json.unreadable, xml.unreadable), (0, 0));
        assert_eq!(xml.items.len(), 4);
        assert_eq!(xml.items, json.items);
    }

    /// The link from wikipedia.org to example.org, both mapped, is a Domain value; the
    /// adapter can only see that once the snak is typed `wikibase-item`.
    #[test]
    fn an_xml_link_to_a_mapped_item_becomes_a_domain_value() {
        let xml = prepare_internetdomains(&fixture("internetdomains.xml")).unwrap();
        let wikipedia = xml
            .items
            .iter()
            .find_map(|i| match i {
                SyncItem::State {
                    entity,
                    upstream_id: Some(q),
                    ..
                } if q == "Q11" => Some(entity),
                _ => None,
            })
            .expect("Q11 is mapped");
        assert_eq!(wikipedia.entity_type, EntityType::Domain);
        let p8 = EntityId::parse("XDP8").unwrap();
        let snak = &wikipedia.statements[&p8][0].mainsnak;
        assert_eq!(snak.datatype, Some(DataType::WikibaseDomain));
        let SnakKind::Value(DataValue::EntityId(v)) = &snak.kind else {
            panic!("an entity value");
        };
        assert_eq!(v.id.as_str(), "domain:example.org");
        // Every snak of every state carries a datatype.
        for item in &xml.items {
            if let SyncItem::State { entity, .. } = item {
                for s in entity.statements.values().flatten() {
                    for snak in std::iter::once(&s.mainsnak)
                        .chain(s.qualifiers.values().flatten())
                        .chain(s.references.iter().flat_map(|r| r.snaks.values().flatten()))
                    {
                        assert!(
                            snak.datatype.is_some(),
                            "{}: untyped {}",
                            entity.id,
                            snak.property
                        );
                    }
                }
            }
        }
    }
}
