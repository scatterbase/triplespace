//! A backup's carried sections (0103 A4, A9): the `ops` tables classed `carried`
//! ([`triplespace_db::ops`]), each its binary `COPY` in the clear, and the value of every
//! sequence in `log`, which numbers revisions, log events, pages, users, minted entities and
//! surrogates per tenant. Written from the backup's own snapshot, beside the log and the
//! sealed sections, so that a restore hands out no ID a restored record already holds.
//!
//! ```text
//! {bundle}/carried/
//!   manifest.cbor        {"format": 1, "tables": [{"schema", "table", "file", "columns"}],
//!                         "sequences": [{"name", "value", "called"}],
//!                         "migrations": [{"crate", "name", "checksum"}]}
//!   ops.{table}.bin      each table's binary COPY
//! ```
//!
//! `digests` is each table file's length and SHA-256 (`[{"file", "bytes", "sha256"}]`,
//! 0103 A13), which a restore checks as it loads; a backup from before A13 lacks the key.
//!
//! `migrations` is every migration the database had applied (0103 A11), so that a restore
//! makes the schema the tables came from even for a backup without sealed sections, whose
//! manifest records the same; a backup from before A11 lacks the key.
//!
//! The sequences of `ops` and `private` are not carried: each numbers a column of a table the
//! backup holds, and a restore sets it from that column.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use scatter_log::cbor::{self, Value};

use sha2::{Digest as _, Sha256};
use std::io::Write as _;

use crate::sealed::{
    Table, applied_migrations, close, columns, copy_out, create, migrations_from, migrations_value,
    table_from, table_value,
};

/// The directory of the carried sections inside a bundle.
pub const DIR: &str = "carried";
const MANIFEST_FILE: &str = "manifest.cbor";
/// The format of `carried/manifest.cbor`.
pub const FORMAT: u64 = 1;

/// A sequence's state, as `setval` takes it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    /// The sequence's name in `log`, `{tenant}.{kind}`.
    pub name: String,
    /// Its last value, or its start while never called.
    pub value: i64,
    /// Whether `nextval` has been called: the next value is `value + 1`, else `value`.
    pub called: bool,
}

/// What `carried/manifest.cbor` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The carried tables.
    pub tables: Vec<Table>,
    /// Every sequence in `log`.
    pub sequences: Vec<Sequence>,
    /// `(crate, migration, checksum)` of every migration the database had applied; `None`
    /// in a backup written before they were recorded here.
    pub migrations: Option<Vec<(String, String, String)>>,
    /// Each table file's length and SHA-256 (0103 A13): the carried tables are in the
    /// clear, with nothing else to say that a file is whole. `None` in a backup written
    /// before they were recorded.
    pub digests: Option<Vec<Digest>>,
}

/// A carried file's length and digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    /// The file, as its table names it.
    pub file: String,
    /// Its length in bytes.
    pub bytes: u64,
    /// Its SHA-256.
    pub sha256: [u8; 32],
}

/// Writes the carried sections of a backup into `bundle` from `client`'s snapshot.
///
/// # Errors
///
/// A database or file failure.
pub async fn write(client: &tokio_postgres::Transaction<'_>, bundle: &Path) -> Result<Manifest> {
    let dir = bundle.join(DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let mut tables = Vec::new();
    let mut digests = Vec::new();
    for table in triplespace_db::ops::carried_tables() {
        let columns = columns(client, "ops", table).await?;
        let file = format!("ops.{table}.bin");
        let mut out = create(&dir.join(&file))?;
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        copy_out(client, "ops", table, &columns, |piece| {
            hash.update(piece);
            bytes += piece.len() as u64;
            out.write_all(piece)
        })
        .await?;
        close(out)?;
        digests.push(Digest {
            file: file.clone(),
            bytes,
            sha256: hash.finalize().into(),
        });
        tables.push(Table {
            schema: "ops".into(),
            table: table.into(),
            file,
            columns,
        });
    }
    let sequences = client
        .query(
            "SELECT sequencename::text, coalesce(last_value, start_value), last_value IS NOT NULL
             FROM pg_sequences WHERE schemaname = 'log' ORDER BY sequencename",
            &[],
        )
        .await?
        .iter()
        .map(|r| Sequence {
            name: r.get(0),
            value: r.get(1),
            called: r.get(2),
        })
        .collect();
    let manifest = Manifest {
        tables,
        sequences,
        migrations: Some(applied_migrations(client).await?),
        digests: Some(digests),
    };
    std::fs::write(dir.join(MANIFEST_FILE), cbor::encode(&manifest.to_value())?)?;
    Ok(manifest)
}

impl Manifest {
    fn to_value(&self) -> Value {
        let text = |s: &str| Value::text(s);
        let mut pairs = vec![
            (text("format"), Value::Int(i128::from(FORMAT))),
            (
                text("tables"),
                Value::Array(self.tables.iter().map(table_value).collect()),
            ),
            (
                text("sequences"),
                Value::Array(
                    self.sequences
                        .iter()
                        .map(|s| {
                            Value::map(vec![
                                (text("name"), text(&s.name)),
                                (text("value"), Value::Int(i128::from(s.value))),
                                (text("called"), Value::Bool(s.called)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ];
        if let Some(m) = &self.migrations {
            pairs.push((text("migrations"), migrations_value(m)));
        }
        if let Some(d) = &self.digests {
            pairs.push((
                text("digests"),
                Value::Array(
                    d.iter()
                        .map(|d| {
                            Value::map(vec![
                                (text("file"), text(&d.file)),
                                (text("bytes"), Value::Int(i128::from(d.bytes))),
                                (text("sha256"), Value::Bytes(d.sha256.to_vec())),
                            ])
                        })
                        .collect(),
                ),
            ));
        }
        Value::map(pairs)
    }

    fn from_value(v: &Value) -> Result<Self> {
        if v.get("format").and_then(Value::as_u64) != Some(FORMAT) {
            bail!("carried manifest: an unknown format");
        }
        let list = |k: &str| -> Result<&Vec<Value>> {
            match v.get(k) {
                Some(Value::Array(a)) => Ok(a),
                _ => bail!("carried manifest: `{k}`"),
            }
        };
        let tables = list("tables")?
            .iter()
            .map(table_from)
            .collect::<Result<_>>()?;
        let sequences = list("sequences")?
            .iter()
            .map(|s| {
                let Some(Value::Int(value)) = s.get("value") else {
                    bail!("carried manifest: a sequence's `value`");
                };
                Ok(Sequence {
                    name: s
                        .get("name")
                        .and_then(Value::as_text)
                        .context("carried manifest: a sequence's `name`")?
                        .to_string(),
                    value: i64::try_from(*value)
                        .context("carried manifest: a sequence's `value`")?,
                    called: matches!(s.get("called"), Some(Value::Bool(true))),
                })
            })
            .collect::<Result<_>>()?;
        let migrations = match v.get("migrations") {
            Some(Value::Array(m)) => Some(migrations_from(m)?),
            Some(_) => bail!("carried manifest: `migrations`"),
            None => None,
        };
        let digests = match v.get("digests") {
            Some(Value::Array(d)) => Some(
                d.iter()
                    .map(|d| {
                        let (Some(file), Some(bytes), Some(Value::Bytes(sha))) = (
                            d.get("file").and_then(Value::as_text),
                            d.get("bytes").and_then(Value::as_u64),
                            d.get("sha256"),
                        ) else {
                            bail!("carried manifest: a digest");
                        };
                        Ok(Digest {
                            file: file.to_string(),
                            bytes,
                            sha256: sha
                                .as_slice()
                                .try_into()
                                .ok()
                                .context("carried manifest: a digest's `sha256`")?,
                        })
                    })
                    .collect::<Result<_>>()?,
            ),
            Some(_) => bail!("carried manifest: `digests`"),
            None => None,
        };
        Ok(Self {
            tables,
            sequences,
            migrations,
            digests,
        })
    }
}

/// A backup's carried sections, opened for reading.
#[derive(Debug, Clone)]
pub struct Carried {
    dir: PathBuf,
    /// The manifest.
    pub manifest: Manifest,
}

impl Carried {
    /// Opens `bundle`'s carried sections.
    ///
    /// # Errors
    ///
    /// A backup without them, or files that do not read.
    pub fn open(bundle: &Path) -> Result<Self> {
        let dir = bundle.join(DIR);
        let manifest = Manifest::from_value(
            &cbor::decode(
                &std::fs::read(dir.join(MANIFEST_FILE))
                    .with_context(|| format!("{} has no carried sections", bundle.display()))?,
            )
            .context("carried manifest")?,
        )?;
        Ok(Self { dir, manifest })
    }

    /// One table's binary `COPY`, opened for reading, with the digest the manifest holds
    /// for it, which the reader checks once it has read the file through.
    ///
    /// # Errors
    ///
    /// A table the backup does not hold, or a file that does not open.
    pub fn open_table(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<(std::fs::File, Option<&Digest>)> {
        let t = self
            .manifest
            .tables
            .iter()
            .find(|t| t.schema == schema && t.table == table)
            .with_context(|| format!("the backup holds no {schema}.{table}"))?;
        let file = std::fs::File::open(self.dir.join(&t.file))
            .with_context(|| format!("the backup's {}", t.file))?;
        let digest = self
            .manifest
            .digests
            .as_ref()
            .map(|d| {
                d.iter()
                    .find(|d| d.file == t.file)
                    .with_context(|| format!("the backup states no digest for {}", t.file))
            })
            .transpose()?;
        Ok((file, digest))
    }
}
