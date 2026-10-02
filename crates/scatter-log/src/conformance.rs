//! The `LogStore` conformance suite (0005 rule 8; 0033 §8): one set of checks every
//! backend passes. A backend's tests call [`run`] with a constructor for a fresh,
//! empty store, and [`run_reopen`] when the store persists, with a function that
//! closes and reopens it. Both are futures: drive them with the backend's runtime, or
//! with `pollster::block_on` where there is none.

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

/// Restoring slots reproduces a partition: every slot of `from` appended to `to`.
async fn restore<S: LogStore>(from: &S, to: &mut S, partition: u64) {
    let head = from.head(partition).await.unwrap();
    to.create_partition(partition, head.segments).await.unwrap();
    for offset in 0..head.size {
        let slot = from.read(partition, offset).await.unwrap();
        let a = to.append_slot(partition, slot).await.unwrap();
        assert_eq!(a.offset, offset);
    }
    assert_eq!(to.head(partition).await.unwrap(), head);
}

async fn restoring_slots<S: LogStore>(make: &impl Fn() -> S) {
    let mut from = make();
    from.create_partition(1, Segments::new(1).unwrap())
        .await
        .unwrap();
    for n in 0..5 {
        from.append(1, draft(n)).await.unwrap();
    }
    from.erase_parts(1, 1, &[COMMENT]).await.unwrap();
    from.compact(1, &[3]).await.unwrap();
    let mut to = make();
    restore(&from, &mut to, 1).await;
    for offset in 0..5 {
        assert_eq!(
            to.read(1, offset).await.unwrap(),
            from.read(1, offset).await.unwrap(),
            "slot {offset}"
        );
    }
    // A record for another offset or partition is refused; so is a wrong commitment.
    let Slot::Record(r) = from.read(1, 0).await.unwrap() else {
        panic!("a record")
    };
    assert!(matches!(
        to.append_slot(1, Slot::Record(r.clone())).await,
        Err(StoreError::NotNext { offset: 5, .. })
    ));
    let mut other = make();
    other
        .create_partition(2, Segments::new(1).unwrap())
        .await
        .unwrap();
    assert!(matches!(
        other.append_slot(2, Slot::Record(r)).await,
        Err(StoreError::NotNext { .. })
    ));
    let a = to
        .append_slot(1, Slot::Compacted { leaf: [9; 32] })
        .await
        .unwrap();
    assert_eq!(a.offset, 5);
    assert_eq!(a.leaf, [9; 32]);
}

/// Runs the suite against fresh stores from `make`.
///
/// # Panics
///
/// On the first check the backend fails.
pub async fn run<S: LogStore>(make: impl Fn() -> S) {
    partitions_and_heads(&mut make()).await;
    appends_are_gapless_and_hashed(&mut make()).await;
    reads_and_scans(&mut make()).await;
    erasure_keeps_leaves(&mut make()).await;
    compaction_leaves_holes(&mut make()).await;
    segment_boundaries(&mut make()).await;
    restoring_slots(&make).await;
}

/// Runs the persistence checks: everything written before `reopen` is read after it.
///
/// # Panics
///
/// On the first check the backend fails.
pub async fn run_reopen<S: LogStore>(make: impl Fn() -> S, reopen: impl Fn(S) -> S) {
    let mut s = make();
    s.create_partition(7, Segments::new(2).unwrap())
        .await
        .unwrap();
    s.create_partition(CONFIG_PARTITION, Segments::new(16).unwrap())
        .await
        .unwrap();
    let mut leaves = Vec::new();
    for n in 0..11 {
        leaves.push(s.append(7, draft(n)).await.unwrap().leaf);
    }
    s.append(CONFIG_PARTITION, draft(99)).await.unwrap();
    s.erase_parts(7, 3, &[ATTESTATION]).await.unwrap();
    s.compact(7, &[5, 6]).await.unwrap();
    let before = s.head(7).await.unwrap();

    let s = reopen(s);
    assert_eq!(s.partitions().await.unwrap(), vec![CONFIG_PARTITION, 7]);
    let after = s.head(7).await.unwrap();
    assert_eq!(after, before, "the head survives a reopen");
    assert_eq!(after.root, root_of(&leaves));
    assert_eq!(s.head(CONFIG_PARTITION).await.unwrap().size, 1);
    for (n, leaf) in leaves.iter().enumerate() {
        let slot = s.read(7, n as u64).await.unwrap();
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
    let scanned = s.scan(7, 4, 10).await.unwrap();
    assert_eq!(
        scanned.iter().map(|(o, _)| *o).collect::<Vec<_>>(),
        vec![4, 7, 8, 9, 10]
    );

    // Appending after a reopen continues the tree.
    let mut s = s;
    let a = s.append(7, draft(11)).await.unwrap();
    assert_eq!(a.offset, 11);
    leaves.push(a.leaf);
    assert_eq!(a.root, root_of(&leaves));
}

async fn partitions_and_heads<S: LogStore>(s: &mut S) {
    assert!(s.partitions().await.unwrap().is_empty());
    assert!(matches!(s.head(1).await, Err(StoreError::NoPartition(1))));
    assert!(matches!(
        s.read(1, 0).await,
        Err(StoreError::NoPartition(1))
    ));
    assert!(matches!(
        s.append(1, draft(0)).await,
        Err(StoreError::NoPartition(1))
    ));
    s.create_partition(1, Segments::new(3).unwrap())
        .await
        .unwrap();
    assert!(matches!(
        s.create_partition(1, Segments::new(3).unwrap()).await,
        Err(StoreError::PartitionExists(1))
    ));
    s.create_partition(CONFIG_PARTITION, Segments::new(16).unwrap())
        .await
        .unwrap();
    assert_eq!(s.partitions().await.unwrap(), vec![CONFIG_PARTITION, 1]);
    let h = s.head(1).await.unwrap();
    assert_eq!(h.size, 0);
    assert_eq!(h.root, Frontier::new().root(), "the empty tree's head");
    assert_eq!(h.segments, Segments::new(3).unwrap());
}

async fn appends_are_gapless_and_hashed<S: LogStore>(store: &mut S) {
    store
        .create_partition(1, Segments::new(2).unwrap())
        .await
        .unwrap();
    let mut frontier = Frontier::new();
    for n in 0..9 {
        let next = draft(n);
        let expected = next.clone().seal(1, n);
        let a = store.append(1, next).await.unwrap();
        assert_eq!(a.offset, n, "offsets are gapless");
        assert_eq!(
            a.leaf,
            expected.leaf(),
            "the store fills in only its own fields"
        );
        frontier.push_leaf(a.leaf);
        assert_eq!(a.root, frontier.root(), "the root after append {n}");
        let h = store.head(1).await.unwrap();
        assert_eq!(h.size, n + 1);
        assert_eq!(h.root, a.root);
        let stored = store.read(1, n).await.unwrap();
        let r = stored.record().expect("a record");
        assert_eq!(*r, expected);
        assert_eq!(r.header().partition, 1);
        assert_eq!(r.header().commitment, r.body().commitment());
    }
    assert!(matches!(
        store.read(1, 9).await,
        Err(StoreError::NoOffset {
            partition: 1,
            offset: 9
        })
    ));
}

async fn reads_and_scans<S: LogStore>(s: &mut S) {
    s.create_partition(1, Segments::new(1).unwrap())
        .await
        .unwrap();
    for n in 0..5 {
        s.append(1, draft(n)).await.unwrap();
    }
    let all = s.scan(1, 0, 100).await.unwrap();
    assert_eq!(all.len(), 5);
    for (i, (offset, r)) in all.iter().enumerate() {
        assert_eq!(*offset, i as u64);
        assert_eq!(r.header().offset, i as u64);
        assert_eq!(r.header().key.as_deref(), Some(format!("Q{i}").as_str()));
    }
    let some = s.scan(1, 2, 2).await.unwrap();
    assert_eq!(some.iter().map(|(o, _)| *o).collect::<Vec<_>>(), vec![2, 3]);
    assert!(
        s.scan(1, 5, 10).await.unwrap().is_empty(),
        "a scan from the head is empty"
    );
    assert!(
        s.scan(1, 50, 10).await.unwrap().is_empty(),
        "a scan past the head is empty"
    );
    assert!(s.scan(1, 0, 0).await.unwrap().is_empty());
}

async fn erasure_keeps_leaves<S: LogStore>(s: &mut S) {
    s.create_partition(1, Segments::new(4).unwrap())
        .await
        .unwrap();
    let mut extra = draft(0);
    extra
        .body
        .push(Part::of(&Value::text("a fourth part")).unwrap())
        .unwrap();
    let a = s.append(1, extra).await.unwrap();
    s.append(1, draft(1)).await.unwrap();
    let root = s.head(1).await.unwrap().root;

    s.erase_parts(1, 0, &[ATTESTATION, 3]).await.unwrap();
    let r = s.read(1, 0).await.unwrap();
    let r = r.record().unwrap();
    assert!(r.body().attestation().is_erased());
    assert!(r.body().part(3).unwrap().is_erased());
    assert!(!r.body().content().is_erased());
    assert_eq!(r.leaf(), a.leaf, "erasure does not touch the header");
    assert_eq!(r.header().commitment, r.body().commitment());
    assert_eq!(s.head(1).await.unwrap().root, root, "nor the tree");

    s.erase_parts(1, 0, &[ATTESTATION]).await.unwrap();
    s.erase_parts(1, 0, &[]).await.unwrap();
    assert!(
        matches!(s.erase_parts(1, 0, &[4]).await, Err(StoreError::Body(_))),
        "a part the body does not have"
    );
    assert!(matches!(
        s.erase_parts(1, 2, &[COMMENT]).await,
        Err(StoreError::NoOffset { .. })
    ));
    let scanned = s.scan(1, 0, 10).await.unwrap();
    assert_eq!(scanned.len(), 2);
    assert!(scanned[0].1.body().attestation().is_erased());
    assert!(!scanned[1].1.body().attestation().is_erased());
}

async fn compaction_leaves_holes<S: LogStore>(s: &mut S) {
    s.create_partition(1, Segments::new(2).unwrap())
        .await
        .unwrap();
    let mut leaves = Vec::new();
    for n in 0..6 {
        leaves.push(s.append(1, draft(n)).await.unwrap().leaf);
    }
    let root = s.head(1).await.unwrap().root;
    s.compact(1, &[1, 4]).await.unwrap();
    assert_eq!(s.head(1).await.unwrap().size, 6, "offsets are never reused");
    assert_eq!(
        s.head(1).await.unwrap().root,
        root,
        "the tree still folds over the holes"
    );
    for (n, leaf) in leaves.iter().enumerate() {
        let slot = s.read(1, n as u64).await.unwrap();
        assert_eq!(slot.leaf(), *leaf);
        assert_eq!(slot.record().is_none(), n == 1 || n == 4);
    }
    assert_eq!(
        s.scan(1, 0, 10)
            .await
            .unwrap()
            .iter()
            .map(|(o, _)| *o)
            .collect::<Vec<_>>(),
        vec![0, 2, 3, 5]
    );
    assert_eq!(
        s.scan(1, 1, 1).await.unwrap()[0].0,
        2,
        "a scan starting in a hole"
    );
    s.compact(1, &[1]).await.unwrap();
    assert!(matches!(
        s.erase_parts(1, 1, &[COMMENT]).await,
        Err(StoreError::Compacted { .. })
    ));
    assert!(matches!(
        s.compact(1, &[6]).await,
        Err(StoreError::NoOffset { .. })
    ));
    let a = s.append(1, draft(6)).await.unwrap();
    assert_eq!(a.offset, 6);
}

async fn segment_boundaries<S: LogStore>(s: &mut S) {
    // k = 1: two records per segment; every operation crosses segment files.
    let seg = Segments::new(1).unwrap();
    s.create_partition(1, seg).await.unwrap();
    let mut leaves = Vec::new();
    for n in 0..7 {
        leaves.push(s.append(1, draft(n)).await.unwrap().leaf);
    }
    assert_eq!(s.head(1).await.unwrap().root, root_of(&leaves));
    for (n, leaf) in leaves.iter().enumerate() {
        assert_eq!(s.read(1, n as u64).await.unwrap().leaf(), *leaf);
    }
    s.erase_parts(1, 2, &[COMMENT]).await.unwrap();
    s.erase_parts(1, 6, &[COMMENT]).await.unwrap();
    s.compact(1, &[3]).await.unwrap();
    assert_eq!(s.head(1).await.unwrap().root, root_of(&leaves));
    let scanned = s.scan(1, 0, 10).await.unwrap();
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
        let mut chunk = Vec::new();
        for o in r {
            chunk.push(s.read(1, o).await.unwrap().leaf());
        }
        f.push_subtree(1, root_of(&chunk)).unwrap();
    }
    f.push_leaf(leaves[6]);
    assert_eq!(f.root(), s.head(1).await.unwrap().root);
}

#[cfg(test)]
mod tests {
    use crate::memory::MemoryStore;

    #[test]
    fn memory_store_conforms() {
        pollster::block_on(super::run(MemoryStore::new));
        pollster::block_on(super::run_reopen(MemoryStore::new, |s| s));
    }
}
