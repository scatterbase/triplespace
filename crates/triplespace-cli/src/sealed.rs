//! A backup's sealed sections (0103 A2, A3, A7, A8): every `private` table and every `ops`
//! table classed `sealed` ([`triplespace_db::ops`]), each its binary `COPY`, encrypted under
//! the backup's data key, whose shares are sealed to the backup key set in force. Written
//! from the backup's own snapshot, so that they and the log are of one instant.
//!
//! ```text
//! {bundle}/sealed/
//!   key.cbor             the data key, a share sealed to each key of the set (SealedKey)
//!   manifest.cbor        {"format": 1, "context", "set", "tables": [{"schema", "table",
//!                         "file", "columns": [{"name", "type", "identity"}]}],
//!                         "migrations": [{"crate", "name", "checksum"}]}
//!   {schema}.{table}.bin each table's binary COPY, encrypted in chunks
//! ```
//!
//! A table's associated data is the backup's context and the table's name, so that a file
//! moved to another backup or renamed does not open. The columns and the migrations are
//! what a restore needs to load a table into the schema of the instance it came from, and
//! to migrate it forward (SCA-358).

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use futures_util::TryStreamExt as _;
use scatter_integrity::backup_key::{DataKey, SealedKey, seal_new};
use scatter_log::cbor::{self, Value};
use tokio_postgres::GenericClient;

/// The directory of the sealed sections inside a bundle.
pub const DIR: &str = "sealed";
const KEY_FILE: &str = "key.cbor";
const MANIFEST_FILE: &str = "manifest.cbor";
/// The format of `sealed/manifest.cbor`.
pub const FORMAT: u64 = 1;

/// A column of a sealed table, as the backup's schema had it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The name.
    pub name: String,
    /// The type, as `format_type` writes it.
    pub type_name: String,
    /// `a` or `d` for an identity column, empty otherwise.
    pub identity: String,
}

/// One sealed table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// `private` or `ops`.
    pub schema: String,
    /// The table.
    pub table: String,
    /// Its file in `sealed/`.
    pub file: String,
    /// Its columns, in the order of the `COPY`; generated columns are left out.
    pub columns: Vec<Column>,
}

/// What `sealed/manifest.cbor` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The backup's context, bound into every share and every table.
    pub context: String,
    /// The offset of the `backup-keys` record the backup was sealed under.
    pub set: u64,
    /// The tables.
    pub tables: Vec<Table>,
    /// `(crate, migration, checksum)` of every migration the database had applied.
    pub migrations: Vec<(String, String, String)>,
}

/// The context of a backup: the instance and the backup's instant.
#[must_use]
pub fn context(farm_slug: &str, created: u64) -> String {
    format!("backup:{farm_slug}:{created}")
}

fn aad(context: &str, schema: &str, table: &str) -> Vec<u8> {
    format!("{context}\0{schema}.{table}").into_bytes()
}

/// The tables a backup seals: every table in `private`, and every `ops` table classed
/// `sealed`; each schema's classification checked against the live schema first, so that
/// no table is left out unclassified.
async fn tables<C: GenericClient>(client: &C) -> Result<Vec<(String, String)>> {
    triplespace_db::private::check(client)
        .await
        .context("the `private` tables' portability classes")?;
    triplespace_db::ops::check(client)
        .await
        .context("the `ops` tables' backup classes")?;
    let mut out: Vec<(String, String)> = client
        .query(
            "SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'private' AND c.relkind IN ('r', 'p') ORDER BY c.relname",
            &[],
        )
        .await?
        .iter()
        .map(|r| ("private".to_string(), r.get(0)))
        .collect();
    out.extend(
        triplespace_db::ops::sealed_tables()
            .into_iter()
            .map(|t| ("ops".to_string(), t.to_string())),
    );
    Ok(out)
}

/// A table's columns, in order, generated ones left out: what its `COPY` holds.
pub(crate) async fn columns<C: GenericClient>(
    client: &C,
    schema: &str,
    table: &str,
) -> Result<Vec<Column>> {
    Ok(client
        .query(
            "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), a.attidentity::text
             FROM pg_attribute a
             JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
               AND a.attgenerated = ''
             ORDER BY a.attnum",
            &[&schema, &table],
        )
        .await?
        .iter()
        .map(|r| Column {
            name: r.get(0),
            type_name: r.get(1),
            identity: r.get(2),
        })
        .collect())
}

pub(crate) fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// The binary `COPY` of `columns` of `schema.table`, read on `client`.
pub(crate) async fn copy_out(
    client: &tokio_postgres::Transaction<'_>,
    schema: &str,
    table: &str,
    columns: &[Column],
) -> Result<Vec<u8>> {
    let list = columns
        .iter()
        .map(|c| quote(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let stream = client
        .copy_out(&format!(
            "COPY {}.{} ({list}) TO STDOUT (FORMAT binary)",
            quote(schema),
            quote(table)
        ))
        .await
        .with_context(|| format!("copy {schema}.{table}"))?;
    let chunks: Vec<bytes::Bytes> = stream.try_collect().await?;
    Ok(chunks.concat())
}

/// Writes the sealed sections of a backup into `bundle`, from `client`'s snapshot, sealed to
/// the backup key set in force. Returns the manifest written.
///
/// # Errors
///
/// No backup key set registered, a table without a class, or a database or file failure.
pub async fn write(
    client: &tokio_postgres::Transaction<'_>,
    bundle: &Path,
    farm_slug: &str,
    created: u64,
) -> Result<Manifest> {
    let Some(registered) = triplespace_api_ingest::backup_keys::current(client)
        .await
        .map_err(anyhow::Error::msg)?
    else {
        bail!(
            "no backup key set is registered, and a backup seals `private` to one (0103 A3): \
             register it (`PUT /backup-keys` as the owner, or `triplespace backup-key \
             register` where there is none), or pass --without-private for a backup without it"
        );
    };
    let context = context(farm_slug, created);
    let (key, sealed) = seal_new(&registered.set, &context)?;
    let dir = bundle.join(DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    std::fs::write(dir.join(KEY_FILE), sealed.encode()?)?;
    let mut tables = Vec::new();
    for (schema, table) in self::tables(client).await? {
        let columns = columns(client, &schema, &table).await?;
        let plain = copy_out(client, &schema, &table, &columns).await?;
        let file = format!("{schema}.{table}.bin");
        std::fs::write(
            dir.join(&file),
            key.encrypt_chunks(&aad(&context, &schema, &table), &plain),
        )
        .with_context(|| format!("write {file}"))?;
        tables.push(Table {
            schema,
            table,
            file,
            columns,
        });
    }
    let migrations = triplespace_db::migrator()
        .status(client)
        .await?
        .into_iter()
        .filter(|m| m.applied_at.is_some())
        .map(|m| (m.crate_name, m.name, m.checksum))
        .collect();
    let manifest = Manifest {
        context,
        set: registered.offset,
        tables,
        migrations,
    };
    std::fs::write(dir.join(MANIFEST_FILE), cbor::encode(&manifest.to_value())?)?;
    Ok(manifest)
}

/// A table's entry, as both manifests write it.
pub(crate) fn table_value(t: &Table) -> Value {
    let text = |s: &str| Value::text(s);
    Value::map(vec![
        (text("schema"), text(&t.schema)),
        (text("table"), text(&t.table)),
        (text("file"), text(&t.file)),
        (
            text("columns"),
            Value::Array(
                t.columns
                    .iter()
                    .map(|c| {
                        Value::map(vec![
                            (text("name"), text(&c.name)),
                            (text("type"), text(&c.type_name)),
                            (text("identity"), text(&c.identity)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// A table's entry, from [`table_value`]'s form.
pub(crate) fn table_from(t: &Value) -> Result<Table> {
    let text = |v: &Value, k: &str| -> Result<String> {
        Ok(v.get(k)
            .and_then(Value::as_text)
            .with_context(|| format!("manifest: `{k}`"))?
            .to_string())
    };
    let Some(Value::Array(cols)) = t.get("columns") else {
        bail!("manifest: `columns`");
    };
    Ok(Table {
        schema: text(t, "schema")?,
        table: text(t, "table")?,
        file: text(t, "file")?,
        columns: cols
            .iter()
            .map(|c| {
                Ok(Column {
                    name: text(c, "name")?,
                    type_name: text(c, "type")?,
                    identity: text(c, "identity")?,
                })
            })
            .collect::<Result<_>>()?,
    })
}

impl Manifest {
    fn to_value(&self) -> Value {
        let text = |s: &str| Value::text(s);
        Value::map(vec![
            (text("format"), Value::Int(i128::from(FORMAT))),
            (text("context"), text(&self.context)),
            (text("set"), Value::Int(i128::from(self.set))),
            (
                text("tables"),
                Value::Array(self.tables.iter().map(table_value).collect()),
            ),
            (
                text("migrations"),
                Value::Array(
                    self.migrations
                        .iter()
                        .map(|(c, n, sum)| {
                            Value::map(vec![
                                (text("crate"), text(c)),
                                (text("name"), text(n)),
                                (text("checksum"), text(sum)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }

    fn from_value(v: &Value) -> Result<Self> {
        let text = |v: &Value, k: &str| -> Result<String> {
            Ok(v.get(k)
                .and_then(Value::as_text)
                .with_context(|| format!("sealed manifest: `{k}`"))?
                .to_string())
        };
        if v.get("format").and_then(Value::as_u64) != Some(FORMAT) {
            bail!("sealed manifest: an unknown format");
        }
        let list = |k: &str| -> Result<&Vec<Value>> {
            match v.get(k) {
                Some(Value::Array(a)) => Ok(a),
                _ => bail!("sealed manifest: `{k}`"),
            }
        };
        let tables = list("tables")?
            .iter()
            .map(table_from)
            .collect::<Result<_>>()?;
        let migrations = list("migrations")?
            .iter()
            .map(|m| Ok((text(m, "crate")?, text(m, "name")?, text(m, "checksum")?)))
            .collect::<Result<_>>()?;
        Ok(Self {
            context: text(v, "context")?,
            set: v
                .get("set")
                .and_then(Value::as_u64)
                .context("sealed manifest: `set`")?,
            tables,
            migrations,
        })
    }
}

/// A backup's sealed sections, opened for reading: its manifest and its sealed key.
#[derive(Debug, Clone)]
pub struct Sealed {
    dir: PathBuf,
    /// The manifest.
    pub manifest: Manifest,
    /// The data key, a share sealed to each key of the set.
    pub key: SealedKey,
}

impl Sealed {
    /// Opens `bundle`'s sealed sections.
    ///
    /// # Errors
    ///
    /// A backup without them, or files that do not read.
    pub fn open(bundle: &Path) -> Result<Self> {
        let dir = bundle.join(DIR);
        let manifest = Manifest::from_value(
            &cbor::decode(
                &std::fs::read(dir.join(MANIFEST_FILE))
                    .with_context(|| format!("{} has no sealed sections", bundle.display()))?,
            )
            .context("sealed manifest")?,
        )?;
        let key = SealedKey::decode(&std::fs::read(dir.join(KEY_FILE))?)?;
        if key.context != manifest.context {
            bail!("the sealed key is for another backup");
        }
        Ok(Self { dir, manifest, key })
    }

    /// The path of the sealed key, for `backup-key unseal` and `reseal`.
    #[must_use]
    pub fn key_path(&self) -> PathBuf {
        self.dir.join(KEY_FILE)
    }

    /// One table's binary `COPY`, decrypted with the backup's data key.
    ///
    /// # Errors
    ///
    /// A table the backup does not hold, or a file that does not open under the key.
    pub fn table(&self, key: &DataKey, schema: &str, table: &str) -> Result<Vec<u8>> {
        let t = self
            .manifest
            .tables
            .iter()
            .find(|t| t.schema == schema && t.table == table)
            .with_context(|| format!("the backup holds no {schema}.{table}"))?;
        let sealed = std::fs::read(self.dir.join(&t.file))?;
        Ok(key.decrypt_chunks(&aad(&self.manifest.context, schema, table), &sealed)?)
    }
}
