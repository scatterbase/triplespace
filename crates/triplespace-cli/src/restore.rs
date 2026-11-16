//! `triplespace restore` (0103 A2, A9): an instance back from a backup bundle, into an empty
//! database, with no Postgres tools. In order:
//!
//! 1. the bundle is checked: a backup of the whole instance, every partition with its
//!    registry row, and its carried sections; its sealed sections opened with `k` shares,
//!    unless it has none or `--without-private` leaves them;
//! 2. the database is migrated, which makes the schemas, the roles and their grants. For
//!    a backup written under an older schema (0103 A11), it is migrated only as far as the
//!    migrations the backup records; the carried and sealed tables are loaded there; the
//!    later migrations then run over them, as they would have over the live database;
//!    and the rows they leave are set aside in a schema of the restore's own, the tables
//!    emptied, until step 6;
//! 3. each partition is registered as it was and every slot appended verbatim, compacted
//!    and erased ones included, a block at a time by `COPY` (0103 A10), with its
//!    checkpoints and segment manifests, each checked against the tree the slots fold to;
//! 4. the `log` sequences are made at the values the backup holds;
//! 5. every projection is rebuilt from the log, in the replay order of 03 §6.1;
//! 6. the carried `ops` tables and the sealed tables replace what the rebuild wrote there,
//!    since the backup, not the replay, is their record (an inbox's read marks, a queued
//!    job), each checked first against the columns the backup holds;
//! 7. the sequences of `ops` and `private` are set past the rows loaded, and every tenant's
//!    `visibility_epoch` to the restore's time in microseconds (0103 A4);
//! 8. the migrations run once more, giving every partition its grants.
//!
//! A backup that names a migration this binary lacks was written by a newer one, and is
//! refused. One that records no migrations (written before 0103 A11, without sealed
//! sections) is taken to be of this binary's schema, and refused at step 6 if a table's
//! columns differ.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use futures_util::SinkExt as _;
use scatter_integrity::backup_key::DataKey;
use scatter_integrity::backup_key::Share;
use scatter_integrity::bundle::{Bundle, BundlePartition};
use scatter_integrity::checkpoint::{SignedCheckpoint, segment_origin};
use scatter_integrity::checkpoints::CheckpointStore as _;
use scatter_log::hash::Hash;
use scatter_log::registry::{Export, History, Integrity};
use scatter_log::store::LogStore as _;
use scatter_log::tree::{Frontier, Segments};
use scatter_log_postgres::partition::PartitionInfo;
use scatter_log_postgres::{Frontiers, PgCheckpoints};
use sha2::{Digest as _, Sha256};
use tokio_postgres::Client;
use triplespace_db::Migrator;

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
    client: &tokio_postgres::Transaction<'_>,
    schema: &str,
    table: &str,
    columns: &[Column],
    mut next: impl FnMut() -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    let here = self::columns(client, schema, table).await?;
    let names = |cs: &[Column]| {
        cs.iter()
            .map(|c| (c.name.clone(), c.type_name.clone()))
            .collect::<Vec<_>>()
    };
    if names(&here) != names(columns) {
        bail!(
            "{schema}.{table} has other columns here than in the backup: the backup's tables \
             are not of the schema its migrations make"
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
    // A piece at a time: no table is held whole, nor sent as one message (0103 A13).
    while let Some(piece) = next().with_context(|| format!("the backup's {schema}.{table}"))? {
        if !piece.is_empty() {
            sink.send(bytes::Bytes::from(piece)).await?;
        }
    }
    sink.as_mut().finish().await?;
    Ok(())
}

/// A carried table's file, read a piece at a time and held to the length and digest the
/// manifest states for it: at the end of the file, a mismatch is an error.
struct CarriedFile<'a> {
    file: std::fs::File,
    digest: Option<&'a crate::carried::Digest>,
    hash: Sha256,
    bytes: u64,
}

impl<'a> CarriedFile<'a> {
    fn open(carried: &'a Carried, schema: &str, table: &str) -> Result<Self> {
        let (file, digest) = carried.open_table(schema, table)?;
        Ok(Self {
            file,
            digest,
            hash: Sha256::new(),
            bytes: 0,
        })
    }

    fn next(&mut self) -> Result<Option<Vec<u8>>> {
        let mut piece = vec![0; 1 << 20];
        let n = self.file.read(&mut piece)?;
        if n > 0 {
            piece.truncate(n);
            self.hash.update(&piece);
            self.bytes += n as u64;
            return Ok(Some(piece));
        }
        if let Some(d) = self.digest
            && (d.bytes != self.bytes || d.sha256 != <[u8; 32]>::from(self.hash.clone().finalize()))
        {
            bail!(
                "`{}` is not the file the backup wrote ({} bytes, where it wrote {}): it is \
                 cut short or changed",
                d.file,
                self.bytes,
                d.bytes
            );
        }
        Ok(None)
    }
}

/// Reads every carried and sealed table file through before the database is touched
/// (0103 A13): a file cut short or changed, or one the shares do not open, is found now
/// and not after the log is loaded and the projections rebuilt.
fn preflight(carried: &Carried, sealed: Option<&(Sealed, DataKey)>) -> Result<()> {
    for t in &carried.manifest.tables {
        let mut file = CarriedFile::open(carried, &t.schema, &t.table)?;
        while file.next()?.is_some() {}
    }
    if let Some((sealed, key)) = sealed {
        for t in &sealed.manifest.tables {
            let mut chunks = sealed.open_table(key, &t.schema, &t.table)?;
            while chunks
                .next_chunk()
                .with_context(|| {
                    format!(
                        "the backup's sealed {}.{} does not open: the shares are of another \
                         backup, or the file is cut short or changed",
                        t.schema, t.table
                    )
                })?
                .is_some()
            {}
        }
    }
    Ok(())
}

/// Refuses a manifest that names a table outside `schemas`, the bookkeeping table, or a
/// file outside its section: the restore empties and loads what a manifest names, as the
/// database's owner, so a manifest names only what a backup writes.
fn known_tables(tables: &[crate::sealed::Table], schemas: &[&str]) -> Result<()> {
    for t in tables {
        let plain = |s: &str| {
            !s.is_empty()
                && !s.starts_with('.')
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        };
        if !schemas.contains(&t.schema.as_str())
            || (t.schema == "ops" && t.table == "migration")
            || !plain(&t.table)
            || !plain(&t.file)
        {
            bail!(
                "the backup names {}.{} in file `{}`, which no backup writes",
                t.schema,
                t.table,
                t.file
            );
        }
    }
    Ok(())
}

/// Empties the tables the backup carries and seals, and loads the backup's rows into them.
/// Returns how many tables it loaded.
async fn load_tables(
    client: &mut Client,
    carried: &Carried,
    sealed: Option<&(Sealed, DataKey)>,
) -> Result<usize> {
    let mut names: Vec<String> = carried
        .manifest
        .tables
        .iter()
        .map(|t| format!("{}.{}", quote(&t.schema), quote(&t.table)))
        .collect();
    if let Some((sealed, _)) = sealed {
        names.extend(
            sealed
                .manifest
                .tables
                .iter()
                .map(|t| format!("{}.{}", quote(&t.schema), quote(&t.table))),
        );
    }
    // One unit of work: the tables are emptied and loaded together or not at all, and a
    // file that turns out short is not left half loaded.
    let tx = client.transaction().await?;
    if !names.is_empty() {
        tx.batch_execute(&format!("TRUNCATE {}", names.join(", ")))
            .await?;
    }
    let mut tables = 0;
    for t in &carried.manifest.tables {
        let mut file = CarriedFile::open(carried, &t.schema, &t.table)?;
        load(&tx, &t.schema, &t.table, &t.columns, || file.next()).await?;
        tables += 1;
    }
    if let Some((sealed, key)) = sealed {
        for t in &sealed.manifest.tables {
            let mut chunks = sealed.open_table(key, &t.schema, &t.table)?;
            load(&tx, &t.schema, &t.table, &t.columns, || {
                Ok(chunks.next_chunk()?)
            })
            .await?;
            tables += 1;
        }
    }
    tx.commit().await?;
    Ok(tables)
}

/// The schema a restore sets migrated tables aside in, dropped when it puts them back.
const ASIDE: &str = "ts_restore";

/// A table set aside: where it belongs, where it waits, and its column list.
struct Kept {
    table: String,
    aside: String,
    columns: String,
}

/// After the later migrations have run over a backup's tables (0103 A11): copies every
/// table this schema carries, and seals where the sealed sections were loaded, into
/// [`ASIDE`], and empties the originals, so that the rebuild starts from the tables it
/// would find in a restore of this schema's own backup. The tables are this schema's, not
/// the backup's: a migration may have added, dropped or moved one.
async fn set_aside(client: &mut Client, with_sealed: bool) -> Result<Vec<Kept>> {
    let mut tables: Vec<(String, String)> = triplespace_db::ops::carried_tables()
        .into_iter()
        .map(|t| ("ops".to_string(), t.to_string()))
        .collect();
    if with_sealed {
        tables.extend(crate::sealed::tables(client).await?);
    }
    let tx = client.transaction().await?;
    tx.batch_execute(&format!("CREATE SCHEMA {ASIDE}")).await?;
    let mut kept = Vec::new();
    for (i, (schema, table)) in tables.iter().enumerate() {
        let columns = columns(&tx, schema, table)
            .await?
            .iter()
            .map(|c| quote(&c.name))
            .collect::<Vec<_>>()
            .join(", ");
        let k = Kept {
            table: format!("{}.{}", quote(schema), quote(table)),
            aside: format!("{ASIDE}.t{i}"),
            columns,
        };
        tx.batch_execute(&format!(
            "CREATE TABLE {} AS SELECT {} FROM {}; TRUNCATE {}",
            k.aside, k.columns, k.table, k.table
        ))
        .await
        .with_context(|| format!("set {schema}.{table} aside"))?;
        kept.push(k);
    }
    tx.commit().await?;
    Ok(kept)
}

/// Puts the tables [`set_aside`] kept back in place of what the rebuild wrote, and drops
/// [`ASIDE`]. Returns how many tables it loaded.
async fn put_back(client: &mut Client, kept: &[Kept]) -> Result<usize> {
    let tx = client.transaction().await?;
    for k in kept {
        // `OVERRIDING SYSTEM VALUE`: an identity column keeps the value it had.
        tx.batch_execute(&format!(
            "TRUNCATE {table};
             INSERT INTO {table} ({columns}) OVERRIDING SYSTEM VALUE
             SELECT {columns} FROM {aside}",
            table = k.table,
            columns = k.columns,
            aside = k.aside
        ))
        .await
        .with_context(|| format!("put {} back", k.table))?;
    }
    tx.batch_execute(&format!("DROP SCHEMA {ASIDE} CASCADE"))
        .await?;
    tx.commit().await?;
    Ok(kept.len())
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

/// The most slots a block of the load holds, and the most bytes past its first slot: one
/// read of the bundle and one `COPY` each (0103 A10).
const BLOCK_SLOTS: usize = 8192;
const BLOCK_BYTES: u64 = 32 << 20;

/// A partition's checkpoints and segment manifests, checked against its leaves as they
/// load (0103 A10): each signed under the bundle's key chain for its origin, each
/// checkpoint's root the tree's at its size, each manifest's the root of its segment's
/// leaves, and that segment full. What `verify` checks of them (0006 §9), without holding
/// every leaf.
struct ChainCheck {
    partition: u64,
    segments: Segments,
    whole: Frontier,
    segment: Frontier,
    /// Roots still to be met, by tree size and by segment.
    checkpoints: BTreeMap<u64, Hash>,
    manifests: BTreeMap<u64, Hash>,
}

impl ChainCheck {
    fn new(
        bundle: &Bundle,
        p: &BundlePartition,
        segments: Segments,
        signed: &[SignedCheckpoint],
        manifests: &[(u64, SignedCheckpoint)],
    ) -> Result<Self> {
        let id = p.partition;
        let verifiers = bundle.chain().verifiers(&p.origin);
        let mut by_size = BTreeMap::new();
        for cp in signed {
            let size = cp.checkpoint.size;
            cp.verify(&p.origin, &verifiers)
                .with_context(|| format!("partition {id:#018x}: the checkpoint at size {size}"))?;
            if by_size
                .insert(size, cp.checkpoint.root)
                .is_some_and(|other| other != cp.checkpoint.root)
            {
                bail!("partition {id:#018x}: two checkpoints at size {size} disagree");
            }
        }
        let mut by_segment = BTreeMap::new();
        for (n, m) in manifests {
            let origin = segment_origin(&p.origin, *n);
            m.verify(&origin, &bundle.chain().verifiers(&origin))
                .with_context(|| format!("partition {id:#018x}: the manifest of segment {n}"))?;
            if m.checkpoint.size != segments.len() {
                bail!(
                    "partition {id:#018x}: the manifest of segment {n} is not of a whole segment"
                );
            }
            by_segment.insert(*n, m.checkpoint.root);
        }
        if by_size
            .remove(&0)
            .is_some_and(|root| root != scatter_log::tree::empty_root())
        {
            bail!("partition {id:#018x}: the checkpoint at size 0 is not of the empty tree");
        }
        Ok(Self {
            partition: id,
            segments,
            whole: Frontier::new(),
            segment: Frontier::new(),
            checkpoints: by_size,
            manifests: by_segment,
        })
    }

    /// The next leaf.
    fn push(&mut self, leaf: Hash) -> Result<()> {
        let id = self.partition;
        let n = self.segments.segment_of(self.whole.size());
        self.whole.push_leaf(leaf);
        self.segment.push_leaf(leaf);
        let size = self.whole.size();
        if self
            .checkpoints
            .remove(&size)
            .is_some_and(|root| root != self.whole.root())
        {
            bail!(
                "partition {id:#018x}: the checkpoint at size {size} does not match the                  records: the backup does not verify"
            );
        }
        if size == self.segments.range(n).end {
            if self
                .manifests
                .remove(&n)
                .is_some_and(|root| root != self.segment.root())
            {
                bail!(
                    "partition {id:#018x}: the manifest of segment {n} does not match the                      records: the backup does not verify"
                );
            }
            self.segment = Frontier::new();
        }
        Ok(())
    }

    /// After the last leaf: nothing may be left that the leaves did not reach.
    fn finish(&self) -> Result<()> {
        let id = self.partition;
        if let Some(size) = self.checkpoints.keys().next() {
            bail!("partition {id:#018x}: a checkpoint at size {size} is past the records");
        }
        if let Some(n) = self.manifests.keys().next() {
            bail!("partition {id:#018x}: a manifest of segment {n}, which is not full");
        }
        Ok(())
    }
}

/// Moves `private.login_id` past every login the log holds (0103 A13). [`reset_sequences`]
/// sets it from `private.login`, which a restore without the sealed sections leaves
/// empty: the next login would then be minted as 1 and its record keyed `login:1`, a key
/// the restored `logins` partition already holds for someone else.
async fn login_ids_past_the_log(client: &Client) -> Result<()> {
    let held: Option<i64> = client
        .query_one(
            "SELECT max(substring(r.key from '^login:([0-9]{1,18})$')::bigint)
             FROM log.record r JOIN log.partition p ON p.partition = r.partition
             WHERE p.tenant IS NULL AND p.name = 'logins'",
            &[],
        )
        .await?
        .get(0);
    let Some(held) = held else {
        return Ok(());
    };
    client
        .execute(
            "SELECT setval('private.login_id', $1, true)
             FROM private.login_id WHERE NOT is_called OR last_value < $1",
            &[&held],
        )
        .await
        .context("move private.login_id past the logins the log holds")?;
    Ok(())
}

/// Restores the backup at `args.bundle` into the empty database at `args.database`.
///
/// # Errors
///
/// A bundle that is not a whole instance's backup, missing or wrong shares, a database
/// that is not empty, or any step's failure.
pub async fn restore(args: &Restore) -> Result<Restored> {
    restore_with(args, &triplespace_db::migrator()).await
}

/// [`restore`], with the migrations given: this binary's, but for a test of a migration
/// that does not exist yet.
///
/// # Errors
///
/// As [`restore`].
#[allow(clippy::too_many_lines)]
pub async fn restore_with(args: &Restore, migrator: &Migrator) -> Result<Restored> {
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
    if manifest.sealed.is_none() {
        // `sealed` is written last, true or false (0103 A8): without it the backup
        // stopped before its carried and sealed sections were whole.
        bail!(
            "{} does not say whether it has its sealed sections: the backup that wrote it \
             did not finish",
            args.bundle.display()
        );
    }
    if manifest.partitions.iter().any(|p| p.registry.is_none()) {
        bail!("the backup predates registry rows in bundles and cannot be restored by the binary");
    }
    let carried = Carried::open(&args.bundle)
        .context("a restore needs a backup of the whole instance, with its carried sections")?;
    known_tables(&carried.manifest.tables, &["ops"])?;
    let sealed = match (manifest.sealed, args.without_private) {
        (Some(true), false) => {
            let sealed = Sealed::open(&args.bundle)?;
            known_tables(&sealed.manifest.tables, &["private", "ops"])?;
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
    preflight(&carried, sealed.as_ref())?;
    let farm = farm_of(&bundle).await?;

    let mut client = connect(&args.database).await?;
    // Empty means never migrated: a restore that stopped part-way has made schemas, and
    // run again over them would take the backup's tables for ones already migrated.
    let made: Vec<String> = client
        .query(
            "SELECT nspname::text FROM pg_namespace
             WHERE nspname = ANY($1) ORDER BY nspname",
            &[&["log", "ops", "view", "private", ASIDE].as_slice()],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if !made.is_empty() {
        bail!(
            "the database already has the schema(s) {}: a restore needs an empty database, \
             so drop and create it again, also after a restore that did not finish",
            made.join(", ")
        );
    }
    // The schema: the one the backup was written under where it says, its tables loaded
    // there and migrated forward with the rest (0103 A11); this binary's otherwise.
    let recorded = match &carried.manifest.migrations {
        Some(m) => Some(m.clone()),
        // A backup from before the carried manifest recorded them: its sealed manifest
        // does, in the clear, where it has one.
        None if args.bundle.join(crate::sealed::DIR).is_dir() => {
            Some(Sealed::open(&args.bundle)?.manifest.migrations)
        }
        None => None,
    };
    let older = match &recorded {
        Some(recorded) => migrator
            .apply_recorded(&mut client, recorded)
            .await
            .context("migrate to the backup's schema")?,
        None => false,
    };
    if older {
        load_tables(&mut client, &carried, sealed.as_ref())
            .await
            .context("load the backup's tables into the schema it was written under")?;
    }
    migrator.apply(&mut client).await.context("migrate")?;
    let kept = if older {
        Some(set_aside(&mut client, sealed.is_some()).await?)
    } else {
        None
    };

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
        let signed = bundle.checkpoints().list(p.partition).await?;
        let manifests = bundle.checkpoints().manifests(p.partition).await?;
        let mut chain = ChainCheck::new(&bundle, p, head.segments, &signed, &manifests)?;
        let mut frontiers = Frontiers::new();
        let mut offset = 0;
        while offset < head.size {
            let slots = bundle
                .store()
                .read_range(p.partition, offset, BLOCK_SLOTS, BLOCK_BYTES)?;
            let leaves =
                scatter_log_postgres::log::append_slots(&tx, p.partition, &slots, &mut frontiers)
                    .await
                    .with_context(|| {
                        format!("partition {:#018x}, from offset {offset}", p.partition)
                    })?;
            for leaf in leaves {
                chain.push(leaf)?;
            }
            offset += slots.len() as u64;
        }
        chain.finish()?;
        let mut checkpoints = PgCheckpoints::new(&tx);
        for cp in &signed {
            checkpoints.put(p.partition, cp).await?;
        }
        for (n, m) in &manifests {
            checkpoints.put_manifest(p.partition, *n, m).await?;
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

    // The backup's own rows, in place of what the replay wrote: from the bundle, or, for
    // a backup migrated forward, from where the migrations' result was set aside.
    restored.tables = match &kept {
        Some(kept) => put_back(&mut client, kept).await?,
        None => load_tables(&mut client, &carried, sealed.as_ref()).await?,
    };
    reset_sequences(&client).await?;
    login_ids_past_the_log(&client).await?;
    let epoch = i64::try_from(now()).unwrap_or(i64::MAX);
    // Every tenant, not only those the backup holds a row for: a row is made at a
    // tenant's first bump, which the old instance may have reached after the backup.
    client
        .execute(
            "INSERT INTO ops.visibility_epoch (tenant, epoch)
             SELECT DISTINCT tenant, $1::bigint FROM log.partition WHERE tenant IS NOT NULL
             ON CONFLICT (tenant) DO UPDATE SET epoch = EXCLUDED.epoch",
            &[&epoch],
        )
        .await?;
    client
        .execute("UPDATE ops.visibility_epoch SET epoch = $1", &[&epoch])
        .await?;

    // The grants of every partition the restore made.
    migrator.apply(&mut client).await.context("grants")?;
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

#[cfg(test)]
mod tests {
    use scatter_log::tree::root_of;

    use super::*;

    #[test]
    fn a_manifest_names_only_what_a_backup_writes() {
        let table = |schema: &str, table: &str, file: &str| crate::sealed::Table {
            schema: schema.into(),
            table: table.into(),
            file: file.into(),
            columns: Vec::new(),
        };
        known_tables(&[table("ops", "job_queue", "ops.job_queue.bin")], &["ops"]).unwrap();
        known_tables(
            &[table("private", "login", "private.login.bin")],
            &["private", "ops"],
        )
        .unwrap();
        for bad in [
            table("view", "page", "view.page.bin"),
            table("private", "login", "private.login.bin"),
            table("ops", "migration", "ops.migration.bin"),
            table("ops", "job_queue", "../job_queue.bin"),
            table("ops", "job_queue", "a/b.bin"),
        ] {
            assert!(
                known_tables(std::slice::from_ref(&bad), &["ops"]).is_err(),
                "{bad:?}"
            );
        }
    }

    fn leaves(n: u8) -> Vec<Hash> {
        (0..n).map(|i| [i; 32]).collect()
    }

    fn check(
        leaves: &[Hash],
        checkpoints: &[(u64, Hash)],
        manifests: &[(u64, Hash)],
    ) -> Result<()> {
        let mut chain = ChainCheck {
            partition: 7,
            segments: Segments::new(2).unwrap(),
            whole: Frontier::new(),
            segment: Frontier::new(),
            checkpoints: checkpoints.iter().copied().collect(),
            manifests: manifests.iter().copied().collect(),
        };
        for leaf in leaves {
            chain.push(*leaf)?;
        }
        chain.finish()
    }

    /// 0103 §4 (A10): a restore refuses a partition whose checkpoints or segment manifests
    /// are not of the records it loads.
    #[test]
    fn checkpoints_and_manifests_are_checked_against_the_leaves() {
        let l = leaves(10);
        let at = |n: usize| (n as u64, root_of(&l[..n]));
        let segment = |n: usize| (n as u64, root_of(&l[n * 4..n * 4 + 4]));
        check(
            &l,
            &[at(1), at(4), at(7), at(10)],
            &[segment(0), segment(1)],
        )
        .unwrap();
        check(&l, &[], &[]).unwrap();

        let wrong = |r: Result<()>, what: &str| {
            let e = r.unwrap_err().to_string();
            assert!(e.contains(what), "{e}");
        };
        // A checkpoint of other records, and one past the head.
        wrong(check(&l, &[(7, root_of(&l[..6]))], &[]), "size 7");
        wrong(check(&l, &[(11, [0; 32])], &[]), "past the records");
        // A manifest of other records, and one of a segment the records do not fill.
        wrong(check(&l, &[], &[(1, root_of(&l[..4]))]), "segment 1");
        wrong(check(&l, &[], &[(2, [0; 32])]), "not full");
        // A changed record changes every root after it.
        let mut changed = l.clone();
        changed[5] = [99; 32];
        check(&changed, &[at(4)], &[segment(0)]).unwrap();
        wrong(check(&changed, &[at(4), at(10)], &[]), "size 10");
        wrong(check(&changed, &[], &[segment(1)]), "segment 1");
    }
}
