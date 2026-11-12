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
    triplespace_db::scratch::drop_at_thread_end(&name);
    let mut config = admin;
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
async fn migrations_apply_once_and_notice_edits() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    let m = migrator();
    // Before anything is applied, everything is pending; a database an older binary
    // made is the same from the newer binary's side, which is what `triplespace migrate`
    // and the server's startup check read.
    let pending = m.pending(&client).await.unwrap();
    assert_eq!(pending.len(), m.migrations().count());
    assert_eq!(
        pending[0],
        ("scatter-log-postgres".into(), "0001_log".into())
    );
    let first = m.apply(&mut client).await.unwrap();
    assert_eq!(first.applied.len(), m.migrations().count());
    assert!(first.skipped.is_empty());
    assert!(m.pending(&client).await.unwrap().is_empty());
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
    triplespace_db::ops::check(&client).await.unwrap();

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
}

/// 0103 A4: an `ops` table without a backup class, or whose comment disagrees with
/// `OPS_TABLES`, is caught, as a `private` table without a portability class is.
#[tokio::test]
async fn an_unclassified_ops_table_is_caught() {
    use triplespace_db::ops;
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    migrator().apply(&mut client).await.unwrap();
    ops::check(&client).await.unwrap();
    client
        .batch_execute("CREATE TABLE ops.stray (a int)")
        .await
        .unwrap();
    assert!(matches!(
        ops::check(&client).await,
        Err(ops::OpsError::Unclassified(t)) if t == "stray"
    ));
    client
        .batch_execute("DROP TABLE ops.stray; COMMENT ON TABLE ops.delivery IS 'backup: carried'")
        .await
        .unwrap();
    assert!(matches!(
        ops::check(&client).await,
        Err(ops::OpsError::Disagrees { .. })
    ));
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
}

/// 0013 §4, A98; 03 §1.2: the accounts partition is hidden from the public role however it
/// is read. Postgres checks privileges only on the table a query names, so revoking the
/// child table is not enough while the public role may read the parent `log.record`: the
/// parent's row-level security excludes the accounts partition's rows, for reads and for
/// writes.
#[tokio::test]
async fn the_public_role_cannot_read_accounts_through_the_parent() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    migrator().apply(&mut client).await.unwrap();
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
    client
        .batch_execute("UPDATE log.partition SET name = 'accounts' WHERE partition = 8")
        .await
        .unwrap();
    grant_partition(&client, 7, false).await.unwrap();
    grant_partition(&client, 8, true).await.unwrap();

    let count_as = |r: &'static str, partition: i64| {
        let c = &client;
        async move {
            c.batch_execute(&format!("SET ROLE {r}")).await.unwrap();
            let n = c
                .query_one(
                    "SELECT count(*) FROM log.record WHERE partition = $1",
                    &[&partition],
                )
                .await
                .map(|row| row.get::<_, i64>(0));
            c.batch_execute("RESET ROLE").await.unwrap();
            n
        }
    };
    for r in [role::SERVER, role::NOTIFY, role::FEDERATION, role::VERIFY] {
        assert_eq!(
            count_as(r, 7).await.unwrap(),
            1,
            "{r} reads a public partition"
        );
        assert!(
            !matches!(count_as(r, 8).await, Ok(n) if n > 0),
            "{r} read the accounts partition through log.record"
        );
    }
    assert_eq!(count_as(role::ACCOUNTS, 8).await.unwrap(), 1);
    assert_eq!(count_as(role::ACCOUNTS, 7).await.unwrap(), 1);

    // Nor may the public role write there through the parent; it may elsewhere.
    let insert_as = |r: &'static str, partition: i64| {
        let c = &client;
        async move {
            c.batch_execute(&format!("SET ROLE {r}")).await.unwrap();
            let out = c
                .execute(
                    "INSERT INTO log.record (partition, \"offset\", appended_at, payload_type, \
                     commitment, header, leaf, body) VALUES ($1, 99, 0, 'test', '', '', '', '')",
                    &[&partition],
                )
                .await;
            c.batch_execute("RESET ROLE").await.unwrap();
            out
        }
    };
    let refused = insert_as(role::SERVER, 8).await.unwrap_err();
    assert!(
        refused
            .as_db_error()
            .is_some_and(|e| e.message().contains("row-level security")),
        "{refused:?}"
    );
    assert_eq!(insert_as(role::SERVER, 7).await.unwrap(), 1);
}

/// 0013 §4, A101: the public role finds a request's caller through `ops.caller_session`
/// and `ops.caller_key`, and still may not read the tables they read; a session whose bot
/// password was revoked is ended by the lookup; a wrong secret finds nothing.
#[tokio::test]
async fn the_public_role_resolves_callers_without_reading_private() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    migrator().apply(&mut client).await.unwrap();
    // The SHA-256 of "s3cret", in hex: the function compares the digest it is given.
    let hash = "1ec1c26b50d5d3c58d9583181af8076655fe00756bf7285940ba3670f99fcba0";
    client
        .batch_execute(&format!(
            "INSERT INTO private.api_key (actor_key, key_id, label, hash, grants)
               VALUES ('actor:7', 'k1', 'bot', '{hash}', ARRAY['edit']),
                      ('actor:7', 'k2', 'old', '{hash}', ARRAY['edit']);
             UPDATE private.api_key SET revoked_at = now() WHERE key_id = 'k2';
             INSERT INTO private.session (id, tenant, actor_key, key_id, expires)
               VALUES ('plain', 'demo', 'actor:7', NULL, now() + interval '1 hour'),
                      ('bot', 'demo', 'actor:7', 'k1', now() + interval '1 hour'),
                      ('dead', 'demo', 'actor:7', 'k2', now() + interval '1 hour');"
        ))
        .await
        .unwrap();
    client
        .batch_execute(&format!("SET ROLE {}", role::SERVER))
        .await
        .unwrap();
    let session = |id: &'static str, tenant: &'static str| {
        let c = &client;
        async move {
            c.query_opt(
                "SELECT actor_key, key_id, key_grants FROM ops.caller_session($1, $2)",
                &[&id, &tenant],
            )
            .await
            .unwrap()
            .map(|r| {
                (
                    r.get::<_, Option<String>>(0),
                    r.get::<_, Option<String>>(1),
                    r.get::<_, Option<Vec<String>>>(2),
                )
            })
        }
    };
    assert_eq!(
        session("plain", "demo").await,
        Some((Some("actor:7".into()), None, None))
    );
    assert_eq!(
        session("bot", "demo").await,
        Some((
            Some("actor:7".into()),
            Some("k1".into()),
            Some(vec!["edit".into()])
        ))
    );
    assert_eq!(session("plain", "other").await, None, "another tenant's");
    assert_eq!(session("dead", "demo").await, None, "its key was revoked");
    let key = |id: &'static str, h: &'static str| {
        let c = &client;
        async move {
            c.query_opt("SELECT actor_key FROM ops.caller_key($1, $2)", &[&id, &h])
                .await
                .unwrap()
                .map(|r| r.get::<_, String>(0))
        }
    };
    assert_eq!(key("k1", hash).await, Some("actor:7".into()));
    assert_eq!(key("k1", "00").await, None, "a wrong secret");
    assert_eq!(key("k2", hash).await, None, "a revoked key");
    assert!(
        client
            .query("SELECT * FROM private.session", &[])
            .await
            .is_err(),
        "the public role still may not read sessions"
    );
    assert!(
        client
            .query("SELECT hash FROM private.api_key", &[])
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await.unwrap();
    let left: i64 = client
        .query_one(
            "SELECT count(*) FROM private.session WHERE id = 'dead'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(left, 0, "the dead session was ended");
}

/// Every grant to a `ts_*` role in the database: on tables and sequences, on schemas, as
/// default privileges, and as row-level security policies, as `(kind, object, grantee,
/// privilege)`, sorted.
async fn ts_grants(client: &Client) -> Vec<(String, String, String, String)> {
    client
        .query(
            "SELECT 'object', n.nspname || '.' || c.relname, a.grantee::regrole::text, a.privilege_type
               FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace, aclexplode(c.relacl) a
              WHERE n.nspname IN ('log', 'view', 'ops', 'private')
                AND a.grantee::regrole::text LIKE 'ts\\_%'
             UNION ALL
             SELECT 'schema', n.nspname, a.grantee::regrole::text, a.privilege_type
               FROM pg_namespace n, aclexplode(n.nspacl) a
              WHERE a.grantee::regrole::text LIKE 'ts\\_%'
             UNION ALL
             SELECT 'function', n.nspname || '.' || f.proname, a.grantee::regrole::text, a.privilege_type
               FROM pg_proc f JOIN pg_namespace n ON n.oid = f.pronamespace, aclexplode(f.proacl) a
              WHERE n.nspname IN ('log', 'view', 'ops', 'private')
                AND a.grantee::regrole::text LIKE 'ts\\_%'
             UNION ALL
             SELECT 'policy', p.schemaname || '.' || p.tablename || ':' || p.policyname,
                    array_to_string(p.roles, ','), coalesce(p.qual, '') || ' / ' || coalesce(p.with_check, '')
               FROM pg_policies p
              WHERE p.schemaname IN ('log', 'view', 'ops', 'private')
             UNION ALL
             SELECT 'default', coalesce(n.nspname, '') || ':' || d.defaclobjtype::text,
                    a.grantee::regrole::text, a.privilege_type
               FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid = d.defaclnamespace,
                    aclexplode(d.defaclacl) a
              WHERE a.grantee::regrole::text LIKE 'ts\\_%'
             ORDER BY 1, 2, 3, 4",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect()
}

/// 0013 §4, 08 §6.7: a database restored with `pg_restore --no-owner --no-privileges` has
/// its `ops.migration` rows and none of its grants; the next migration run gives back
/// every grant the migrations and the partitions' creation gave, accounts partition
/// excluded as before.
#[tokio::test]
async fn ensure_restores_what_migrations_granted() {
    if skipped() {
        return;
    }
    let mut client = fresh().await;
    migrator().apply(&mut client).await.unwrap();
    let mut log = PgLog::new(client);
    log.create_partition(7, Segments::new(4).unwrap())
        .await
        .unwrap();
    log.create_partition(8, Segments::new(4).unwrap())
        .await
        .unwrap();
    let mut client = log.into_inner();
    client
        .batch_execute("UPDATE log.partition SET name = 'accounts' WHERE partition = 8")
        .await
        .unwrap();
    grant_partition(&client, 7, false).await.unwrap();
    grant_partition(&client, 8, true).await.unwrap();
    let granted = ts_grants(&client).await;
    assert!(granted.contains(&(
        "object".into(),
        "private.email".into(),
        role::NOTIFY.into(),
        "SELECT".into()
    )));
    assert!(
        !granted
            .iter()
            .any(|g| g.1 == "log.record_0000000000000008" && g.2 != role::ACCOUNTS),
        "the accounts partition is hidden from every role but the accounts role"
    );
    assert_eq!(
        granted.iter().filter(|g| g.0 == "policy").count(),
        2,
        "log.record's two policies (A98)"
    );

    // What `--no-privileges` leaves: no grant to any of the roles; and what a restore onto
    // a server without the roles leaves: no policy that names one.
    let roles = role::ALL.join(", ");
    client
        .batch_execute(&format!(
            "REVOKE ALL ON ALL TABLES IN SCHEMA log, view, ops, private FROM {roles};
             REVOKE ALL ON ALL SEQUENCES IN SCHEMA log, view, ops, private FROM {roles};
             REVOKE ALL ON SCHEMA log, view, ops, private FROM {roles};
             ALTER DEFAULT PRIVILEGES IN SCHEMA log, view, ops, private REVOKE ALL ON TABLES FROM {roles};
             ALTER DEFAULT PRIVILEGES IN SCHEMA log, view, ops, private REVOKE ALL ON SEQUENCES FROM {roles};
             REVOKE ALL ON ALL FUNCTIONS IN SCHEMA ops FROM {roles};
             DROP POLICY ts_accounts_reads_all ON log.record;
             DROP POLICY ts_public_skips_accounts ON log.record;"
        ))
        .await
        .unwrap();
    assert_eq!(ts_grants(&client).await, Vec::new());

    let applied = migrator().apply(&mut client).await.unwrap();
    assert!(applied.applied.is_empty(), "nothing to migrate");
    let ensured = applied.grants.expect("the migrator ensures grants");
    assert_eq!(ensured.partitions, 2);
    assert_eq!(ts_grants(&client).await, granted);

    // A second run changes nothing.
    migrator().apply(&mut client).await.unwrap();
    assert_eq!(ts_grants(&client).await, granted);
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
}
