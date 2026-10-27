//! `triplespace sync`: a provider's dump into its mirror partition (0002 §8.4), through the
//! adapter its registry entry names (0091 §4). Every Wikibase provider names
//! `scatter-adapter-wikibase`, parameterised by the entry; no other adapter is built yet.
//!
//! A first pass over the dump comes when the provider has key-map rules (0091 §2), since
//! a value pointing at `Q9` can only become `domain:example.org` once every item is
//! indexed, or when the dump is XML: its stored JSON has no `datatype` on snaks, so the
//! pass collects the property types from the dump's own property pages (as `adopt` does,
//! 0035 §3), and its redirects, so that a merge into a mapped item becomes an alias of the
//! key (0009 A13). A JSON dump of a provider without key maps is read once.
//!
//! Every reject is printed to stderr (`reject Q48: …`) and, with `--rejects`, written to a
//! tab-separated file: the job record keeps only the first 100 (0010 §9).

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_adapter_wikibase::{
    DumpEntry, DumpPropertyTypes, Entities, Index, KeyMappedAdapter, KeyRules, Outcome,
};
use scatter_ingest::sync::{SyncItem, run_sync};
use scatter_ingest::write::Attestation;
use scatter_providers::{Provider, Registry};
use scatter_wikibase_changeset::{JobHeader, Mode, Upstream, WikibaseAdapter};
use triplespace_api_ingest::follow::{KIND_PROVIDER, record_load};

use crate::common::{connect, farm_of, now, store};

/// `sync`.
#[derive(Debug, Args)]
pub struct Sync {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The provider slug (`internetdomains`, `wikidata`, …).
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
    /// Writes every reject to this file, one `upstream ID<TAB>reason` line each.
    #[arg(long)]
    pub rejects: Option<PathBuf>,
}

/// What a dump yields for the sync.
pub struct Prepared {
    /// The items, in dump order.
    pub items: Vec<SyncItem>,
    /// Entities the dump reader could not parse.
    pub unreadable: u64,
}

/// Reads a Wikibase provider's dump through `scatter-adapter-wikibase` with the provider's
/// key-map rules: two passes when the rules or the format need them, one otherwise
/// (0091 §4).
pub fn prepare_wikibase(provider: &Provider, dump: &std::path::Path) -> Result<Prepared> {
    let rules = KeyRules::from_provider(provider)?;
    let inner = WikibaseAdapter::new(&provider.slug)
        .with_context(|| format!("provider `{}` is not registered", provider.slug))?;
    let xml = Entities::open(dump).context("open dump")?.is_xml();
    let mut index = Index::default();
    let mut types = DumpPropertyTypes::default();
    let mut unreadable = 0u64;
    let two_passes = !rules.is_empty() || xml;
    if two_passes {
        first_pass(dump, &rules, &mut index, &mut types, &mut unreadable)?;
    }
    index.finish();
    if !rules.is_empty() {
        eprintln!(
            "  {} mapped to keys, {} keys in conflict, {} redirects",
            index.mapped(),
            index.conflicts().count(),
            index.redirects()
        );
    }
    let adapter = KeyMappedAdapter::new(inner, rules, index);
    second_pass(dump, &adapter, &mut types, unreadable, two_passes)
}

/// The first pass: the key index, the property types and the redirects.
fn first_pass(
    dump: &std::path::Path,
    rules: &KeyRules,
    index: &mut Index,
    types: &mut DumpPropertyTypes,
    unreadable: &mut u64,
) -> Result<()> {
    let mut seen = 0u64;
    eprintln!("indexing {}", dump.display());
    for entry in Entities::open(dump).context("open dump")?.entries() {
        match entry {
            Ok(DumpEntry::Entity(p)) => {
                seen += 1;
                // A reject here is reported on the second pass.
                let _ = index.observe(&p.entity, rules);
                types.observe(&p.entity);
            }
            Ok(DumpEntry::Redirect(r)) => index.observe_redirect(r.from.as_str(), r.to.as_str()),
            Err(e) => {
                *unreadable += 1;
                eprintln!("dump error: {e}");
            }
        }
    }
    eprintln!("  {seen} entities, {} property types", types.len());
    Ok(())
}

/// The second pass: every entity and redirect through the adapter. Entities the first
/// pass could not read are counted there and skipped here.
fn second_pass(
    dump: &std::path::Path,
    adapter: &KeyMappedAdapter,
    types: &mut DumpPropertyTypes,
    mut unreadable: u64,
    counted: bool,
) -> Result<Prepared> {
    let mut items = Vec::new();
    for entry in Entities::open(dump).context("open dump")?.entries() {
        let mut parsed = match entry {
            Ok(DumpEntry::Entity(p)) => *p,
            Ok(DumpEntry::Redirect(r)) => {
                let upstream = Upstream::revision(r.revid, Some(&r.timestamp));
                match adapter.sync_redirect(r.from.as_str(), r.to.as_str(), upstream) {
                    Ok(Some((from, to, upstream))) => {
                        items.push(SyncItem::Redirected { from, to, upstream });
                    }
                    Ok(None) => {}
                    Err(e) => {
                        unreadable += 1;
                        eprintln!("adapter error: {} → {}: {e}", r.from, r.to);
                    }
                }
                continue;
            }
            Err(e) => {
                // Counted by the first pass when there was one.
                if !counted {
                    unreadable += 1;
                    eprintln!("dump error: {e}");
                }
                continue;
            }
        };
        types.apply(&mut parsed.entity);
        match adapter.sync_item(parsed) {
            Ok(Outcome::State {
                entity,
                upstream,
                upstream_id,
                upstream_aliases,
            }) => items.push(SyncItem::State {
                entity,
                upstream,
                upstream_id,
                upstream_aliases,
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
                    upstream_aliases: Vec::new(),
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

/// Prints every reject to stderr and, with a path, writes them as `upstream ID<TAB>reason`
/// lines: the job record keeps only the first 100.
pub fn report_rejects(items: &[SyncItem], path: Option<&std::path::Path>) -> Result<()> {
    let mut file = path
        .map(|p| {
            std::fs::File::create(p)
                .map(std::io::BufWriter::new)
                .with_context(|| format!("create {}", p.display()))
        })
        .transpose()?;
    for item in items {
        if let SyncItem::Rejected {
            upstream_id,
            reason,
        } = item
        {
            eprintln!("reject {upstream_id}: {reason}");
            if let Some(f) = file.as_mut() {
                let reason = reason.replace(['\t', '\n'], " ");
                writeln!(f, "{upstream_id}\t{reason}")?;
            }
        }
    }
    if let Some(mut f) = file {
        f.flush()?;
    }
    Ok(())
}

/// The newest upstream time among a load's items, epoch seconds.
fn latest_time(times: &[String]) -> Option<u64> {
    times
        .iter()
        .filter_map(|t| scatter_adapter_mediawiki::recentchanges::parse_iso(t))
        .max()
}

/// Runs the sync.
pub async fn run(args: Sync) -> Result<()> {
    let provider = Registry::default_registry()
        .by_slug(&args.provider)
        .with_context(|| format!("provider `{}` is not registered", args.provider))?;
    let prepared = match provider.adapter.as_deref() {
        Some(scatter_adapter_wikibase::NAME) => prepare_wikibase(provider, &args.dump)?,
        Some(other) => bail!(
            "provider `{}` is read by `{other}`, which this build does not include",
            args.provider
        ),
        None => bail!("provider `{}` names no adapter", args.provider),
    };
    report_rejects(&prepared.items, args.rejects.as_deref())?;
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
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
        "{} {}",
        scatter_adapter_wikibase::NAME,
        scatter_adapter_wikibase::VERSION
    ));
    let items_times: Vec<String> = prepared
        .items
        .iter()
        .filter_map(|i| match i {
            SyncItem::State { upstream, .. }
            | SyncItem::Deleted { upstream, .. }
            | SyncItem::Redirected { upstream, .. } => upstream.time.clone(),
            SyncItem::Rejected { .. } => None,
        })
        .collect();
    let outcome = run_sync(
        &store,
        &pipeline,
        header,
        Attestation::by(&farm.operator().to_string()),
        prepared.items,
        args.threshold,
        Registry::default_registry(),
        now(),
        args.batch.max(1),
    )
    .await
    .context("sync")?;
    // Where the load ended (0092 §5), for a follower to continue from.
    if let Some(latest) = latest_time(&items_times) {
        record_load(&client, "", KIND_PROVIDER, &args.provider, latest)
            .await
            .context("record where the load ended")?;
        println!(
            "  loaded to  {}",
            scatter_adapter_mediawiki::recentchanges::iso(latest)
        );
    }
    println!("sync job {} finished", outcome.job_id);
    println!("  created    {}", outcome.counts.created);
    println!("  updated    {}", outcome.counts.merged);
    println!("  unchanged  {}", outcome.counts.unchanged);
    println!("  rejected   {}", outcome.counts.rejected);
    println!("  tombstoned {}", outcome.counts.tombstoned);
    if outcome.counts.cleared > 0 {
        println!("  cleared    {}", outcome.counts.cleared);
    }
    if outcome.counts.other > 0 {
        println!("  redirected {}", outcome.counts.other);
    }
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

    fn prepare_internetdomains(dump: &std::path::Path) -> Result<Prepared> {
        prepare_wikibase(
            Registry::default_registry()
                .by_slug("internetdomains")
                .unwrap(),
            dump,
        )
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../triplespace-server/tests/fixtures")
            .join(name)
    }

    fn snaks_of(
        entity: &scatter_wikibase_model::entity::Entity,
    ) -> Vec<&scatter_wikibase_model::statement::Snak> {
        entity
            .statements
            .values()
            .flatten()
            .flat_map(|s| {
                std::iter::once(&s.mainsnak)
                    .chain(s.qualifiers.values().flatten())
                    .chain(s.references.iter().flat_map(|r| r.snaks.values().flatten()))
            })
            .collect()
    }

    /// Any Wikibase provider syncs through the one adapter (0091 §1, §4): Librarybase's
    /// XML sample, with no key map, is mirrored under `LB` with every snak typed from the
    /// dump's own properties, and nothing is rejected.
    #[test]
    fn a_wikibase_without_a_key_map_mirrors_under_its_prefix() {
        let provider = Registry::default_registry().by_slug("librarybase").unwrap();
        let prepared = prepare_wikibase(provider, &fixture("librarybase-sample.xml.gz")).unwrap();
        assert_eq!(prepared.unreadable, 0);
        assert!(prepared.items.len() > 100, "{}", prepared.items.len());
        for item in &prepared.items {
            let SyncItem::State {
                entity,
                upstream,
                upstream_id,
                ..
            } = item
            else {
                panic!("only states: {item:?}");
            };
            assert!(entity.id.as_str().starts_with("LB"), "{}", entity.id);
            assert!(upstream.revid.is_some());
            assert!(upstream_id.is_none());
            for snak in snaks_of(entity) {
                assert!(snak.property.as_str().starts_with("LBP"));
                assert!(
                    snak.datatype.is_some(),
                    "{}: untyped {}",
                    entity.id,
                    snak.property
                );
            }
        }
    }

    /// A JSON dump of a provider without key maps is read in one pass (0091 §4): its snaks
    /// carry their `datatype`, and there is no index to build.
    #[test]
    fn a_json_dump_without_a_key_map_is_read_once() {
        let provider = Registry::default_registry().by_slug("librarybase").unwrap();
        let prepared = prepare_wikibase(provider, &fixture("internetdomains.json")).unwrap();
        assert_eq!(prepared.unreadable, 0);
        let ids: Vec<String> = prepared
            .items
            .iter()
            .filter_map(|i| match i {
                SyncItem::State { entity, .. } => Some(entity.id.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 4);
        assert!(
            ids.iter()
                .all(|id| id.starts_with("LBQ") || id.starts_with("LBP")),
            "{ids:?}"
        );
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

    /// The XML fixture with three more pages: an unmapped registrar (Q20), Q12 merged into
    /// wikipedia.org (Q11) and Q21 merged into the registrar, and an item whose identity
    /// value is not a domain (Q22).
    fn merged_dump() -> std::path::PathBuf {
        let base = std::fs::read_to_string(fixture("internetdomains.xml")).unwrap();
        let page = |id: u64, title: &str, model: &str, text: &str| {
            format!(
                "  <page>\n    <title>{title}</title>\n    <ns>120</ns>\n    <id>{id}</id>\n    <revision>\n      <id>{}</id>\n      <timestamp>2026-10-02T00:00:00Z</timestamp>\n      <contributor><username>Fixture</username><id>1</id></contributor>\n      <model>{model}</model>\n      <format>application/json</format>\n      <text xml:space=\"preserve\">{text}</text>\n    </revision>\n  </page>\n",
                100 + id
            )
        };
        let extra = [
            page(20, "Item:Q20", "wikibase-item", r#"{"type":"item","id":"Q20","labels":{"en":{"language":"en","value":"Registrar"}},"descriptions":[],"aliases":[],"claims":[],"sitelinks":[]}"#),
            page(12, "Item:Q12", "wikibase-item", r#"{"entity":"Q12","redirect":"Q11"}"#),
            page(21, "Item:Q21", "wikibase-item", r#"{"entity":"Q21","redirect":"Q20"}"#),
            page(22, "Item:Q22", "wikibase-item", r#"{"type":"item","id":"Q22","labels":[],"descriptions":[],"aliases":[],"claims":{"P1":[{"mainsnak":{"snaktype":"value","property":"P1","datavalue":{"value":"news.example/us","type":"string"}},"type":"statement","rank":"normal","id":"Q22$00000000-0000-0000-0000-0000000000C1"}]},"sitelinks":[]}"#),
        ]
        .concat();
        let xml = base.replace("</mediawiki>", &format!("{extra}</mediawiki>"));
        let path = std::env::temp_dir().join(format!("ts-merged-{}.xml", std::process::id()));
        std::fs::write(&path, xml).unwrap();
        path
    }

    #[test]
    fn redirects_become_aliases_or_mirror_redirects_and_rejects_are_all_reported() {
        let dump = merged_dump();
        let prepared = prepare_internetdomains(&dump).unwrap();
        std::fs::remove_file(&dump).unwrap();
        let wikipedia = prepared
            .items
            .iter()
            .find_map(|i| match i {
                SyncItem::State {
                    upstream_id: Some(q),
                    upstream_aliases,
                    ..
                } if q == "Q11" => Some(upstream_aliases),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            wikipedia,
            &["Q12".to_string()],
            "a merge into a mapped item"
        );
        let redirects: Vec<(String, String)> = prepared
            .items
            .iter()
            .filter_map(|i| match i {
                SyncItem::Redirected { from, to, .. } => Some((from.to_string(), to.to_string())),
                _ => None,
            })
            .collect();
        assert_eq!(redirects, [("XDQ21".to_string(), "XDQ20".to_string())]);

        let file = std::env::temp_dir().join(format!("ts-rejects-{}.tsv", std::process::id()));
        report_rejects(&prepared.items, Some(&file)).unwrap();
        let tsv = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        let lines: Vec<&str> = tsv.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("Q22\t`news.example/us` is not a valid domain key"),
            "{tsv}"
        );
    }
}
