//! Export bundles (0006 §9.1): a directory that holds, for each exported partition, its
//! segments with headers and bodies, every checkpoint and segment manifest, and the
//! configuration records that register and rotate keys. It needs nothing else to verify.
//!
//! ```text
//! {dir}/
//!   bundle.cbor          {"format": 1, "created": µs, "partitions": [{"partition", "name", "origin",
//!                         "registry"?}], "scope": "backup" | "export" | "transfer"}
//!   keys.seg             the `key:` records of the signing config partition, a CBOR sequence
//!   withheld.cbor        what the bundle left out on purpose, by partition: [[offset, [part, …] | null], …]
//!   {partition as 16 hex digits}/
//!     partition.cbor, 00000000.seg, …      the `segments` layout (payloads.md §10)
//!     checkpoints/{size}.txt, manifests/{n}.txt
//! ```
//!
//! A bundle is itself a `SegmentStore` root, so it can be opened as a log; `export`
//! builds one from any `LogStore` by restoring every slot, and `Bundle::verify` runs
//! [`verify()`](crate::verify::verify) over every partition in it.
//!
//! **Scopes** (0103 §1; 0006 §9.2). A backup restores every slot as it is. An export withholds what
//! its reader may not read, as [`export_scoped`] is told: a part is left out as an erased
//! part is, its leaf kept so that every proof still verifies, and a whole record is
//! written as its leaf alone, as a compacted one is. Each is listed in `withheld.cbor`, so
//! that `verify` accounts for it, and what the list does not name still fails (0006 A30).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use scatter_log::cbor::{self, Value};
use scatter_log::record::Record;
use scatter_log::segments::SegmentStore;
use scatter_log::store::{LogStore, Slot, StoreError};

use crate::checkpoints::{CheckpointStore, CheckpointStoreError, FileCheckpoints};
use crate::keys::{ChainError, KeyChain};
use crate::verify::{KeyLookup, Level, PartNames, Report, Verify, verify};

/// The bundle format version.
pub const BUNDLE_FORMAT: u64 = 1;

const MANIFEST_FILE: &str = "bundle.cbor";
const KEYS_FILE: &str = "keys.seg";
const WITHHELD_FILE: &str = "withheld.cbor";

/// One exported partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundlePartition {
    /// The partition ID.
    pub partition: u64,
    /// The graph name, `local` or `mirror/wikidata`.
    pub name: String,
    /// The checkpoint origin line.
    pub origin: String,
    /// The partition's registry row, which a backup carries so that a restore registers
    /// the partition as it was (0103 A2); `None` in an export, and in a bundle written
    /// before backups carried it. `verify` does not need it.
    pub registry: Option<PartitionRegistry>,
}

/// A partition's registry row, as a backup carries it: what a restore needs to register
/// the partition again, beside its ID, name and origin. The segment exponent is the
/// partition's own `partition.cbor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionRegistry {
    /// The tenant, `None` for an instance partition.
    pub tenant: Option<String>,
    /// The graph IRI.
    pub graph_iri: String,
    /// The history policy: `full` or `latest`.
    pub history: String,
    /// The integrity: `logged` or `hashed`.
    pub integrity: String,
    /// The export policy: `public`, `internal` or `private`.
    pub export: String,
    /// The hash named in the genesis record.
    pub hash: String,
}

impl PartitionRegistry {
    fn to_value(&self) -> Value {
        let mut entries = Vec::new();
        if let Some(t) = &self.tenant {
            entries.push((Value::text("tenant"), Value::text(t)));
        }
        entries.extend([
            (Value::text("graph_iri"), Value::text(&self.graph_iri)),
            (Value::text("history"), Value::text(&self.history)),
            (Value::text("integrity"), Value::text(&self.integrity)),
            (Value::text("export"), Value::text(&self.export)),
            (Value::text("hash"), Value::text(&self.hash)),
        ]);
        Value::map(entries)
    }

    fn from_value(v: &Value) -> Result<Self, BundleError> {
        let text = |key: &'static str| {
            v.get(key)
                .and_then(Value::as_text)
                .map(str::to_string)
                .ok_or(BundleError::Manifest(key))
        };
        Ok(Self {
            tenant: v.get("tenant").and_then(Value::as_text).map(str::to_string),
            graph_iri: text("graph_iri")?,
            history: text("history")?,
            integrity: text("integrity")?,
            export: text("export")?,
            hash: text("hash")?,
        })
    }
}

/// What `bundle.cbor` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleManifest {
    /// When the bundle was written, microseconds since the epoch.
    pub created: u64,
    /// The partitions in it.
    pub partitions: Vec<BundlePartition>,
    /// Its scope (0103 §1): `backup`, `export` or `transfer`; `backup` when the manifest
    /// predates scopes.
    pub scope: String,
}

/// What an export leaves out of one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Withhold {
    /// These parts, by index; an index past the record's parts is ignored.
    Parts(Vec<usize>),
    /// Every part the record has.
    AllParts,
    /// The whole record, header included: written as its leaf alone.
    Record,
}

/// What a bundle left out of one partition: by offset, the parts, or `None` for a record
/// written as its leaf alone.
pub type WithheldList = BTreeMap<u64, Option<Vec<usize>>>;

impl BundleManifest {
    fn to_value(&self) -> Value {
        Value::map(vec![
            (Value::text("format"), Value::Int(i128::from(BUNDLE_FORMAT))),
            (Value::text("created"), Value::Int(i128::from(self.created))),
            (Value::text("scope"), Value::text(&self.scope)),
            (
                Value::text("partitions"),
                Value::Array(
                    self.partitions
                        .iter()
                        .map(|p| {
                            let mut entries = vec![
                                (
                                    Value::text("partition"),
                                    Value::Int(i128::from(p.partition)),
                                ),
                                (Value::text("name"), Value::text(&p.name)),
                                (Value::text("origin"), Value::text(&p.origin)),
                            ];
                            if let Some(r) = &p.registry {
                                entries.push((Value::text("registry"), r.to_value()));
                            }
                            Value::map(entries)
                        })
                        .collect(),
                ),
            ),
        ])
    }

    fn from_value(v: &Value) -> Result<Self, BundleError> {
        let bad = |what: &'static str| BundleError::Manifest(what);
        if v.get("format").and_then(Value::as_u64) != Some(BUNDLE_FORMAT) {
            return Err(bad("unsupported format"));
        }
        let created = v
            .get("created")
            .and_then(Value::as_u64)
            .ok_or(bad("created"))?;
        let Some(Value::Array(parts)) = v.get("partitions") else {
            return Err(bad("partitions"));
        };
        let partitions = parts
            .iter()
            .map(|p| {
                Ok(BundlePartition {
                    partition: p
                        .get("partition")
                        .and_then(Value::as_u64)
                        .ok_or(bad("partition"))?,
                    name: p
                        .get("name")
                        .and_then(Value::as_text)
                        .ok_or(bad("name"))?
                        .to_string(),
                    origin: p
                        .get("origin")
                        .and_then(Value::as_text)
                        .ok_or(bad("origin"))?
                        .to_string(),
                    registry: p
                        .get("registry")
                        .map(PartitionRegistry::from_value)
                        .transpose()?,
                })
            })
            .collect::<Result<_, BundleError>>()?;
        let scope = v
            .get("scope")
            .and_then(Value::as_text)
            .unwrap_or("backup")
            .to_string();
        Ok(Self {
            created,
            partitions,
            scope,
        })
    }
}

/// Why a bundle could not be written or opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BundleError {
    /// The file system failed.
    #[error("bundle: {0}")]
    Io(String),
    /// `bundle.cbor` is wrong.
    #[error("bundle: manifest: {0}")]
    Manifest(&'static str),
    /// `keys.seg` is not a sequence of key records, or the chain does not verify.
    #[error("bundle: keys: {0}")]
    Keys(String),
    /// The log refused.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The checkpoint store refused.
    #[error(transparent)]
    Checkpoints(#[from] CheckpointStoreError),
    /// The key chain is broken.
    #[error(transparent)]
    Chain(#[from] ChainError),
    /// The destination exists and is not empty.
    #[error("bundle: {0} is not empty")]
    NotEmpty(PathBuf),
}

impl From<std::io::Error> for BundleError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Writes a backup bundle of `partitions` from `store` and `checkpoints` to `dest`, which
/// must not exist or be empty: every slot as it is. `key_records` are the `key:` records
/// of the config partition that signs these partitions, in order.
pub async fn export<S: LogStore, C: CheckpointStore>(
    store: &S,
    checkpoints: &C,
    partitions: &[BundlePartition],
    key_records: &[Record],
    created: u64,
    dest: &Path,
) -> Result<(), BundleError> {
    export_scoped(
        store,
        checkpoints,
        partitions,
        key_records,
        created,
        dest,
        "backup",
        &BTreeMap::new(),
    )
    .await
}

/// Writes a bundle of `scope` (0103 §1), leaving out of each `(partition, offset)` what
/// `withhold` says and listing it in `withheld.cbor`; everything else as it is.
#[allow(clippy::too_many_arguments)]
pub async fn export_scoped<S: LogStore, C: CheckpointStore>(
    store: &S,
    checkpoints: &C,
    partitions: &[BundlePartition],
    key_records: &[Record],
    created: u64,
    dest: &Path,
    scope: &str,
    withhold: &BTreeMap<(u64, u64), Withhold>,
) -> Result<(), BundleError> {
    if dest.exists() && fs::read_dir(dest)?.next().is_some() {
        return Err(BundleError::NotEmpty(dest.to_path_buf()));
    }
    fs::create_dir_all(dest)?;
    let mut out = SegmentStore::open(dest)?;
    let mut cps = FileCheckpoints::new(dest);
    let mut left_out: BTreeMap<u64, WithheldList> = BTreeMap::new();
    for p in partitions {
        let head = store.head(p.partition).await?;
        out.create_partition(p.partition, head.segments).await?;
        for offset in 0..head.size {
            let slot = store.read(p.partition, offset).await?;
            match (withhold.get(&(p.partition, offset)), &slot) {
                (Some(Withhold::Record), _) => {
                    let leaf = slot.leaf();
                    out.append_slot(p.partition, Slot::Compacted { leaf })
                        .await?;
                    left_out
                        .entry(p.partition)
                        .or_default()
                        .insert(offset, None);
                }
                (Some(w @ (Withhold::Parts(_) | Withhold::AllParts)), Slot::Record(r)) => {
                    let present: Vec<usize> = r
                        .body()
                        .parts()
                        .iter()
                        .enumerate()
                        .filter(|(_, part)| !part.is_erased())
                        .map(|(i, _)| i)
                        .collect();
                    let parts: Vec<usize> = match w {
                        Withhold::Parts(want) => {
                            present.into_iter().filter(|i| want.contains(i)).collect()
                        }
                        _ => present,
                    };
                    out.append_slot(p.partition, slot.clone()).await?;
                    if !parts.is_empty() {
                        out.erase_parts(p.partition, offset, &parts).await?;
                        left_out
                            .entry(p.partition)
                            .or_default()
                            .insert(offset, Some(parts));
                    }
                }
                _ => {
                    out.append_slot(p.partition, slot).await?;
                }
            }
        }
        for cp in checkpoints.list(p.partition).await? {
            cps.put(p.partition, &cp).await?;
        }
        for (n, m) in checkpoints.manifests(p.partition).await? {
            cps.put_manifest(p.partition, n, &m).await?;
        }
    }
    let mut keys = Vec::new();
    for r in key_records {
        keys.extend_from_slice(&r.encode());
    }
    fs::write(dest.join(KEYS_FILE), keys)?;
    if !left_out.is_empty() {
        fs::write(
            dest.join(WITHHELD_FILE),
            cbor::encode(&withheld_to_value(&left_out)).expect("the list encodes"),
        )?;
    }
    let manifest = BundleManifest {
        created,
        partitions: partitions.to_vec(),
        scope: scope.to_string(),
    };
    fs::write(
        dest.join(MANIFEST_FILE),
        cbor::encode(&manifest.to_value()).expect("the manifest encodes"),
    )?;
    Ok(())
}

fn withheld_to_value(w: &BTreeMap<u64, WithheldList>) -> Value {
    Value::map(
        w.iter()
            .map(|(partition, list)| {
                (
                    Value::Int(i128::from(*partition)),
                    Value::Array(
                        list.iter()
                            .map(|(offset, parts)| {
                                Value::Array(vec![
                                    Value::Int(i128::from(*offset)),
                                    parts.as_ref().map_or(Value::Null, |ps| {
                                        Value::Array(
                                            ps.iter().map(|i| Value::Int(*i as i128)).collect(),
                                        )
                                    }),
                                ])
                            })
                            .collect(),
                    ),
                )
            })
            .collect(),
    )
}

fn withheld_from_value(v: &Value) -> Result<BTreeMap<u64, WithheldList>, BundleError> {
    let bad = || BundleError::Manifest("withheld list");
    let Value::Map(entries) = v else {
        return Err(bad());
    };
    let mut out = BTreeMap::new();
    for (k, list) in entries {
        let partition = k.as_u64().ok_or_else(bad)?;
        let Value::Array(items) = list else {
            return Err(bad());
        };
        let mut l = WithheldList::new();
        for item in items {
            let Value::Array(pair) = item else {
                return Err(bad());
            };
            let [offset, parts] = pair.as_slice() else {
                return Err(bad());
            };
            let offset = offset.as_u64().ok_or_else(bad)?;
            let parts = match parts {
                Value::Null => None,
                Value::Array(ps) => Some(
                    ps.iter()
                        .map(|p| {
                            p.as_u64()
                                .and_then(|n| usize::try_from(n).ok())
                                .ok_or_else(bad)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                _ => return Err(bad()),
            };
            l.insert(offset, parts);
        }
        out.insert(partition, l);
    }
    Ok(out)
}

/// An opened bundle.
#[derive(Debug)]
pub struct Bundle {
    dir: PathBuf,
    manifest: BundleManifest,
    store: SegmentStore,
    checkpoints: FileCheckpoints,
    chain: KeyChain,
    withheld: BTreeMap<u64, WithheldList>,
}

impl Bundle {
    /// Opens a bundle, indexing its partitions and checking its key chain.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, BundleError> {
        let dir = dir.into();
        let manifest = cbor::decode(&fs::read(dir.join(MANIFEST_FILE))?)
            .map_err(|_| BundleError::Manifest("not canonical CBOR"))?;
        let manifest = BundleManifest::from_value(&manifest)?;
        let keys = fs::read(dir.join(KEYS_FILE))?;
        let mut records = Vec::new();
        let mut pos = 0;
        while pos < keys.len() {
            let (v, len) =
                cbor::decode_prefix(&keys[pos..]).map_err(|e| BundleError::Keys(e.to_string()))?;
            records.push(Record::from_value(&v).map_err(|e| BundleError::Keys(e.to_string()))?);
            pos += len;
        }
        let chain = KeyChain::from_records(&records)?;
        let store = SegmentStore::open(&dir)?;
        let checkpoints = FileCheckpoints::new(&dir);
        let withheld = match fs::read(dir.join(WITHHELD_FILE)) {
            Ok(bytes) => withheld_from_value(
                &cbor::decode(&bytes).map_err(|_| BundleError::Manifest("withheld list"))?,
            )?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            dir,
            manifest,
            store,
            checkpoints,
            chain,
            withheld,
        })
    }

    /// The directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What `bundle.cbor` says.
    #[must_use]
    pub fn manifest(&self) -> &BundleManifest {
        &self.manifest
    }

    /// The partitions as a log.
    #[must_use]
    pub fn store(&self) -> &SegmentStore {
        &self.store
    }

    /// The checkpoints and manifests.
    #[must_use]
    pub fn checkpoints(&self) -> &FileCheckpoints {
        &self.checkpoints
    }

    /// The key chain from `keys.seg`.
    #[must_use]
    pub fn chain(&self) -> &KeyChain {
        &self.chain
    }

    /// What the bundle withheld on purpose, by partition (0006 A30).
    #[must_use]
    pub fn withheld(&self) -> &BTreeMap<u64, WithheldList> {
        &self.withheld
    }

    /// Verifies every partition in the bundle, one report each, in manifest order.
    pub async fn verify(
        &self,
        level: Level,
        actor_keys: Option<KeyLookup<'_>>,
        part_names: Option<PartNames<'_>>,
    ) -> Vec<Report> {
        let mut reports = Vec::new();
        for p in &self.manifest.partitions {
            let what = Verify {
                partition: p.partition,
                origin: &p.origin,
                chain: &self.chain,
                level,
                actor_keys,
                part_names,
                is_config: p.name == "config",
                withheld: self.withheld.get(&p.partition),
            };
            reports.push(verify(&self.store, &self.checkpoints, &what).await);
        }
        reports
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use scatter_log::body::ATTESTATION;
    use scatter_log::conformance::draft;
    use scatter_log::genesis::Genesis;
    use scatter_log::header::CONFIG_PARTITION;
    use scatter_log::memory::MemoryStore;
    use scatter_log::registry::{GraphRegistry, KeyEntry, Scope};
    use scatter_log::tree::Segments;

    use crate::checkpoint::{instance_origin, tenant_origin};
    use crate::checkpoints::{MemoryCheckpoints, checkpoint, segment_manifest};
    use crate::verify::Problem;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn export_open_verify() {
        pollster::block_on(async {
            let dest = std::env::temp_dir().join(format!("scatter-bundle-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dest);
            let key = SigningKey::from_bytes(&[21; 32]);
            let mut store = MemoryStore::new();
            Genesis {
                partition: CONFIG_PARTITION,
                scope: Scope::Instance,
                key: KeyEntry::ed25519(key.verifying_key().to_bytes()),
                segment_exponent: 16,
                appended_at: 1,
                founding: Value::map(Vec::new()),

                attestation: Value::map(vec![(
                    Value::text("actor"),
                    Value::text("instance:scatter"),
                )]),
                comment: None,
            }
            .append(&mut store, &GraphRegistry::embedded())
            .await
            .unwrap();
            let config_origin = instance_origin("farm.example", "config");
            let local_origin = tenant_origin("librarybase.org", "local");
            store
                .create_partition(7, Segments::new(1).unwrap())
                .await
                .unwrap();
            let mut cps = MemoryCheckpoints::new();
            for n in 0..5 {
                store.append(7, draft(n)).await.unwrap();
            }
            store.erase_parts(7, 1, &[ATTESTATION]).await.unwrap();
            store.compact(7, &[2]).await.unwrap();
            cps.put(
                7,
                &checkpoint(&store, 7, &local_origin, &key).await.unwrap(),
            )
            .await
            .unwrap();
            cps.put_manifest(
                7,
                0,
                &segment_manifest(&store, 7, 0, &local_origin, &key)
                    .await
                    .unwrap(),
            )
            .await
            .unwrap();
            cps.put(
                0,
                &checkpoint(&store, 0, &config_origin, &key).await.unwrap(),
            )
            .await
            .unwrap();
            let key_records = vec![store.read(0, 0).await.unwrap().record().unwrap().clone()];

            let partitions = vec![
                BundlePartition {
                    partition: 0,
                    name: "config".into(),
                    origin: config_origin.clone(),
                    registry: None,
                },
                BundlePartition {
                    partition: 7,
                    name: "local".into(),
                    origin: local_origin.clone(),
                    registry: Some(PartitionRegistry {
                        tenant: Some("wiki".into()),
                        graph_iri: "https://wiki.example/graph/local".into(),
                        history: "full".into(),
                        integrity: "logged".into(),
                        export: "public".into(),
                        hash: "sha-256".into(),
                    }),
                },
            ];
            export(&store, &cps, &partitions, &key_records, 123, &dest)
                .await
                .unwrap();
            assert!(dest.join("bundle.cbor").exists());
            assert!(dest.join("keys.seg").exists());
            assert!(dest.join("0000000000000007/00000002.seg").exists());
            assert!(
                dest.join("0000000000000007/checkpoints/00000000000000000005.txt")
                    .exists()
            );
            assert!(
                dest.join("0000000000000007/manifests/00000000.txt")
                    .exists()
            );
            assert!(matches!(
                export(&store, &cps, &partitions, &key_records, 123, &dest).await,
                Err(BundleError::NotEmpty(_))
            ));

            let bundle = Bundle::open(&dest).unwrap();
            assert_eq!(bundle.manifest().created, 123);
            assert_eq!(bundle.manifest().partitions, partitions);
            assert_eq!(bundle.chain().keys().len(), 1);
            assert_eq!(bundle.store().partitions().await.unwrap(), vec![0, 7]);
            assert_eq!(
                bundle.store().head(7).await.unwrap(),
                store.head(7).await.unwrap()
            );
            for offset in 0..5 {
                assert_eq!(
                    bundle.store().read(7, offset).await.unwrap(),
                    store.read(7, offset).await.unwrap()
                );
            }
            // The erased attestation at offset 1 has no erase record: the one problem.
            let reports = bundle.verify(Level::Bodies, None, None).await;
            assert_eq!(reports.len(), 2);
            assert!(reports[0].is_clean(), "{:?}", reports[0].problems);
            assert_eq!(
                reports[1].problems,
                vec![Problem::Erasure { offset: 1, part: 2 }]
            );
            assert_eq!(reports[1].compacted, 1);
            assert_eq!(reports[1].checkpoints, 1);
            assert_eq!(reports[1].manifests, 1);
            let reports = bundle.verify(Level::Structure, None, None).await;
            assert!(reports.iter().all(Report::is_clean));

            // An export-scope bundle (0103 §1; 0006 A30): what it withholds is listed and
            // accounted for, and verify still reports the unlisted erasure at offset 1.
            let scoped = dest.with_extension("scoped");
            let _ = fs::remove_dir_all(&scoped);
            let withhold: BTreeMap<(u64, u64), Withhold> = [
                ((7, 0), Withhold::AllParts),
                ((7, 4), Withhold::Parts(vec![scatter_log::body::COMMENT])),
                ((7, 3), Withhold::Record),
            ]
            .into_iter()
            .collect();
            export_scoped(
                &store,
                &cps,
                &partitions,
                &key_records,
                124,
                &scoped,
                "export",
                &withhold,
            )
            .await
            .unwrap();
            let bundle = Bundle::open(&scoped).unwrap();
            assert_eq!(bundle.manifest().scope, "export");
            let withheld_list = &bundle.withheld()[&7];
            assert_eq!(withheld_list.get(&3), Some(&None));
            assert_eq!(withheld_list.get(&4), Some(&Some(vec![1])));
            assert!(matches!(
                bundle.store().read(7, 3).await.unwrap(),
                Slot::Compacted { .. }
            ));
            let reports = bundle.verify(Level::Bodies, None, None).await;
            assert!(reports[0].is_clean(), "{:?}", reports[0].problems);
            assert_eq!(
                reports[1].problems,
                vec![Problem::Erasure { offset: 1, part: 2 }],
                "only the unlisted erasure"
            );
            assert_eq!(reports[1].withheld, 3);
            // Without its list, the withheld parts are unaccounted erasures.
            fs::remove_file(scoped.join("withheld.cbor")).unwrap();
            let reports = Bundle::open(&scoped)
                .unwrap()
                .verify(Level::Bodies, None, None)
                .await;
            assert!(reports[1].problems.len() > 1, "{:?}", reports[1].problems);
            fs::remove_dir_all(&scoped).unwrap();

            // Tampering with a body is caught on open (strict decoding) or by verify.
            let seg = dest.join("0000000000000007/00000000.seg");
            let mut bytes = fs::read(&seg).unwrap();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            fs::write(&seg, bytes).unwrap();
            assert!(Bundle::open(&dest).is_err());
            fs::remove_dir_all(&dest).unwrap();
        });
    }
}
