//! The step-1 projections against a real database named by
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use ed25519_dalek::SigningKey;
use scatter_actors::acl::{Acl, ReadKind, Target};
use scatter_actors::actor::ActorRecord;
use scatter_actors::issuer::IssuerRegistry;
use scatter_actors::key::ActorKey;
use scatter_actors::membership::{Block, Membership};
use scatter_log::body::{ATTESTATION, Body, CONTENT};
use scatter_log::cbor::{self, Value};
use scatter_log::genesis::Genesis;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{
    Export, GraphRegistry, History, Integrity, KeyEntry, PAYLOAD_CONFIG, Scope,
};
use scatter_log::store::{Draft, LogStore};
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, PgLog, log};
use scatter_projection::{Backend, Budget, Pipeline};
use tokio_postgres::{Client, Config, NoTls};
use triplespace_projections::actors::payload;
use triplespace_projections::{Farm, PgBackend, milestone_pipeline};

static COUNTER: AtomicU32 = AtomicU32::new(0);
static CREATED: Mutex<Vec<String>> = Mutex::new(Vec::new());

const TENANT: &str = "librarybase";
const ACTORS: u64 = 0x1001;
const TENANT_LOG: u64 = 0x1002;

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

/// A fresh, migrated database: its name, a client, and a pool-backed projection backend.
async fn fresh() -> (String, Client, PgBackend) {
    let admin = admin_config().expect("checked by the caller");
    let name = format!(
        "tsproj_test_{}_{}",
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
    let pool = triplespace_db::pool::pool(&url, 4).unwrap();
    (name, client, PgBackend::new(pool))
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

fn farm() -> Farm {
    Farm {
        slug: "scatter".into(),
        base: "https://scatter.example".into(),
    }
}

fn pipeline() -> Pipeline<PgBackend> {
    milestone_pipeline(IssuerRegistry::default_registry().clone(), farm()).unwrap()
}

fn draft(payload_type: &str, key: &str, content: &Value) -> Draft {
    Draft {
        appended_at: 1_790_000_000_000_000,
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: Some(1),
        page_id: None,
        body: Body::core(
            content,
            &Value::text("test"),
            &Value::map(vec![(Value::text("actor"), Value::text("local:1"))]),
        )
        .unwrap(),
    }
}

fn config_draft(kind: &str, code: &str, entry: &Value) -> Draft {
    draft(PAYLOAD_CONFIG, &format!("{kind}:{code}"), entry)
}

fn serde_value<T: serde::Serialize>(v: &T) -> Value {
    cbor::to_value(v).unwrap()
}

/// Genesis, a tenant with its actors and log partitions, and a set of actor-side records.
#[allow(clippy::too_many_lines)]
async fn seed(client: Client) -> Client {
    let key = SigningKey::from_bytes(&[17; 32]);
    let mut store = PgLog::new(client);
    Genesis {
        partition: CONFIG_PARTITION,
        scope: Scope::Instance,
        key: KeyEntry::ed25519(key.verifying_key().to_bytes()),
        segment_exponent: 16,
        appended_at: 1,
        attestation: Value::map(vec![(
            Value::text("actor"),
            Value::text("instance:scatter"),
        )]),
        comment: None,
    }
    .append(&mut store, &GraphRegistry::embedded())
    .await
    .unwrap();
    store
        .append(
            0,
            config_draft(
                "tenant",
                TENANT,
                &Value::map(vec![
                    (Value::text("kind"), Value::text("tenant")),
                    (Value::text("slug"), Value::text(TENANT)),
                    (Value::text("base"), Value::text("https://librarybase.org")),
                ]),
            ),
        )
        .await
        .unwrap();
    store
        .append(
            0,
            config_draft(
                "group",
                "sysop",
                &Value::map(vec![
                    (Value::text("kind"), Value::text("group")),
                    (Value::text("name"), Value::text("sysop")),
                    (
                        Value::text("permissions"),
                        Value::Array(vec![
                            Value::text("edit"),
                            Value::text("delete"),
                            Value::text("block"),
                        ]),
                    ),
                ]),
            ),
        )
        .await
        .unwrap();
    let client = store.into_inner();
    for (partition, name, export) in [
        (ACTORS, "actors", Export::Internal),
        (TENANT_LOG, "log", Export::Internal),
    ] {
        log::create_partition(
            &client,
            partition,
            Segments::new(8).unwrap(),
            &PartitionInfo {
                tenant: Some(TENANT.into()),
                name: name.into(),
                graph_iri: format!("https://librarybase.org/graph/{name}"),
                history: History::Full,
                integrity: Integrity::Logged,
                export,
                hash: "sha-256".into(),
            },
        )
        .await
        .unwrap();
    }
    let mut store = PgLog::new(client);
    let alice = ActorKey::local(1);
    let bot = ActorKey::local(2);
    for d in [
        draft(
            payload::ACTOR,
            &alice.to_string(),
            &serde_value(&ActorRecord::registered("Alice")),
        ),
        draft(
            payload::ACTOR,
            &bot.to_string(),
            &serde_value(&ActorRecord::subsidiary("Alice-bot", alice.clone())),
        ),
        draft(
            payload::LINK,
            &alice.to_string(),
            &Value::map(vec![(
                Value::text("foreign"),
                Value::text("wikidatawiki:12345"),
            )]),
        ),
        draft(
            payload::MEMBERSHIP,
            &alice.to_string(),
            &serde_value(&Membership::add("sysop", None)),
        ),
        draft(
            payload::MEMBERSHIP,
            &alice.to_string(),
            &serde_value(&Membership::add("bot", None)),
        ),
        draft(
            payload::MEMBERSHIP,
            &alice.to_string(),
            &serde_value(&Membership::remove("bot")),
        ),
        draft(
            payload::BLOCK,
            &bot.to_string(),
            &serde_value(&Block::full(None)),
        ),
    ] {
        store.append(ACTORS, d).await.unwrap();
    }
    let acl = Acl::read(
        Target::parse("acl:page:7").unwrap(),
        ReadKind::Moderation,
        "sysop",
        None,
    );
    store
        .append(
            TENANT_LOG,
            draft(payload::ACL, "acl:page:7", &serde_value(&acl)),
        )
        .await
        .unwrap();
    store.into_inner()
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn step_one_catches_up() {
    if skipped() {
        return;
    }
    let (_, client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let store = PgLog::new(client);
    for partition in [CONFIG_PARTITION, ACTORS, TENANT_LOG] {
        let progress = p.catch_up(&backend, &store, partition, 3).await.unwrap();
        assert_eq!(progress.head, store.head(partition).await.unwrap().size);
        assert_eq!(progress.records, progress.head);
    }
    let client = store.into_inner();

    // Registry: key:, graph:config, tenant:, group: — all at the instance tenant ''.
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.registry WHERE tenant = ''"
        )
        .await,
        4
    );
    let tenant_base: serde_json::Value = client
        .query_one(
            "SELECT config FROM view.registry WHERE kind = 'tenant' AND code = 'librarybase'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(tenant_base["base"], "https://librarybase.org");
    let public_key: serde_json::Value = client
        .query_one("SELECT config FROM view.registry WHERE kind = 'key'", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        public_key["public_key"].as_str().unwrap().len(),
        64,
        "bytes become hex"
    );

    // Groups.
    let perms: Vec<String> = client
        .query_one(
            "SELECT permissions FROM view.\"group\" WHERE tenant = '' AND name = 'sysop'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(perms, vec!["edit", "delete", "block"]);

    // Actors, with IRIs minted under the tenant base; the subsidiary names its operator.
    let rows = client
        .query(
            "SELECT actor_key, kind, name, status, operator, iri, groups FROM view.actor WHERE tenant = $1 ORDER BY actor_key",
            &[&TENANT],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<_, String>(0), "local:1");
    assert_eq!(rows[0].get::<_, String>(1), "registered");
    assert_eq!(
        rows[0].get::<_, Option<String>>(2).as_deref(),
        Some("Alice")
    );
    assert_eq!(
        rows[0].get::<_, String>(5),
        "https://librarybase.org/user/1"
    );
    assert_eq!(
        rows[0].get::<_, Vec<String>>(6),
        vec!["sysop"],
        "bot was added then removed"
    );
    assert_eq!(rows[1].get::<_, String>(1), "bot");
    assert_eq!(rows[1].get::<_, String>(3), "active");
    assert_eq!(
        rows[1].get::<_, Option<String>>(4).as_deref(),
        Some("local:1")
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.account_link WHERE foreign_actor = 'wikidatawiki:12345'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.membership WHERE actor_key = 'local:1' AND layer = 'tenant'"
        )
        .await,
        1
    );
    let removes: Option<Vec<String>> = client
        .query_one(
            "SELECT removes FROM view.block WHERE actor_key = 'local:2'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(removes, None, "all-but-read");
    let acl: serde_json::Value = client
        .query_one(
            "SELECT restrictions FROM view.acl WHERE target = 'acl:page:7'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(acl["read"]["group"], "sysop");
    let read_kind: String = client
        .query_one(
            "SELECT read_kind FROM view.acl WHERE target = 'acl:page:7'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(read_kind, "moderation");

    // Lag is zero everywhere.
    let lags = p.lags(&backend, &PgLog::new(client), ACTORS).await.unwrap();
    assert!(lags.iter().all(|l| l.behind() == 0), "{lags:?}");
    cleanup().await;
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn inline_in_the_appending_transaction_then_rebuild() {
    if skipped() {
        return;
    }
    let (_, client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let store = PgLog::new(client);
    for partition in [CONFIG_PARTITION, ACTORS, TENANT_LOG] {
        p.catch_up(&backend, &store, partition, 100).await.unwrap();
    }
    let client = store.into_inner();

    // The write path: append and project in one unit of work, then commit.
    let mut cx = backend.begin().await.unwrap();
    let appended = log::append(
        cx.conn(),
        ACTORS,
        draft(
            payload::ACTOR,
            "local:3",
            &serde_value(&ActorRecord::registered("Carol")),
        ),
    )
    .await
    .unwrap();
    let record = log::read(cx.conn(), ACTORS, appended.offset)
        .await
        .unwrap()
        .record()
        .unwrap()
        .clone();
    let inline = p
        .apply_inline(&backend, &mut cx, &record, Budget::default())
        .await
        .unwrap();
    assert_eq!(inline.rows, 1);
    // Not visible outside the transaction yet.
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.actor WHERE actor_key = 'local:3'"
        )
        .await,
        0
    );
    backend.commit(cx).await.unwrap();
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.actor WHERE actor_key = 'local:3'"
        )
        .await,
        1
    );
    let lags = p
        .lags(&backend, &PgLog::new(connect_same(&client).await), ACTORS)
        .await
        .unwrap();
    assert!(lags.iter().all(|l| l.behind() == 0), "{lags:?}");

    // A rolled-back unit of work leaves neither the record nor the row.
    let mut cx = backend.begin().await.unwrap();
    let appended = log::append(
        cx.conn(),
        ACTORS,
        draft(
            payload::ACTOR,
            "local:4",
            &serde_value(&ActorRecord::registered("Dave")),
        ),
    )
    .await
    .unwrap();
    let record = log::read(cx.conn(), ACTORS, appended.offset)
        .await
        .unwrap()
        .record()
        .unwrap()
        .clone();
    p.apply_inline(&backend, &mut cx, &record, Budget::default())
        .await
        .unwrap();
    backend.rollback(cx).await.unwrap();
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.actor WHERE actor_key = 'local:4'"
        )
        .await,
        0
    );
    assert_eq!(
        PgLog::new(connect_same(&client).await)
            .head(ACTORS)
            .await
            .unwrap()
            .size,
        8,
        "the offset was released"
    );

    // Erasing an actor record's content drops the name on the next apply.
    let mut store = PgLog::new(connect_same(&client).await);
    store
        .erase_parts(ACTORS, 0, &[CONTENT, ATTESTATION])
        .await
        .unwrap();
    // Corrupt a derived row and rebuild everything from the log.
    client
        .batch_execute("UPDATE view.actor SET name = 'Mallory' WHERE actor_key = 'local:3'; INSERT INTO view.registry (tenant, kind, code, config, \"offset\") VALUES ('', 'stray', 'x', '{}', 0)")
        .await
        .unwrap();
    let progress = p
        .rebuild(&backend, &store, &[CONFIG_PARTITION, ACTORS, TENANT_LOG], 4)
        .await
        .unwrap();
    assert_eq!(progress.len(), 3);
    assert_eq!(
        client
            .query_one(
                "SELECT name FROM view.actor WHERE actor_key = 'local:3'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, Option<String>>(0)
            .as_deref(),
        Some("Carol")
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.registry WHERE kind = 'stray'"
        )
        .await,
        0
    );
    assert_eq!(
        client
            .query_one(
                "SELECT name FROM view.actor WHERE actor_key = 'local:1'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, Option<String>>(0),
        None,
        "the erased record projects no name"
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.actor WHERE tenant = 'librarybase'"
        )
        .await,
        3
    );
    cleanup().await;
}

async fn connect_same(client: &Client) -> Client {
    let name: String = client
        .query_one("SELECT current_database()", &[])
        .await
        .unwrap()
        .get(0);
    let mut config = admin_config().unwrap();
    config.dbname(&name);
    connect(&config).await
}
