//! Integration tests against a real database named by `TRIPLESPACE_TEST_DATABASE_URL`
//! (a role that may create databases and roles). Without it they pass vacuously.

use std::sync::atomic::{AtomicU32, Ordering};

use scatter_log::conformance::draft;
use scatter_log::store::LogStore;
use scatter_log::tree::Segments;
use scatter_log_postgres::{Migration, PgLog};
use tokio_postgres::{Client, Config, NoTls};
use triplespace_db::grants::{grant_partition, role};
use triplespace_db::migrate::MigrateError;
use triplespace_db::routing::{Lsn, Route, current_lsn, replay_lsn, route};
use triplespace_db::{Migrator, migrator, private, projection_state};

static COUNTER: AtomicU32 = AtomicU32::new(0);
static EDITED: &[Migration] = &[Migration {
    name: "0001_log",
    sql: "SELECT 1",
}];
static BROKEN: &[Migration] = &[Migration {
    name: "9999_broken",
    sql: "CREATE TABLE ops.half (a int); SELECT nonsense;",
}];
thread_local! {
    /// The scratch databases this test made. Per thread, not per process: the tests run
    /// in parallel, each on its own thread, and `cleanup` must drop only its own
    /// databases — `DROP DATABASE … WITH (FORCE)` on another test's would terminate
    /// that test's connections mid-run (SQLSTATE 57P01).
    static CREATED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
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

/// A fresh, empty database and a client on it.
async fn fresh() -> Client {
    let admin = admin_config().expect("checked by the caller");
    let name = format!(
        "tsdb_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    connect(&admin)
        .await
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    CREATED.with_borrow_mut(|c| c.push(name.clone()));
    let mut config = admin;
    config.dbname(&name);
    connect(&config).await
}

async fn cleanup() {
    let Some(admin) = admin_config() else { return };
    let client = connect(&admin).await;
    let names: Vec<String> = CREATED.with_borrow_mut(std::mem::take);
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
async fn migrations_apply_once_and_notice_edits() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    let m = migrator();
    let first = m.apply(&mut client).await.unwrap();
    assert_eq!(first.applied.len(), m.migrations().count());
    assert!(first.skipped.is_empty());
    let second = m.apply(&mut client).await.unwrap();
    assert!(second.applied.is_empty());
    assert_eq!(second.skipped.len(), first.applied.len());
    let status = m.status(&client).await.unwrap();
    assert!(status.iter().all(|s| s.applied_at.is_some()));

    // Every schema is there and the private tables are classified.
    for schema in ["log", "view", "private", "ops"] {
        let n: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.tables WHERE table_schema = $1",
                &[&schema],
            )
            .await
            .unwrap()
            .get(0);
        assert!(n > 0, "{schema} has tables");
    }
    private::check(&client).await.unwrap();

    // An edited migration is refused.
    let edited = Migrator::new().with("scatter-log-postgres", EDITED);
    assert!(matches!(
        edited.apply(&mut client).await,
        Err(MigrateError::Changed { .. })
    ));
    // A failing migration applies nothing and records nothing.
    let broken = Migrator::new().with("triplespace-db", BROKEN);
    assert!(matches!(
        broken.apply(&mut client).await,
        Err(MigrateError::Sql { .. })
    ));
    let half = client
        .query_opt("SELECT 1 FROM information_schema.tables WHERE table_schema = 'ops' AND table_name = 'half'", &[])
        .await
        .unwrap();
    assert!(half.is_none(), "rolled back");
    let recorded = client
        .query_opt(
            "SELECT 1 FROM ops.migration WHERE name = '9999_broken'",
            &[],
        )
        .await
        .unwrap();
    assert!(recorded.is_none());
    cleanup().await;
}

#[tokio::test]
async fn an_unclassified_private_table_is_caught() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    migrator().apply(&mut client).await.unwrap();
    client
        .batch_execute("CREATE TABLE private.stray (a int)")
        .await
        .unwrap();
    assert!(matches!(
        private::check(&client).await,
        Err(private::PrivateError::Unclassified(t)) if t == "stray"
    ));
    client
        .batch_execute("DROP TABLE private.stray; COMMENT ON TABLE private.password IS 'portability: portable'")
        .await
        .unwrap();
    assert!(matches!(
        private::check(&client).await,
        Err(private::PrivateError::Disagrees { .. })
    ));
    cleanup().await;
}

#[tokio::test]
async fn the_public_role_cannot_read_private() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    migrator().apply(&mut client).await.unwrap();
    // A log partition for the grants to apply to, and an accounts one that is hidden.
    let mut log = PgLog::new(client);
    log.create_partition(7, Segments::new(4).unwrap())
        .await
        .unwrap();
    log.create_partition(8, Segments::new(4).unwrap())
        .await
        .unwrap();
    log.append(7, draft(0)).await.unwrap();
    log.append(8, draft(1)).await.unwrap();
    let client = log.into_inner();
    grant_partition(&client, 7, false).await.unwrap();
    grant_partition(&client, 8, true).await.unwrap();

    let as_role = |r: &str, sql: &str| {
        let sql = sql.to_string();
        let r = r.to_string();
        let c = &client;
        async move {
            c.batch_execute(&format!("SET ROLE {r}")).await.unwrap();
            let out = c.query(&sql, &[]).await.map(|rows| rows.len());
            c.batch_execute("RESET ROLE").await.unwrap();
            out
        }
    };
    assert!(
        as_role(role::SERVER, "SELECT * FROM view.entity")
            .await
            .is_ok()
    );
    assert!(
        as_role(role::SERVER, "SELECT * FROM ops.projection_state")
            .await
            .is_ok()
    );
    assert_eq!(
        as_role(role::SERVER, "SELECT * FROM log.record_0000000000000007")
            .await
            .unwrap(),
        1
    );
    assert!(
        as_role(role::SERVER, "SELECT * FROM private.password")
            .await
            .is_err(),
        "0013 §4"
    );
    assert!(
        as_role(role::SERVER, "SELECT * FROM private.api_key")
            .await
            .is_err()
    );
    assert!(
        as_role(role::SERVER, "SELECT * FROM log.record_0000000000000008")
            .await
            .is_err(),
        "the accounts partition is hidden from the public role"
    );
    assert!(
        as_role(role::ACCOUNTS, "SELECT * FROM private.password")
            .await
            .is_ok()
    );
    assert_eq!(
        as_role(role::ACCOUNTS, "SELECT * FROM log.record_0000000000000008")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        as_role(role::VERIFY, "SELECT * FROM log.record_0000000000000007")
            .await
            .unwrap(),
        1
    );
    assert!(
        as_role(role::VERIFY, "SELECT * FROM view.entity")
            .await
            .is_err()
    );
    assert!(as_role(role::VERIFY, "INSERT INTO log.partition (partition, name, graph_iri, history, integrity, export, segment_k, hash) VALUES (99, 'x', '', 'full', 'logged', 'public', 1, 'sha-256') RETURNING partition").await.is_err());
    cleanup().await;
}

#[tokio::test]
async fn projection_state_and_lag() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    migrator().apply(&mut client).await.unwrap();
    let mut log = PgLog::new(client);
    log.create_partition(3, Segments::new(4).unwrap())
        .await
        .unwrap();
    for n in 0..5 {
        log.append(3, draft(n)).await.unwrap();
    }
    let client = log.into_inner();
    assert_eq!(
        projection_state::applied(&client, "entity", 3)
            .await
            .unwrap(),
        0
    );
    projection_state::set_applied(&client, "entity", 3, 2)
        .await
        .unwrap();
    projection_state::set_applied(&client, "term", 3, 5)
        .await
        .unwrap();
    projection_state::set_applied(&client, "entity", 3, 3)
        .await
        .unwrap();
    assert_eq!(
        projection_state::applied(&client, "entity", 3)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        projection_state::lags(&client, 3).await.unwrap(),
        vec![("entity".to_string(), 3, 5), ("term".to_string(), 5, 5)]
    );
    cleanup().await;
}

#[tokio::test]
async fn lsn_routing_on_a_primary() {
    if skipped() {
        return;
    }
    let client = fresh().await;
    let before = current_lsn(&client).await.unwrap();
    client
        .batch_execute("CREATE TABLE t (a int); INSERT INTO t VALUES (1)")
        .await
        .unwrap();
    let after = current_lsn(&client).await.unwrap();
    assert!(after > before, "writes advance the WAL");
    assert_eq!(
        replay_lsn(&client).await.unwrap(),
        None,
        "a primary replays nothing"
    );
    assert_eq!(route(Some(after), None), Route::Primary);
    assert_eq!(route(Some(before), Some(after)), Route::Replica);
    assert_eq!(Lsn::parse(&after.to_text()), Some(after));
    cleanup().await;
}

#[tokio::test]
async fn pools_connect() {
    if skipped() {
        return;
    }
    let client = fresh().await;
    let name: String = client
        .query_one("SELECT current_database()", &[])
        .await
        .unwrap()
        .get(0);
    let mut config = admin_config().unwrap();
    config.dbname(&name);
    // Rebuild a URL the pool can parse from the pieces we have.
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    let url = url
        .rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap();
    let pool = triplespace_db::pool::pool(&url, 2).unwrap();
    let c = pool.get().await.unwrap();
    let one: i32 = c.query_one("SELECT 1", &[]).await.unwrap().get(0);
    assert_eq!(one, 1);
    assert!(triplespace_db::pool::pool("not a url", 1).is_err());
    cleanup().await;
}
