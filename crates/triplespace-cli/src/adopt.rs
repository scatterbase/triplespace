//! `triplespace adopt` (0035): a frozen Wikibase's MediaWiki XML dump becomes the tenant's
//! `local` graph. Two passes over the dump: a survey for the floors and the accounts, then
//! the adoption job itself.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_actors::actor::ActorRecord;
use scatter_actors::key::ActorKey;
use scatter_adapter_wikidata::adoption::{Survey, adopt_page};
use scatter_adapter_wikidata::xml_dump::XmlDump;
use scatter_ingest::adopt::{Adopted, Adoption, Floors, run_adoption};
use scatter_ingest::write::Attestation;
use scatter_providers::Registry;
use triplespace_projections::Farm;
use triplespace_projections::actors::payload;

use crate::common::{connect, draft, now, store};

/// `adopt`.
#[derive(Debug, Args)]
pub struct Adopt {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// The MediaWiki XML dump of the frozen source (`.xml`, `.xml.gz`, `.xml.bz2`).
    #[arg(long)]
    pub dump: PathBuf,
    /// The source's base URL, `https://librarybase.org/`.
    #[arg(long)]
    pub source: String,
    /// The dump's identity, the source version (`librarybase-20260928.xml.gz`).
    #[arg(long)]
    pub version: Option<String>,
    /// The operator declares the source frozen (required).
    #[arg(long)]
    pub frozen: bool,
    /// The source's `wb_id_counters`, `item=350000,property=1200`; the dump's highest
    /// IDs otherwise, with a warning (0035 §4).
    #[arg(long)]
    pub counters: Option<String>,
    /// The highest log ID the source consumed (not in the dump).
    #[arg(long, default_value_t = 0)]
    pub log_floor: u64,
    /// Entity-source prefixes the source used, `wikidata=WD` (wikibase-compat §5.1).
    #[arg(long = "entity-source")]
    pub entity_sources: Vec<String>,
    /// The subsidiary running the job, `librarybase:2`; the owner's own key by default.
    #[arg(long)]
    pub subsidiary: Option<String>,
    /// Entities per unit of work.
    #[arg(long, default_value_t = 200)]
    pub batch: usize,
    /// Skip the source's accounts.
    #[arg(long)]
    pub no_accounts: bool,
    /// The farm slug (the tenant's by default) and base, for the pipeline.
    #[arg(long)]
    pub farm_slug: Option<String>,
    /// The farm base.
    #[arg(long)]
    pub farm_base: Option<String>,
}

fn parse_pairs(text: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part
            .split_once('=')
            .with_context(|| format!("`{part}` is not `name=value`"))?;
        out.insert(k.trim().to_string(), v.trim().to_string());
    }
    Ok(out)
}

/// Runs the adoption.
#[allow(clippy::too_many_lines)]
pub async fn run(args: Adopt) -> Result<()> {
    if !args.frozen {
        bail!(
            "adoption is a one-way door: declare the source frozen with --frozen once its final dump is taken (0035 §2)"
        );
    }
    let registry = Registry::default_registry();
    let sources: BTreeMap<String, String> = args
        .entity_sources
        .iter()
        .map(|s| parse_pairs(s))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    // Pass 1: the survey.
    eprintln!("surveying {}", args.dump.display());
    let mut survey = Survey::default();
    for page in XmlDump::open(&args.dump).context("open dump")? {
        let page = page.context("read dump")?;
        survey.observe(&page);
        if survey.pages % 50_000 == 0 {
            eprintln!("  {} pages, {} entities", survey.pages, survey.entities);
        }
    }
    eprintln!(
        "  {} pages, {} entities, {} redirects, {} accounts; highest page {}, revision {}",
        survey.pages,
        survey.entities,
        survey.redirects,
        survey.accounts.len(),
        survey.max_page_id,
        survey.max_revid
    );
    let mut floors = Floors {
        entity: survey.max_entity.clone(),
        page: survey.max_page_id,
        revision: survey.max_revid,
        log: args.log_floor,
        user: survey.max_user_id(),
    };
    match &args.counters {
        Some(text) => {
            for (t, n) in parse_pairs(text)? {
                let n: u64 = n.parse().with_context(|| format!("counter `{t}`"))?;
                floors.entity.insert(t, n);
            }
        }
        None => eprintln!(
            "warning: no --counters; entity floors come from the dump's highest IDs, so numbers the source allocated and deleted may be reused (0035 §4)"
        ),
    }

    // The tenant's base, for the pipeline's IRIs.
    let client = connect(&args.database).await?;
    let base: String = client
        .query_opt(
            "SELECT config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&args.tenant],
        )
        .await?
        .and_then(|r| r.get::<_, Option<String>>(0))
        .with_context(|| format!("tenant `{}` is not registered; run `instance create` first", args.tenant))?;
    let farm = Farm {
        slug: args
            .farm_slug
            .clone()
            .unwrap_or_else(|| args.tenant.clone()),
        base: args.farm_base.clone().unwrap_or(base),
    };
    let owner: Option<String> = client
        .query_opt(
            "SELECT m.actor_key FROM view.membership m WHERE m.tenant = $1 AND m.\"group\" = 'owner' ORDER BY m.actor_key LIMIT 1",
            &[&args.tenant],
        )
        .await?
        .map(|r| r.get(0));
    let actor = match (&args.subsidiary, owner) {
        (Some(s), _) => ActorKey::parse(s)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .to_string(),
        (None, Some(o)) => o,
        (None, None) => bail!("no owner found for `{}`; pass --subsidiary", args.tenant),
    };
    let t = now();
    let accounts = if args.no_accounts {
        Vec::new()
    } else {
        survey
            .accounts()
            .iter()
            .map(|a| {
                draft(
                    payload::ACTOR,
                    &ActorKey::numeric(&args.tenant, a.id).to_string(),
                    &serde_json::to_value(ActorRecord::registered(&a.name))?,
                    &actor,
                    Some("adopted account"),
                    t,
                )
            })
            .collect::<Result<Vec<_>>>()?
    };
    let adoption = Adoption {
        source: args.source.clone(),
        version: args.version.clone().unwrap_or_else(|| {
            args.dump
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        }),
        adapter_version: Some(format!(
            "scatter-adapter-wikidata {}",
            env!("CARGO_PKG_VERSION")
        )),
        frozen: true,
        floors,
        accounts,
        property_types: survey.property_types.clone(),
    };
    if survey.property_types.is_empty() {
        eprintln!(
            "warning: the dump defines no properties, so adopted snaks keep no datatype and identifiers are not indexed"
        );
    }

    // Pass 2: the job.
    let (store, pipeline) = store(&args.database, &farm)?;
    let mut read_errors = 0u64;
    let mut seen = 0u64;
    let mut untyped_snaks = 0u64;
    let entities = XmlDump::open(&args.dump)
        .context("open dump")?
        .filter_map(|page| match page {
            Ok(page) => match adopt_page(&page, &sources, registry, &survey.property_types) {
                Ok(Some(a)) => {
                    seen += 1;
                    untyped_snaks += a.untyped_snaks;
                    if seen.is_multiple_of(10_000) {
                        eprintln!("  {seen} entities adopted");
                    }
                    Some(Adopted {
                        entity: a.entity,
                        source_revid: a.source_revid,
                        source_time: a.source_time,
                        source_pageid: a.source_pageid,
                    })
                }
                Ok(None) => None,
                Err(e) => {
                    read_errors += 1;
                    eprintln!("skipping page {} (`{}`): {e}", page.id, page.title);
                    None
                }
            },
            Err(e) => {
                read_errors += 1;
                eprintln!("dump error: {e}");
                None
            }
        });
    let outcome = run_adoption(
        &store,
        &pipeline,
        &args.tenant,
        Attestation::by(&actor),
        adoption,
        entities,
        registry,
        t,
        args.batch.max(1),
    )
    .await
    .context("adoption")?;
    println!("adoption job {} finished", outcome.job_id);
    println!("  adopted    {}", outcome.counts.adopted);
    println!("  unchanged  {}", outcome.counts.unchanged);
    println!("  rejected   {}", outcome.counts.rejected);
    println!("  accounts   {}", outcome.accounts);
    if read_errors > 0 {
        println!("  unreadable {read_errors} (see stderr)");
    }
    if untyped_snaks > 0 {
        println!(
            "  untyped    {untyped_snaks} snaks (their property is not defined in the dump)"
        );
    }
    for r in outcome.rejects.iter().take(20) {
        println!("  reject #{}: {}", r.line, r.reason);
    }
    Ok(())
}
