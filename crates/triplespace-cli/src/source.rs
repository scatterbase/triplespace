//! `source`: a tenant's entity sources (0078 §1–§4). `source add` writes the
//! `entity-source:{name}` record as an instance act, the server assigning the number and
//! opening `source/{name}`; `source list` and `source show` read the tenant's table of
//! names; `source retire` retires the record (the partition and its records stay).

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_projection::Backend as _;
use scatter_providers::{EntitySource, SourceType};
use triplespace_api_ingest::sources::write;
use triplespace_projections::entity_sources::{declared, declared_one};

use crate::common::{connect, farm_of, now, store};
use crate::rights::operator;

/// The `source` commands.
#[derive(Debug, Subcommand)]
pub enum SourceCommand {
    /// Declare (or redeclare) an entity source on a tenant.
    Add(Add),
    /// List a tenant's entity sources.
    List(At),
    /// Show one source's record.
    Show(Which),
    /// Retire a source's record; its partition and records stay.
    Retire(Which),
}

/// The database and the tenant.
#[derive(Debug, Args)]
pub struct At {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
}

/// A source on a tenant.
#[derive(Debug, Args)]
pub struct Which {
    /// The database and the tenant.
    #[command(flatten)]
    pub at: At,
    /// The source's name.
    #[arg(long)]
    pub name: String,
    /// The reason, kept with the record.
    #[arg(long, default_value = "")]
    pub reason: String,
}

/// `source add`.
#[derive(Debug, Args)]
pub struct Add {
    /// The source on the tenant.
    #[command(flatten)]
    pub which: Which,
    /// The name shown on chips and origin lines.
    #[arg(long)]
    pub label: String,
    /// An entity type: `Q=item`, `P=property`; the upstream prefix and the Wikibase type.
    #[arg(long = "type", required = true)]
    pub types: Vec<String>,
    /// The concept IRI template over `{upstream_id}`, for every type.
    #[arg(long)]
    pub iri: String,
    /// The source's Action API endpoint.
    #[arg(long)]
    pub api: Option<String>,
    /// Its `Special:EntityData/` base.
    #[arg(long)]
    pub entity_data: Option<String>,
    /// The source publishes revision IDs.
    #[arg(long)]
    pub revision_ids: bool,
    /// The licence of its data, an SPDX identifier.
    #[arg(long)]
    pub licence: Option<String>,
}

/// Runs a `source` command.
pub async fn run(cmd: SourceCommand) -> Result<()> {
    match cmd {
        SourceCommand::Add(args) => {
            let types = args
                .types
                .iter()
                .map(|t| {
                    let (prefix, entity_type) = t
                        .split_once('=')
                        .with_context(|| format!("`{t}` is not `PREFIX=type` (`Q=item`)"))?;
                    Ok(SourceType {
                        upstream_prefix: prefix.to_string(),
                        entity_type: entity_type.to_string(),
                        id_grammar: "digits".into(),
                        iri: args.iri.clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let source = EntitySource {
                name: args.which.name.clone(),
                label: args.label.clone(),
                types,
                api: args.api.clone(),
                entity_data: args.entity_data.clone(),
                revision_ids: args.revision_ids,
                licence: args.licence.clone(),
                number: None,
                promoted_to: None,
            };
            let written = write_source(&args.which, Some(source)).await?;
            if let Some(s) = written {
                println!(
                    "source `{}` declared on `{}`: number {}, IDs `{}:{}…`, partition source/{}",
                    s.name,
                    args.which.at.tenant,
                    s.number.unwrap_or_default(),
                    s.name,
                    s.types.first().map_or("Q", |t| t.upstream_prefix.as_str()),
                    s.name
                );
            }
        }
        SourceCommand::List(at) => {
            let client = connect(&at.database).await?;
            let sources = declared(&client, &at.tenant)
                .await
                .map_err(anyhow::Error::msg)?;
            if sources.is_empty() {
                println!("`{}` declares no entity sources", at.tenant);
            }
            for s in sources {
                println!(
                    "{}\t{}\tnumber {}\t{}",
                    s.name,
                    s.label,
                    s.number.unwrap_or_default(),
                    s.api.as_deref().unwrap_or("(no api)")
                );
            }
        }
        SourceCommand::Show(which) => {
            let client = connect(&which.at.database).await?;
            match declared_one(&client, &which.at.tenant, &which.name)
                .await
                .map_err(anyhow::Error::msg)?
            {
                Some(s) => println!("{}", serde_json::to_string_pretty(&s)?),
                None => bail!("`{}` declares no source `{}`", which.at.tenant, which.name),
            }
        }
        SourceCommand::Retire(which) => {
            write_source(&which, None).await?;
            println!("source `{}` retired on `{}`", which.name, which.at.tenant);
        }
    }
    Ok(())
}

/// Writes or retires the record as an instance act, in one unit of work.
async fn write_source(which: &Which, source: Option<EntitySource>) -> Result<Option<EntitySource>> {
    let client = connect(&which.at.database).await?;
    let farm = farm_of(&client).await?;
    let base = client
        .query_opt(
            "SELECT config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&which.at.tenant],
        )
        .await?
        .and_then(|r| r.get::<_, Option<String>>(0))
        .with_context(|| format!("no tenant `{}`", which.at.tenant))?;
    let (store, pipeline) = store(&which.at.database, &farm)?;
    let mut cx = store.begin().await?;
    let out = write(
        &store,
        &pipeline,
        &mut cx,
        &which.at.tenant,
        &base,
        &farm.operator().to_string(),
        &operator([]),
        &which.name,
        source,
        &which.reason,
        now(),
    )
    .await;
    match out {
        Ok(s) => {
            store.commit(cx).await?;
            Ok(s)
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(e.into())
        }
    }
}
