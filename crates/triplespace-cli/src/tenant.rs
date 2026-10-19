//! Tenants (0018 §1–3; 0046; 0079; 0089 §1): creating one, which `instance create` does
//! for the primary tenant and `tenant create` for every later one, and listing them.
//!
//! **The order of a tenant's first records** is fixed by its issuer code, which is
//! derived from the leaf of its founding record (0079 §1–2) and which every later record
//! that names one of its actors needs:
//!
//! 1. the tenant's `config` partition is created, and its **founding record**, a copy of
//!    the current instance `key:` record, is appended at offset 0 under the founding
//!    attestation, citing the instance's own founding record as authority (0015 A52);
//! 2. the issuer code is derived from that record's leaf;
//! 3. the `tenant:` record is appended to the instance `config`, carrying the slug, the
//!    base and the code. On `instance create` the instance attests it, citing the
//!    instance's founding record (0079 §4); on `tenant create` the operator who acted, an
//!    account of the primary tenant, attests it (0046 §4), since it is the authority
//!    record of the tenant's creation (0040 §4);
//! 4. everything else the creation writes into the tenant (its `graph:` records, the
//!    `owner` group, the owner's account and membership) is a **provision**: an instance
//!    act with the `tenant:` record as authority and `binding: false` (0040 §1, §6);
//! 5. the wiki's name, when given, is the tenant's own `site.name`, attested by the owner,
//!    so that settings resolution takes it as the tenant's value (0085 §2.1).

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use ed25519_dalek::SigningKey;
use scatter_actors::actor::ActorRecord;
use scatter_actors::issuer::{IssuerRegistry, derived_code};
use scatter_actors::key::ActorKey;
use scatter_actors::membership::Membership;
use scatter_integrity::attest::{Authority, founding_attestation, instance_attestation};
use scatter_log::cbor::Value;
use scatter_log::genesis::Genesis;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{
    GraphEntry, GraphRegistry, History, Integrity, KeyEntry, PAYLOAD_CONFIG, Scope,
};
use scatter_log::store::Appended;
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, log, sequences};
use scatter_projection::Backend as _;
use tokio_postgres::Client;
use triplespace_projections::Farm;
use triplespace_projections::actors::payload;
use triplespace_projections::tenants::{self, TenantEntry};

use crate::common::{
    connect, draft, draft_attested, farm_of, now, random_partition, signing_key, store,
};

/// The segment exponent for every partition the CLI makes: 2^16 records a segment.
pub const SEGMENT_K: u8 = 16;

/// The entity types a tenant numbers itself (0015 §2).
const ENTITY_TYPES: [&str; 3] = ["item", "property", "lexeme"];

/// A tenant's source partitions besides `config` (0018 §2).
const PARTITIONS: [&str; 5] = ["local", "pages", "log", "actors", "accounts"];

/// Whether `s` can be a slug: lower-case ASCII letters, digits and hyphens, not empty and
/// not `instance` (0018 §1; 0046 §6).
#[must_use]
pub fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s != sequences::INSTANCE
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Why `slug` cannot name a tenant or the farm: the shared namespace of tenant slugs,
/// provider slugs and registered issuer codes (0015 §5; 0046 §6; 0079 §3).
///
/// One exception, from 0079 §9 and 0080 §3: a wiki the registry already knows, as an
/// issuer and as the provider that issuer serves, may be adopted under its own name.
/// `adopt` is the source the new tenant adopts; when its host is the host of the
/// registered issuer's site, the tenant may take that issuer's code and the slug and code
/// of the providers whose `issuer` it is, as Librarybase's tenant takes `librarybase`.
pub fn check_name(slug: &str, adopt: Option<&str>) -> Result<()> {
    if !is_slug(slug) {
        bail!("`{slug}` is not a slug (lowercase letters, digits, hyphens; not `instance`)");
    }
    if IssuerRegistry::is_reserved(slug) {
        bail!("`{slug}` is reserved");
    }
    let issuers = IssuerRegistry::default_registry();
    let providers = scatter_providers::Registry::default_registry();
    // The registered issuer whose wiki `adopt` is, if any.
    let adopting = adopt.and_then(host_of).and_then(|host| {
        issuers.issuers().iter().find(|i| {
            !i.template
                && [i.users_api.as_deref(), i.iri.as_deref()]
                    .into_iter()
                    .flatten()
                    .any(|u| host_of(u).as_deref() == Some(host.as_str()))
        })
    });
    let adopted = |issuer: &str| adopting.is_some_and(|i| i.code == issuer);
    if issuers.by_code(slug).is_some() && !adopted(slug) {
        bail!("`{slug}` is a registered issuer code");
    }
    if let Some(p) = providers
        .providers()
        .iter()
        .find(|p| p.slug == slug || p.code.eq_ignore_ascii_case(slug))
        && !adopted(&p.issuer)
    {
        bail!(
            "`{slug}` is the registered provider `{}`'s slug or code",
            p.code
        );
    }
    Ok(())
}

/// The host of a base URL, lower-cased, with any port.
fn host_of(base: &str) -> Option<String> {
    url::Url::parse(base).ok().and_then(|u| {
        u.host_str().map(|h| match u.port() {
            Some(p) => format!("{}:{p}", h.to_ascii_lowercase()),
            None => h.to_ascii_lowercase(),
        })
    })
}

/// Checks a base URL: http or https, with a host, and no path but `/` (0018 §1).
pub fn check_base(base: &str) -> Result<String> {
    let url = url::Url::parse(base).with_context(|| format!("`{base}` is not a URL"))?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        bail!("`{base}` is not an http or https base");
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        bail!("`{base}` has a path; a base is a scheme and a host");
    }
    host_of(base).context("base host")
}

/// What a new tenant is.
#[derive(Debug, Clone)]
pub struct Spec {
    /// The slug.
    pub slug: String,
    /// The base URI.
    pub base: String,
    /// The wiki's name, `site.name` (0085 §7).
    pub name: Option<String>,
    /// The source wiki it adopts (0035), its base URL.
    pub adopt: Option<String>,
    /// The owner's user ID: the source wiki's with `adopt`, else 1.
    pub owner: Option<u64>,
    /// The owner's name.
    pub owner_name: String,
}

/// Who attests the `tenant:` record (0046 §4; 0079 §4).
#[derive(Debug, Clone)]
pub enum Attester {
    /// The instance, citing its founding record: `instance create`'s primary tenant.
    Instance(Authority),
    /// An account of the primary tenant: `tenant create`.
    Operator(String),
}

/// A tenant just created.
#[derive(Debug, Clone)]
pub struct Created {
    /// The registry entry.
    pub entry: TenantEntry,
    /// The `config` partition.
    pub config: u64,
    /// The other source partitions by name.
    pub partitions: BTreeMap<&'static str, u64>,
    /// The owner's actor key.
    pub owner: ActorKey,
    /// The owner's user ID.
    pub owner_id: u64,
}

fn info(
    graph: &scatter_log::registry::Graph,
    name: &str,
    tenant: Option<&str>,
    iri_base: &str,
) -> PartitionInfo {
    let iri_base = iri_base.trim_end_matches('/');
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
pub async fn open_partition(
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
        Segments::new(SEGMENT_K).expect("valid exponent"),
        &info(graph, name, tenant, iri_base),
    )
    .await
    .with_context(|| format!("create partition `{name}`"))?;
    Ok((
        partition,
        GraphEntry::source(graph, name.to_string(), partition, SEGMENT_K),
    ))
}

/// Creates a `config` partition for `scope` and appends its founding record (0015 A52),
/// returning what the append returned. The instance's is partition 0 and cites nothing;
/// a tenant's cites the instance's founding record.
#[allow(clippy::too_many_arguments)]
pub async fn found(
    client: &Client,
    partition: u64,
    scope: Scope,
    key: &SigningKey,
    authority: Option<&Authority>,
    info_base: &str,
    tenant: Option<&str>,
    comment: &str,
    t: u64,
) -> Result<(Genesis, Appended)> {
    let registry = GraphRegistry::embedded();
    let (config_graph, _) = registry.lookup("config").context("config graph")?;
    log::create_partition(
        client,
        partition,
        Segments::new(SEGMENT_K).expect("valid exponent"),
        &info(config_graph, "config", tenant, info_base),
    )
    .await
    .context("create config partition")?;
    let mut genesis = Genesis {
        partition,
        scope,
        key: KeyEntry::ed25519(key.verifying_key().to_bytes()),
        segment_exponent: SEGMENT_K,
        appended_at: t,
        founding: Value::Null,
        attestation: Value::Null,
        comment: Some(comment.into()),
    };
    let (content, comment) = genesis.founding_parts();
    genesis.founding = founding_attestation(key, &content, &comment, authority)?;
    let [founding, _] = genesis.drafts(&registry).context("founding record")?;
    let appended = log::append(client, partition, founding).await?;
    Ok((genesis, appended))
}

/// Appends `graph:config` at offset 1 of a partition [`found`] began, attested by `attest`.
pub async fn graph_config(
    client: &Client,
    mut genesis: Genesis,
    attestation: impl FnOnce(&Value, &Value) -> Result<Value>,
) -> Result<Appended> {
    let registry = GraphRegistry::embedded();
    // The content and comment of `graph:config` are fixed by the genesis; sign them.
    let [_, unsigned] = genesis.drafts(&registry).context("graph:config")?;
    let content = cbor_part(&unsigned, 0)?;
    let comment = cbor_part(&unsigned, 1)?;
    genesis.attestation = attestation(&content, &comment)?;
    let [_, graph] = genesis.drafts(&registry).context("graph:config")?;
    Ok(log::append(client, genesis.partition, graph).await?)
}

fn cbor_part(draft: &scatter_log::store::Draft, index: usize) -> Result<Value> {
    let bytes = draft
        .body
        .part(index)
        .and_then(scatter_log::body::Part::bytes)
        .context("part")?;
    scatter_log::cbor::decode(bytes).map_err(|e| anyhow::anyhow!("part: {e}"))
}

/// The authority of a record just appended to `partition`.
#[must_use]
pub fn authority_of(partition: u64, appended: &Appended) -> Authority {
    Authority {
        partition,
        offset: appended.offset,
        leaf: appended.leaf,
    }
}

/// The instance's founding record as an authority: offset 0 of the instance `config`.
pub async fn instance_founding(client: &Client) -> Result<Authority> {
    let row = client
        .query_opt(
            "SELECT leaf FROM log.record WHERE partition = $1 AND \"offset\" = 0",
            &[&i64::try_from(CONFIG_PARTITION)?],
        )
        .await?
        .context("the instance `config` has no founding record")?;
    let leaf: Vec<u8> = row.get(0);
    Ok(Authority {
        partition: CONFIG_PARTITION,
        offset: 0,
        leaf: leaf
            .try_into()
            .map_err(|_| anyhow::anyhow!("leaf is not 32 bytes"))?,
    })
}

/// An attestation maker for a provision: the instance's, citing `authority`, not binding.
pub fn provision<'a>(
    key: &'a SigningKey,
    farm: &'a Farm,
    authority: &'a Authority,
) -> impl Fn(&Value, &Value) -> Result<Value> + 'a {
    move |content, comment| {
        Ok(instance_attestation(
            key,
            &farm.operator().to_string(),
            authority,
            None,
            false,
            content,
            comment,
        )?)
    }
}

/// Checks that `slug` and `base` are free on this instance: no tenant has the slug, the
/// farm's included, and no tenant, the farm base or the auth base has the host.
async fn check_free(
    client: &Client,
    spec: &Spec,
    farm: &Farm,
    auth_base: Option<&str>,
) -> Result<()> {
    check_name(&spec.slug, spec.adopt.as_deref())?;
    let host = check_base(&spec.base)?;
    if spec.slug == farm.slug {
        bail!("`{}` is the farm slug", spec.slug);
    }
    for t in tenants::tenants(client).await? {
        if t.slug == spec.slug {
            bail!("a tenant `{}` already exists", spec.slug);
        }
        if host_of(&t.base).as_deref() == Some(host.as_str()) {
            bail!("`{host}` is already the host of tenant `{}`", t.slug);
        }
    }
    if host_of(&farm.base).as_deref() == Some(host.as_str()) {
        bail!("`{host}` is the farm base's host");
    }
    if let Some(a) = auth_base
        && host_of(a).as_deref() == Some(host.as_str())
    {
        bail!("`{host}` is the auth base's host (0089 §1)");
    }
    Ok(())
}

/// Creates a tenant: its founding record and code, its `tenant:` record, its partitions,
/// its sequences, the `owner` group and the owner's account (0016 §3; 0018 §2; 0079).
/// The caller has checked the slug and base are free, and projects afterwards.
#[allow(clippy::too_many_lines)]
pub async fn create(
    client: &Client,
    key: &SigningKey,
    farm: &Farm,
    spec: &Spec,
    attester: &Attester,
    t: u64,
) -> Result<Created> {
    let registry = GraphRegistry::embedded();

    // 1–2. The founding record, then the issuer code from its leaf.
    let founding_authority = match attester {
        Attester::Instance(a) => *a,
        Attester::Operator(_) => instance_founding(client).await?,
    };
    let config = random_partition()?;
    let (genesis, founded) = found(
        client,
        config,
        Scope::Tenant,
        key,
        Some(&founding_authority),
        &spec.base,
        Some(&spec.slug),
        "tenant create",
        t,
    )
    .await?;
    let issuer = derived_code(&founded.leaf);
    if tenants::by_issuer(client, &issuer).await?.is_some() {
        bail!("issuer code `{issuer}` is already a tenant's");
    }

    // 3. The `tenant:` record.
    let mut entry = serde_json::json!({
        "kind": "tenant", "slug": spec.slug, "base": spec.base, "issuer": issuer,
    });
    if let Some(source) = &spec.adopt {
        entry["adopted_from"] = serde_json::Value::String(source.clone());
    }
    let tenant_key = format!("tenant:{}", spec.slug);
    let d = match attester {
        Attester::Instance(a) => draft_attested(
            PAYLOAD_CONFIG,
            &tenant_key,
            &entry,
            Some("tenant create"),
            t,
            provision(key, farm, a),
        )?,
        Attester::Operator(actor) => draft(
            PAYLOAD_CONFIG,
            &tenant_key,
            &entry,
            actor,
            Some("tenant create"),
            t,
        )?,
    };
    let created = log::append(client, CONFIG_PARTITION, d).await?;
    let authority = authority_of(CONFIG_PARTITION, &created);
    let provide = provision(key, farm, &authority);

    // 4. Provisions: `graph:config`, the partitions, the sequences, `owner`.
    graph_config(client, genesis, &provide).await?;
    let mut partitions: BTreeMap<&'static str, u64> = BTreeMap::new();
    for name in PARTITIONS {
        let (partition, graph) =
            open_partition(client, &registry, name, Some(&spec.slug), &spec.base).await?;
        partitions.insert(name, partition);
        let d = draft_attested(
            PAYLOAD_CONFIG,
            &graph.key(),
            &serde_json::to_value(&graph)?,
            None,
            t,
            &provide,
        )?;
        log::append(client, config, d).await?;
    }
    sequences::create(client, &spec.slug, &ENTITY_TYPES).await?;
    let d = draft_attested(
        PAYLOAD_CONFIG,
        "group:owner",
        &serde_json::json!({"kind": "group", "name": "owner", "permissions": ["*"]}),
        None,
        t,
        &provide,
    )?;
    log::append(client, config, d).await?;

    let owner_id = spec.owner.unwrap_or(1);
    let owner = ActorKey::numeric(&issuer, owner_id);
    let actors = partitions["actors"];
    let mut actor = draft_attested(
        payload::ACTOR,
        &owner.to_string(),
        &serde_json::to_value(ActorRecord::registered(&spec.owner_name))?,
        Some("tenant create"),
        t,
        &provide,
    )?;
    actor.logid = Some(sequences::next(client, &spec.slug, &sequences::Sequence::Log).await?);
    log::append(client, actors, actor).await?;
    let mut membership = draft_attested(
        payload::MEMBERSHIP,
        &owner.to_string(),
        &serde_json::to_value(Membership::add("owner", None))?,
        Some("tenant create"),
        t,
        &provide,
    )?;
    membership.logid = Some(sequences::next(client, &spec.slug, &sequences::Sequence::Log).await?);
    log::append(client, actors, membership).await?;
    // The owner's number is floored here so nothing takes it first; the source's full
    // user-ID floor comes with the adoption job (0035 §4).
    sequences::floor(client, &spec.slug, &sequences::Sequence::User, owner_id).await?;

    // 5. The wiki's name, the tenant's own value.
    if let Some(name) = &spec.name {
        let d = draft(
            PAYLOAD_CONFIG,
            "site:site.name",
            &triplespace_accounts::settings::plan::site_record(
                "site.name",
                &serde_json::Value::String(name.clone()),
            ),
            &owner.to_string(),
            Some("tenant create"),
            t,
        )?;
        log::append(client, config, d).await?;
    }

    Ok(Created {
        entry: TenantEntry {
            slug: spec.slug.clone(),
            base: spec.base.clone(),
            issuer,
            adopted_from: spec.adopt.clone(),
        },
        config,
        partitions,
        owner,
        owner_id,
    })
}

/// Runs `work` inside `BEGIN … COMMIT` on `client`, rolling back if it fails. The log's
/// appends must run inside a transaction (`scatter_log_postgres::log::append`).
pub async fn transaction<T>(
    client: &Client,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    client.batch_execute("BEGIN").await?;
    match work.await {
        Ok(v) => {
            client.batch_execute("COMMIT").await?;
            Ok(v)
        }
        Err(e) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(e)
        }
    }
}

/// Projects a new tenant's partitions, and the instance `config`, in dependency order.
pub async fn project(database: &str, farm: &Farm, client: Client, first: &[u64]) -> Result<()> {
    let (store, pipeline) = store(database, farm)?;
    let log_store = scatter_log_postgres::PgLog::new(client);
    let mut order: Vec<u64> = first.to_vec();
    order.extend(
        log::partitions(log_store.client())
            .await?
            .into_iter()
            .filter(|p| !first.contains(p)),
    );
    for p in &order {
        pipeline
            .catch_up(&store, &log_store, *p, 100)
            .await
            .with_context(|| format!("project partition {p:#x}"))?;
    }
    drop(store);
    Ok(())
}

/// Sets the owner's password from a file or `TRIPLESPACE_OWNER_PASSWORD`, if either is
/// given; returns whether one was set.
pub async fn owner_password(
    database: &str,
    farm: &Farm,
    created: &Created,
    file: Option<&std::path::Path>,
    t: u64,
) -> Result<bool> {
    let password = match file {
        Some(f) => Some(crate::accounts::secret_from(
            Some(f),
            "TRIPLESPACE_OWNER_PASSWORD",
        )?),
        None => std::env::var("TRIPLESPACE_OWNER_PASSWORD").ok(),
    };
    let Some(text) = password else {
        return Ok(false);
    };
    let (store, pipeline) = store(database, farm)?;
    let mut cx = store.begin().await?;
    crate::accounts::set_password(
        &store,
        &pipeline,
        &mut cx,
        &created.entry.slug,
        &created.owner.to_string(),
        created.owner_id,
        &text,
        &farm.operator().to_string(),
        t,
    )
    .await?;
    store.commit(cx).await?;
    Ok(true)
}

/// Prints what was created, for `instance create` and `tenant create`.
pub fn print_created(created: &Created, spec: &Spec, password: bool) {
    let e = &created.entry;
    println!("  tenant        {} ({})", e.slug, e.base);
    println!("  issuer        {}", e.issuer);
    if let Some(name) = &spec.name {
        println!("  name          {name}");
    }
    println!("  owner         {} \"{}\"", created.owner, spec.owner_name);
    println!("  partition     {:<9} {:#018x}", "config", created.config);
    for (name, p) in &created.partitions {
        println!("  partition     {name:<9} {p:#018x}");
    }
    if spec.adopt.is_some() {
        println!(
            "  next          triplespace adopt --tenant {} --dump <xml> --source <base> --version <id>",
            e.slug
        );
    }
    if password {
        println!(
            "  login         action=clientlogin as \"{}\"",
            spec.owner_name
        );
    } else {
        println!(
            "  note          the owner has no password yet: triplespace password set --tenant {} --id {} --password-file <file>",
            e.slug, created.owner_id
        );
    }
}

/// `tenant …`.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // parsed once from the command line
pub enum TenantCommand {
    /// Create a tenant on an existing instance.
    Create(Create),
    /// List the instance's tenants.
    List(List),
}

/// `tenant create`.
#[derive(Debug, Args)]
pub struct Create {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The instance signing key, which must be the instance's current key.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: PathBuf,
    /// The new tenant's slug.
    #[arg(long)]
    pub tenant: String,
    /// Its base URI, `https://example.wiki`.
    #[arg(long)]
    pub base: String,
    /// The wiki's name, the `site.name` setting (0085 §7); the slug by default.
    #[arg(long)]
    pub name: Option<String>,
    /// The tenant adopts this source wiki (0035): its base URL.
    #[arg(long)]
    pub adopt: Option<String>,
    /// The owner's user ID: the source wiki's with `--adopt`, else 1.
    #[arg(long)]
    pub owner: Option<u64>,
    /// The owner's name.
    #[arg(long, default_value = "Owner")]
    pub owner_name: String,
    /// A file holding the owner's password (else `TRIPLESPACE_OWNER_PASSWORD`).
    #[arg(long)]
    pub owner_password_file: Option<PathBuf>,
    /// The user ID, on the primary tenant, of the operator creating the tenant, who must
    /// be a member of `owner` there (0046 §1); the lowest-numbered such member by default.
    #[arg(long)]
    pub operator: Option<u64>,
    /// The auth host, `server.auth_base` (0089 §1): required for a second tenant.
    #[arg(long, env = "TRIPLESPACE_AUTH_BASE")]
    pub auth_base: Option<String>,
}

/// `tenant list`.
#[derive(Debug, Args)]
pub struct List {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// Runs a `tenant` command.
pub async fn run(cmd: TenantCommand) -> Result<()> {
    match cmd {
        TenantCommand::Create(args) => run_create(args).await,
        TenantCommand::List(args) => run_list(args).await,
    }
}

/// The operator for `tenant create`: an `owner` of the primary tenant (0046 §1).
async fn operator(client: &Client, id: Option<u64>) -> Result<String> {
    let primary = tenants::primary(client)
        .await?
        .context("the instance has no primary tenant (no `primary` record)")?;
    let issuer = crate::common::issuer_of(client, &primary).await?;
    let owners: Vec<String> = client
        .query(
            "SELECT actor_key FROM view.membership WHERE tenant = $1 AND \"group\" = 'owner'",
            &[&primary],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let numbered = |k: &String| {
        ActorKey::parse(k)
            .ok()
            .filter(|a| a.issuer() == issuer)
            .and_then(|a| a.numeric_subject())
    };
    match id {
        Some(id) => {
            let key = ActorKey::numeric(&issuer, id).to_string();
            if !owners.contains(&key) {
                bail!("{key} is not a member of `owner` on the primary tenant `{primary}`");
            }
            Ok(key)
        }
        None => owners
            .iter()
            .filter_map(|k| numbered(k).map(|n| (n, k)))
            .min()
            .map(|(_, k)| k.clone())
            .with_context(|| {
                format!("the primary tenant `{primary}` has no `owner`; pass --operator")
            }),
    }
}

async fn run_create(args: Create) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let existing = tenants::tenants(&client).await?;
    // 0089 §1: a second tenant needs the auth host.
    let auth_base = args.auth_base.as_deref().filter(|a| !a.is_empty());
    if !existing.is_empty() && auth_base.is_none() {
        bail!(
            "ts-auth-base-required: an instance with more than one tenant needs an auth host; \
             set server.auth_base (--auth-base or TRIPLESPACE_AUTH_BASE) first (0089 §1)"
        );
    }
    if let Some(a) = auth_base {
        let host = check_base(a).context("--auth-base")?;
        if host_of(&farm.base).as_deref() == Some(host.as_str())
            || existing
                .iter()
                .any(|t| host_of(&t.base).as_deref() == Some(host.as_str()))
        {
            bail!("the auth base `{a}` is the farm base or a tenant's base (0089 §1)");
        }
    }
    if let Some(name) = &args.name {
        triplespace_accounts::settings::plan::check_site_name(name)
            .map_err(|e| anyhow::anyhow!("--name: {e}"))?;
    }
    let spec = Spec {
        slug: args.tenant.clone(),
        base: args.base.clone(),
        name: args.name.clone(),
        adopt: args.adopt.clone(),
        owner: args.owner,
        owner_name: args.owner_name.clone(),
    };
    check_free(&client, &spec, &farm, auth_base).await?;
    let key = signing_key_existing(&args.key_file)?;
    let key_id = scatter_log::registry::key_id(&key.verifying_key().to_bytes());
    let current = client
        .query_opt(
            "SELECT code FROM view.registry WHERE tenant = '' AND kind = 'key' ORDER BY \"offset\" DESC LIMIT 1",
            &[],
        )
        .await?
        .map(|r| r.get::<_, String>(0));
    if current.as_deref() != Some(key_id.as_str()) {
        bail!(
            "{} is not the instance's current key",
            args.key_file.display()
        );
    }
    let operator = operator(&client, args.operator).await?;
    let t = now();
    // One transaction: the instance is live, and its `config` takes appends from the
    // server too; a tenant is created whole or not at all.
    let created = transaction(
        &client,
        create(
            &client,
            &key,
            &farm,
            &spec,
            &Attester::Operator(operator.clone()),
            t,
        ),
    )
    .await?;
    project(
        &args.database,
        &farm,
        client,
        &[CONFIG_PARTITION, created.config],
    )
    .await?;
    let password = owner_password(
        &args.database,
        &farm,
        &created,
        args.owner_password_file.as_deref(),
        t,
    )
    .await?;
    println!("tenant created by {operator}");
    print_created(&created, &spec, password);
    Ok(())
}

/// The signing key, which must already exist: `tenant create` never makes one.
fn signing_key_existing(path: &std::path::Path) -> Result<SigningKey> {
    if !path.exists() {
        bail!("no key file at {}", path.display());
    }
    signing_key(path)
}

async fn run_list(args: List) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let primary = tenants::primary(&client).await?;
    println!("farm  {} ({})  code {}", farm.slug, farm.base, farm.code);
    for t in tenants::tenants(&client).await? {
        let mark = if primary.as_deref() == Some(t.slug.as_str()) {
            "  primary"
        } else {
            ""
        };
        println!("{:<24} {:<26} {}{mark}", t.slug, t.issuer, t.base);
    }
    Ok(())
}
