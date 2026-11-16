//! An in-memory [`LogStore`], for tests and for building a partition before it is
//! written out.

use std::collections::BTreeMap;

use crate::hash::Hash;
use crate::store::{Appended, Draft, Head, LogStore, Slot, StoreError, check_restorable};
use crate::tree::{Frontier, Segments};

#[derive(Debug, Clone)]
struct Partition {
    segments: Segments,
    slots: Vec<Slot>,
    frontier: Frontier,
}

/// A store that holds everything in memory.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    partitions: BTreeMap<u64, Partition>,
}

impl MemoryStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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

    /// The leaves of a partition, in offset order.
    pub fn leaves(&self, partition: u64) -> Result<Vec<Hash>, StoreError> {
        Ok(self
            .partition(partition)?
            .slots
            .iter()
            .map(Slot::leaf)
            .collect())
    }
}

fn slot_mut(p: &mut Partition, partition: u64, offset: u64) -> Result<&mut Slot, StoreError> {
    usize::try_from(offset)
        .ok()
        .and_then(|i| p.slots.get_mut(i))
        .ok_or(StoreError::NoOffset { partition, offset })
}

impl LogStore for MemoryStore {
    async fn create_partition(
        &mut self,
        partition: u64,
        segments: Segments,
    ) -> Result<(), StoreError> {
        if self.partitions.contains_key(&partition) {
            return Err(StoreError::PartitionExists(partition));
        }
        self.partitions.insert(
            partition,
            Partition {
                segments,
                slots: Vec::new(),
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
        p.frontier.push_leaf(leaf);
        p.slots.push(Slot::Record(record));
        Ok(Appended {
            offset,
            leaf,
            root: p.frontier.root(),
        })
    }

    async fn append_slot(&mut self, partition: u64, slot: Slot) -> Result<Appended, StoreError> {
        let p = self.partition_mut(partition)?;
        let offset = p.frontier.size();
        check_restorable(&slot, partition, offset)?;
        let leaf = slot.leaf();
        p.frontier.push_leaf(leaf);
        p.slots.push(slot);
        Ok(Appended {
            offset,
            leaf,
            root: p.frontier.root(),
        })
    }

    async fn read(&self, partition: u64, offset: u64) -> Result<Slot, StoreError> {
        let p = self.partition(partition)?;
        usize::try_from(offset)
            .ok()
            .and_then(|i| p.slots.get(i))
            .cloned()
            .ok_or(StoreError::NoOffset { partition, offset })
    }

    async fn scan(
        &self,
        partition: u64,
        from: u64,
        limit: usize,
    ) -> Result<Vec<(u64, crate::record::Record)>, StoreError> {
        let p = self.partition(partition)?;
        let start = usize::try_from(from)
            .unwrap_or(usize::MAX)
            .min(p.slots.len());
        Ok(p.slots[start..]
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.record().map(|r| (from + i as u64, r.clone())))
            .take(limit)
            .collect())
    }

    async fn erase_parts(
        &mut self,
        partition: u64,
        offset: u64,
        parts: &[usize],
    ) -> Result<(), StoreError> {
        let p = self.partition_mut(partition)?;
        match slot_mut(p, partition, offset)? {
            Slot::Record(r) => {
                for &i in parts {
                    r.body_mut().erase(i)?;
                }
                Ok(())
            }
            Slot::Compacted { .. } => Err(StoreError::Compacted { partition, offset }),
        }
    }

    async fn compact(&mut self, partition: u64, offsets: &[u64]) -> Result<(), StoreError> {
        let p = self.partition_mut(partition)?;
        for &offset in offsets {
            let slot = slot_mut(p, partition, offset)?;
            if let Slot::Record(r) = slot {
                *slot = Slot::Compacted { leaf: r.leaf() };
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::draft;

    fn assert_send<T: Send>(_: &T) {}

    #[test]
    fn futures_are_send() {
        // A store must be usable from a multi-threaded server: every future is `Send`.
        let mut s = MemoryStore::new();
        assert_send(&s.create_partition(1, Segments::new(1).unwrap()));
        assert_send(&s.append(1, draft(0)));
        assert_send(&s.read(1, 0));
        assert_send(&s.scan(1, 0, 1));
        assert_send(&s.head(1));
        assert_send(&s.partitions());
        assert_send(&s.erase_parts(1, 0, &[]));
        assert_send(&s.compact(1, &[]));
    }
}
