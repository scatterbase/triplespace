//! The `LogStore` contract (0005 §4.2; 0013 §1): `append`, `read`, `scan`,
//! `erase_parts`, `compact` and `head`, over partitions of gapless offsets.
//!
//! The store owns the header fields it alone can fill — the partition, the offset and
//! the body commitment — and takes the rest from a [`Draft`]: the time, the payload
//! type, the key and the global IDs, which the appending layer assigns in the same
//! transaction (0015 §2). Offsets are never reused: an erased record keeps its header,
//! and a compacted one leaves a hole that keeps only its Merkle leaf, which `read`
//! reports as [`Slot::Compacted`], so the tree over every offset still folds.
//!
//! The two backends are [`MemoryStore`](crate::memory::MemoryStore) and the
//! [`SegmentStore`](crate::segments::SegmentStore) file backend here, and
//! `scatter-log-postgres`. All of them pass [`conformance`](crate::conformance).

use crate::body::{Body, BodyError};
use crate::hash::Hash;
use crate::header::{FORMAT_VERSION, Header, HeaderError};
use crate::record::{Record, RecordError};
use crate::tree::Segments;

/// What a store is asked to append: everything in a header but what the store fills in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// Header field 3: microseconds since the epoch.
    pub appended_at: u64,
    /// Header field 4.
    pub payload_type: String,
    /// Header field 5.
    pub key: Option<String>,
    /// Header field 7.
    pub revid: Option<u64>,
    /// Header field 8.
    pub logid: Option<u64>,
    /// Header field 9.
    pub page_id: Option<u64>,
    /// The body, whose commitment becomes header field 6.
    pub body: Body,
}

impl Draft {
    /// The record this draft becomes at `offset` of `partition`.
    #[must_use]
    pub fn seal(self, partition: u64, offset: u64) -> Record {
        let header = Header {
            version: FORMAT_VERSION,
            partition,
            offset,
            appended_at: self.appended_at,
            payload_type: self.payload_type,
            key: self.key,
            commitment: [0; 32],
            revid: self.revid,
            logid: self.logid,
            page_id: self.page_id,
        };
        Record::seal(header, self.body)
    }
}

/// What `append` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appended {
    /// The offset the record took.
    pub offset: u64,
    /// The record's Merkle leaf.
    pub leaf: Hash,
    /// The tree head after the append.
    pub root: Hash,
}

/// The state of a partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The number of offsets assigned: the next offset.
    pub size: u64,
    /// The RFC 6962 tree head over the headers.
    pub root: Hash,
    /// The segment layout.
    pub segments: Segments,
}

/// What is at an offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// A record, possibly with erased parts.
    Record(Record),
    /// A record compaction removed; the offset is never reused and its leaf remains.
    Compacted {
        /// The Merkle leaf the record had.
        leaf: Hash,
    },
}

impl Slot {
    /// The record, if the slot holds one.
    #[must_use]
    pub fn record(&self) -> Option<&Record> {
        match self {
            Self::Record(r) => Some(r),
            Self::Compacted { .. } => None,
        }
    }

    /// The Merkle leaf, whether or not the record is still there.
    #[must_use]
    pub fn leaf(&self) -> Hash {
        match self {
            Self::Record(r) => r.leaf(),
            Self::Compacted { leaf } => *leaf,
        }
    }
}

/// Why a store call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The partition does not exist in this store.
    #[error("partition {0} does not exist")]
    NoPartition(u64),
    /// The partition already exists.
    #[error("partition {0} already exists")]
    PartitionExists(u64),
    /// The offset is past the head.
    #[error("partition {partition} has no offset {offset}")]
    NoOffset {
        /// The partition.
        partition: u64,
        /// The offset asked for.
        offset: u64,
    },
    /// The offset was compacted, so there is nothing to erase.
    #[error("partition {partition} offset {offset} was compacted")]
    Compacted {
        /// The partition.
        partition: u64,
        /// The offset asked for.
        offset: u64,
    },
    /// A body operation failed.
    #[error(transparent)]
    Body(#[from] BodyError),
    /// Stored bytes are not a record.
    #[error(transparent)]
    Record(#[from] RecordError),
    /// Stored bytes are not a header.
    #[error(transparent)]
    Header(#[from] HeaderError),
    /// The backend's storage failed.
    #[error("storage: {0}")]
    Storage(String),
    /// Stored data is inconsistent: a gap, a wrong partition or offset in a header, a
    /// commitment that does not match.
    #[error("corrupt: {0}")]
    Corrupt(String),
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

/// An append-only log of partitions.
pub trait LogStore {
    /// Creates an empty partition with the given segment layout.
    fn create_partition(&mut self, partition: u64, segments: Segments) -> Result<(), StoreError>;

    /// The partitions this store holds, ascending.
    fn partitions(&self) -> Result<Vec<u64>, StoreError>;

    /// The partition's size, root and layout.
    fn head(&self, partition: u64) -> Result<Head, StoreError>;

    /// Appends a record at the next offset.
    fn append(&mut self, partition: u64, draft: Draft) -> Result<Appended, StoreError>;

    /// The slot at an offset.
    fn read(&self, partition: u64, offset: u64) -> Result<Slot, StoreError>;

    /// Up to `limit` slots from `from`, with their offsets, in order. Compacted offsets
    /// are skipped but count towards nothing; the scan ends at the head.
    fn scan(
        &self,
        partition: u64,
        from: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Record)>, StoreError>;

    /// Erases the named parts of the record at `offset`, keeping its leaves (0015 §1).
    /// Erasing an erased part is a no-op; an index past the body is an error.
    fn erase_parts(
        &mut self,
        partition: u64,
        offset: u64,
        parts: &[usize],
    ) -> Result<(), StoreError>;

    /// Removes whole records, leaving holes (0002 §2; 0013 §1). Compacting a compacted
    /// offset is a no-op.
    fn compact(&mut self, partition: u64, offsets: &[u64]) -> Result<(), StoreError>;
}
