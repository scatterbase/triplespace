//! Integration tests against a real database (0033 §4, §15), named by
//! `TRIPLESPACE_TEST_DATABASE_URL`, a libpq-style URL for a role that may create
//! databases. Without it every test here passes vacuously and says so.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use scatter_integrity::checkpoint::Checkpoint;
use scatter_integrity::checkpoints::{CheckpointStore, checkpoint, segment_manifest};
use scatter_integrity::proof::verify_inclusion;
use scatter_log::body::{ATTESTATION, COMMENT};
use scatter_log::conformance::{self, draft};
use scatter_log::header::Header;
use scatter_log::store::{LogStore, Slot};
use scatter_log::tree::{Segments, root_of};
use scatter_log_postgres::sequences::{self, Sequence};
use scatter_log_postgres::{PgCheckpoints, PgLog, log, merkle, migrate};
use tokio_postgres::{Client, Config, NoTls};

static COUNTER: AtomicU32 = AtomicU32::new(0);
static CREATED: Mutex<Vec<String>> = Mutex::new(Vec::new());

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
    CREATED.lock().unwrap().push(name.clone());
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

async fn cleanup() {
    let Some(admin) = admin_config() else { return };
    let client = connect(&admin).await;
    let names: Vec<String> = std::mem::take(&mut *CREATED.lock().unwrap());
    for n in names {
        let _ = client
            .batch_execute(&format!("DROP DATABASE IF EXISTS {n} WITH (FORCE)"))
            .await;
    }
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
    cleanup().await;
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
    cleanup().await;
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
    }
    // Compacted offsets still read as their leaves.
    assert_eq!(
        store.read(1, 4).await.unwrap(),
        Slot::Compacted { leaf: leaves[4] }
    );
    assert!(matches!(store.read(1, 6).await.unwrap(), Slot::Record(_)));
    cleanup().await;
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
    cleanup().await;
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
    cleanup().await;
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
    cleanup().await;
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
    cleanup().await;
}
