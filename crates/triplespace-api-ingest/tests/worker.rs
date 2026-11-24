//! The work queue's operator commands and the page ID lookup, against a real database
//! named by `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::sync::atomic::{AtomicU32, Ordering};

use scatter_log::registry::{Export, History, Integrity};
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, log};
use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::worker::{Which, discard, parked, retry};

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn fresh() -> Option<Client> {
    let Ok(url) = std::env::var("TRIPLESPACE_TEST_DATABASE_URL") else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return None;
    };
    let admin: Config = url.parse().expect("the URL parses");
    let name = format!(
        "tsworker_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let connect = |config: Config| async move {
        let (client, connection) = config.connect(NoTls).await.expect("connect");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
    };
    connect(admin.clone())
        .await
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(&name);
    let mut config = admin;
    config.dbname(&name);
    let mut client = connect(config).await;
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .expect("migrate");
    Some(client)
}

// 03 §6.1 (0083 §3, A4): an item that failed as often as the worker retries is parked;
// an operator lists the parked items, puts them back in the queue or removes them, all
// or those of one consumer, tenant or key. Items still being retried are left alone.
#[tokio::test]
async fn parked_items_are_listed_retried_and_discarded() {
    let Some(client) = fresh().await else {
        return;
    };
    client
        .batch_execute(
            "INSERT INTO ops.projection_work (projection, tenant, partition, \"offset\", key, attempts, last_error)
             VALUES ('report', 'alpha', 1, 1, 'Main Page', 5, 'boom'),
                    ('report', 'beta', 1, 2, 'Other', 7, 'bang'),
                    ('entity', 'alpha', 1, 3, 'Q1', 5, 'no record'),
                    ('entity', 'alpha', 1, 4, 'Q2', 2, 'still trying'),
                    ('entity', '', 1, 5, 'WDQ1', 0, NULL)",
        )
        .await
        .unwrap();
    let all = Which::default();
    let keys = |items: Vec<triplespace_api_ingest::worker::Parked>| -> Vec<String> {
        items.into_iter().map(|p| p.key).collect()
    };
    let listed = parked(&client, 5, &all, 100).await.unwrap();
    assert_eq!(listed[0].last_error.as_deref(), Some("boom"));
    assert_eq!(keys(listed), ["Main Page", "Other", "Q1"]);
    let alpha = Which {
        tenant: Some("alpha".into()),
        ..Which::default()
    };
    assert_eq!(
        keys(parked(&client, 5, &alpha, 100).await.unwrap()),
        ["Main Page", "Q1"]
    );

    // One consumer's are retried: back in the queue, attempts forgotten.
    let reports = Which {
        consumer: Some("report".into()),
        ..Which::default()
    };
    assert_eq!(retry(&client, 5, &reports).await.unwrap(), 2);
    assert_eq!(keys(parked(&client, 5, &all, 100).await.unwrap()), ["Q1"]);
    let waiting: i64 = client
        .query_one(
            "SELECT count(*) FROM ops.projection_work WHERE projection = 'report' AND attempts = 0",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(waiting, 2);

    // One key's is discarded; what was never parked is untouched.
    let q1 = Which {
        key: Some("Q1".into()),
        ..Which::default()
    };
    assert_eq!(discard(&client, 5, &q1).await.unwrap(), 1);
    assert!(parked(&client, 5, &all, 100).await.unwrap().is_empty());
    assert_eq!(discard(&client, 5, &all).await.unwrap(), 0);
    let left: i64 = client
        .query_one("SELECT count(*) FROM ops.projection_work", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(left, 4);
}

// 0013 A109: the page ID lookup opens the record tables of the tenant's partitions and the
// shared ones, never another tenant's.
#[tokio::test]
async fn the_page_id_lookup_opens_only_the_tenants_partitions() {
    let Some(client) = fresh().await else {
        return;
    };
    for (partition, tenant, name) in [
        (0x3001_u64, Some("alpha"), "local"),
        (0x3002, Some("beta"), "local"),
        (0x3003, None, "mirror/wikidata"),
    ] {
        log::create_partition(
            &client,
            partition,
            Segments::new(8).unwrap(),
            &PartitionInfo {
                tenant: tenant.map(str::to_owned),
                name: name.into(),
                graph_iri: format!("https://scatter.example/graph/{name}/{partition}"),
                history: History::Full,
                integrity: Integrity::Logged,
                export: Export::Public,
                hash: "sha-256".into(),
                origin: None,
            },
        )
        .await
        .unwrap();
    }
    let keys = vec!["Q1".to_string()];
    for (sql, key) in [
        (
            triplespace_api_ingest::store::PAGE_ID_OF,
            &"Q1" as &(dyn tokio_postgres::types::ToSql + Sync),
        ),
        (triplespace_api_ingest::store::PAGE_IDS_OF, &keys),
    ] {
        explain(&client, sql, key).await;
    }
}

/// Runs the query for tenant `alpha` and checks which record tables its plan opened.
async fn explain(client: &Client, sql: &str, key: &(dyn tokio_postgres::types::ToSql + Sync)) {
    let plan: String = client
        .query(
            &format!("EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF) {sql}"),
            &[key, &Some("alpha")],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect::<Vec<_>>()
        .join("\n");
    let scanned = |table: &str| {
        plan.lines()
            .any(|l| l.contains(table) && !l.contains("never executed"))
    };
    assert!(scanned("record_0000000000003001"), "{plan}");
    assert!(scanned("record_0000000000003003"), "{plan}");
    assert!(!scanned("record_0000000000003002"), "{plan}");
}
