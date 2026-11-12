//! `triplespace restore` (0103 A2, A9): an instance back from a backup bundle, into an empty
//! database, with no Postgres tools. In order:
//!
//! 1. the bundle is checked: a backup of the whole instance, every partition with its
//!    registry row, and its carried sections; its sealed sections opened with `k` shares,
//!    unless it has none or `--without-private` leaves them;
//! 2. the database is migrated, which makes the schemas, the roles and their grants;
//! 3. each partition is registered as it was and every slot appended verbatim, compacted
//!    and erased ones included, with its checkpoints and segment manifests;
//! 4. the `log` sequences are made at the values the backup holds;
//! 5. every projection is rebuilt from the log, in the replay order of 03 §6.1;
//! 6. the carried `ops` tables and the sealed tables replace what the rebuild wrote there,
//!    since the backup, not the replay, is their record (an inbox's read marks, a queued
//!    job), each checked first against the columns the backup holds;
//! 7. the sequences of `ops` and `private` are set past the rows loaded, and every tenant's
//!    `visibility_epoch` to the restore's time in microseconds (0103 A4);
//! 8. the migrations run once more, giving every partition its grants.
//!
//! A backup written under another schema is refused at step 6; migrating one forward is
//! SCA-358.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use futures_util::SinkExt as _;
use scatter_integrity::backup_key::Share;
use scatter_integrity::bundle::Bundle;
use scatter_integrity::checkpoints::CheckpointStore as _;
use scatter_log::registry::{Export, History, Integrity};
use scatter_log::store::LogStore as _;
use scatter_log_postgres::partition::PartitionInfo;
use scatter_log_postgres::{Frontiers, PgCheckpoints};
use tokio_postgres::Client;

use crate::carried::Carried;
use crate::common::{connect, now};
use crate::sealed::{Column, Sealed, columns, quote};

/// `triplespace restore`'s arguments.
#[derive(Debug, clap::Args)]
pub struct Restore {
    /// The Postgres URL of an empty database, as its owner.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The backup bundle's directory.
    #[arg(long)]
    pub bundle: PathBuf,
    /// A share file of the backup's key, from `triplespace backup-key unseal`; as many as
    /// the backup needs.
    #[arg(long = "share")]
    pub shares: Vec<PathBuf>,
    /// Restore without the sealed sections: no `private`, no sealed `ops` tables. Accounts
    /// can then not log in, and the inboxes and preferences are empty.
    #[arg(long)]
    pub without_private: bool,
}

/// What a restore did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    /// Partitions registered.
    pub partitions: usize,
    /// Slots appended.
    pub slots: u64,
    /// Tables loaded, carried and sealed.
    pub tables: usize,
}

fn policy<T: serde::de::DeserializeOwned>(what: &str, value: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .with_context(|| format!("the backup's {what} `{value}`"))
}

/// The farm, from the newest `farm` record of the backup's instance `config` partition:
/// what the projections need before the registry they read it from exists.
async fn farm_of(bundle: &Bundle) -> Result<triplespace_projections::Farm> {
    let store = bundle.store();
    let head = store
        .head(0)
        .await
        .context("the instance config partition")?;
    let mut farm = None;
    for offset in 0..head.size {
        let slot = store.read(0, offset).await?;
        let Some(record) = slot.record() else {
            continue;
        };
        // Its key is `farm`; the registry gives it the code `farm` too.
        if !matches!(record.header().key.as_deref(), Some("farm" | "farm:farm")) {
            continue;
        }
        if let Ok(Some(v)) = record.body().content().value() {
            farm = Some(v.to_json()?);
        }
    }
    let c = farm.context("the backup's instance config holds no `farm` record")?;
    let text = |k: &str| {
        c.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("the `farm` record's `{k}`"))
    };
    Ok(triplespace_projections::Farm {
        slug: text("slug")?,
        code: text("code")?,
        base: text("base")?,
    })
}

/// Loads `bytes`, a binary `COPY` of `columns`, into `schema.table`, after checking that the
/// database's table has the columns the backup holds.
async fn load(
    client: &Client,
    schema: &str,
    table: &str,
    columns: &[Column],
    bytes: Vec<u8>,
) -> Result<()> {
    let here = self::columns(client, schema, table).await?;
    let names = |cs: &[Column]| {
        cs.iter()
            .map(|c| (c.name.clone(), c.type_name.clone()))
            .collect::<Vec<_>>()
    };
    if names(&here) != names(columns) {
        bail!(
            "{schema}.{table} has other columns here than in the backup: the backup was written \
             under another schema, and migrating one forward is not built yet (SCA-358)"
        );
    }
    let list = columns
        .iter()
        .map(|c| quote(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let sink = client
        .copy_in::<_, bytes::Bytes>(&format!(
            "COPY {}.{} ({list}) FROM STDIN (FORMAT binary)",
            quote(schema),
            quote(table)
        ))
        .await
        .with_context(|| format!("copy into {schema}.{table}"))?;
    let mut sink = std::pin::pin!(sink);
    sink.send(bytes::Bytes::from(bytes)).await?;
    sink.as_mut().finish().await?;
    Ok(())
}

/// Sets every sequence in `ops` and `private` past the column it numbers: one it owns, or
/// one whose default draws on it (`private.login_id`).
async fn reset_sequences(client: &Client) -> Result<()> {
    let rows = client
        .query(
            // A sequence a column owns (identity, serial)...
            "SELECT s.oid::regclass::text, t.oid::regclass::text, quote_ident(a.attname)
             FROM pg_class s
             JOIN pg_depend d ON d.objid = s.oid AND d.classid = 'pg_class'::regclass
                             AND d.refclassid = 'pg_class'::regclass AND d.deptype IN ('a', 'i')
             JOIN pg_class t ON t.oid = d.refobjid
             JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = d.refobjsubid
             WHERE s.relkind = 'S' AND s.relnamespace::regnamespace::text IN ('ops', 'private')
             UNION
             -- ...or one a column's default draws on.
             SELECT s.oid::regclass::text, t.oid::regclass::text, quote_ident(a.attname)
             FROM pg_class s
             JOIN pg_depend d ON d.refobjid = s.oid AND d.refclassid = 'pg_class'::regclass
                             AND d.classid = 'pg_attrdef'::regclass
             JOIN pg_attrdef ad ON ad.oid = d.objid
             JOIN pg_class t ON t.oid = ad.adrelid
             JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ad.adnum
             WHERE s.relkind = 'S' AND s.relnamespace::regnamespace::text IN ('ops', 'private')",
            &[],
        )
        .await?;
    for r in rows {
        let (seq, table, column): (String, String, String) = (r.get(0), r.get(1), r.get(2));
        client
            .batch_execute(&format!(
                "SELECT setval('{seq}', coalesce((SELECT max({column}) FROM {table}), 1),
                               (SELECT max({column}) FROM {table}) IS NOT NULL)"
            ))
            .await
            .with_context(|| format!("reset {seq}"))?;
    }
    Ok(())
}

/// Restores the backup at `args.bundle` into the empty database at `args.database`.
///
/// # Errors
///
/// A bundle that is not a whole instance's backup, missing or wrong shares, a database
/// that is not empty, or any step's failure.
#[allow(clippy::too_many_lines)]
pub async fn restore(args: &Restore) -> Result<Restored> {
    let bundle =
        Bundle::open(&args.bundle).with_context(|| format!("open {}", args.bundle.display()))?;
    let manifest = bundle.manifest();
    if manifest.scope != "backup" {
        bail!(
            "{} is an {} bundle, not a backup",
            args.bundle.display(),
            manifest.scope
        );
    }
    if manifest.partitions.iter().any(|p| p.registry.is_none()) {
        bail!("the backup predates registry rows in bundles and cannot be restored by the binary");
    }
    let carried = Carried::open(&args.bundle)
        .context("a restore needs a backup of the whole instance, with its carried sections")?;
    let sealed = match (manifest.sealed, args.without_private) {
        (Some(true), false) => {
            let sealed = Sealed::open(&args.bundle)?;
            let shares = args
                .shares
                .iter()
                .map(|p| {
                    Share::parse(
                        &std::fs::read_to_string(p)
                            .with_context(|| format!("read {}", p.display()))?,
                    )
                    .with_context(|| format!("{} is not a share", p.display()))
                })
                .collect::<Result<Vec<_>>>()?;
            if shares.is_empty() {
                bail!(
                    "the backup's `private` is sealed: give {} share file(s) with --share \
                     (`triplespace backup-key unseal`), or --without-private",
                    sealed.key.threshold
                );
            }
            let key = sealed.key.combine(&shares)?;
            Some((sealed, key))
        }
        _ => None,
    };
    let farm = farm_of(&bundle).await?;

    let mut client = connect(&args.database).await?;
    // Two statements: Postgres resolves `log.partition` while planning, before the first
    // condition could spare it on a database that has none.
    let has_log: bool = client
        .query_one("SELECT to_regclass('log.partition') IS NOT NULL", &[])
        .await?
        .get(0);
    let occupied = has_log
        && client
            .query_one("SELECT EXISTS (SELECT 1 FROM log.partition)", &[])
            .await?
            .get::<_, bool>(0);
    if occupied {
        bail!("the database already holds partitions: a restore needs an empty one");
    }
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .context("migrate")?;

    // The log, partition by partition, each in a unit of work of its own.
    let mut restored = Restored::default();
    for p in &manifest.partitions {
        let registry = p.registry.as_ref().expect("checked above");
        let head = bundle.store().head(p.partition).await?;
        let info = PartitionInfo {
            tenant: registry.tenant.clone(),
            name: p.name.clone(),
            graph_iri: registry.graph_iri.clone(),
            history: policy::<History>("history", &registry.history)?,
            integrity: policy::<Integrity>("integrity", &registry.integrity)?,
            export: policy::<Export>("export policy", &registry.export)?,
            hash: registry.hash.clone(),
            origin: Some(p.origin.clone()),
        };
        let tx = client.transaction().await?;
        scatter_log_postgres::log::create_partition(&tx, p.partition, head.segments, &info)
            .await
            .with_context(|| format!("register partition {:#018x}", p.partition))?;
        let mut frontiers = Frontiers::new();
        for offset in 0..head.size {
            let slot = bundle.store().read(p.partition, offset).await?;
            scatter_log_postgres::log::append_slot_with(&tx, p.partition, slot, &mut frontiers)
                .await
                .with_context(|| format!("partition {:#018x}, offset {offset}", p.partition))?;
        }
        let mut checkpoints = PgCheckpoints::new(&tx);
        for cp in bundle.checkpoints().list(p.partition).await? {
            checkpoints.put(p.partition, &cp).await?;
        }
        for (n, m) in bundle.checkpoints().manifests(p.partition).await? {
            checkpoints.put_manifest(p.partition, n, &m).await?;
        }
        tx.commit().await?;
        restored.partitions += 1;
        restored.slots += head.size;
    }

    // The `log` sequences, at the values the backup holds.
    for s in &carried.manifest.sequences {
        let name = format!("log.{}", quote(&s.name));
        client
            .batch_execute(&format!(
                "CREATE SEQUENCE IF NOT EXISTS {name} AS bigint START 1;
                 SELECT setval('{}', {}, {});",
                name.replace('\'', "''"),
                s.value,
                s.called
            ))
            .await
            .with_context(|| format!("sequence {}", s.name))?;
    }

    // Every projection, from the log.
    crate::status::rebuild_database(&args.database, &farm)
        .await
        .context("rebuild the projections")?;

    // The backup's own rows, in place of what the replay wrote.
    let tx = client.transaction().await?;
    for t in &carried.manifest.tables {
        tx.batch_execute(&format!(
            "TRUNCATE {}.{}",
            quote(&t.schema),
            quote(&t.table)
        ))
        .await?;
    }
    if let Some((sealed, _)) = &sealed {
        let names = sealed
            .manifest
            .tables
            .iter()
            .map(|t| format!("{}.{}", quote(&t.schema), quote(&t.table)))
            .collect::<Vec<_>>()
            .join(", ");
        tx.batch_execute(&format!("TRUNCATE {names}")).await?;
    }
    tx.commit().await?;
    for t in &carried.manifest.tables {
        load(
            &client,
            &t.schema,
            &t.table,
            &t.columns,
            carried.table(&t.schema, &t.table)?,
        )
        .await?;
        restored.tables += 1;
    }
    if let Some((sealed, key)) = &sealed {
        for t in &sealed.manifest.tables {
            let bytes = sealed.table(key, &t.schema, &t.table)?;
            load(&client, &t.schema, &t.table, &t.columns, bytes).await?;
            restored.tables += 1;
        }
    }
    reset_sequences(&client).await?;
    let epoch = i64::try_from(now()).unwrap_or(i64::MAX);
    client
        .execute("UPDATE ops.visibility_epoch SET epoch = $1", &[&epoch])
        .await?;

    // The grants of every partition the restore made.
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .context("grants")?;
    Ok(restored)
}

/// Runs `triplespace restore`.
///
/// # Errors
///
/// As [`restore`].
pub async fn run(args: Restore) -> Result<()> {
    let r = restore(&args).await?;
    println!(
        "restored {} partitions ({} records), {} tables; projections rebuilt",
        r.partitions, r.slots, r.tables
    );
    if args.without_private {
        println!(
            "note: restored without `private`: accounts cannot log in until their \
             credentials are set again"
        );
    }
    Ok(())
}

/// The path a bundle's sealed key lives at, for the operator's `unseal`.
#[must_use]
pub fn sealed_key_path(bundle: &Path) -> PathBuf {
    bundle.join(crate::sealed::DIR).join("key.cbor")
}
