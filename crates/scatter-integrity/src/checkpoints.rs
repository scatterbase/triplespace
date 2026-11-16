//! Where signed checkpoints and segment manifests live, and how they are produced.
//!
//! Checkpoints are stored beside their partition, never in it (0006 §6); every one is
//! kept so consistency between any two can be proved. [`CheckpointStore`] is the
//! contract, with the same asynchronous shape as `LogStore`; [`FileCheckpoints`] keeps
//! them next to a `segments` partition directory:
//!
//! ```text
//! {root}/{partition as 16 hex digits}/
//!   checkpoints/{size as 20 digits}.txt
//!   manifests/{segment as 8 digits}.txt
//! ```
//!
//! [`checkpoint`] and [`segment_manifest`] read a partition's head or a sealed segment
//! from a `LogStore` and sign it with the instance key.

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use scatter_log::hash::Hash;
use scatter_log::store::{LogStore, StoreError};
use scatter_log::tree::root_of;

use crate::checkpoint::{Checkpoint, CheckpointError, SignedCheckpoint, segment_origin};
use crate::proof::{consistency_proof, inclusion_proof};

/// Why a checkpoint could not be stored, read or made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckpointStoreError {
    /// The backend's storage failed.
    #[error("checkpoints: {0}")]
    Storage(String),
    /// Stored bytes are not a signed checkpoint.
    #[error("checkpoints: {0}")]
    Checkpoint(#[from] CheckpointError),
    /// The log refused.
    #[error(transparent)]
    Log(#[from] StoreError),
    /// A segment that is not full yet has no manifest.
    #[error("checkpoints: segment {0} is not sealed")]
    NotSealed(u64),
}

impl From<std::io::Error> for CheckpointStoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

/// Signed checkpoints and segment manifests, per partition.
pub trait CheckpointStore: Send + Sync {
    /// Stores a checkpoint; one per size, a later one for the same size replaces it.
    fn put(
        &mut self,
        partition: u64,
        checkpoint: &SignedCheckpoint,
    ) -> impl Future<Output = Result<(), CheckpointStoreError>> + Send;

    /// Every checkpoint of a partition, by ascending size.
    fn list(
        &self,
        partition: u64,
    ) -> impl Future<Output = Result<Vec<SignedCheckpoint>, CheckpointStoreError>> + Send;

    /// The largest checkpoint.
    fn latest(
        &self,
        partition: u64,
    ) -> impl Future<Output = Result<Option<SignedCheckpoint>, CheckpointStoreError>> + Send {
        async move { Ok(self.list(partition).await?.pop()) }
    }

    /// Stores segment `n`'s manifest.
    fn put_manifest(
        &mut self,
        partition: u64,
        n: u64,
        manifest: &SignedCheckpoint,
    ) -> impl Future<Output = Result<(), CheckpointStoreError>> + Send;

    /// Every manifest of a partition, by segment.
    fn manifests(
        &self,
        partition: u64,
    ) -> impl Future<Output = Result<Vec<(u64, SignedCheckpoint)>, CheckpointStoreError>> + Send;
}

/// Checkpoints in memory.
#[derive(Debug, Clone, Default)]
pub struct MemoryCheckpoints {
    checkpoints: BTreeMap<u64, BTreeMap<u64, SignedCheckpoint>>,
    manifests: BTreeMap<u64, BTreeMap<u64, SignedCheckpoint>>,
}

impl MemoryCheckpoints {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl CheckpointStore for MemoryCheckpoints {
    async fn put(
        &mut self,
        partition: u64,
        checkpoint: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        self.checkpoints
            .entry(partition)
            .or_default()
            .insert(checkpoint.checkpoint.size, checkpoint.clone());
        Ok(())
    }

    async fn list(&self, partition: u64) -> Result<Vec<SignedCheckpoint>, CheckpointStoreError> {
        Ok(self
            .checkpoints
            .get(&partition)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default())
    }

    async fn put_manifest(
        &mut self,
        partition: u64,
        n: u64,
        manifest: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        self.manifests
            .entry(partition)
            .or_default()
            .insert(n, manifest.clone());
        Ok(())
    }

    async fn manifests(
        &self,
        partition: u64,
    ) -> Result<Vec<(u64, SignedCheckpoint)>, CheckpointStoreError> {
        Ok(self
            .manifests
            .get(&partition)
            .map(|m| m.iter().map(|(n, c)| (*n, c.clone())).collect())
            .unwrap_or_default())
    }
}

/// Checkpoints as files beside a `segments` partition directory.
#[derive(Debug, Clone)]
pub struct FileCheckpoints {
    root: PathBuf,
}

const CHECKPOINTS_DIR: &str = "checkpoints";
const MANIFESTS_DIR: &str = "manifests";

impl FileCheckpoints {
    /// A store under `root`, the same root as the `SegmentStore`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn partition_dir(&self, partition: u64) -> PathBuf {
        self.root.join(format!("{partition:016x}"))
    }

    fn write(path: &Path, checkpoint: &SignedCheckpoint) -> Result<(), CheckpointStoreError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, checkpoint.to_text())?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    fn read_dir(dir: &Path) -> Result<Vec<(u64, SignedCheckpoint)>, CheckpointStoreError> {
        let mut out = Vec::new();
        if !dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".txt"))
            else {
                continue;
            };
            let Ok(n) = stem.parse::<u64>() else {
                continue;
            };
            out.push((n, SignedCheckpoint::parse(&fs::read(&path)?)?));
        }
        out.sort_by_key(|(n, _)| *n);
        Ok(out)
    }
}

impl CheckpointStore for FileCheckpoints {
    async fn put(
        &mut self,
        partition: u64,
        checkpoint: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        let path = self
            .partition_dir(partition)
            .join(CHECKPOINTS_DIR)
            .join(format!("{:020}.txt", checkpoint.checkpoint.size));
        Self::write(&path, checkpoint)
    }

    async fn list(&self, partition: u64) -> Result<Vec<SignedCheckpoint>, CheckpointStoreError> {
        Ok(
            Self::read_dir(&self.partition_dir(partition).join(CHECKPOINTS_DIR))?
                .into_iter()
                .map(|(_, c)| c)
                .collect(),
        )
    }

    async fn put_manifest(
        &mut self,
        partition: u64,
        n: u64,
        manifest: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        let path = self
            .partition_dir(partition)
            .join(MANIFESTS_DIR)
            .join(format!("{n:08}.txt"));
        Self::write(&path, manifest)
    }

    async fn manifests(
        &self,
        partition: u64,
    ) -> Result<Vec<(u64, SignedCheckpoint)>, CheckpointStoreError> {
        Self::read_dir(&self.partition_dir(partition).join(MANIFESTS_DIR))
    }
}

/// The leaves of offsets `range` of a partition, compacted ones included.
pub async fn leaves<S: LogStore>(
    store: &S,
    partition: u64,
    range: std::ops::Range<u64>,
) -> Result<Vec<Hash>, StoreError> {
    let mut out = Vec::with_capacity(usize::try_from(range.end - range.start).unwrap_or(0));
    for offset in range {
        out.push(store.read(partition, offset).await?.leaf());
    }
    Ok(out)
}

/// Signs the partition's current head under `origin` with the instance key.
pub async fn checkpoint<S: LogStore>(
    store: &S,
    partition: u64,
    origin: &str,
    key: &SigningKey,
) -> Result<SignedCheckpoint, CheckpointStoreError> {
    let head = store.head(partition).await?;
    Ok(Checkpoint {
        origin: origin.to_string(),
        size: head.size,
        root: head.root,
    }
    .sign(key)?)
}

/// Signs the manifest of sealed segment `n`: its own tree head, under
/// `{origin}/segment/{n}` (0006 §5–6). The segment must be full.
pub async fn segment_manifest<S: LogStore>(
    store: &S,
    partition: u64,
    n: u64,
    origin: &str,
    key: &SigningKey,
) -> Result<SignedCheckpoint, CheckpointStoreError> {
    let head = store.head(partition).await?;
    if !head.segments.is_full(n, head.size) {
        return Err(CheckpointStoreError::NotSealed(n));
    }
    let range = head.segments.range(n);
    let seg_leaves = leaves(store, partition, range).await?;
    Ok(Checkpoint {
        origin: segment_origin(origin, n),
        size: head.segments.len(),
        root: root_of(&seg_leaves),
    }
    .sign(key)?)
}

/// The leaf at `index` and its inclusion proof against the tree of `size` leaves.
pub async fn inclusion<S: LogStore>(
    store: &S,
    partition: u64,
    index: u64,
    size: u64,
) -> Result<(Hash, Vec<Hash>), StoreError> {
    let all = leaves(store, partition, 0..size).await?;
    let leaf = all
        .get(usize::try_from(index).unwrap_or(usize::MAX))
        .copied();
    let leaf = leaf.ok_or(StoreError::NoOffset {
        partition,
        offset: index,
    })?;
    Ok((leaf, inclusion_proof(&all, index)))
}

/// The consistency proof between the trees of `first` and `second` leaves.
pub async fn consistency<S: LogStore>(
    store: &S,
    partition: u64,
    first: u64,
    second: u64,
) -> Result<Vec<Hash>, StoreError> {
    let all = leaves(store, partition, 0..second).await?;
    Ok(consistency_proof(&all, first, second))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_log::conformance::draft;
    use scatter_log::memory::MemoryStore;
    use scatter_log::segments::SegmentStore;
    use scatter_log::tree::Segments;

    use crate::checkpoint::tenant_origin;
    use crate::note::NoteVerifier;
    use crate::proof::{verify_consistency, verify_inclusion};

    #[test]
    fn checkpoints_manifests_and_proofs() {
        pollster::block_on(async {
            let key = SigningKey::from_bytes(&[5; 32]);
            let verifier =
                |origin: &str| NoteVerifier::new(origin, key.verifying_key().to_bytes()).unwrap();
            let origin = tenant_origin("librarybase.org", "local");
            let mut store = MemoryStore::new();
            store
                .create_partition(7, Segments::new(2).unwrap())
                .await
                .unwrap();
            let mut cps = MemoryCheckpoints::new();

            let empty = checkpoint(&store, 7, &origin, &key).await.unwrap();
            assert_eq!(empty.checkpoint.size, 0);
            cps.put(7, &empty).await.unwrap();
            let mut appended = Vec::new();
            for n in 0..10 {
                appended.push(store.append(7, draft(n)).await.unwrap());
                if n % 3 == 2 {
                    cps.put(7, &checkpoint(&store, 7, &origin, &key).await.unwrap())
                        .await
                        .unwrap();
                }
            }
            let list = cps.list(7).await.unwrap();
            assert_eq!(
                list.iter().map(|c| c.checkpoint.size).collect::<Vec<_>>(),
                vec![0, 3, 6, 9]
            );
            let latest = cps.latest(7).await.unwrap().unwrap();
            assert_eq!(latest.checkpoint.size, 9);
            assert_eq!(latest.checkpoint.root, appended[8].root);
            latest.verify(&origin, &[verifier(&origin)]).unwrap();

            // Inclusion of offset 4 against the checkpoint at 9, and consistency 3 → 9.
            let (leaf, proof) = inclusion(&store, 7, 4, 9).await.unwrap();
            assert_eq!(leaf, appended[4].leaf);
            verify_inclusion(&leaf, 4, 9, &proof, &latest.checkpoint.root).unwrap();
            let proof = consistency(&store, 7, 3, 9).await.unwrap();
            verify_consistency(
                3,
                9,
                &list[1].checkpoint.root,
                &latest.checkpoint.root,
                &proof,
            )
            .unwrap();

            // Manifests for the two full segments of four; the third is not sealed.
            let m0 = segment_manifest(&store, 7, 0, &origin, &key).await.unwrap();
            assert_eq!(m0.checkpoint.origin, "librarybase.org/log/local/segment/0");
            assert_eq!(m0.checkpoint.size, 4);
            assert_eq!(
                m0.checkpoint.root,
                root_of(&appended[..4].iter().map(|a| a.leaf).collect::<Vec<_>>())
            );
            m0.verify(&m0.checkpoint.origin, &[verifier(&m0.checkpoint.origin)])
                .unwrap();
            cps.put_manifest(7, 0, &m0).await.unwrap();
            cps.put_manifest(
                7,
                1,
                &segment_manifest(&store, 7, 1, &origin, &key).await.unwrap(),
            )
            .await
            .unwrap();
            assert!(matches!(
                segment_manifest(&store, 7, 2, &origin, &key).await,
                Err(CheckpointStoreError::NotSealed(2))
            ));
            assert_eq!(
                cps.manifests(7)
                    .await
                    .unwrap()
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>(),
                vec![0, 1]
            );
        });
    }

    #[test]
    fn file_checkpoints_live_beside_the_partition() {
        pollster::block_on(async {
            let root =
                std::env::temp_dir().join(format!("scatter-integrity-cps-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let key = SigningKey::from_bytes(&[6; 32]);
            let mut store = SegmentStore::open(&root).unwrap();
            store
                .create_partition(0x42, Segments::new(1).unwrap())
                .await
                .unwrap();
            for n in 0..4 {
                store.append(0x42, draft(n)).await.unwrap();
            }
            let mut cps = FileCheckpoints::new(&root);
            let cp = checkpoint(&store, 0x42, "h/log/local", &key).await.unwrap();
            cps.put(0x42, &cp).await.unwrap();
            let m1 = segment_manifest(&store, 0x42, 1, "h/log/local", &key)
                .await
                .unwrap();
            cps.put_manifest(0x42, 1, &m1).await.unwrap();
            assert!(
                root.join("0000000000000042/checkpoints/00000000000000000004.txt")
                    .exists()
            );
            assert!(
                root.join("0000000000000042/manifests/00000001.txt")
                    .exists()
            );
            let again = FileCheckpoints::new(&root);
            assert_eq!(again.list(0x42).await.unwrap(), vec![cp.clone()]);
            assert_eq!(again.latest(0x42).await.unwrap(), Some(cp));
            assert_eq!(again.manifests(0x42).await.unwrap(), vec![(1, m1)]);
            assert!(again.list(0x43).await.unwrap().is_empty());
            fs::write(
                root.join("0000000000000042/checkpoints/00000000000000000005.txt"),
                "junk",
            )
            .unwrap();
            assert!(matches!(
                again.list(0x42).await,
                Err(CheckpointStoreError::Checkpoint(_))
            ));
            fs::remove_dir_all(&root).unwrap();
        });
    }
}
