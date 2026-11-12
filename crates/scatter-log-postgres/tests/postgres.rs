//! Integration tests against a real database (0033 §4, §15), named by
//! `TRIPLESPACE_TEST_DATABASE_URL`, a libpq-style URL for a role that may create
//! databases. Without it every test here passes vacuously and says so.

use std::sync::atomic::{AtomicU32, Ordering};

use scatter_integrity::checkpoint::Checkpoint;
use scatter_integrity::checkpoints::{CheckpointStore, checkpoint, segment_manifest};
use scatter_integrity::proof::{verify_consistency, verify_inclusion};
use scatter_log::body::{ATTESTATION, COMMENT};
use scatter_log::conformance::{self, draft};
use scatter_log::header::Header;
use scatter_log::store::{LogStore, Slot};
use scatter_log::tree::{Segments, root_of};
use scatter_log_postgres::sequences::{self, Sequence};
use scatter_log_postgres::{PgCheckpoints, PgLog, log, merkle, migrate};
use tokio_postgres::{Client, Config, NoTls};

static COUNTER: AtomicU32 = AtomicU32::new(0);
/// The scratch databases this thread made, dropped when the thread ends, whether its test
/// passed or panicked: the harness runs each test on a thread of its own. Per thread, not
/// per process: `DROP DATABASE … WITH (FORCE)` on another test's would terminate that
/// test's connections mid-run (SQLSTATE 57P01). `triplespace_db::scratch` does the same
/// for the triplespace crates, which this one cannot depend on.
#[derive(Default)]
struct Made(Vec<String>);

impl Drop for Made {
    fn drop(&mut self) {
        let names = std::mem::take(&mut self.0);
        let Ok(url) = std::env::var("TRIPLESPACE_TEST_DATABASE_URL") else {
            return;
        };
        if names.is_empty() {
            return;
        }
        // The test's runtime is gone by now: drop them on a thread with one of its own.
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async {
                let Ok((client, connection)) = tokio_postgres::connect(&url, NoTls).await else {
                    return;
                };
                tokio::spawn(connection);
                for n in names {
                    let _ = client
                        .batch_execute(&format!("DROP DATABASE IF EXISTS {n} WITH (FORCE)"))
                        .await;
                }
            });
        })
        .join();
    }
}

thread_local! {
    static CREATED: std::cell::RefCell<Made> = std::cell::RefCell::new(Made::default());
}

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection: {e}");
        }
    });
    client
}

/// A fresh, migrated database, and a client on it.
async fn fresh() -> Client {
    let admin = admin_config().expect("checked by the caller");
    let name = format!(
        "scatter_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    connect(&admin)
        .await
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    CREATED.with_borrow_mut(|c| c.0.push(name.clone()));
    let mut config = admin;
    config.dbname(&name);
    let client = connect(&config).await;
    migrate(&client).await.expect("migrate");
    client
}

/// Reconnects to the database a client is on.
async fn reconnect(client: Client) -> Client {
    let name: String = client
        .query_one("SELECT current_database()", &[])
        .await
        .unwrap()
        .get(0);
    drop(client);
    let mut config = admin_config().unwrap();
    config.dbname(&name);
    connect(&config).await
}

fn skipped() -> bool {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return true;
    }
    false
}

#[tokio::test]
async fn conformance_suite() {
    if skipped() {
        return;
    }
    conformance::run(async || PgLog::new(fresh().await)).await;
    conformance::run_reopen(
        async || PgLog::new(fresh().await),
        async |s: PgLog<Client>| PgLog::new(reconnect(s.into_inner()).await),
    )
    .await;
}

#[tokio::test]
async fn a_rolled_back_append_releases_its_offset() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    {
        let tx = client.transaction().await.unwrap();
        let mut log = PgLog::new(&tx);
        log.create_partition(7, Segments::new(4).unwrap())
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let root_before = {
        let tx = client.transaction().await.unwrap();
        let mut log = PgLog::new(&tx);
        log.append(7, draft(0)).await.unwrap();
        log.append(7, draft(1)).await.unwrap();
        tx.commit().await.unwrap();
        PgLog::new(reconnect_same(&client).await)
            .head(7)
            .await
            .unwrap()
    };
    {
        let tx = client.transaction().await.unwrap();
        let mut log = PgLog::new(&tx);
        let a = log.append(7, draft(2)).await.unwrap();
        assert_eq!(a.offset, 2);
        assert_eq!(
            log.head(7).await.unwrap().size,
            3,
            "visible inside the transaction"
        );
        tx.rollback().await.unwrap();
    }
    let head = PgLog::new(reconnect_same(&client).await)
        .head(7)
        .await
        .unwrap();
    assert_eq!(head.size, 2, "the offset was released");
    assert_eq!(head, root_before);
    let tx = client.transaction().await.unwrap();
    let a = PgLog::new(&tx).append(7, draft(2)).await.unwrap();
    assert_eq!(a.offset, 2, "and is taken again");
    tx.commit().await.unwrap();
}

async fn reconnect_same(client: &Client) -> Client {
    let name: String = client
        .query_one("SELECT current_database()", &[])
        .await
        .unwrap()
        .get(0);
    let mut config = admin_config().unwrap();
    config.dbname(&name);
    connect(&config).await
}

#[tokio::test]
async fn merkle_nodes_give_roots_and_proofs() {
    if skipped() {
        return;
    }
    let mut store = PgLog::new(fresh().await);
    store
        .create_partition(1, Segments::new(2).unwrap())
        .await
        .unwrap();
    let mut leaves = Vec::new();
    for n in 0..21 {
        let a = store.append(1, draft(n)).await.unwrap();
        leaves.push(a.leaf);
        assert_eq!(a.root, root_of(&leaves), "root after {n}");
    }
    store.compact(1, &[3, 4, 5]).await.unwrap();
    let client = store.client();
    for size in 1..=21u64 {
        let want = root_of(&leaves[..usize::try_from(size).unwrap()]);
        assert_eq!(
            merkle::root(client, 1, size).await.unwrap(),
            want,
            "root at {size}"
        );
        for index in 0..size {
            let proof = merkle::inclusion_proof(client, 1, index, size)
                .await
                .unwrap();
            verify_inclusion(
                &leaves[usize::try_from(index).unwrap()],
                index,
                size,
                &proof,
                &want,
            )
            .unwrap_or_else(|e| panic!("size {size} index {index}: {e}"));
        }
        // Consistency with every smaller tree (RFC 9162 §2.1.4).
        for first in 1..size {
            let old = root_of(&leaves[..usize::try_from(first).unwrap()]);
            let proof = merkle::consistency_proof(client, 1, first, size)
                .await
                .unwrap();
            verify_consistency(first, size, &old, &want, &proof)
                .unwrap_or_else(|e| panic!("{first} to {size}: {e}"));
        }
    }
    // A range's proofs share their nodes, and each one verifies.
    let size = 21u64;
    let want = root_of(&leaves);
    let (nodes, paths) = merkle::range_proof(client, 1, 5, 17, size).await.unwrap();
    assert_eq!(paths.len(), 12);
    let mut separate = 0;
    for (n, path) in paths.iter().enumerate() {
        let index = 5 + u64::try_from(n).unwrap();
        let proof: Vec<_> = path.iter().map(|&at| nodes[at].2).collect();
        separate += proof.len();
        verify_inclusion(&leaves[n + 5], index, size, &proof, &want)
            .unwrap_or_else(|e| panic!("range proof of {index}: {e}"));
    }
    assert!(nodes.len() < separate, "the shared nodes are sent once");
    // Compacted offsets still read as their leaves.
    assert_eq!(
        store.read(1, 4).await.unwrap(),
        Slot::Compacted { leaf: leaves[4] }
    );
    assert!(matches!(store.read(1, 6).await.unwrap(), Slot::Record(_)));
}

#[tokio::test]
async fn header_columns_reproduce_the_header_bytes() {
    if skipped() {
        return;
    }
    let mut store = PgLog::new(fresh().await);
    store
        .create_partition(0x42, Segments::new(3).unwrap())
        .await
        .unwrap();
    for n in 0..4 {
        store.append(0x42, draft(n)).await.unwrap();
    }
    store
        .erase_parts(0x42, 1, &[COMMENT, ATTESTATION])
        .await
        .unwrap();
    let rows = store
        .client()
        .query(
            "SELECT partition, \"offset\", appended_at, payload_type, key, commitment, revid, logid, page_id,
                    header, leaf, erased
             FROM log.record WHERE partition = $1 ORDER BY \"offset\"",
            &[&scatter_log_postgres::ids::partition_to_db(0x42)],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 4);
    for r in &rows {
        let commitment: Vec<u8> = r.get(5);
        let h = Header {
            version: 1,
            partition: scatter_log_postgres::ids::partition_from_db(r.get(0)),
            offset: u64::try_from(r.get::<_, i64>(1)).unwrap(),
            appended_at: u64::try_from(r.get::<_, i64>(2)).unwrap(),
            payload_type: r.get(3),
            key: r.get(4),
            commitment: commitment.try_into().unwrap(),
            revid: r
                .get::<_, Option<i64>>(6)
                .map(|v| u64::try_from(v).unwrap()),
            logid: r
                .get::<_, Option<i64>>(7)
                .map(|v| u64::try_from(v).unwrap()),
            page_id: r
                .get::<_, Option<i64>>(8)
                .map(|v| u64::try_from(v).unwrap()),
        };
        let header: Vec<u8> = r.get(9);
        assert_eq!(
            h.encode(),
            header,
            "0013 §2: the columns reproduce the bytes"
        );
        let leaf: Vec<u8> = r.get(10);
        assert_eq!(h.leaf().to_vec(), leaf);
        let erased: i16 = r.get(11);
        assert_eq!(erased, if h.offset == 1 { 0b110 } else { 0 });
    }
}

#[tokio::test]
async fn checkpoints_in_rows() {
    if skipped() {
        return;
    }
    let key = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
    let mut store = PgLog::new(fresh().await);
    store
        .create_partition(9, Segments::new(1).unwrap())
        .await
        .unwrap();
    let mut cps = PgCheckpoints::new(reconnect_same(store.client()).await);
    for n in 0..5 {
        store.append(9, draft(n)).await.unwrap();
        cps.put(
            9,
            &checkpoint(&store, 9, "h/log/local", &key).await.unwrap(),
        )
        .await
        .unwrap();
    }
    let list = cps.list(9).await.unwrap();
    assert_eq!(
        list.iter().map(|c| c.checkpoint.size).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert_eq!(cps.latest(9).await.unwrap().unwrap().checkpoint.size, 5);
    // Replacing a size keeps one row.
    let again = Checkpoint {
        origin: "h/log/local".into(),
        size: 5,
        root: list[4].checkpoint.root,
    }
    .sign(&key)
    .unwrap();
    cps.put(9, &again).await.unwrap();
    assert_eq!(cps.list(9).await.unwrap().len(), 5);
    let covering = scatter_log_postgres::checkpoints::covering(cps.client(), 9, 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(covering.checkpoint.size, 3);
    assert!(
        scatter_log_postgres::checkpoints::covering(cps.client(), 9, 5)
            .await
            .unwrap()
            .is_none()
    );
    let m0 = segment_manifest(&store, 9, 0, "h/log/local", &key)
        .await
        .unwrap();
    cps.put_manifest(9, 0, &m0).await.unwrap();
    cps.put_manifest(9, 0, &m0).await.unwrap();
    cps.put_manifest(
        9,
        1,
        &segment_manifest(&store, 9, 1, "h/log/local", &key)
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let manifests = cps.manifests(9).await.unwrap();
    assert_eq!(
        manifests.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(manifests[0].1, m0);
}

#[tokio::test]
async fn sequences_and_floors() {
    if skipped() {
        return;
    }
    let client = fresh().await;
    sequences::create(&client, "librarybase", &["item", "property"])
        .await
        .unwrap();
    sequences::create(&client, "librarybase", &["item"])
        .await
        .unwrap();
    assert_eq!(
        sequences::next(&client, "librarybase", &Sequence::Revision)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sequences::next(&client, "librarybase", &Sequence::Revision)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sequences::next(&client, "librarybase", &Sequence::Log)
            .await
            .unwrap(),
        1
    );
    let item = Sequence::Entity("item".into());
    assert_eq!(
        sequences::next(&client, "librarybase", &item)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sequences::next(&client, "librarybase", &Sequence::User)
            .await
            .unwrap(),
        1
    );
    // The instance's own, under the reserved name.
    assert!(sequences::create(&client, "", &[]).await.is_err());
    sequences::create_instance(&client, &["domain"])
        .await
        .unwrap();
    assert_eq!(
        sequences::next(&client, "", &Sequence::Job).await.unwrap(),
        1
    );
    assert_eq!(
        sequences::next(&client, "", &Sequence::Surrogate("domain".into()))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sequences::next(&client, "instance", &Sequence::Job)
            .await
            .unwrap(),
        2,
        "`` and `instance` name the same sequences"
    );
    // An adoption floors the item counter at what the source consumed.
    sequences::floor(&client, "librarybase", &item, 350_000)
        .await
        .unwrap();
    assert_eq!(
        sequences::next(&client, "librarybase", &item)
            .await
            .unwrap(),
        350_001
    );
    // A lower floor never lowers it.
    sequences::floor(&client, "librarybase", &item, 10)
        .await
        .unwrap();
    assert_eq!(
        sequences::next(&client, "librarybase", &item)
            .await
            .unwrap(),
        350_002
    );
    assert!(
        sequences::next(&client, "other", &Sequence::Revision)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn compaction_by_key() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    let tx = client.transaction().await.unwrap();
    let mut store = PgLog::new(&tx);
    store
        .create_partition(5, Segments::new(4).unwrap())
        .await
        .unwrap();
    for n in [1, 2, 1, 3, 1, 2] {
        store.append(5, draft(n)).await.unwrap(); // keys Q1, Q2, Q1, Q3, Q1, Q2
    }
    let removed = log::compact_keys(&tx, 5, &["Q1".into(), "Q2".into()])
        .await
        .unwrap();
    assert_eq!(removed, vec![0, 1, 2]);
    assert_eq!(
        store
            .scan(5, 0, 10)
            .await
            .unwrap()
            .iter()
            .map(|(o, _)| *o)
            .collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
    assert_eq!(store.head(5).await.unwrap().size, 6);
    assert!(matches!(
        store.read(5, 0).await.unwrap(),
        Slot::Compacted { .. }
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn compaction_keeps_the_highest_revision() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    let tx = client.transaction().await.unwrap();
    let mut store = PgLog::new(&tx);
    store
        .create_partition(5, Segments::new(4).unwrap())
        .await
        .unwrap();
    // Q1: puts at revisions 10 and 20, then a deletion and a redirect without one.
    for revid in [Some(10), Some(20), None, None] {
        let mut d = draft(1);
        d.revid = revid;
        store.append(5, d).await.unwrap();
    }
    let removed = log::compact_keys(&tx, 5, &["Q1".into()]).await.unwrap();
    assert_eq!(removed, vec![0, 2]);
    assert_eq!(
        store
            .scan(5, 0, 10)
            .await
            .unwrap()
            .iter()
            .map(|(o, _)| *o)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    tx.commit().await.unwrap();
}

/// 0103 §4 (A10): a restore's block of slots, by `COPY`, leaves what restoring them one
/// statement each leaves, erased parts and compacted slots included, and refuses a block
/// with a slot that is not what its header commits to before writing any of it.
#[tokio::test]
async fn slots_restored_in_bulk_are_the_slots_restored_one_by_one() {
    if skipped() {
        return;
    }
    // A source with an erased part and a compacted slot.
    let mut source = PgLog::new(fresh().await);
    source
        .create_partition(3, Segments::new(2).unwrap())
        .await
        .unwrap();
    for n in 0..11 {
        source.append(3, draft(n)).await.unwrap();
    }
    source.erase_parts(3, 4, &[COMMENT]).await.unwrap();
    source.compact(3, &[6]).await.unwrap();
    let head = source.head(3).await.unwrap();
    let mut slots = Vec::new();
    for offset in 0..head.size {
        slots.push(source.read(3, offset).await.unwrap());
    }

    let mut one = fresh().await;
    let mut bulk = fresh().await;
    for client in [&mut one, &mut bulk] {
        let tx = client.transaction().await.unwrap();
        PgLog::new(&tx)
            .create_partition(3, head.segments)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    {
        let tx = one.transaction().await.unwrap();
        let mut frontiers = log::Frontiers::new();
        for slot in &slots {
            log::append_slot_with(&tx, 3, slot.clone(), &mut frontiers)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
    {
        // Two blocks, so the second starts from a frontier the first left.
        let tx = bulk.transaction().await.unwrap();
        let mut frontiers = log::Frontiers::new();
        let mut leaves = log::append_slots(&tx, 3, &slots[..7], &mut frontiers)
            .await
            .unwrap();
        leaves.extend(
            log::append_slots(&tx, 3, &slots[7..], &mut frontiers)
                .await
                .unwrap(),
        );
        assert_eq!(leaves, slots.iter().map(Slot::leaf).collect::<Vec<_>>());
        assert_eq!(root_of(&leaves), head.root);
        tx.commit().await.unwrap();
    }
    for sql in [
        "SELECT \"offset\"::text, appended_at::text, payload_type, key, encode(commitment, 'hex'),
                revid::text, logid::text, page_id::text, encode(header, 'hex'), encode(leaf, 'hex'),
                encode(body, 'hex'), erased::text
         FROM log.record ORDER BY 1",
        "SELECT level::text, index::text, encode(hash, 'hex') FROM log.merkle_node ORDER BY 1, 2",
        "SELECT next_offset::text FROM log.partition",
    ] {
        let rows = async |c: &Client| -> Vec<Vec<Option<String>>> {
            c.query(sql, &[])
                .await
                .unwrap()
                .iter()
                .map(|r| (0..r.len()).map(|i| r.get(i)).collect())
                .collect()
        };
        let (a, b) = (rows(&one).await, rows(&bulk).await);
        assert!(!a.is_empty());
        assert_eq!(a, b, "{sql}");
    }
    let restored = PgLog::new(bulk);
    assert_eq!(restored.head(3).await.unwrap(), head);
    assert_eq!(restored.read(3, 4).await.unwrap(), slots[4]);
    assert!(matches!(
        restored.read(3, 6).await.unwrap(),
        Slot::Compacted { .. }
    ));

    // A record out of place stops the block whole.
    let mut refused = fresh().await;
    {
        let tx = refused.transaction().await.unwrap();
        PgLog::new(&tx)
            .create_partition(3, head.segments)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    // (A record whose body is not its header's cannot be built at all: `Record::new`
    // refuses it, so a tampered file fails where the bundle is read.)
    for bad in [
        vec![slots[0].clone(), slots[2].clone()],
        vec![slots[1].clone()],
    ] {
        let tx = refused.transaction().await.unwrap();
        let err = log::append_slots(&tx, 3, &bad, &mut log::Frontiers::new()).await;
        assert!(err.is_err());
        tx.commit().await.unwrap();
        let n: i64 = refused
            .query_one("SELECT count(*) FROM log.record", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(n, 0, "nothing of a refused block is written");
    }
}
