//! The row rules of the feeds, stated once as SQL (`feed::visible_targets`,
//! `feed::visible_suppressions`; 0006 A31), against a real database named by
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it. Fixture rows are written straight
//! into `view.activity`, `view.page`, `view.entity` and `log.partition`, and the rules are
//! run both ways they are used: as the feed's `WHERE`, through `feed::query`, and as a
//! selected column per row, as an export bundle's reader runs them (0103 §1).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio_postgres::{Client, Config, NoTls};
use triplespace_projections::feed::{self, Feed, Hidden, Viewer};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const TENANT: &str = "librarybase";
const LOCAL: i64 = 0x1001;
const PAGES: i64 = 0x1002;
const MIRROR: i64 = 0x1003;
const CONFIG: i64 = 0x1004;

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

/// A fresh, migrated database of this test's own, dropped when the thread ends.
async fn fresh() -> Client {
    let admin = admin_config().expect("checked by the caller");
    let name = format!(
        "tsvis_test_{}_{}",
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
    client
}

fn skipped() -> bool {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return true;
    }
    false
}

/// The fixture: the cases `Hidden::hides_activity` once pinned row by row, as rows.
/// Offsets name the cases; every row is on `LOCAL` unless the case is about its partition.
async fn fixture(db: &Client) {
    db.batch_execute(&format!(
        "INSERT INTO log.partition (partition, tenant, name, graph_iri, history, integrity, export, segment_k, hash) VALUES
           ({LOCAL}, '{TENANT}', 'local', 'x', 'full', 'logged', 'public', 10, 'sha-256'),
           ({PAGES}, '{TENANT}', 'pages', 'x', 'full', 'logged', 'public', 10, 'sha-256'),
           ({MIRROR}, '{TENANT}', 'mirror/wikidata', 'x', 'latest', 'hashed', 'public', 10, 'sha-256'),
           ({CONFIG}, '{TENANT}', 'config', 'x', 'full', 'logged', 'internal', 10, 'sha-256');
         INSERT INTO view.page (tenant, page_id, ns, title, content_model) VALUES
           ('{TENANT}', 42, 0, 'Hidden page', 'wikitext'),
           ('{TENANT}', 7, 4, 'Project page', 'wikitext'),
           ('{TENANT}', 8, 0, 'Plain page', 'wikitext');
         INSERT INTO view.entity (tenant, id, type, page_id, canonical_id, first_seen, resolved_version, resolved_kind, resolved_size) VALUES
           ('{TENANT}', 'Q5', 'item', 1005, 'Q5', now(), 1, 'source', 0),
           ('{TENANT}', 'Q6', 'item', 1006, 'Q6', now(), 1, 'source', 0),
           ('{TENANT}', 'P1', 'property', 1101, 'P1', now(), 1, 'source', 0),
           ('', 'WDQ1', 'item', 1201, 'WDQ1', now(), 1, 'source', 0);
         INSERT INTO view.activity (tenant, time, partition, \"offset\", kind, source, target_kind, target_id, log_type, log_action, params) VALUES
           -- 1 a hidden entity, 2 a visible one
           ('{TENANT}', now() - interval '20 s', {LOCAL}, 1, 'edit', 'local', 1, 'Q5', NULL, NULL, NULL),
           ('{TENANT}', now() - interval '19 s', {LOCAL}, 2, 'edit', 'local', 1, 'Q6', NULL, NULL, NULL),
           -- 3 a hidden page, 4 a page of a hidden namespace, 5 a plain page
           ('{TENANT}', now() - interval '18 s', {PAGES}, 3, 'edit', 'pages', 2, '42', NULL, NULL, NULL),
           ('{TENANT}', now() - interval '17 s', {PAGES}, 4, 'edit', 'pages', 2, '7', NULL, NULL, NULL),
           ('{TENANT}', now() - interval '16 s', {PAGES}, 5, 'edit', 'pages', 2, '8', NULL, NULL, NULL),
           -- 6, 7 revision-deletion events follow the record they name
           ('{TENANT}', now() - interval '15 s', {LOCAL}, 6, 'log', 'log', 5, '4097:1', 'delete', 'revision', '{{\"of\": {{\"kind\": 1, \"id\": \"Q5\"}}}}'),
           ('{TENANT}', now() - interval '14 s', {LOCAL}, 7, 'log', 'log', 5, '4097:2', 'delete', 'revision', '{{\"of\": {{\"kind\": 1, \"id\": \"Q6\"}}}}'),
           -- 8 a hidden namespace ACL's own event, 9 another namespace's
           ('{TENANT}', now() - interval '13 s', {CONFIG}, 8, 'log', 'log', NULL, 'acl:namespace:4', 'protect', 'protect', '{{}}'),
           ('{TENANT}', now() - interval '12 s', {CONFIG}, 9, 'log', 'log', NULL, 'acl:namespace:5', 'protect', 'protect', '{{}}'),
           -- 10 every row of a restricted graph's partition
           ('{TENANT}', now() - interval '11 s', {MIRROR}, 10, 'sync', 'mirror/wikidata', 1, 'WDQ1', NULL, NULL, NULL),
           -- 11 an entity of a hidden entity namespace, by its type
           ('{TENANT}', now() - interval '10 s', {LOCAL}, 11, 'edit', 'local', 1, 'P1', NULL, NULL, NULL),
           -- 12 a revision deletion of a page in a hidden namespace
           ('{TENANT}', now() - interval '9 s', {LOCAL}, 12, 'log', 'log', 5, '4098:4', 'delete', 'revision', '{{\"of\": {{\"kind\": 2, \"id\": \"7\"}}}}'),
           -- 13 a suppression for one group, 14 one naming no group
           ('{TENANT}', now() - interval '8 s', {LOCAL}, 13, 'log', 'log', 5, '4097:1', 'suppress', 'revision', '{{\"restrictions\": {{\"read\": {{\"group\": \"oversight\"}}}}}}'),
           ('{TENANT}', now() - interval '7 s', {LOCAL}, 14, 'log', 'log', 5, '4097:2', 'suppress', 'revision', '{{}}');"
    ))
    .await
    .expect("fixture");
}

/// What `viewer` sees of the fixture through the feed.
async fn through_feed(db: &Client, viewer: &Viewer) -> BTreeSet<u64> {
    let rc = Feed::everything(100);
    feed::query(db, TENANT, &rc, viewer)
        .await
        .expect("feed")
        .rows
        .iter()
        .map(|r| r.offset)
        .collect()
}

/// What `viewer` sees of the fixture with the rules selected per row, as the export
/// reader runs them: the offsets whose target is visible and whose suppression is.
async fn as_columns(db: &Client, viewer: &Viewer) -> BTreeSet<u64> {
    let mut params: Vec<feed::Param> = vec![Box::new(TENANT.to_string())];
    let targets =
        feed::visible_targets(&viewer.hidden, &mut params).unwrap_or_else(|| "true".into());
    let suppressions = feed::visible_suppressions(viewer, &mut params);
    let sql = format!(
        "SELECT a.\"offset\", coalesce(({targets}), false), coalesce(({suppressions}), false)
         FROM view.activity a WHERE a.tenant = $1"
    );
    let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
        .iter()
        .map(|p| p.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    db.query(&sql, &refs)
        .await
        .expect("columns")
        .iter()
        .filter(|r| r.get::<_, bool>(1) && r.get::<_, bool>(2))
        .map(|r| u64::try_from(r.get::<_, i64>(0)).unwrap())
        .collect()
}

fn set(offsets: &[u64]) -> BTreeSet<u64> {
    offsets.iter().copied().collect()
}

/// The row rules, once, both ways they are run (0006 A31; 0103 §1).
#[tokio::test]
async fn the_rules_decide_rows_the_same_in_a_where_and_as_a_column() {
    if skipped() {
        return;
    }
    let db = fresh().await;
    fixture(&db).await;
    let all: BTreeSet<u64> = (1..=14).collect();

    // Nothing hidden, no groups: everything but the suppressions.
    let anyone = Viewer::default();
    let seen = through_feed(&db, &anyone).await;
    assert_eq!(seen, &all - &set(&[13, 14]), "{seen:?}");
    assert_eq!(as_columns(&db, &anyone).await, seen);

    // The pinned cases: an entity, a page, a namespace, a namespace ACL's key and a graph.
    let mut hidden = Hidden::default();
    hidden.entities.insert("Q5".into());
    hidden.pages.insert(42);
    hidden.namespaces.insert(4);
    hidden.acl_keys.insert("acl:namespace:4".into());
    hidden.graphs.insert("mirror/wikidata".into());
    let viewer = Viewer {
        groups: vec!["oversight".into()],
        hidden,
        ..Viewer::default()
    };
    let seen = through_feed(&db, &viewer).await;
    assert_eq!(seen, set(&[2, 5, 7, 9, 11, 13]), "{seen:?}");
    assert_eq!(as_columns(&db, &viewer).await, seen);

    // An entity namespace hides its entities by type; `all_pages` every page's row and
    // revision deletion, and leaves the other rules in force.
    let mut hidden = Hidden {
        all_pages: true,
        ..Hidden::default()
    };
    hidden.namespaces.insert(122);
    hidden.entities.insert("Q5".into());
    hidden.graphs.insert("mirror/wikidata".into());
    let viewer = Viewer {
        every: true,
        excluded: vec!["oversight".into()],
        hidden,
        ..Viewer::default()
    };
    let seen = through_feed(&db, &viewer).await;
    assert_eq!(seen, set(&[2, 7, 8, 9]), "{seen:?}");
    assert_eq!(as_columns(&db, &viewer).await, seen);

    // A private tenant: nothing.
    let viewer = Viewer {
        every: true,
        hidden: Hidden {
            tenant: true,
            ..Hidden::default()
        },
        ..Viewer::default()
    };
    assert!(through_feed(&db, &viewer).await.is_empty());
    assert!(as_columns(&db, &viewer).await.is_empty());
}
