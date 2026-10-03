//! The `segments` file backend (0013 §1; 0005 §4.2): one directory per partition, one
//! file per segment, and the format export bundles use (0006 §9).
//!
//! ```text
//! {root}/
//!   {partition as 16 hex digits}/
//!     partition.cbor        {"format": 1, "partition": …, "segment_exponent": k}
//!     00000000.seg          a CBOR sequence (RFC 8742) of slots, one per offset
//!     00000001.seg
//! ```
//!
//! A slot is the record `[header, body]`, or, for a compacted offset, its 32-byte
//! Merkle leaf as a byte string. Segment `n` holds offsets `[n·2^k, (n+1)·2^k)`;
//! appends go to the end of the last file, and an erasure or compaction rewrites the
//! one file it touches, through a temporary file and a rename. Every file is
//! canonical CBOR, so a bundle verifies with nothing but this crate.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use crate::cbor::{self, Value};
use crate::hash::Hash;
use crate::record::Record;
use crate::store::{Appended, Draft, Head, LogStore, Slot, StoreError, check_restorable};
use crate::tree::{Frontier, Segments};

/// The file format version written to `partition.cbor`.
pub const FILE_FORMAT: u64 = 1;

const PARTITION_FILE: &str = "partition.cbor";
const SEGMENT_SUFFIX: &str = ".seg";

/// Where each slot of a segment file starts, and where the file ends.
#[derive(Debug, Clone, Default)]
struct SegmentIndex {
    starts: Vec<u64>,
    end: u64,
}

#[derive(Debug, Clone)]
struct Partition {
    dir: PathBuf,
    segments: Segments,
    files: Vec<SegmentIndex>,
    frontier: Frontier,
}

/// The file backend.
#[derive(Debug)]
pub struct SegmentStore {
    root: PathBuf,
    partitions: BTreeMap<u64, Partition>,
}

fn corrupt(msg: impl Into<String>) -> StoreError {
    StoreError::Corrupt(msg.into())
}

fn partition_dir_name(partition: u64) -> String {
    format!("{partition:016x}")
}

fn segment_file_name(n: u64) -> String {
    format!("{n:08}{SEGMENT_SUFFIX}")
}

fn decode_slot(bytes: &[u8]) -> Result<Slot, StoreError> {
    match cbor::decode(bytes).map_err(|e| corrupt(e.to_string()))? {
        Value::Bytes(leaf) => {
            let leaf: Hash = leaf
                .as_slice()
                .try_into()
                .map_err(|_| corrupt("a compacted slot is a 32-byte leaf"))?;
            Ok(Slot::Compacted { leaf })
        }
        v => Ok(Slot::Record(Record::from_value(&v)?)),
    }
}

fn encode_slot(slot: &Slot) -> Vec<u8> {
    match slot {
        Slot::Record(r) => r.encode(),
        Slot::Compacted { leaf } => {
            cbor::encode(&Value::Bytes(leaf.to_vec())).expect("bytes encode")
        }
    }
}

/// Writes `bytes` to `path` through a temporary file and a rename.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

impl SegmentStore {
    /// Opens a store at `root`, creating the directory if it is missing, and indexes
    /// every partition in it.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let mut partitions = BTreeMap::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|n| u64::from_str_radix(n, 16).ok()) else {
                continue;
            };
            if name.len() != 16 {
                continue;
            }
            partitions.insert(id, Self::load_partition(id, entry.path())?);
        }
        Ok(Self { root, partitions })
    }

    /// The directory the store lives in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of a partition.
    #[must_use]
    pub fn partition_dir(&self, partition: u64) -> PathBuf {
        self.root.join(partition_dir_name(partition))
    }

    fn load_partition(id: u64, dir: PathBuf) -> Result<Partition, StoreError> {
        let meta = fs::read(dir.join(PARTITION_FILE))?;
        let meta = cbor::decode(&meta).map_err(|e| corrupt(format!("{PARTITION_FILE}: {e}")))?;
        let Value::Map(pairs) = &meta else {
            return Err(corrupt(format!("{PARTITION_FILE}: not a map")));
        };
        let field = |name: &str| match pairs.iter().find(|(segments, _)| segments.text_eq(name)) {
            Some((_, Value::Int(i))) => u64::try_from(*i).ok(),
            _ => None,
        };
        if field("format") != Some(FILE_FORMAT) {
            return Err(corrupt(format!("{PARTITION_FILE}: unsupported format")));
        }
        if field("partition") != Some(id) {
            return Err(corrupt(format!(
                "{PARTITION_FILE}: partition does not match its directory"
            )));
        }
        let segments = field("segment_exponent")
            .and_then(|segments| u8::try_from(segments).ok())
            .and_then(Segments::new)
            .ok_or_else(|| corrupt(format!("{PARTITION_FILE}: segment_exponent")))?;
        let mut p = Partition {
            dir,
            segments,
            files: Vec::new(),
            frontier: Frontier::new(),
        };
        let mut n = 0;
        loop {
            let path = p.dir.join(segment_file_name(n));
            if !path.exists() {
                break;
            }
            let bytes = fs::read(&path)?;
            let mut index = SegmentIndex::default();
            let mut pos = 0usize;
            while pos < bytes.len() {
                let (value, len) = cbor::decode_prefix(&bytes[pos..])
                    .map_err(|e| corrupt(format!("{}: at byte {pos}: {e}", path.display())))?;
                let offset = p.frontier.size();
                let leaf = match value {
                    Value::Bytes(leaf) => leaf
                        .as_slice()
                        .try_into()
                        .map_err(|_| corrupt("a compacted slot is a 32-byte leaf"))?,
                    value => {
                        let record = Record::from_value(&value)?;
                        if record.header().partition != id || record.header().offset != offset {
                            return Err(corrupt(format!(
                                "{}: expected partition {id} offset {offset}, found partition {} offset {}",
                                path.display(),
                                record.header().partition,
                                record.header().offset
                            )));
                        }
                        record.leaf()
                    }
                };
                index.starts.push(pos as u64);
                p.frontier.push_leaf(leaf);
                pos += len;
            }
            index.end = pos as u64;
            if index.starts.len() as u64 > segments.len() {
                return Err(corrupt(format!(
                    "{}: more than 2^segments slots",
                    path.display()
                )));
            }
            p.files.push(index);
            n += 1;
        }
        // Every file but the last is full.
        if let Some((_, full)) = p.files.split_last()
            && full.iter().any(|f| f.starts.len() as u64 != segments.len())
        {
            return Err(corrupt("a segment file before the last one is not full"));
        }
        Ok(p)
    }

    fn partition(&self, partition: u64) -> Result<&Partition, StoreError> {
        self.partitions
            .get(&partition)
            .ok_or(StoreError::NoPartition(partition))
    }

    fn partition_mut(&mut self, partition: u64) -> Result<&mut Partition, StoreError> {
        self.partitions
            .get_mut(&partition)
            .ok_or(StoreError::NoPartition(partition))
    }
}

impl Partition {
    /// Appends one encoded slot with its leaf at the next offset.
    fn append_bytes(&mut self, bytes: &[u8], leaf: Hash) -> Result<Appended, StoreError> {
        let offset = self.frontier.size();
        let n = self.segments.segment_of(offset);
        let new_file = offset == self.segments.range(n).start;
        let path = self.dir.join(segment_file_name(n));
        if new_file {
            if path.exists() {
                return Err(corrupt(format!(
                    "{} exists before its first append",
                    path.display()
                )));
            }
            self.files.push(SegmentIndex::default());
        }
        let index = self.files.last_mut().expect("pushed");
        let mut f = OpenOptions::new()
            .append(true)
            .create(new_file)
            .open(&path)?;
        let start = f.seek(SeekFrom::End(0))?;
        if start != index.end {
            return Err(corrupt("a segment file changed underneath the store"));
        }
        f.write_all(bytes)?;
        f.sync_data()?;
        index.starts.push(start);
        index.end = start + bytes.len() as u64;
        self.frontier.push_leaf(leaf);
        Ok(Appended {
            offset,
            leaf,
            root: self.frontier.root(),
        })
    }

    fn locate(&self, partition: u64, offset: u64) -> Result<(u64, usize), StoreError> {
        if offset >= self.frontier.size() {
            return Err(StoreError::NoOffset { partition, offset });
        }
        let n = self.segments.segment_of(offset);
        let i = usize::try_from(offset - self.segments.range(n).start).expect("fits");
        Ok((n, i))
    }

    fn read_slot_bytes(&self, n: u64, i: usize) -> Result<Vec<u8>, StoreError> {
        let index = &self.files[usize::try_from(n).expect("fits")];
        let start = index.starts[i];
        let end = index.starts.get(i + 1).copied().unwrap_or(index.end);
        let mut f = File::open(self.dir.join(segment_file_name(n)))?;
        f.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0; usize::try_from(end - start).expect("fits")];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn read_slot(&self, n: u64, i: usize) -> Result<Slot, StoreError> {
        decode_slot(&self.read_slot_bytes(n, i)?)
    }

    /// All the slots of segment file `n`, as their bytes.
    fn read_segment(&self, n: u64) -> Result<Vec<Vec<u8>>, StoreError> {
        let index = &self.files[usize::try_from(n).expect("fits")];
        let bytes = fs::read(self.dir.join(segment_file_name(n)))?;
        if bytes.len() as u64 != index.end {
            return Err(corrupt("a segment file changed underneath the store"));
        }
        Ok(index
            .starts
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let e = index.starts.get(i + 1).copied().unwrap_or(index.end);
                bytes[usize::try_from(s).expect("fits")..usize::try_from(e).expect("fits")].to_vec()
            })
            .collect())
    }

    /// Replaces segment file `n` with these slots.
    fn write_segment(&mut self, n: u64, slots: &[Vec<u8>]) -> Result<(), StoreError> {
        let mut index = SegmentIndex::default();
        let mut bytes = Vec::new();
        for s in slots {
            index.starts.push(bytes.len() as u64);
            bytes.extend_from_slice(s);
        }
        index.end = bytes.len() as u64;
        write_atomically(&self.dir.join(segment_file_name(n)), &bytes)?;
        self.files[usize::try_from(n).expect("fits")] = index;
        Ok(())
    }

    /// Rewrites one slot of segment `n`.
    fn rewrite(
        &mut self,
        n: u64,
        i: usize,
        change: impl FnOnce(Slot) -> Result<Slot, StoreError>,
    ) -> Result<(), StoreError> {
        let mut slots = self.read_segment(n)?;
        let new = change(decode_slot(&slots[i])?)?;
        slots[i] = encode_slot(&new);
        self.write_segment(n, &slots)
    }
}

impl LogStore for SegmentStore {
    async fn create_partition(
        &mut self,
        partition: u64,
        segments: Segments,
    ) -> Result<(), StoreError> {
        if self.partitions.contains_key(&partition) {
            return Err(StoreError::PartitionExists(partition));
        }
        let dir = self.partition_dir(partition);
        if dir.exists() {
            return Err(StoreError::PartitionExists(partition));
        }
        fs::create_dir_all(&dir)?;
        let meta = Value::map(vec![
            (Value::text("format"), Value::Int(i128::from(FILE_FORMAT))),
            (Value::text("partition"), Value::Int(i128::from(partition))),
            (
                Value::text("segment_exponent"),
                Value::Int(i128::from(segments.exponent())),
            ),
        ]);
        write_atomically(
            &dir.join(PARTITION_FILE),
            &cbor::encode(&meta).expect("the manifest encodes"),
        )?;
        self.partitions.insert(
            partition,
            Partition {
                dir,
                segments,
                files: Vec::new(),
                frontier: Frontier::new(),
            },
        );
        Ok(())
    }

    async fn partitions(&self) -> Result<Vec<u64>, StoreError> {
        Ok(self.partitions.keys().copied().collect())
    }

    async fn head(&self, partition: u64) -> Result<Head, StoreError> {
        let p = self.partition(partition)?;
        Ok(Head {
            size: p.frontier.size(),
            root: p.frontier.root(),
            segments: p.segments,
        })
    }

    async fn append(&mut self, partition: u64, draft: Draft) -> Result<Appended, StoreError> {
        let p = self.partition_mut(partition)?;
        let offset = p.frontier.size();
        let record = draft.seal(partition, offset);
        let leaf = record.leaf();
        p.append_bytes(&record.encode(), leaf)
    }

    async fn append_slot(&mut self, partition: u64, slot: Slot) -> Result<Appended, StoreError> {
        let p = self.partition_mut(partition)?;
        check_restorable(&slot, partition, p.frontier.size())?;
        let leaf = slot.leaf();
        p.append_bytes(&encode_slot(&slot), leaf)
    }

    async fn read(&self, partition: u64, offset: u64) -> Result<Slot, StoreError> {
        let p = self.partition(partition)?;
        let (n, i) = p.locate(partition, offset)?;
        p.read_slot(n, i)
    }

    async fn scan(
        &self,
        partition: u64,
        from: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Record)>, StoreError> {
        let p = self.partition(partition)?;
        let mut out = Vec::new();
        if limit == 0 || from >= p.frontier.size() {
            return Ok(out);
        }
        let (mut n, mut i) = p.locate(partition, from)?;
        let mut offset = from;
        'files: while let Some(index) = p.files.get(usize::try_from(n).expect("fits")) {
            let slots = p.read_segment(n)?;
            while i < index.starts.len() {
                if let Slot::Record(r) = decode_slot(&slots[i])? {
                    out.push((offset, r));
                    if out.len() == limit {
                        break 'files;
                    }
                }
                i += 1;
                offset += 1;
            }
            n += 1;
            i = 0;
        }
        Ok(out)
    }

    async fn erase_parts(
        &mut self,
        partition: u64,
        offset: u64,
        parts: &[usize],
    ) -> Result<(), StoreError> {
        let p = self.partition_mut(partition)?;
        let (n, i) = p.locate(partition, offset)?;
        p.rewrite(n, i, |slot| match slot {
            Slot::Record(mut r) => {
                for &part in parts {
                    r.body_mut().erase(part)?;
                }
                Ok(Slot::Record(r))
            }
            Slot::Compacted { .. } => Err(StoreError::Compacted { partition, offset }),
        })
    }

    async fn compact(&mut self, partition: u64, offsets: &[u64]) -> Result<(), StoreError> {
        let p = self.partition_mut(partition)?;
        // Group by segment file so each is rewritten once.
        let mut by_file: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
        for &offset in offsets {
            let (n, i) = p.locate(partition, offset)?;
            by_file.entry(n).or_default().push(i);
        }
        for (n, indexes) in by_file {
            let mut slots = p.read_segment(n)?;
            for i in indexes {
                if let Slot::Record(r) = decode_slot(&slots[i])? {
                    slots[i] = encode_slot(&Slot::Compacted { leaf: r.leaf() });
                }
            }
            p.write_segment(n, &slots)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scatter-log-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn segment_store_conforms() {
        let root = temp_root("conformance");
        let counter = std::cell::Cell::new(0u32);
        let make = || {
            counter.set(counter.get() + 1);
            SegmentStore::open(root.join(counter.get().to_string())).unwrap()
        };
        pollster::block_on(conformance::run(make));
        pollster::block_on(conformance::run_reopen(make, |s| {
            let r = s.root().to_path_buf();
            drop(s);
            SegmentStore::open(r).unwrap()
        }));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn layout_on_disk() {
        pollster::block_on(async {
            let root = temp_root("layout");
            let mut s = SegmentStore::open(&root).unwrap();
            s.create_partition(0x1234, Segments::new(1).unwrap())
                .await
                .unwrap();
            for n in 0..3 {
                s.append(0x1234, conformance::draft(n)).await.unwrap();
            }
            let dir = root.join("0000000000001234");
            assert!(dir.join("partition.cbor").exists());
            assert!(dir.join("00000000.seg").exists());
            assert!(dir.join("00000001.seg").exists());
            assert!(!dir.join("00000002.seg").exists());
            // The first file is two records, each a canonical [header, body].
            let bytes = fs::read(dir.join("00000000.seg")).unwrap();
            let (first, len) = cbor::decode_prefix(&bytes).unwrap();
            let r = Record::from_value(&first).unwrap();
            assert_eq!(r.header().offset, 0);
            let (second, len2) = cbor::decode_prefix(&bytes[len..]).unwrap();
            assert_eq!(Record::from_value(&second).unwrap().header().offset, 1);
            assert_eq!(len + len2, bytes.len());
            // Compaction leaves a 32-byte leaf in the file.
            s.compact(0x1234, &[1]).await.unwrap();
            let bytes = fs::read(dir.join("00000000.seg")).unwrap();
            let (_, len) = cbor::decode_prefix(&bytes).unwrap();
            assert_eq!(&bytes[len..len + 2], &[0x58, 0x20]);
            assert_eq!(bytes.len(), len + 34);
            // A damaged file is refused on open.
            fs::write(dir.join("00000001.seg"), b"\x80").unwrap();
            assert!(matches!(
                SegmentStore::open(&root),
                Err(StoreError::Corrupt(_) | StoreError::Record(_))
            ));
            fs::remove_dir_all(&root).unwrap();
        });
    }
}
