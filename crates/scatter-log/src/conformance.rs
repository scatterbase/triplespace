//! The `LogStore` conformance suite (0005 rule 8; 0033 §8): one set of checks every
//! backend passes. A backend's tests call [`run`] with a constructor for a fresh,
//! empty store, and [`run_reopen`] when the store persists, with a function that
//! closes and reopens it.

use crate::body::{ATTESTATION, Body, COMMENT, Part};
use crate::cbor::Value;
use crate::header::CONFIG_PARTITION;
use crate::store::{Draft, LogStore, Slot, StoreError};
use crate::tree::{Frontier, Segments, root_of};

/// A draft with a recognisable content.
#[must_use]
pub fn draft(n: u64) -> Draft {
    Draft {
        appended_at: 1_790_000_000_000_000 + n,
        payload_type: "scatter:v0/changeset".into(),
        key: Some(format!("Q{n}")),
        revid: Some(n + 1),
        logid: None,
        page_id: Some(n + 100),
        body: Body::core(
            &Value::map(vec![
                (Value::text("op"), Value::text("put")),
                (Value::text("n"), Value::Int(i128::from(n))),
            ]),
            &Value::Text(format!("record {n}")),
            &Value::map(vec![(Value::text("actor"), Value::text("local:1"))]),
        )
        .expect("three parts"),
    }
}

/// Runs the suite against fresh stores from `make`.
///
/// # Panics
///
/// On the first check the backend fails.
pub fn run<S: LogStore>(make: impl Fn() -> S) {
    partitions_and_heads(&mut make());
    appends_are_gapless_and_hashed(&mut make());
    reads_and_scans(&mut make());
    erasure_keeps_leaves(&mut make());
    compaction_leaves_holes(&mut make());
    segment_boundaries(&mut make());
}

/// Runs the persistence checks: everything written before `reopen` is read after it.
///
/// # Panics
///
/// On the first check the backend fails.
pub fn run_reopen<S: LogStore>(make: impl Fn() -> S, reopen: impl Fn(S) -> S) {
    let mut s = make();
    s.create_partition(7, Segments::new(2).unwrap()).unwrap();
    s.create_partition(CONFIG_PARTITION, Segments::new(16).unwrap())
        .unwrap();
    let mut leaves = Vec::new();
    for n in 0..11 {
        leaves.push(s.append(7, draft(n)).unwrap().leaf);
    }
    s.append(CONFIG_PARTITION, draft(99)).unwrap();
    s.erase_parts(7, 3, &[ATTESTATION]).unwrap();
    s.compact(7, &[5, 6]).unwrap();
    let before = s.head(7).unwrap();

    let s = reopen(s);
    assert_eq!(s.partitions().unwrap(), vec![CONFIG_PARTITION, 7]);
    let after = s.head(7).unwrap();
    assert_eq!(after, before, "the head survives a reopen");
    assert_eq!(after.root, root_of(&leaves));
    assert_eq!(s.head(CONFIG_PARTITION).unwrap().size, 1);
    for (n, leaf) in leaves.iter().enumerate() {
        let slot = s.read(7, n as u64).unwrap();
        assert_eq!(slot.leaf(), *leaf, "leaf {n} after reopen");
        match n {
            5 | 6 => assert!(matches!(slot, Slot::Compacted { .. })),
            _ => {
                let r = slot.record().unwrap();
                assert_eq!(r.header().offset, n as u64);
                assert_eq!(r.body().attestation().is_erased(), n == 3);
            }
        }
    }
    let scanned = s.scan(7, 4, 10).unwrap();
    assert_eq!(
        scanned.iter().map(|(o, _)| *o).collect::<Vec<_>>(),
        vec![4, 7, 8, 9, 10]
    );

    // Appending after a reopen continues the tree.
    let mut s = s;
    let a = s.append(7, draft(11)).unwrap();
    assert_eq!(a.offset, 11);
    leaves.push(a.leaf);
    assert_eq!(a.root, root_of(&leaves));
}

fn partitions_and_heads<S: LogStore>(s: &mut S) {
    assert!(s.partitions().unwrap().is_empty());
    assert!(matches!(s.head(1), Err(StoreError::NoPartition(1))));
    assert!(matches!(s.read(1, 0), Err(StoreError::NoPartition(1))));
    assert!(matches!(
        s.append(1, draft(0)),
        Err(StoreError::NoPartition(1))
    ));
    s.create_partition(1, Segments::new(3).unwrap()).unwrap();
    assert!(matches!(
        s.create_partition(1, Segments::new(3).unwrap()),
        Err(StoreError::PartitionExists(1))
    ));
    s.create_partition(CONFIG_PARTITION, Segments::new(16).unwrap())
        .unwrap();
    assert_eq!(s.partitions().unwrap(), vec![CONFIG_PARTITION, 1]);
    let h = s.head(1).unwrap();
    assert_eq!(h.size, 0);
    assert_eq!(h.root, Frontier::new().root(), "the empty tree's head");
    assert_eq!(h.segments, Segments::new(3).unwrap());
}

fn appends_are_gapless_and_hashed<S: LogStore>(store: &mut S) {
    store
        .create_partition(1, Segments::new(2).unwrap())
        .unwrap();
    let mut frontier = Frontier::new();
    for n in 0..9 {
        let next = draft(n);
        let expected = next.clone().seal(1, n);
        let a = store.append(1, next).unwrap();
        assert_eq!(a.offset, n, "offsets are gapless");
        assert_eq!(
            a.leaf,
            expected.leaf(),
            "the store fills in only its own fields"
        );
        frontier.push_leaf(a.leaf);
        assert_eq!(a.root, frontier.root(), "the root after append {n}");
        let h = store.head(1).unwrap();
        assert_eq!(h.size, n + 1);
        assert_eq!(h.root, a.root);
        let stored = store.read(1, n).unwrap();
        let r = stored.record().expect("a record");
        assert_eq!(*r, expected);
        assert_eq!(r.header().partition, 1);
        assert_eq!(r.header().commitment, r.body().commitment());
    }
    assert!(matches!(
        store.read(1, 9),
        Err(StoreError::NoOffset {
            partition: 1,
            offset: 9
        })
    ));
}

fn reads_and_scans<S: LogStore>(s: &mut S) {
    s.create_partition(1, Segments::new(1).unwrap()).unwrap();
    for n in 0..5 {
        s.append(1, draft(n)).unwrap();
    }
    let all = s.scan(1, 0, 100).unwrap();
    assert_eq!(all.len(), 5);
    for (i, (offset, r)) in all.iter().enumerate() {
        assert_eq!(*offset, i as u64);
        assert_eq!(r.header().offset, i as u64);
        assert_eq!(r.header().key.as_deref(), Some(format!("Q{i}").as_str()));
    }
    let some = s.scan(1, 2, 2).unwrap();
    assert_eq!(some.iter().map(|(o, _)| *o).collect::<Vec<_>>(), vec![2, 3]);
    assert!(
        s.scan(1, 5, 10).unwrap().is_empty(),
        "a scan from the head is empty"
    );
    assert!(
        s.scan(1, 50, 10).unwrap().is_empty(),
        "a scan past the head is empty"
    );
    assert!(s.scan(1, 0, 0).unwrap().is_empty());
}

fn erasure_keeps_leaves<S: LogStore>(s: &mut S) {
    s.create_partition(1, Segments::new(4).unwrap()).unwrap();
    let mut extra = draft(0);
    extra
        .body
        .push(Part::of(&Value::text("a fourth part")).unwrap())
        .unwrap();
    let a = s.append(1, extra).unwrap();
    s.append(1, draft(1)).unwrap();
    let root = s.head(1).unwrap().root;

    s.erase_parts(1, 0, &[ATTESTATION, 3]).unwrap();
    let r = s.read(1, 0).unwrap();
    let r = r.record().unwrap();
    assert!(r.body().attestation().is_erased());
    assert!(r.body().part(3).unwrap().is_erased());
    assert!(!r.body().content().is_erased());
    assert_eq!(r.leaf(), a.leaf, "erasure does not touch the header");
    assert_eq!(r.header().commitment, r.body().commitment());
    assert_eq!(s.head(1).unwrap().root, root, "nor the tree");

    s.erase_parts(1, 0, &[ATTESTATION]).unwrap();
    s.erase_parts(1, 0, &[]).unwrap();
    assert!(
        matches!(s.erase_parts(1, 0, &[4]), Err(StoreError::Body(_))),
        "a part the body does not have"
    );
    assert!(matches!(
        s.erase_parts(1, 2, &[COMMENT]),
        Err(StoreError::NoOffset { .. })
    ));
    let scanned = s.scan(1, 0, 10).unwrap();
    assert_eq!(scanned.len(), 2);
    assert!(scanned[0].1.body().attestation().is_erased());
    assert!(!scanned[1].1.body().attestation().is_erased());
}

fn compaction_leaves_holes<S: LogStore>(s: &mut S) {
    s.create_partition(1, Segments::new(2).unwrap()).unwrap();
    let leaves: Vec<_> = (0..6)
        .map(|n| s.append(1, draft(n)).unwrap().leaf)
        .collect();
    let root = s.head(1).unwrap().root;
    s.compact(1, &[1, 4]).unwrap();
    assert_eq!(s.head(1).unwrap().size, 6, "offsets are never reused");
    assert_eq!(
        s.head(1).unwrap().root,
        root,
        "the tree still folds over the holes"
    );
    for (n, leaf) in leaves.iter().enumerate() {
        let slot = s.read(1, n as u64).unwrap();
        assert_eq!(slot.leaf(), *leaf);
        assert_eq!(slot.record().is_none(), n == 1 || n == 4);
    }
    assert_eq!(
        s.scan(1, 0, 10)
            .unwrap()
            .iter()
            .map(|(o, _)| *o)
            .collect::<Vec<_>>(),
        vec![0, 2, 3, 5]
    );
    assert_eq!(
        s.scan(1, 1, 1).unwrap()[0].0,
        2,
        "a scan starting in a hole"
    );
    s.compact(1, &[1]).unwrap();
    assert!(matches!(
        s.erase_parts(1, 1, &[COMMENT]),
        Err(StoreError::Compacted { .. })
    ));
    assert!(matches!(
        s.compact(1, &[6]),
        Err(StoreError::NoOffset { .. })
    ));
    let a = s.append(1, draft(6)).unwrap();
    assert_eq!(a.offset, 6);
}

fn segment_boundaries<S: LogStore>(s: &mut S) {
    // k = 1: two records per segment; every operation crosses segment files.
    let seg = Segments::new(1).unwrap();
    s.create_partition(1, seg).unwrap();
    let mut leaves = Vec::new();
    for n in 0..7 {
        leaves.push(s.append(1, draft(n)).unwrap().leaf);
    }
    assert_eq!(s.head(1).unwrap().root, root_of(&leaves));
    for (n, leaf) in leaves.iter().enumerate() {
        assert_eq!(s.read(1, n as u64).unwrap().leaf(), *leaf);
    }
    s.erase_parts(1, 2, &[COMMENT]).unwrap();
    s.erase_parts(1, 6, &[COMMENT]).unwrap();
    s.compact(1, &[3]).unwrap();
    assert_eq!(s.head(1).unwrap().root, root_of(&leaves));
    let scanned = s.scan(1, 0, 10).unwrap();
    assert_eq!(
        scanned.iter().map(|(o, _)| *o).collect::<Vec<_>>(),
        vec![0, 1, 2, 4, 5, 6]
    );
    assert!(scanned[2].1.body().comment().is_erased());
    assert!(scanned[5].1.body().comment().is_erased());
    assert!(!scanned[1].1.body().comment().is_erased());
    // The segment roots of the full segments fold to the partition root.
    let mut f = Frontier::new();
    for n in 0..seg.full_segments(7) {
        let r = seg.range(n);
        let chunk: Vec<_> = (r.start..r.end)
            .map(|o| s.read(1, o).unwrap().leaf())
            .collect();
        f.push_subtree(1, root_of(&chunk)).unwrap();
    }
    f.push_leaf(leaves[6]);
    assert_eq!(f.root(), s.head(1).unwrap().root);
}

#[cfg(test)]
mod tests {
    use crate::memory::MemoryStore;

    #[test]
    fn memory_store_conforms() {
        super::run(MemoryStore::new);
        super::run_reopen(MemoryStore::new, |s| s);
    }
}
