//! Export bundles (0006 §9): a directory that holds, for each exported partition, its
//! segments with headers and bodies, every checkpoint and segment manifest, and the
//! configuration records that register and rotate keys. It needs nothing else to verify.
//!
//! ```text
//! {dir}/
//!   bundle.cbor          {"format": 1, "created": µs, "partitions": [{"partition", "name", "origin"}]}
//!   keys.seg             the `key:` records of the signing config partition, a CBOR sequence
//!   {partition as 16 hex digits}/
//!     partition.cbor, 00000000.seg, …      the `segments` layout (payloads.md §10)
//!     checkpoints/{size}.txt, manifests/{n}.txt
//! ```
//!
//! A bundle is itself a `SegmentStore` root, so it can be opened as a log; `export`
//! builds one from any `LogStore` by restoring every slot, and `Bundle::verify` runs
//! [`verify()`](crate::verify::verify) over every partition in it.

use std::fs;
use std::path::{Path, PathBuf};

use scatter_log::cbor::{self, Value};
use scatter_log::record::Record;
use scatter_log::segments::SegmentStore;
use scatter_log::store::{LogStore, StoreError};

use crate::checkpoints::{CheckpointStore, CheckpointStoreError, FileCheckpoints};
use crate::keys::{ChainError, KeyChain};
use crate::verify::{KeyLookup, Level, PartNames, Report, Verify, verify};

/// The bundle format version.
pub const BUNDLE_FORMAT: u64 = 1;

const MANIFEST_FILE: &str = "bundle.cbor";
const KEYS_FILE: &str = "keys.seg";

/// One exported partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundlePartition {
    /// The partition ID.
    pub partition: u64,
    /// The graph name, `local` or `mirror/wikidata`.
    pub name: String,
    /// The checkpoint origin line.
    pub origin: String,
}

/// What `bundle.cbor` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleManifest {
    /// When the bundle was written, microseconds since the epoch.
    pub created: u64,
    /// The partitions in it.
    pub partitions: Vec<BundlePartition>,
}

impl BundleManifest {
    fn to_value(&self) -> Value {
        Value::map(vec![
            (Value::text("format"), Value::Int(i128::from(BUNDLE_FORMAT))),
            (Value::text("created"), Value::Int(i128::from(self.created))),
            (
                Value::text("partitions"),
                Value::Array(
                    self.partitions
                        .iter()
                        .map(|p| {
                            Value::map(vec![
                                (
                                    Value::text("partition"),
                                    Value::Int(i128::from(p.partition)),
                                ),
                                (Value::text("name"), Value::text(&p.name)),
                                (Value::text("origin"), Value::text(&p.origin)),
                            ])
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
                })
            })
            .collect::<Result<_, BundleError>>()?;
        Ok(Self {
            created,
            partitions,
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

/// Writes a bundle of `partitions` from `store` and `checkpoints` to `dest`, which must
/// not exist or be empty. `key_records` are the `key:` records of the config partition
/// that signs these partitions, in order.
pub async fn export<S: LogStore, C: CheckpointStore>(
    store: &S,
    checkpoints: &C,
    partitions: &[BundlePartition],
    key_records: &[Record],
    created: u64,
    dest: &Path,
) -> Result<(), BundleError> {
    if dest.exists() && fs::read_dir(dest)?.next().is_some() {
        return Err(BundleError::NotEmpty(dest.to_path_buf()));
    }
    fs::create_dir_all(dest)?;
    let mut out = SegmentStore::open(dest)?;
    let mut cps = FileCheckpoints::new(dest);
    for p in partitions {
        let head = store.head(p.partition).await?;
        out.create_partition(p.partition, head.segments).await?;
        for offset in 0..head.size {
            let slot = store.read(p.partition, offset).await?;
            out.append_slot(p.partition, slot).await?;
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
    let manifest = BundleManifest {
        created,
        partitions: partitions.to_vec(),
    };
    fs::write(
        dest.join(MANIFEST_FILE),
        cbor::encode(&manifest.to_value()).expect("the manifest encodes"),
    )?;
    Ok(())
}

/// An opened bundle.
#[derive(Debug)]
pub struct Bundle {
    dir: PathBuf,
    manifest: BundleManifest,
    store: SegmentStore,
    checkpoints: FileCheckpoints,
    chain: KeyChain,
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
        Ok(Self {
            dir,
            manifest,
            store,
            checkpoints,
            chain,
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
                },
                BundlePartition {
                    partition: 7,
                    name: "local".into(),
                    origin: local_origin.clone(),
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
