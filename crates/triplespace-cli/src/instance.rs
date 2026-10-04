//! `triplespace instance create` (0015 §3; 0016 §3; 0018 §1–3; 0035 §5): the schema, the
//! instance key and its genesis, the instance log and the mirrors, one tenant with its
//! config partition, source partitions and sequences, the `owner` group, and the owner
//! account; then the projections caught up.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_actors::actor::ActorRecord;
use scatter_actors::key::ActorKey;
use scatter_actors::membership::Membership;
use scatter_log::cbor;
use scatter_log::genesis::Genesis;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{
    GraphEntry, GraphRegistry, History, Integrity, KeyEntry, PAYLOAD_CONFIG, Scope,
};
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, log, sequences};
use scatter_projection::Backend as _;
use tokio_postgres::Client;
use triplespace_projections::Farm;
use triplespace_projections::actors::payload;

use crate::common::{connect, draft, now, random_partition, signing_key, store};

/// The segment exponent for every partition this command makes: 2^16 records a segment.
const SEGMENT_K: u8 = 16;

/// `instance create`.
#[derive(Debug, Args)]
pub struct Create {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// Where the instance signing key lives (created if missing).
    #[arg(long, default_value = "triplespace.key")]
    pub key_file: PathBuf,
    /// The first tenant's slug.
    #[arg(long)]
    pub tenant: String,
    /// The tenant's base URI, `https://librarybase.org`.
    #[arg(long)]
    pub base: String,
    /// The farm slug; the tenant's by default.
    #[arg(long)]
    pub farm_slug: Option<String>,
    /// The farm base; the tenant's by default.
    #[arg(long)]
    pub farm_base: Option<String>,
    /// Providers to open mirror partitions for, by slug (`internetdomains`).
    #[arg(long = "provider")]
    pub providers: Vec<String>,
    /// The tenant adopts this source wiki (0035): its base URL.
    #[arg(long)]
    pub adopt: Option<String>,
    /// The owner's user ID: the source wiki's with `--adopt`, else 1.
    #[arg(long)]
    pub owner: Option<u64>,
    /// The owner's name.
    #[arg(long, default_value = "Owner")]
    pub owner_name: String,
    /// A file holding the owner's password (else `TRIPLESPACE_OWNER_PASSWORD`); without
    /// either, the owner has no password until `triplespace password set`.
    #[arg(long)]
    pub owner_password_file: Option<PathBuf>,
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s != sequences::INSTANCE
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn info(
    graph: &scatter_log::registry::Graph,
    name: &str,
    tenant: Option<&str>,
    iri_base: &str,
) -> PartitionInfo {
    PartitionInfo {
        tenant: tenant.map(str::to_owned),
        name: name.to_string(),
        graph_iri: match tenant {
            Some(_) => format!("{iri_base}/graph/{name}"),
            None => format!("{iri_base}/instance/graph/{name}"),
        },
        history: graph.history.unwrap_or(History::Full),
        integrity: graph.integrity.unwrap_or(Integrity::Logged),
        export: graph.export,
        hash: scatter_log::registry::HASH_SHA256.into(),
    }
}

/// Makes a source partition for a registry graph and returns its ID with its entry.
async fn open_partition(
    client: &Client,
    registry: &GraphRegistry,
    name: &str,
    tenant: Option<&str>,
    iri_base: &str,
) -> Result<(u64, GraphEntry)> {
    let (graph, _) = registry
        .lookup(name)
        .with_context(|| format!("`{name}` is not a registered graph"))?;
    let partition = random_partition()?;
    log::create_partition(
        client,
        partition,
        Segments::new(SEGMENT_K).unwrap(),
        &info(graph, name, tenant, iri_base),
    )
    .await
    .with_context(|| format!("create partition `{name}`"))?;
    Ok((
        partition,
        GraphEntry::source(graph, name.to_string(), partition, SEGMENT_K),
    ))
}

/// Creates the instance.
#[allow(clippy::too_many_lines)]
pub async fn run(args: Create) -> Result<()> {
    if !is_slug(&args.tenant) {
        bail!(
            "`{}` is not a tenant slug (lowercase letters, digits, hyphens; not `instance`)",
            args.tenant
        );
    }
    let farm_slug = args
        .farm_slug
        .clone()
        .unwrap_or_else(|| args.tenant.clone());
    let farm_base = args.farm_base.clone().unwrap_or_else(|| args.base.clone());
    let farm = Farm {
        slug: farm_slug.clone(),
        base: farm_base.clone(),
    };
    let instance_actor = ActorKey::instance(&farm_slug).to_string();
    let t = now();
    let registry = GraphRegistry::embedded();

    let mut client = connect(&args.database).await?;
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .context("migrate")?;
    let exists: Option<tokio_postgres::Row> = client
        .query_opt("SELECT 1 FROM log.partition WHERE partition = 0", &[])
        .await?;
    if exists.is_some() {
        bail!("this database already holds an instance (partition 0 exists)");
    }
    let key = signing_key(&args.key_file)?;
    let public = key.verifying_key().to_bytes();

    // The instance: genesis at partition 0, the instance log, the mirrors, the sequences.
    let (config_graph, _) = registry.lookup("config").context("config graph")?;
    log::create_partition(
        &client,
        CONFIG_PARTITION,
        Segments::new(SEGMENT_K).unwrap(),
        &info(config_graph, "config", None, &farm_base),
    )
    .await
    .context("create instance config")?;
    let genesis = Genesis {
        partition: CONFIG_PARTITION,
        scope: Scope::Instance,
        key: KeyEntry::ed25519(public),
        segment_exponent: SEGMENT_K,
        appended_at: t,
        attestation: cbor::Value::map(vec![(
            cbor::Value::text("actor"),
            cbor::Value::text(&instance_actor),
        )]),
        comment: Some("instance create".into()),
    };
    for d in genesis.drafts(&registry).context("genesis")? {
        log::append(&client, CONFIG_PARTITION, d).await?;
    }
    let mut instance_entries = Vec::new();
    let (_, entry) = open_partition(&client, &registry, "log", None, &farm_base).await?;
    instance_entries.push(entry);
    for slug in &args.providers {
        let provider = scatter_providers::Registry::default_registry()
            .by_slug(slug)
            .with_context(|| format!("`{slug}` is not a registered provider"))?;
        let (_, entry) = open_partition(
            &client,
            &registry,
            &provider.mirror_graph(),
            None,
            &farm_base,
        )
        .await?;
        instance_entries.push(entry);
    }
    for entry in &instance_entries {
        log::append(
            &client,
            CONFIG_PARTITION,
            draft(
                PAYLOAD_CONFIG,
                &entry.key(),
                &serde_json::to_value(entry)?,
                &instance_actor,
                None,
                t,
            )?,
        )
        .await?;
    }
    let keyed_types: Vec<&str> = vec!["domain", "keyword", "notation"];
    sequences::create_instance(&client, &keyed_types).await?;

    // The tenant: its registry entry on the instance, its own config partition with a
    // genesis under the same key, its source partitions, its sequences.
    let mut tenant_entry =
        serde_json::json!({"kind": "tenant", "slug": args.tenant, "base": args.base});
    if let Some(source) = &args.adopt {
        tenant_entry["adopted_from"] = serde_json::Value::String(source.clone());
    }
    log::append(
        &client,
        CONFIG_PARTITION,
        draft(
            PAYLOAD_CONFIG,
            &format!("tenant:{}", args.tenant),
            &tenant_entry,
            &instance_actor,
            None,
            t,
        )?,
    )
    .await?;
    let tenant_config = random_partition()?;
    log::create_partition(
        &client,
        tenant_config,
        Segments::new(SEGMENT_K).unwrap(),
        &info(config_graph, "config", Some(&args.tenant), &args.base),
    )
    .await
    .context("create tenant config")?;
    let tenant_genesis = Genesis {
        partition: tenant_config,
        scope: Scope::Tenant,
        ..genesis
    };
    for d in tenant_genesis.drafts(&registry).context("tenant genesis")? {
        log::append(&client, tenant_config, d).await?;
    }
    let mut partitions: BTreeMap<&str, u64> = BTreeMap::new();
    for name in ["local", "pages", "log", "actors", "accounts"] {
        let (partition, entry) =
            open_partition(&client, &registry, name, Some(&args.tenant), &args.base).await?;
        partitions.insert(name, partition);
        log::append(
            &client,
            tenant_config,
            draft(
                PAYLOAD_CONFIG,
                &entry.key(),
                &serde_json::to_value(&entry)?,
                &instance_actor,
                None,
                t,
            )?,
        )
        .await?;
    }
    sequences::create(&client, &args.tenant, &["item", "property", "lexeme"]).await?;

    // The owner (0016 §3; 0035 §5): the `owner` group, the account, the membership.
    log::append(
        &client,
        tenant_config,
        draft(
            PAYLOAD_CONFIG,
            "group:owner",
            &serde_json::json!({"kind": "group", "name": "owner", "permissions": ["*"]}),
            &instance_actor,
            None,
            t,
        )?,
    )
    .await?;
    let owner_id = args.owner.unwrap_or(1);
    let owner = ActorKey::numeric(&args.tenant, owner_id);
    let actors = partitions["actors"];
    let logid = sequences::next(&client, &args.tenant, &sequences::Sequence::Log).await?;
    let mut actor = draft(
        payload::ACTOR,
        &owner.to_string(),
        &serde_json::to_value(ActorRecord::registered(&args.owner_name))?,
        &instance_actor,
        Some("instance create"),
        t,
    )?;
    actor.logid = Some(logid);
    log::append(&client, actors, actor).await?;
    let logid = sequences::next(&client, &args.tenant, &sequences::Sequence::Log).await?;
    let mut membership = draft(
        payload::MEMBERSHIP,
        &owner.to_string(),
        &serde_json::to_value(Membership::add("owner", None))?,
        &instance_actor,
        Some("instance create"),
        t,
    )?;
    membership.logid = Some(logid);
    log::append(&client, actors, membership).await?;
    // The owner's number is floored here so nothing takes it first; the source's full
    // user-ID floor comes with the adoption job (0035 §4).
    sequences::floor(&client, &args.tenant, &sequences::Sequence::User, owner_id).await?;

    // Projections, in dependency order.
    let (store, pipeline) = store(&args.database, &farm)?;
    let log_store = scatter_log_postgres::PgLog::new(client);
    let mut order = vec![CONFIG_PARTITION, tenant_config];
    order.extend(
        log::partitions(log_store.client())
            .await?
            .into_iter()
            .filter(|p| *p != CONFIG_PARTITION && *p != tenant_config),
    );
    for p in &order {
        pipeline
            .catch_up(&store, &log_store, *p, 100)
            .await
            .with_context(|| format!("project partition {p:#x}"))?;
    }

    // The owner's password (0016 §3 as amended): the hash in `private`, the binding in
    // `accounts`.
    let password = match &args.owner_password_file {
        Some(f) => Some(crate::accounts::secret_from(
            Some(f),
            "TRIPLESPACE_OWNER_PASSWORD",
        )?),
        None => std::env::var("TRIPLESPACE_OWNER_PASSWORD").ok(),
    };
    if let Some(text) = &password {
        let mut cx = store.begin().await?;
        crate::accounts::set_password(
            &store,
            &pipeline,
            &mut cx,
            &args.tenant,
            &owner.to_string(),
            owner_id,
            text,
            &instance_actor,
            t,
        )
        .await?;
        store.commit(cx).await?;
    }
    println!("instance created");
    println!("  key file      {}", args.key_file.display());
    println!("  farm          {farm_slug} ({farm_base})");
    println!("  tenant        {} ({})", args.tenant, args.base);
    println!("  owner         {owner} \"{}\"", args.owner_name);
    for (name, p) in &partitions {
        println!("  partition     {name:<9} {p:#018x}");
    }
    if !args.providers.is_empty() {
        println!("  mirrors       {}", args.providers.join(", "));
    }
    if args.adopt.is_some() {
        println!(
            "  next          triplespace adopt --tenant {} --dump <xml> --source <base> --version <id>",
            args.tenant
        );
    }
    if password.is_some() {
        println!(
            "  login         action=clientlogin as \"{}\"",
            args.owner_name
        );
    } else {
        println!(
            "  note          the owner has no password yet: triplespace password set --tenant {} --id {owner_id} --password-file <file>",
            args.tenant
        );
    }
    Ok(())
}
