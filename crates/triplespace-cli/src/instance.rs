//! `triplespace instance create` (0015 §3; 0016 §3; 0018 §1–3; 0028 §1; 0035 §5; 0046;
//! 0079): the schema; the instance key and the instance `config`, opened by its founding
//! record, from whose leaf the farm code is derived; the `farm` record; the instance log
//! and the mirrors; the settings registry and the tenancy policy; the primary tenant
//! ([`crate::tenant::create`]) and the `primary` record naming it; then the projections
//! caught up.
//!
//! Every record of the instance `config` after the founding record is attested by the
//! operator, `instance:{farm code}`, citing the founding record as authority (0079 §4),
//! since no account of the primary tenant exists yet to attest them.

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_actors::issuer::derived_code;
use scatter_actors::tenancy::TenancyRegistry;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{GraphRegistry, PAYLOAD_CONFIG, Scope};
use scatter_log_postgres::{log, sequences};
use std::path::PathBuf;
use triplespace_projections::Farm;

use crate::common::{connect, draft_attested, now, signing_key};
use crate::tenant::{
    self, Attester, Spec, authority_of, check_base, check_name, found, graph_config,
    open_partition, provision,
};

/// `instance create`.
#[derive(Debug, Args)]
pub struct Create {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// Where the instance signing key lives (created if missing).
    #[arg(long, default_value = "triplespace.key")]
    pub key_file: PathBuf,
    /// The farm slug, the instance's own name: required, never changed, and different
    /// from every tenant slug (0046 §6).
    #[arg(long)]
    pub farm_slug: String,
    /// The farm base; the primary tenant's by default (0018 §1).
    #[arg(long)]
    pub farm_base: Option<String>,
    /// The tenancy preset (0028 §1): `isolated`, the only one this version can honour.
    #[arg(long, default_value = "isolated")]
    pub tenancy: String,
    /// The primary tenant's slug.
    #[arg(long)]
    pub tenant: String,
    /// The primary tenant's base URI, `https://librarybase.org`.
    #[arg(long)]
    pub base: String,
    /// The wiki's name, the `site.name` setting (0085 §7); the slug by default.
    #[arg(long)]
    pub name: Option<String>,
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

/// The tenancy records a preset writes, `tenancy:{switch}` (0028 §1). Presets that give
/// the farm identity are refused: farm accounts and their partitions (0028 §2) are not
/// built yet, and a policy the instance cannot honour is not written.
fn tenancy_records(preset: &str) -> Result<Vec<(String, String)>> {
    let registry = TenancyRegistry::default_registry();
    let policy = registry
        .policy_from_preset(preset)
        .map_err(|e| anyhow::anyhow!("--tenancy: {e}"))?;
    if policy.has_farm_identity() {
        bail!(
            "--tenancy {preset}: the preset gives the farm identity (0028 §2), which this \
             version does not implement yet; use `isolated`"
        );
    }
    Ok(policy.switches.into_iter().collect())
}

/// Creates the instance.
#[allow(clippy::too_many_lines)]
pub async fn run(args: Create) -> Result<()> {
    check_name(&args.farm_slug, None).context("--farm-slug")?;
    check_name(&args.tenant, args.adopt.as_deref()).context("--tenant")?;
    if args.farm_slug == args.tenant {
        bail!(
            "the farm slug must differ from every tenant slug, the primary tenant's included (0046 §6)"
        );
    }
    check_base(&args.base).context("--base")?;
    let farm_base = args.farm_base.clone().unwrap_or_else(|| args.base.clone());
    check_base(&farm_base).context("--farm-base")?;
    if let Some(name) = &args.name {
        triplespace_accounts::settings::plan::check_site_name(name)
            .map_err(|e| anyhow::anyhow!("--name: {e}"))?;
    }
    let tenancy = tenancy_records(&args.tenancy)?;
    let mut mirrors = Vec::new();
    for slug in &args.providers {
        mirrors.push(
            scatter_providers::Registry::default_registry()
                .by_slug(slug)
                .with_context(|| format!("`{slug}` is not a registered provider"))?
                .mirror_graph(),
        );
    }
    let t = now();

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

    // One transaction: an instance is created whole or not at all.
    let (farm, spec, created) = tenant::transaction(
        &client,
        write(&client, &key, &args, &farm_base, &tenancy, mirrors, t),
    )
    .await?;

    tenant::project(
        &args.database,
        &farm,
        client,
        &[CONFIG_PARTITION, created.config],
    )
    .await?;
    let password = tenant::owner_password(
        &args.database,
        &farm,
        &created,
        args.owner_password_file.as_deref(),
        t,
    )
    .await?;
    println!("instance created");
    println!("  key file      {}", args.key_file.display());
    println!("  farm          {} ({})", farm.slug, farm.base);
    println!("  farm code     {}", farm.code);
    println!("  tenancy       {}", args.tenancy);
    if !args.providers.is_empty() {
        println!("  mirrors       {}", args.providers.join(", "));
    }
    tenant::print_created(&created, &spec, password);
    Ok(())
}

/// Writes the instance's records and its primary tenant's (see the module docs).
#[allow(clippy::too_many_lines)]
async fn write(
    client: &tokio_postgres::Client,
    key: &ed25519_dalek::SigningKey,
    args: &Create,
    farm_base: &str,
    tenancy: &[(String, String)],
    mirrors: Vec<String>,
    t: u64,
) -> Result<(Farm, Spec, tenant::Created)> {
    let registry = GraphRegistry::embedded();
    // The founding record, and the farm code from its leaf (0079 §4).
    let (genesis, founded) = found(
        client,
        CONFIG_PARTITION,
        Scope::Instance,
        key,
        None,
        farm_base,
        None,
        "instance create",
        t,
    )
    .await?;
    let founding = authority_of(CONFIG_PARTITION, &founded);
    let farm = Farm {
        slug: args.farm_slug.clone(),
        code: derived_code(&founded.leaf),
        base: farm_base.to_string(),
    };
    let attest = provision(key, &farm, &founding);
    graph_config(client, genesis, &attest).await?;
    let instance_record = |key: &str, content: &serde_json::Value| {
        draft_attested(PAYLOAD_CONFIG, key, content, None, t, &attest)
    };

    // The farm: its slug, code and base (0046 §6; 0079 §4).
    log::append(
        client,
        CONFIG_PARTITION,
        instance_record(
            "farm",
            &serde_json::json!({"kind": "farm", "slug": farm.slug, "code": farm.code, "base": farm.base}),
        )?,
    )
    .await?;

    // The instance log and the mirrors.
    let mut names = vec!["log".to_string()];
    names.extend(mirrors);
    for name in &names {
        let (_, entry) = open_partition(client, &registry, name, None, farm_base).await?;
        log::append(
            client,
            CONFIG_PARTITION,
            instance_record(&entry.key(), &serde_json::to_value(&entry)?)?,
        )
        .await?;
    }
    // The settings registry, copied into the instance `config` as `setting:` records
    // (0085 §1), so an instance's own registrations sit beside the shipped ones.
    for def in triplespace_accounts::settings::SettingsRegistry::embedded().iter() {
        log::append(
            client,
            CONFIG_PARTITION,
            instance_record(&format!("setting:{}", def.key), &def.to_json())?,
        )
        .await?;
    }
    // The tenancy policy (0028 §1).
    for (switch, value) in tenancy {
        log::append(
            client,
            CONFIG_PARTITION,
            instance_record(
                &format!("tenancy:{switch}"),
                &serde_json::json!({"kind": "tenancy", "value": value}),
            )?,
        )
        .await?;
    }
    sequences::create_instance(client, &["domain", "keyword", "notation"]).await?;

    // The primary tenant, and the `primary` record naming it (0046 §2).
    let spec = Spec {
        slug: args.tenant.clone(),
        base: args.base.clone(),
        name: args.name.clone(),
        adopt: args.adopt.clone(),
        owner: args.owner,
        owner_name: args.owner_name.clone(),
    };
    let created =
        tenant::create(client, key, &farm, &spec, &Attester::Instance(founding), t).await?;
    log::append(
        client,
        CONFIG_PARTITION,
        instance_record(
            "primary",
            &serde_json::json!({"kind": "primary", "slug": args.tenant}),
        )?,
    )
    .await?;

    Ok((farm.clone(), spec, created))
}
