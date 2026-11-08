//! The log's own copies (0006 §6, §9; 01 §4.2, §8; 0103 §1): `triplespace checkpoint` signs
//! every partition's head, `triplespace export` writes a bundle, and `triplespace verify`
//! checks one.
//!
//! - **`checkpoint`** signs the current head of every `logged` partition that has grown
//!   since its latest checkpoint, and the manifest of every full segment of a `hashed`
//!   partition that has none, with the instance key, each partition under the advisory
//!   lock the signing worker is to take (01 §4.2). Until the worker signs on a cadence
//!   (0006 Q4), an operator runs it by hand or by cron, and before an export (0006 A28).
//! - **`export --scope backup`** writes every partition, or one tenant's, verbatim from one
//!   `REPEATABLE READ` snapshot, with their checkpoints and manifests and the instance's
//!   `key:` records: the operator's own copy (0103 §1).
//! - **`export --scope export --tenant`** writes the visible form of one tenant's `public`
//!   partitions (0103 §1): a record about a target a confidential restriction hides is
//!   written as its leaf alone; every part of a record whose target an anonymous reader
//!   may not read (deleted, or under a moderation restriction), and of a suppression
//!   event, is withheld; and the parts a record's visibility hides, content, comment or
//!   the actor, are withheld, each listed in the bundle (0006 A30, A31). A tenant an
//!   anonymous reader may not read has no export. The transfer scope is not built yet.
//! - **`verify`** checks a bundle at level 1 (structure: the tree, checkpoints, consistency,
//!   manifests, the key chain) or level 2 (also bodies against their commitments,
//!   erasures and client signatures), and exits non-zero on any problem. Level 3, which
//!   rebuilds the projections, needs a database and is not built.
//!
//! **Origins.** A partition's origin line is fixed when its first checkpoint is signed and
//! reused for every later one, as an `alias` must not change it (01 §4.2): the first is
//! derived from the tenant's current host, or the farm's for an instance partition
//! (0006 A29).

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use scatter_actors::{GrantRegistry, Principal, evaluate};
use scatter_integrity::bundle::{Bundle, BundlePartition, export_scoped};
use scatter_integrity::checkpoints::{CheckpointStore, checkpoint as sign_head, segment_manifest};
use scatter_integrity::keys::KeyChain;
use scatter_integrity::verify::Level;
use scatter_log::store::LogStore;
use scatter_log_postgres::checkpoints::PgCheckpoints;
use scatter_log_postgres::log::PgLog;
use tokio_postgres::IsolationLevel;
use triplespace_rdf::bundle::{Reader, host_of, key_records, origin_of, partition_rows};

use crate::common::{connect, farm_of, now, signing_key};

/// `triplespace checkpoint`'s arguments.
#[derive(Debug, clap::Args)]
pub struct Checkpoint {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The instance signing key, which must be the instance's current key.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: PathBuf,
    /// Sign only this tenant's partitions; every partition by default.
    #[arg(long)]
    pub tenant: Option<String>,
}

/// What a checkpoint run signed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Signed {
    /// Checkpoints signed.
    pub checkpoints: usize,
    /// Segment manifests signed.
    pub manifests: usize,
}

/// Signs every partition's new head and every unsigned full segment.
///
/// # Errors
///
/// A missing key file, a key that is not the instance's current key, or a database failure.
pub async fn checkpoint(args: &Checkpoint) -> Result<Signed> {
    if !args.key_file.exists() {
        bail!("no key file at {}", args.key_file.display());
    }
    let key = signing_key(&args.key_file)?;
    let mut client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let keys = {
        let tx = client.transaction().await?;
        let keys = key_records(&PgLog::new(&tx))
            .await
            .map_err(anyhow::Error::msg)?;
        tx.commit().await?;
        keys
    };
    let chain = KeyChain::from_records(&keys).context("the instance key chain")?;
    let current = chain
        .current()
        .and_then(scatter_integrity::keys::ChainKey::public_key);
    if current != Some(key.verifying_key().to_bytes()) {
        bail!(
            "{} is not the instance's current key",
            args.key_file.display()
        );
    }
    let farm_host = host_of(&farm.base);
    let mut signed = Signed::default();
    for row in partition_rows(&client, args.tenant.as_deref())
        .await
        .map_err(anyhow::Error::msg)?
    {
        let tx = client.transaction().await?;
        // The signing worker's lock (01 §4.2), so that two signers never race.
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&format!("checkpoint:{}", row.partition)],
        )
        .await?;
        let store = PgLog::new(&tx);
        let mut checkpoints = PgCheckpoints::new(&tx);
        let head = store.head(row.partition).await?;
        if head.size == 0 {
            continue;
        }
        let origin = origin_of(&tx, &checkpoints, &row, &farm_host)
            .await
            .map_err(anyhow::Error::msg)?;
        if row.integrity == "hashed" {
            let have: std::collections::BTreeSet<u64> = checkpoints
                .manifests(row.partition)
                .await?
                .into_iter()
                .map(|(n, _)| n)
                .collect();
            let full = head.size / head.segments.len();
            for n in (0..full).filter(|n| !have.contains(n)) {
                let manifest = segment_manifest(&store, row.partition, n, &origin, &key).await?;
                checkpoints
                    .put_manifest(row.partition, n, &manifest)
                    .await?;
                signed.manifests += 1;
            }
        } else {
            let latest = checkpoints.latest(row.partition).await?;
            if latest.is_none_or(|c| c.checkpoint.size < head.size) {
                let cp = sign_head(&store, row.partition, &origin, &key).await?;
                checkpoints.put(row.partition, &cp).await?;
                signed.checkpoints += 1;
            }
        }
        tx.commit().await?;
    }
    Ok(signed)
}

/// Runs `triplespace checkpoint`.
///
/// # Errors
///
/// As [`checkpoint`].
pub async fn run_checkpoint(args: Checkpoint) -> Result<()> {
    let s = checkpoint(&args).await?;
    println!(
        "signed {} checkpoints and {} segment manifests",
        s.checkpoints, s.manifests
    );
    Ok(())
}

/// A bundle's scope (0103 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Scope {
    /// The operator's own copy: every partition verbatim, `private` ones included.
    Backup,
    /// What an anonymous reader may read; not built yet.
    Export,
    /// One tenant to another operator, sealed; not built yet.
    Transfer,
}

/// `triplespace export`'s arguments.
#[derive(Debug, clap::Args)]
pub struct Export {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The bundle's scope.
    #[arg(long, value_enum)]
    pub scope: Scope,
    /// Bundle only this tenant's partitions; the whole instance by default.
    #[arg(long)]
    pub tenant: Option<String>,
    /// The directory to write, which must not exist or be empty.
    #[arg(long)]
    pub out: PathBuf,
}

/// Writes a bundle.
///
/// # Errors
///
/// A scope not built yet, a database failure, or a directory that is not empty.
pub async fn export(args: &Export) -> Result<Vec<BundlePartition>> {
    let (scope, tenant) = match (args.scope, &args.tenant) {
        (Scope::Backup, t) => ("backup", t.clone()),
        (Scope::Export, Some(t)) => ("export", Some(t.clone())),
        (Scope::Export, None) => bail!("the export scope is one tenant's: name it with --tenant"),
        (Scope::Transfer, _) => bail!("the transfer scope (0103 §3) is not built yet"),
    };
    let mut client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let farm_host = host_of(&farm.base);
    // One instant for every partition.
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let store = PgLog::new(&tx);
    let checkpoints = PgCheckpoints::new(&tx);
    let mut rows = partition_rows(&tx, tenant.as_deref())
        .await
        .map_err(anyhow::Error::msg)?;
    let mut withhold = std::collections::BTreeMap::new();
    if scope == "export" {
        let tenant = tenant.as_deref().unwrap_or_default();
        let reader = anonymous_reader(&tx, tenant).await?;
        // Only what an export may carry: `public` partitions, none of a restricted graph.
        rows.retain(|r| r.export == "public" && !reader.viewer.hidden.graphs.contains(&r.name));
        let ids: Vec<i64> = rows
            .iter()
            .map(|r| scatter_log_postgres::ids::partition_to_db(r.partition))
            .collect();
        withhold = reader
            .decide(&tx, tenant, &ids, None)
            .await
            .map_err(anyhow::Error::msg)?;
    }
    let mut partitions = Vec::new();
    for row in rows {
        partitions.push(BundlePartition {
            partition: row.partition,
            origin: origin_of(&tx, &checkpoints, &row, &farm_host)
                .await
                .map_err(anyhow::Error::msg)?,
            name: row.name,
        });
    }
    if partitions.is_empty() {
        bail!("no partitions to bundle");
    }
    let keys = key_records(&store).await.map_err(anyhow::Error::msg)?;
    export_scoped(
        &store,
        &checkpoints,
        &partitions,
        &keys,
        now(),
        &args.out,
        scope,
        &withhold,
    )
    .await
    .with_context(|| format!("writing {}", args.out.display()))?;
    tx.commit().await?;
    Ok(partitions)
}

/// The anonymous reader of `tenant`, for the export scope.
async fn anonymous_reader(tx: &tokio_postgres::Transaction<'_>, tenant: &str) -> Result<Reader> {
    let groups = triplespace_accounts::groups::registry(tx, tenant)
        .await
        .context("the tenant's groups")?;
    let effective = evaluate(
        &Principal::anonymous(),
        &groups,
        GrantRegistry::default_registry(),
    );
    Reader::new(tx, tenant, effective)
        .await
        .map_err(anyhow::Error::msg)
}

/// Runs `triplespace export`.
///
/// # Errors
///
/// As [`export`].
pub async fn run_export(args: Export) -> Result<()> {
    let partitions = export(&args).await?;
    println!(
        "wrote a {:?} bundle of {} partitions to {}; file bytes are not included (no blob store yet)",
        args.scope,
        partitions.len(),
        args.out.display()
    );
    Ok(())
}

/// How deep `verify` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum VerifyLevel {
    /// Level 1: headers, the tree, checkpoints, consistency, manifests and the key chain.
    Structure,
    /// Level 2: also bodies, erasures and client signatures.
    Bodies,
}

/// `triplespace verify`'s arguments.
#[derive(Debug, clap::Args)]
pub struct Verify {
    /// The bundle's directory.
    pub bundle: PathBuf,
    /// How deep to go.
    #[arg(long, value_enum, default_value = "bodies")]
    pub level: VerifyLevel,
}

/// Verifies a bundle, one report per partition.
///
/// # Errors
///
/// A bundle that cannot be opened: a missing manifest or a key chain that does not verify.
pub async fn verify(
    args: &Verify,
) -> Result<Vec<(BundlePartition, scatter_integrity::verify::Report)>> {
    let bundle =
        Bundle::open(&args.bundle).with_context(|| format!("opening {}", args.bundle.display()))?;
    let level = match args.level {
        VerifyLevel::Structure => Level::Structure,
        VerifyLevel::Bodies => Level::Bodies,
    };
    let reports = bundle.verify(level, None, None).await;
    Ok(bundle
        .manifest()
        .partitions
        .iter()
        .cloned()
        .zip(reports)
        .collect())
}

/// Runs `triplespace verify`, failing when any partition has a problem.
///
/// # Errors
///
/// As [`verify`], or any problem found.
pub async fn run_verify(args: Verify) -> Result<()> {
    let results = verify(&args).await?;
    let mut problems = 0;
    for (p, r) in &results {
        println!(
            "{:<28} size {:>9}  records {:>9}  checkpoints {:>4}  manifests {:>4}  erased {:>5}  {}",
            p.origin,
            r.size,
            r.records,
            r.checkpoints,
            r.manifests,
            r.erased,
            if r.is_clean() { "ok" } else { "PROBLEMS" }
        );
        for problem in &r.problems {
            println!("    {problem:?}");
        }
        problems += r.problems.len();
    }
    if problems > 0 {
        bail!("{problems} problems in {} partitions", results.len());
    }
    println!("verified {} partitions", results.len());
    Ok(())
}
