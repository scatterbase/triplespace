//! Compacting a mirror over Postgres (0013 §3, A96): the keys of a range of offsets keep
//! their newest record and the one carrying their highest revision ID; a retained key and
//! a `full` partition are left whole. Named by `TRIPLESPACE_TEST_DATABASE_URL`; vacuous
//! without it.

use std::sync::atomic::{AtomicU32, Ordering};

use scatter_ingest::IngestStore;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::registry::{Export, History, Integrity};
use scatter_log::store::{Draft, LogStore};
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, PgLog, log};
use scatter_projection::Backend;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::PgIngest;
use triplespace_projections::PgBackend;

static COUNTER: AtomicU32 = AtomicU32::new(0);

const MIRROR: u64 = 0x4001;
const FULL: u64 = 0x4002;

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn fresh(admin: Config) -> (Client, PgIngest) {
    let name = format!(
        "tscompact_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    connect(&admin)
        .await
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(&name);
    let mut config = admin;
    config.dbname(&name);
    let mut client = connect(&config).await;
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .expect("migrate");
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    let url = url
        .rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap();
    let pool = triplespace_db::pool::pool(&url, 2).unwrap();
    (client, PgIngest::new(PgBackend::new(pool)))
}

async fn partition(client: &Client, partition: u64, name: &str, history: History) {
    log::create_partition(
        client,
        partition,
        Segments::new(8).unwrap(),
        &PartitionInfo {
            tenant: None,
            name: name.into(),
            graph_iri: format!("https://scatter.example/instance/graph/{name}"),
            history,
            integrity: Integrity::Hashed,
            export: Export::Public,
            hash: "sha-256".into(),
            origin: None,
        },
    )
    .await
    .unwrap();
}

fn record(n: u64, key: &str, revid: Option<u64>) -> Draft {
    Draft {
        appended_at: 1_790_000_000_000_000 + n,
        payload_type: "scatter:v0/changeset".into(),
        key: Some(key.into()),
        revid,
        logid: None,
        page_id: Some(100),
        body: Body::core(
            &Value::map(vec![(Value::text("n"), Value::Int(i128::from(n)))]),
            &Value::Null,
            &Value::map(vec![(
                Value::text("actor"),
                Value::text("instance:scatter"),
            )]),
        )
        .unwrap(),
    }
}

async fn offsets(client: &Client, partition: u64) -> Vec<i64> {
    client
        .query(
            "SELECT \"offset\" FROM log.record WHERE partition = $1 ORDER BY 1",
            &[&scatter_log_postgres::ids::partition_to_db(partition)],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

async fn compact_range(store: &PgIngest, partition: u64, from: u64, to: u64) -> u64 {
    let mut cx = store.begin().await.unwrap();
    let n = store
        .compact_range(&mut cx, partition, from, to)
        .await
        .unwrap();
    store.commit(cx).await.unwrap();
    n
}

#[tokio::test]
async fn a_mirror_compacts_by_the_keys_of_a_range() {
    let Some(admin) = admin_config() else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let (client, store) = fresh(admin).await;
    partition(&client, MIRROR, "mirror/wikidata", History::Latest).await;
    partition(&client, FULL, "mirror/kept", History::Full).await;
    let records = [
        ("Q1", Some(10)), // 0
        ("Q2", Some(20)), // 1
        ("Q3", Some(30)), // 2
        ("Q1", Some(11)), // 3: Q1's newest state
        ("Q2", None),     // 4: Q2 deleted; 1 still claims revision 20
        ("Q3", Some(31)), // 5: Q3 is retained
        ("Q4", Some(40)), // 6: Q4 is outside the range
        ("Q4", Some(41)), // 7
    ];
    let mut log = PgLog::new(client);
    for p in [MIRROR, FULL] {
        for (n, (key, revid)) in (0u64..).zip(records) {
            log.append(p, record(n, key, revid)).await.unwrap();
        }
    }
    let client = log.into_inner();
    client
        .execute(
            "INSERT INTO view.entity (tenant, id, type, page_id, canonical_id, retention,
                                      first_seen, resolved_version, resolved_kind, resolved_size)
             VALUES ('librarybase', 'Q3', 'item', 100, 'Q3', 'retain', now(), 0, 'source', 0)",
            &[],
        )
        .await
        .unwrap();

    assert_eq!(compact_range(&store, MIRROR, 3, 6).await, 1);
    assert_eq!(offsets(&client, MIRROR).await, [1, 2, 3, 4, 5, 6, 7]);
    // Again: nothing more goes.
    assert_eq!(compact_range(&store, MIRROR, 3, 6).await, 0);
    // A `full` partition keeps everything.
    assert_eq!(compact_range(&store, FULL, 0, 8).await, 0);
    assert_eq!(offsets(&client, FULL).await.len(), 8);
    // The highest revision a key claims survives its deletion.
    let mut cx = store.begin().await.unwrap();
    assert_eq!(
        store.highest_revid(&mut cx, MIRROR, "Q2").await.unwrap(),
        Some(20)
    );
    store.rollback(cx).await.unwrap();
}
