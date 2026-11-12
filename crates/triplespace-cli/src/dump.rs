//! `triplespace dump`: writes a tenant's dumps (02 §4.1; 17 §1.1) into its directory under
//! `dumps.dir`, which the server serves at `{base}/dumps/`. Run by hand or by cron; the
//! server's job runner writes the same dump for `triplespace job start dump` (0104 A6).
//!
//! The dump reads `--replica` where one is given, else the primary, in one exported
//! snapshot, and holds what an anonymous reader may read; a tenant that restricts reading
//! has no dump.

use std::path::PathBuf;

use anyhow::{Context, Result};
use scatter_actors::{GrantRegistry, Principal, evaluate};
use triplespace_projections::entities::EntityProjection;

use crate::common::connect;
use crate::follow::tenant_registry;

/// The dump's arguments.
#[derive(Debug, clap::Args)]
pub struct Dump {
    /// The primary database: the tenant's registry, settings and groups are read here.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// A read replica to take the dump from; without it the primary is read, under the
    /// same snapshot (0032 A12).
    #[arg(long, env = "TRIPLESPACE_REPLICA_URL")]
    pub replica: Option<String>,
    /// The tenant to dump.
    #[arg(long)]
    pub tenant: String,
    /// The dumps directory (`dumps.dir`); the tenant's dumps go into `{dir}/{tenant}/`.
    #[arg(long, env = "TRIPLESPACE_DUMPS_DIR", default_value = "data/dumps")]
    pub dir: PathBuf,
    /// How many dumps to keep, the new one included; by default the tenant's `dumps.keep`.
    #[arg(long)]
    pub keep: Option<usize>,
}

/// Runs the dump.
///
/// # Errors
///
/// An unknown or private tenant, or a database or file failure.
pub async fn run(args: Dump) -> Result<()> {
    let primary = connect(&args.database).await?;
    let base = primary
        .query_opt(
            "SELECT config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&args.tenant],
        )
        .await?
        .and_then(|r| r.get::<_, Option<String>>(0))
        .with_context(|| format!("no tenant `{}`", args.tenant))?;
    let registry = tenant_registry(&primary, &args.tenant).await?;
    let slugs: Vec<&str> = registry
        .providers()
        .iter()
        .map(|p| p.slug.as_str())
        .collect();
    let order = EntityProjection::new(&slugs, registry);
    let groups = triplespace_accounts::groups::registry(&primary, &args.tenant)
        .await
        .context("the tenant's groups")?;
    let reader = evaluate(
        &Principal::anonymous(),
        &groups,
        GrantRegistry::default_registry(),
    );
    let keep = if let Some(k) = args.keep {
        k
    } else {
        let (snapshot, _) = triplespace_accounts::settings::load(&primary, Some(&args.tenant))
            .await
            .context("settings")?;
        snapshot
            .value("dumps.keep")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(7)
    };

    let mut source = match &args.replica {
        Some(url) => connect(url).await.context("the replica")?,
        None => connect(&args.database).await?,
    };
    let dir = args.dir.join(&args.tenant);
    let manifest = triplespace_rdf::dump(
        &mut source,
        &triplespace_rdf::Request {
            tenant: &args.tenant,
            base: &base,
            registry,
            order: &order,
            reader: &reader,
            dir: &dir,
            keep,
            replica: args.replica.is_some(),
        },
    )
    .await?;
    println!(
        "dumped `{}` as of {}: {} entities, {} with a local state, into {}",
        manifest.tenant,
        manifest.taken,
        manifest.entities,
        manifest.local_entities,
        dir.join(&manifest.name).display()
    );
    for f in &manifest.files {
        println!(
            "  {:<24} {:>10} records {:>12} bytes  sha256 {}",
            f.name, f.records, f.bytes, f.sha256
        );
    }
    Ok(())
}
