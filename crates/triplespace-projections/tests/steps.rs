//! The step 2–5 projections against a real database named by
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it. A tenant adopts an entity and
//! edits it, a key-mapped provider mirrors a Domain the tenant also asserts about, jobs
//! start and finish, and the rows come out as 0013 §5 describes them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ed25519_dalek::SigningKey;
use scatter_actors::issuer::IssuerRegistry;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::genesis::Genesis;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{
    Export, GraphRegistry, History, Integrity, KeyEntry, PAYLOAD_CONFIG, Scope,
};
use scatter_log::store::{Draft, LogStore};
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, PgLog, log};
use scatter_projection::{Consumer, Pipeline, Worker};
use serde_json::json;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_projections::common::{
    PAYLOAD_CHANGESET, PAYLOAD_KEYED_SURROGATE, PAYLOAD_LOGEVENT,
};
use triplespace_projections::refs::RefCountConsumer;
use triplespace_projections::{Farm, PgBackend, milestone_pipeline};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const TENANT: &str = "librarybase";
const LOCAL: u64 = 0x2001;
const MIRROR: u64 = 0x2002;
const INSTANCE_LOG: u64 = 0x2003;
const TENANT_LOG: u64 = 0x2004;
const ALL: [u64; 5] = [CONFIG_PARTITION, INSTANCE_LOG, MIRROR, LOCAL, TENANT_LOG];

const DOMAIN: &str = "domain:example.org";
const SURROGATE: &str = "domain#1";
const M_P1: &str = "domain:example.org$00000000-0000-0000-0000-0000000000A1";
const M_P5: &str = "domain:example.org$00000000-0000-0000-0000-0000000000A5";
const L_P1: &str = "domain:example.org$00000000-0000-0000-0000-0000000000B1";
const L_P3: &str = "domain:example.org$00000000-0000-0000-0000-0000000000B3";
const Q6_P2: &str = "Q6$00000000-0000-0000-0000-000000000062";
const Q6_P4: &str = "Q6$00000000-0000-0000-0000-000000000064";

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

async fn fresh() -> (Client, PgBackend) {
    let admin = admin_config().expect("checked by the caller");
    let name = format!(
        "tssteps_test_{}_{}",
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
    let pool = triplespace_db::pool::pool(&url, 4).unwrap();
    (client, PgBackend::new(pool))
}

fn skipped() -> bool {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return true;
    }
    false
}

fn pipeline() -> Pipeline<PgBackend> {
    milestone_pipeline(
        IssuerRegistry::default_registry().clone(),
        Farm {
            slug: "scatter".into(),
            code: "scatter".into(),
            base: "https://scatter.example".into(),
        },
    )
    .unwrap()
}

/// A record: content from JSON, a comment, and an attestation with an actor and a job.
fn draft(
    payload_type: &str,
    key: &str,
    content: &serde_json::Value,
    job: Option<u64>,
    page_id: Option<u64>,
) -> Draft {
    let mut attestation = vec![(Value::text("actor"), Value::text("local:1"))];
    if let Some(j) = job {
        attestation.push((Value::text("job"), Value::Int(i128::from(j))));
        attestation.push((Value::text("tags"), Value::Array(vec![Value::text("bulk")])));
    }
    Draft {
        appended_at: 1_790_000_000_000_000 + u64::from(COUNTER.load(Ordering::Relaxed)),
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id,
        body: Body::core(
            &Value::from_json(content),
            &Value::text("test edit"),
            &Value::map(attestation),
        )
        .unwrap(),
    }
}

fn string_statement(p: &str, v: &str, id: &str, datatype: &str) -> serde_json::Value {
    json!({"mainsnak": {"snaktype": "value", "property": p, "datavalue": {"value": v, "type": "string"}, "datatype": datatype},
        "type": "statement", "rank": "normal", "id": id})
}

fn label(lang: &str, v: &str) -> serde_json::Value {
    json!({"language": lang, "value": v})
}

/// Genesis, the tenant, four partitions, and the records of the scenario.
#[allow(clippy::too_many_lines)]
async fn seed(client: Client) -> Client {
    let key = SigningKey::from_bytes(&[23; 32]);
    let mut store = PgLog::new(client);
    Genesis {
        partition: CONFIG_PARTITION,
        scope: Scope::Instance,
        key: KeyEntry::ed25519(key.verifying_key().to_bytes()),
        segment_exponent: 16,
        appended_at: 1,
        founding: Value::map(Vec::new()),
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
            draft(
                PAYLOAD_CONFIG,
                &format!("tenant:{TENANT}"),
                &json!({"kind": "tenant", "slug": TENANT, "base": "https://librarybase.org", "issuer": TENANT}),
                None,
                None,
            ),
        )
        .await
        .unwrap();
    let client = store.into_inner();
    // The tenant's sequences: its records take log IDs at append (0006 A24).
    scatter_log_postgres::sequences::create(&client, TENANT, &["item"])
        .await
        .unwrap();
    for (partition, tenant, name) in [
        (LOCAL, Some(TENANT), "local"),
        (TENANT_LOG, Some(TENANT), "log"),
        (MIRROR, None, "mirror/internetdomains"),
        (INSTANCE_LOG, None, "log"),
    ] {
        log::create_partition(
            &client,
            partition,
            Segments::new(8).unwrap(),
            &PartitionInfo {
                tenant: tenant.map(str::to_owned),
                name: name.into(),
                graph_iri: format!("https://scatter.example/graph/{name}"),
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
    let mut store = PgLog::new(client);

    // The instance log: the Domain's surrogate, then the mirror sync job.
    store
        .append(
            INSTANCE_LOG,
            draft(
                PAYLOAD_KEYED_SURROGATE,
                SURROGATE,
                &json!({"keyed_type": "domain", "surrogate": 1, "key": "example.org"}),
                None,
                None,
            ),
        )
        .await
        .unwrap();
    store
        .append(
            INSTANCE_LOG,
            draft(
                PAYLOAD_LOGEVENT,
                "7",
                &json!({"type": "job", "action": "start", "time": 1_790_000_000_000_000u64,
                    "target": {"kind": "job", "id": 7},
                    "params": {"source": "internetdomains", "version": "2026-10-01", "mode": "upsert",
                               "graph": "mirror/internetdomains", "adapter": "scatter-adapter-internetdomains 0.0.1",
                               "args": {"subset": "all", "follow": true}}}),
                None,
                None,
            ),
        )
        .await
        .unwrap();

    // The mirror: the Domain, mapped from the provider's Q9, and an ordinary item.
    let domain_entity = json!({"type": "domain", "id": DOMAIN,
        "labels": {"en": label("en", "example.org"), "de": label("de", "example.org (de)")},
        "claims": {"P1": [string_statement("P1", "a", M_P1, "string")],
                   "P5": [string_statement("P5", "10.1/x", M_P5, "external-id")]}});
    store
        .append(
            MIRROR,
            draft(
                PAYLOAD_CHANGESET,
                SURROGATE,
                &json!({"op": "put", "id": DOMAIN, "entity": domain_entity, "upstream": {"revid": 55, "time": "2026-10-01T00:00:00Z"},
                    "size": 400, "prev_size": 380, "upstream_id": "Q9",
                    "changes": {"labels": {"de": {"added": 1, "removed": 0, "changed": 0}}}}),
                Some(7),
                Some(100),
            ),
        )
        .await
        .unwrap();
    let item = json!({"type": "item", "id": "XDQ10", "labels": {"en": label("en", "Registrar")}, "claims": {}});
    store
        .append(
            MIRROR,
            draft(
                PAYLOAD_CHANGESET,
                "XDQ10",
                &json!({"op": "put", "id": "XDQ10", "entity": item, "upstream": {"revid": 56}, "size": 90}),
                Some(7),
                Some(101),
            ),
        )
        .await
        .unwrap();
    store
        .append(
            INSTANCE_LOG,
            draft(
                PAYLOAD_LOGEVENT,
                "7",
                &json!({"type": "job", "action": "finish", "time": 1_790_000_001_000_000u64,
                    "target": {"kind": "job", "id": 7},
                    "params": {"counts": {"created": 2, "merged": 0, "unchanged": 0, "rejected": 1},
                               "hash_mismatches": {"time": 2},
                               "checkpoint": "scatter.example/log/mirror/internetdomains\n2\nROOT\n\n— sig\n",
                               "rejects": [{"line": 4, "match": {"P1": "not a domain"}, "reason": "invalid key"}]}}),
                None,
                None,
            ),
        )
        .await
        .unwrap();

    // The tenant: adopt Q6, edit it, assert about the Domain, correct the mirror.
    let q6 = json!({"type": "item", "id": "Q6",
        "labels": {"en": label("en", "Six")},
        "claims": {"P2": [string_statement("P2", "abc", Q6_P2, "external-id")]}});
    let mut adopt = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "adopt", "id": "Q6", "entity": q6, "source_revid": 41877, "source_time": "2026-09-20T14:02:11Z", "source_pageid": 6}),
        Some(8),
        Some(6),
    );
    adopt.revid = Some(900);
    store.append(LOCAL, adopt).await.unwrap();
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "add", "id": "Q6", "labels": {"fr": "Six"}, "claims": {"P4": [string_statement("P4", "x", Q6_P4, "string")]}}),
        None,
        Some(6),
    );
    edit.revid = Some(901);
    store.append(LOCAL, edit).await.unwrap();
    for op in [
        json!({"op": "add", "id": DOMAIN, "labels": {"en": "Example"},
            "claims": {"P1": [string_statement("P1", "a", L_P1, "string")], "P3": [string_statement("P3", "c", L_P3, "string")]}}),
        json!({"op": "override", "id": DOMAIN, "statement": M_P5, "rank": "deprecated"}),
        json!({"op": "retain", "id": DOMAIN, "policy": "retain"}),
    ] {
        store
            .append(
                LOCAL,
                draft(PAYLOAD_CHANGESET, SURROGATE, &op, None, Some(100)),
            )
            .await
            .unwrap();
    }
    // A local job that failed.
    for ev in [
        json!({"type": "job", "action": "start", "time": 1_790_000_002_000_000u64, "target": {"kind": "job", "id": 8},
            "params": {"source": "https://librarybase.org/", "version": "librarybase-20260928.json.gz", "mode": "adopt", "graph": "local"}}),
        json!({"type": "job", "action": "fail", "time": 1_790_000_003_000_000u64, "target": {"kind": "job", "id": 8},
            "params": {"error": "ts-adopt-not-empty"}}),
    ] {
        store
            .append(TENANT_LOG, draft(PAYLOAD_LOGEVENT, "8", &ev, None, None))
            .await
            .unwrap();
    }
    store.into_inner()
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn rows(client: &Client, sql: &str) -> Vec<String> {
    client
        .query(sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect()
}

/// Rows sorted bytewise, independent of the database's collation.
async fn sorted(client: &Client, sql: &str) -> Vec<String> {
    let mut out = rows(client, sql).await;
    out.sort();
    out
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn steps_two_to_five_project_and_rebuild() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let store = PgLog::new(client);
    for partition in ALL {
        let progress = p.catch_up(&backend, &store, partition, 2).await.unwrap();
        assert_eq!(progress.records, progress.head, "partition {partition:#x}");
    }
    let client = store.into_inner();

    // Step 1: the surrogate.
    let key: Option<String> = client
        .query_one("SELECT key FROM view.keyed_surrogate WHERE tenant = '' AND keyed_type = 'domain' AND surrogate = 1", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(key.as_deref(), Some("example.org"));

    // Step 2: cursors, the keyed map, jobs.
    assert_eq!(
        sorted(&client, "SELECT tenant || '|' || entity_id || '|' || graph || '|' || coalesce(upstream_version, '-') || '|' || coalesce(prev_upstream, '-') || '|' || (size > 0)::text || '|' || coalesce(job_id::text, '-') FROM view.entity_source").await,
        vec![
            "librarybase|Q6|local|-|-|true|-",
            "librarybase|domain:example.org|local|-|-|true|-",
            "|XDQ10|mirror/internetdomains|56|-|true|7",
            "|domain:example.org|mirror/internetdomains|55|-|true|7",
        ]
    );
    // The local cursor is the newest local record: Q6's edit, not its adoption.
    let q6_offset: i64 = client
        .query_one("SELECT \"offset\" FROM view.entity_source WHERE tenant = 'librarybase' AND entity_id = 'Q6'", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(q6_offset, 1);
    assert_eq!(
        sorted(&client, "SELECT provider || '|' || upstream_id || '|' || keyed_type || '|' || key FROM view.keyed_map WHERE tenant = ''").await,
        vec!["internetdomains|Q9|domain|example.org"]
    );
    // The upstream ID is an alias of the key (05 §4.1): `XDQ9` resolves to the Domain,
    // an unmapped or unknown one to nothing.
    let registry = scatter_providers::Registry::default_registry();
    let alias = triplespace_projections::read::mapped_alias(
        &client,
        registry,
        TENANT,
        &"XDQ9".parse().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        alias.map(|a| a.as_str().to_string()),
        Some(DOMAIN.to_string())
    );
    for id in ["XDQ999", "WDQ9", "Q9", DOMAIN] {
        assert!(
            triplespace_projections::read::mapped_alias(
                &client,
                registry,
                TENANT,
                &id.parse().unwrap()
            )
            .await
            .unwrap()
            .is_none(),
            "{id}"
        );
    }
    let job7 = client
        .query_one("SELECT status, source, source_version, mode, graph, adapter_version, counts::text, checkpoint_size, start_offset, finish_offset, actor_key, params::text FROM view.job WHERE tenant = '' AND job_id = 7", &[])
        .await
        .unwrap();
    assert_eq!(job7.get::<_, String>(0), "finished");
    assert_eq!(job7.get::<_, String>(1), "internetdomains");
    assert_eq!(
        job7.get::<_, Option<String>>(2).as_deref(),
        Some("2026-10-01")
    );
    assert_eq!(job7.get::<_, Option<String>>(3).as_deref(), Some("upsert"));
    assert_eq!(job7.get::<_, String>(4), "mirror/internetdomains");
    assert_eq!(
        job7.get::<_, Option<String>>(5).as_deref(),
        Some("scatter-adapter-internetdomains 0.0.1")
    );
    let counts: serde_json::Value = serde_json::from_str(&job7.get::<_, String>(6)).unwrap();
    assert_eq!(counts["created"], 2);
    assert_eq!(counts["hash_mismatches"]["time"], 2);
    assert_eq!(job7.get::<_, Option<i64>>(7), Some(2));
    assert_eq!(
        (job7.get::<_, i64>(8), job7.get::<_, Option<i64>>(9)),
        (1, Some(2))
    );
    assert_eq!(job7.get::<_, String>(10), "local:1");
    assert_eq!(
        job7.get::<_, String>(11),
        r#"{"follow": true, "subset": "all"}"#
    );
    assert_eq!(
        sorted(&client, "SELECT line::text || '|' || coalesce(match_key, '') || '|' || reason FROM view.job_reject WHERE job_id = 7").await,
        vec![r#"4|{"P1":"not a domain"}|invalid key"#]
    );
    let job8 = client
        .query_one("SELECT status, counts::text, mode FROM view.job WHERE tenant = 'librarybase' AND job_id = 8", &[])
        .await
        .unwrap();
    assert_eq!(job8.get::<_, String>(0), "failed");
    assert!(job8.get::<_, String>(1).contains("ts-adopt-not-empty"));
    assert_eq!(job8.get::<_, Option<String>>(2).as_deref(), Some("adopt"));

    // Step 4: entities, terms, identifiers.
    let entities = sorted(&client, "SELECT tenant || '|' || id || '|' || type || '|' || page_id::text || '|' || coalesce(surrogate::text, '-') || '|' || coalesce(retention, '-') || '|' || resolved_kind || '|' || resolved_version::text || '|' || coalesce(local_revid::text, '-') FROM view.entity").await;
    assert_eq!(
        entities,
        vec![
            "librarybase|Q6|item|6|-|-|materialized|2|901",
            // Resolved three times: the add, the override and the retain each re-resolve.
            "librarybase|domain:example.org|domain|100|1|retain|materialized|3|-",
            "|XDQ10|item|101|-|-|source|1|-",
            "|domain:example.org|domain|100|1|-|source|1|-",
        ]
    );
    let shared: Option<Vec<u8>> = client
        .query_one(
            "SELECT resolved FROM view.entity WHERE tenant = '' AND id = $1",
            &[&DOMAIN],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        shared.is_none(),
        "a single mirror source is not stored twice"
    );
    let overlay: Vec<u8> = client
        .query_one(
            "SELECT resolved FROM view.entity WHERE tenant = 'librarybase' AND id = $1",
            &[&DOMAIN],
        )
        .await
        .unwrap()
        .get::<_, Option<Vec<u8>>>(0)
        .unwrap();
    let overlay: serde_json::Value = serde_json::from_slice(&overlay).unwrap();
    assert_eq!(
        overlay["labels"]["en"]["value"], "Example",
        "local wins the label"
    );
    assert_eq!(
        overlay["labels"]["de"]["value"], "example.org (de)",
        "the mirror keeps its own"
    );
    assert_eq!(
        overlay["claims"]["P1"].as_array().unwrap().len(),
        1,
        "P1 fused across graphs"
    );
    assert_eq!(
        overlay["claims"]["P1"][0]["id"], L_P1,
        "the local member's ID, local being first"
    );
    assert_eq!(
        overlay["claims"]["P5"][0]["rank"], "deprecated",
        "the override applied"
    );
    assert_eq!(overlay["claims"]["P3"][0]["id"], L_P3);
    let q6: Vec<u8> = client
        .query_one(
            "SELECT resolved FROM view.entity WHERE tenant = 'librarybase' AND id = 'Q6'",
            &[],
        )
        .await
        .unwrap()
        .get::<_, Option<Vec<u8>>>(0)
        .unwrap();
    let q6: serde_json::Value = serde_json::from_slice(&q6).unwrap();
    assert_eq!(q6["labels"]["fr"]["value"], "Six");
    assert_eq!(q6["claims"].as_object().unwrap().len(), 2);
    assert_eq!(
        sorted(&client, "SELECT tenant || '|' || entity_id || '|' || kind::text || '|' || lang || '|' || text FROM view.term").await,
        vec![
            "librarybase|Q6|1|en|Six",
            "librarybase|Q6|1|fr|Six",
            "librarybase|domain:example.org|1|de|example.org (de)",
            "librarybase|domain:example.org|1|en|Example",
            "|XDQ10|1|en|Registrar",
            // A mirrored entity's shared rows hold the instance's term languages only
            // (05 §5.8; 0082 §6): `mul`, `en` and the tenants' content languages, here
            // `en`. Its German label stays in the record; the tenant's rows keep it.
            "|domain:example.org|1|en|example.org",
        ]
    );
    assert_eq!(
        sorted(&client, "SELECT tenant || '|' || property || '|' || entity_id || '|' || graph || '|' || statement_id FROM view.identifier").await,
        vec![
            format!("librarybase|P2|Q6|local|{Q6_P2}"),
            format!("librarybase|P5|domain:example.org|mirror/internetdomains|{M_P5}"),
            format!("|P5|domain:example.org|mirror/internetdomains|{M_P5}"),
        ]
    );

    // Step 5: activity, in partition then offset order: local, mirror, instance log, tenant log.
    assert_eq!(
        rows(&client, "SELECT tenant || '|' || kind || '|' || source || '|' || coalesce(summary_op, log_type || '/' || log_action) || '|' || coalesce(target_id, '-') || '|' || coalesce(actor_key, '-') || '|' || coalesce(job_id::text, '-') || '|' || coalesce(new::text, '-') FROM view.activity ORDER BY partition, \"offset\"").await,
        vec![
            "librarybase|edit|local|adopt|Q6|local:1|8|true",
            "librarybase|edit|local|add|Q6|local:1|-|false",
            "librarybase|edit|local|add|domain:example.org|local:1|-|false",
            "librarybase|edit|local|override|domain:example.org|local:1|-|false",
            "librarybase|edit|local|retain|domain:example.org|local:1|-|false",
            // Job 7 is the follower's (`follow`), so its mirror records are rows; a load's
            // are not (03 §4.6; 0013 §5.5), as `end_to_end.rs` and the site test check.
            "|sync|mirror/internetdomains|put|domain:example.org|local:1|7|false",
            "|sync|mirror/internetdomains|put|XDQ10|local:1|7|false",
            "|job|log|job/start|7|local:1|-|-",
            "|job|log|job/finish|7|local:1|-|-",
            "librarybase|job|log|job/start|8|local:1|-|-",
            "librarybase|job|log|job/fail|8|local:1|-|-",
        ]
    );
    let put_row = client
        .query_one("SELECT revid, size, delta, changes::text, tags, comment, summary_args FROM view.activity WHERE source = 'mirror/internetdomains' AND target_id = $1", &[&DOMAIN])
        .await
        .unwrap();
    assert_eq!(put_row.get::<_, Option<i64>>(0), None);
    assert_eq!(put_row.get::<_, Option<i32>>(1), Some(400));
    assert_eq!(put_row.get::<_, Option<i32>>(2), Some(20));
    assert!(
        put_row
            .get::<_, Option<String>>(3)
            .unwrap()
            .contains("\"de\"")
    );
    assert_eq!(put_row.get::<_, Vec<String>>(4), vec!["bulk"]);
    assert_eq!(
        put_row.get::<_, Option<String>>(5).as_deref(),
        Some("test edit")
    );
    assert_eq!(put_row.get::<_, Option<String>>(6), None);
    let add_row = client
        .query_one("SELECT revid, summary_args FROM view.activity WHERE source = 'local' AND summary_op = 'add' AND target_id = 'Q6'", &[])
        .await
        .unwrap();
    assert_eq!(add_row.get::<_, Option<i64>>(0), Some(901));
    assert_eq!(
        add_row.get::<_, Option<String>>(1).as_deref(),
        Some("claims:P4 labels:fr")
    );

    // The feeds over those rows (0020 §1–2): recent changes, contributions, a history.
    feeds(&client).await;

    // The tier-1 state tables (0083 §4): one row per local revision, one per key.
    Box::pin(revision_state(&client)).await;

    // A tombstone retracts the mirror's contribution and the rows that hung off it.
    let mut store = PgLog::new(client);
    store
        .append(
            MIRROR,
            draft(
                PAYLOAD_CHANGESET,
                "XDQ10",
                &json!({"op": "tombstone", "id": "XDQ10", "upstream": {"revid": 57}}),
                Some(7),
                Some(101),
            ),
        )
        .await
        .unwrap();
    p.catch_up(&backend, &store, MIRROR, 10).await.unwrap();
    let client = store.into_inner();
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.entity_source WHERE entity_id = 'XDQ10'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.entity WHERE id = 'XDQ10'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.term WHERE entity_id = 'XDQ10'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.activity WHERE summary_op = 'tombstone'"
        )
        .await,
        1
    );

    // A rebuild from offset 0, with the partitions in dependency order, reproduces the rows.
    let before = snapshot(&client).await;
    let store = PgLog::new(client);
    let progress = p.rebuild(&backend, &store, &ALL, 3).await.unwrap();
    assert_eq!(progress.len(), ALL.len());
    assert_eq!(snapshot(store.client()).await, before);
    // The lag is zero everywhere.
    for partition in ALL {
        for lag in p.lags(&backend, &store, partition).await.unwrap() {
            assert_eq!(
                lag.applied, lag.head,
                "{} on {partition:#x}",
                lag.projection
            );
        }
    }
    // Without the state rows, the readers fold (0083 A1).
    revision_state_fallback(store.client()).await;

    // A key with no graph_state row is seeded from its earlier records on its next edit,
    // so the state is whole without a rebuild (0083 A1).
    let mut store = store;
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "add", "id": "Q6", "labels": {"de": "Sechs"}}),
        None,
        Some(6),
    );
    edit.revid = Some(902);
    store.append(LOCAL, edit).await.unwrap();
    p.catch_up(&backend, &store, LOCAL, 10).await.unwrap();
    let (state, offset, version) =
        triplespace_projections::revision_state::graph_state(store.client(), TENANT, "Q6", "local")
            .await
            .unwrap()
            .unwrap();
    assert_eq!((offset, version), (5, 1), "a fresh row at the new edit");
    assert_eq!(
        state.offsets,
        vec![0, 1, 5],
        "seeded from the records before it"
    );
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 3, "en, fr, de");
    let (state, revid) =
        triplespace_projections::revision_state::state_at_record(store.client(), LOCAL, 5)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(revid, 902);
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 3);
}

/// `view.graph_state` and `view.entity_revision` (0083 §4) after the catch-up, and the
/// readers over them: a past state is one row; where the row is missing (a database
/// migrated but not rebuilt, 0083 A1) the fold answers the same; a key whose row is
/// missing is seeded whole on its next edit.
#[allow(clippy::too_many_lines)]
async fn revision_state(client: &Client) {
    use scatter_wikibase_model::id::EntityId;
    use triplespace_projections::revision_state::{graph_state, revision_state, state_at_record};
    use triplespace_projections::revisions::{
        LocalRevision, change_at, fold_to, local_change_at, local_revisions, local_state_at,
    };
    let q6: EntityId = "Q6".parse().unwrap();
    let domain: EntityId = DOMAIN.parse().unwrap();

    // One graph_state row per locally edited key, at the key's latest local offset; one
    // entity_revision row per local revision that has a revision ID (the Domain's
    // records are surrogate-keyed assertions without one, 01 §2.5).
    assert_eq!(
        sorted(
            client,
            "SELECT tenant || '|' || entity_id || '|' || graph || '|' || \"offset\" || '|' || version FROM view.graph_state"
        )
        .await,
        vec![
            "librarybase|Q6|local|1|2",
            "librarybase|domain:example.org|local|4|3",
        ]
    );
    assert_eq!(
        sorted(
            client,
            "SELECT tenant || '|' || revid || '|' || entity_id || '|' || \"offset\" FROM view.entity_revision"
        )
        .await,
        vec!["librarybase|900|Q6|0", "librarybase|901|Q6|1"]
    );

    // The stored states are the fold's.
    let revs = local_revisions(client, TENANT, &q6).await.unwrap();
    assert_eq!(revs.len(), 2);
    let (state, offset, version) = graph_state(client, TENANT, "Q6", "local")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((offset, version), (1, 2));
    assert_eq!(state, fold_to(&revs, 1).unwrap().1);
    let (state, partition, offset) = revision_state(client, TENANT, 900).await.unwrap().unwrap();
    assert_eq!((partition, offset), (LOCAL, 0));
    assert_eq!(state, fold_to(&revs, 0).unwrap().1);
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 1, "en only");
    let (state, revid) = state_at_record(client, LOCAL, 1).await.unwrap().unwrap();
    assert_eq!(revid, 901);
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 2, "en and fr");
    assert!(revision_state(client, TENANT, 902).await.unwrap().is_none());
    let domain_revs = local_revisions(client, TENANT, &domain).await.unwrap();
    assert_eq!(domain_revs.len(), 3);
    let (state, offset, _) = graph_state(client, TENANT, DOMAIN, "local")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(offset, 4);
    assert_eq!(state, fold_to(&domain_revs, 4).unwrap().1);
    assert_eq!(
        state.retention,
        Some(scatter_wikibase_changeset::Retention::Retain)
    );

    // The readers: via the table and via the fold, the same answer.
    let via_table = local_state_at(client, TENANT, &q6, 0)
        .await
        .unwrap()
        .unwrap();
    let via_fold = fold_to(&revs, 0).unwrap();
    assert_eq!(via_table.0.revid(), via_fold.0.revid());
    assert_eq!(via_table.1, via_fold.1);
    let change = local_change_at(client, TENANT, &q6, 1)
        .await
        .unwrap()
        .unwrap();
    let folded = change_at(&revs, 1).unwrap();
    assert_eq!(change.before, folded.before);
    assert_eq!(change.after, folded.after);
    assert_eq!(change.previous.as_ref().map(LocalRevision::offset), Some(0));
    assert_eq!(change.revision.revid(), Some(901));
    let first = local_change_at(client, TENANT, &q6, 0)
        .await
        .unwrap()
        .unwrap();
    assert!(first.before.entity.is_none());
    assert!(first.previous.is_none());
    assert!(
        local_state_at(client, TENANT, &q6, 3)
            .await
            .unwrap()
            .is_none(),
        "the Domain's record, not Q6's"
    );
    assert!(
        local_state_at(client, TENANT, &q6, 9)
            .await
            .unwrap()
            .is_none()
    );
}

/// A database migrated to the tables but not yet rebuilt (0083 A1) has no rows for its
/// earlier revisions: the fold answers the same, and the latest state folds too.
async fn revision_state_fallback(client: &Client) {
    use scatter_wikibase_model::id::EntityId;
    use triplespace_projections::revisions::{
        LocalRevision, change_at, fold_to, local_change_at, local_revisions, local_state_at,
    };
    let q6: EntityId = "Q6".parse().unwrap();
    let revs = local_revisions(client, TENANT, &q6).await.unwrap();
    let folded = change_at(&revs, 1).unwrap();
    client
        .execute("DELETE FROM view.entity_revision WHERE revid = 901", &[])
        .await
        .unwrap();
    let fallback = local_state_at(client, TENANT, &q6, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fallback.1, fold_to(&revs, 1).unwrap().1);
    let fallback = local_change_at(client, TENANT, &q6, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fallback.before, folded.before);
    assert_eq!(fallback.after, folded.after);
    assert_eq!(
        fallback.previous.as_ref().map(LocalRevision::offset),
        Some(0)
    );
    client
        .execute("DELETE FROM view.graph_state WHERE entity_id = 'Q6'", &[])
        .await
        .unwrap();
    let order = triplespace_projections::EntityProjection::default();
    let current = triplespace_projections::read::current(client, &order, TENANT, &q6)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.local().unwrap().labels.len(),
        2,
        "the latest state folds without a graph_state row"
    );
    assert_eq!(current.local_revid, Some(901));
}

/// Every derived row, as text. `resolved_version` and `first_seen` may legitimately differ
/// after a rebuild that interleaves partitions differently; everything else must not.
/// Recent changes, contributions and a history are one query with three target sets.
#[allow(clippy::too_many_lines)]
async fn feeds(client: &Client) {
    use triplespace_projections::feed::{
        self, Cursor, Direction, Feed, Filters, Flag, TargetSet, Viewer, target_kind,
    };
    let viewer = Viewer::default();
    let brief = |p: &feed::Page| -> Vec<String> {
        p.rows
            .iter()
            .map(|r| {
                format!(
                    "{}|{}|{}",
                    r.kind,
                    r.summary_op
                        .clone()
                        .or_else(|| r.log_action.clone())
                        .unwrap_or_default(),
                    r.target.id.clone().unwrap_or_default()
                )
            })
            .collect()
    };

    // Recent changes: everything but syncs, newest first; the mirror job's own rows
    // from the instance log are in it.
    let mut rc = Feed::everything(50);
    rc.filters.kinds = vec!["edit".into(), "log".into(), "job".into()];
    let page = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(page.rows.len(), 9, "{:?}", brief(&page));
    assert!(page.next.is_none());
    assert!(page.rows.iter().all(|r| r.kind != "sync"));
    assert!(
        page.rows
            .windows(2)
            .all(|w| w[0].cursor().time >= w[1].cursor().time),
        "newest first"
    );
    let adopt = page
        .rows
        .iter()
        .find(|r| r.summary_op.as_deref() == Some("adopt"))
        .unwrap();
    assert!(
        adopt.new,
        "an adopted entity's first record is new (16 §3.1)"
    );
    assert_eq!(adopt.target.entity_type.as_deref(), Some("item"));
    assert_eq!(
        adopt.target.label,
        Some(("Six".to_string(), "en".to_string()))
    );
    assert_eq!(adopt.actor.key.as_deref(), Some("local:1"));

    // Paging: two at a time, then from the cursor, covers the set once in order.
    rc.limit = 2;
    let first = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(first.rows.len(), 2);
    let mut all = brief(&first);
    let mut next = first.next;
    while let Some(c) = next {
        rc.cursor = Some(c);
        let p = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
        all.extend(brief(&p));
        next = p.next;
    }
    assert_eq!(
        all,
        brief(&page),
        "paging walks the same rows in the same order"
    );
    rc.cursor = None;
    rc.limit = 50;

    // Oldest first, and a window.
    rc.direction = Direction::Newer;
    let asc = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    let mut expected = brief(&page);
    expected.reverse();
    assert_eq!(brief(&asc), expected);
    let since = asc.rows[2].time;
    rc.filters.since = Some(since);
    let windowed = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(
        windowed.rows.len(),
        asc.rows.iter().filter(|r| r.time >= since).count()
    );
    assert!(windowed.rows.iter().all(|r| r.time >= since));
    rc.filters.since = None;
    rc.filters.until = Some(since);
    let windowed = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert!(windowed.rows.iter().all(|r| r.time <= since));
    rc.filters.until = None;

    // Flags and tags narrow the set.
    rc.filters.new = Flag::Only;
    let new = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(brief(&new), vec!["edit|adopt|Q6"]);
    rc.filters.new = Flag::Not;
    rc.filters.tag = Some("bot".into());
    let bot = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert!(bot.rows.is_empty());
    rc.filters = Filters::default();
    rc.filters.entity_types = vec!["domain".into()];
    let domains = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert!(
        domains
            .rows
            .iter()
            .all(|r| r.target.id.as_deref() == Some(DOMAIN))
    );
    assert_eq!(domains.rows.len(), 4, "{:?}", brief(&domains));
    rc.filters = Filters::default();
    rc.filters.log_type = Some("job".into());
    rc.filters.log_action = Some("fail".into());
    let failed = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(brief(&failed), vec!["job|fail|8"]);

    // Contributions: the actor set.
    let contribs = Feed {
        set: TargetSet::Actor("local:1".into()),
        filters: Filters {
            kinds: vec!["edit".into()],
            ..Filters::default()
        },
        direction: Direction::Older,
        cursor: None,
        limit: 50,
        languages: vec!["en".into()],
    };
    let page = feed::query(client, TENANT, &contribs, &viewer)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 5);
    assert!(
        page.rows
            .iter()
            .all(|r| r.actor.key.as_deref() == Some("local:1"))
    );

    // A history: the one-target set keeps syncs, and reaches the shared mirror partition.
    let history = Feed {
        set: TargetSet::Target {
            kind: target_kind::ENTITY,
            id: DOMAIN.into(),
        },
        filters: Filters::default(),
        direction: Direction::Newer,
        cursor: None,
        limit: 50,
        languages: vec!["en".into()],
    };
    let page = feed::query(client, TENANT, &history, &viewer)
        .await
        .unwrap();
    assert_eq!(
        brief(&page),
        vec![
            "edit|add|domain:example.org",
            "edit|override|domain:example.org",
            "edit|retain|domain:example.org",
            "sync|put|domain:example.org",
        ],
        "in time order, the mirror's sync after the local edits that preceded it"
    );
    let sync = &page.rows[3];
    assert_eq!(sync.source, "mirror/internetdomains");
    assert_eq!(sync.size, Some(400));
    assert_eq!(sync.delta, Some(20));
    assert!(sync.changes.is_some());
    assert_eq!(sync.tags, vec!["bulk"]);

    // The source switch narrows a history to one graph.
    let mut made_here = history.clone();
    made_here.filters.sources = vec!["local".into()];
    let page = feed::query(client, TENANT, &made_here, &viewer)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 3);
    assert!(page.rows.iter().all(|r| r.source == "local"));
    made_here.filters.sources = vec!["mirror/internetdomains".into()];
    let page = feed::query(client, TENANT, &made_here, &viewer)
        .await
        .unwrap();
    assert_eq!(brief(&page), vec!["sync|put|domain:example.org"]);

    // A cursor is a token a client hands back.
    let c = Cursor {
        time: 1,
        partition: 2,
        offset: 3,
    };
    assert_eq!(Cursor::parse(&c.token()), Some(c));
}

async fn snapshot(client: &Client) -> Vec<String> {
    let mut all = Vec::new();
    all.extend(
        sorted(
            client,
            "SELECT (tenant, id, type, page_id, surrogate, canonical_id, retention, local_offset, local_revid, resolved_kind,
                     convert_from(resolved, 'UTF8'), resolved_size, deleted)::text FROM view.entity",
        )
        .await,
    );
    for table in [
        "view.entity_source",
        "view.term",
        "view.identifier",
        "view.entity_ref",
        "view.keyed_map",
        "view.job",
        "view.job_reject",
        "view.activity",
        "view.graph_state",
        "view.entity_revision",
    ] {
        all.extend(sorted(client, &format!("SELECT t::text FROM {table} t")).await);
    }
    all
}

/// The instance's term languages (05 §5.8; 0082 §6, A2): by default `mul`, `en` and the
/// instance's content languages, every tenant's `site.language` and the instance's default
/// for it; an instance `entities.term_languages` ceiling replaces the default.
#[tokio::test]
async fn the_instance_term_languages() {
    if skipped() {
        return;
    }
    let (client, _) = fresh().await;
    let languages = || async {
        rows(
            &client,
            &format!(
                "WITH {} SELECT lang FROM shared_term_languages ORDER BY lang",
                triplespace_projections::entities::SHARED_TERM_LANGUAGES
            ),
        )
        .await
    };
    assert_eq!(languages().await, ["en", "mul"], "no configuration");
    let set = |tenant: &'static str,
               kind: &'static str,
               code: &'static str,
               config: serde_json::Value| {
        let client = &client;
        async move {
            client
                .execute(
                    "INSERT INTO view.registry (tenant, kind, code, config, \"offset\") VALUES ($1, $2, $3, $4, 1)
                     ON CONFLICT (tenant, kind, code) DO UPDATE SET config = EXCLUDED.config",
                    &[&tenant, &kind, &code, &config],
                )
                .await
                .unwrap();
        }
    };
    set(
        "librarybase",
        "site",
        "site.language",
        json!({"kind": "site", "name": "site.language", "value": "de"}),
    )
    .await;
    set(
        "other",
        "site",
        "site.language",
        json!({"kind": "site", "name": "site.language", "value": "en"}),
    )
    .await;
    set("", "template", "site:site.language", json!({"kind": "template", "record": {"kind": "site", "name": "site.language", "value": "fr"}})).await;
    assert_eq!(
        languages().await,
        ["de", "en", "fr", "mul"],
        "each tenant's content language and the instance's default for it"
    );
    set(
        "",
        "tenancy",
        "entities.term_languages",
        json!({"kind": "tenancy", "value": ["nl", "en"]}),
    )
    .await;
    assert_eq!(
        languages().await,
        ["en", "nl"],
        "the instance ceiling replaces the default"
    );
}

#[tokio::test]
async fn a_rebuild_in_the_wrong_order_fails_loudly() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let store = PgLog::new(client);
    // The local partition before the instance log: the Domain's surrogate is unknown when
    // its local records are replayed.
    p.catch_up(&backend, &store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let err = p.catch_up(&backend, &store, LOCAL, 10).await.unwrap_err();
    assert!(err.to_string().contains("no surrogate"), "{err}");
    // Once the instance log is in, the same catch-up succeeds.
    p.catch_up(&backend, &store, INSTANCE_LOG, 10)
        .await
        .unwrap();
    p.catch_up(&backend, &store, LOCAL, 10).await.unwrap();
}

fn item_statement(p: &str, v: &str, id: &str) -> serde_json::Value {
    json!({"mainsnak": {"snaktype": "value", "property": p,
            "datavalue": {"value": {"entity-type": "item", "id": v}, "type": "wikibase-entityid"},
            "datatype": "wikibase-item"},
        "type": "statement", "rank": "normal", "id": id})
}

// 0070 §2.2, A10: a tenant's write that names a foreign entity no mirror holds queues it for
// its provider's follower; one the mirror holds, a keyed ID, and a type the tenant's
// `entities.mirror` turns off are not queued.
#[tokio::test]
async fn a_linked_foreign_entity_is_queued_for_its_mirror() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let mut statement = item_statement("P7", "WDQ42", "Q6$00000000-0000-0000-0000-000000000067");
    statement["qualifiers"] = json!({"P8": [{"snaktype": "value", "property": "P8",
        "datavalue": {"value": {"entity-type": "item", "id": "XDQ10"}, "type": "wikibase-entityid"},
        "datatype": "wikibase-item"}]});
    statement["references"] = json!([{"snaks": {"P9": [{"snaktype": "value", "property": "P9",
        "datavalue": {"value": {"entity-type": "item", "id": "WDQ43"}, "type": "wikibase-entityid"},
        "datatype": "wikibase-item"}]}}]);
    let mut store = PgLog::new(client);
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "add", "id": "Q6", "claims": {"P7": [statement]}}),
        None,
        Some(6),
    );
    edit.revid = Some(902);
    store.append(LOCAL, edit).await.unwrap();
    let p = pipeline();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let client = store.into_inner();
    let queue = || async {
        sorted(
            &client,
            "SELECT tenant || '|' || provider || '|' || entity_id || '|' || reason FROM ops.entity_fetch",
        )
        .await
    };
    assert_eq!(
        queue().await,
        ["|wikidata|Q42|linked", "|wikidata|Q43|linked"],
        "the value and the reference value; the mirrored qualifier value is not queued"
    );

    // Off for Wikidata's items on this tenant: a later link queues nothing.
    client
        .execute(
            "INSERT INTO view.registry (tenant, kind, code, config, \"offset\")
             VALUES ($1, 'site', 'entities.mirror', $2, 1)",
            &[
                &TENANT,
                &json!({"kind": "site", "name": "entities.mirror", "value": {"WD": {"item": "off"}}}),
            ],
        )
        .await
        .unwrap();
    let mut store = PgLog::new(client);
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "add", "id": "Q6", "claims": {"P7": [item_statement("P7", "WDQ44", "Q6$00000000-0000-0000-0000-000000000068")]}}),
        None,
        Some(6),
    );
    edit.revid = Some(903);
    store.append(LOCAL, edit).await.unwrap();
    p.catch_up(&backend, &store, LOCAL, 2).await.unwrap();
    let client = store.into_inner();
    assert_eq!(
        sorted(&client, "SELECT entity_id FROM ops.entity_fetch").await,
        ["Q42", "Q43"],
        "`off` queues nothing"
    );
}

// 03 §4.2, §4.3 (0013 A108): composition writes a row of `view.entity_ref` for each entity
// a resolved state refers to, with the roles it is referred to in, whether or not any
// graph holds the target; the worker folds the changes into `view.entity_ref_count`.
#[tokio::test]
async fn references_are_indexed_and_counted() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let sid = "Q6$00000000-0000-0000-0000-000000000067";
    let mut statement = item_statement("P7", "WDQ42", sid);
    statement["qualifiers"] = json!({"P8": [{"snaktype": "value", "property": "P8",
        "datavalue": {"value": {"entity-type": "item", "id": "WDQ42"}, "type": "wikibase-entityid"},
        "datatype": "wikibase-item"}]});
    statement["references"] = json!([{"snaks": {"P9": [{"snaktype": "value", "property": "P9",
        "datavalue": {"value": {"entity-type": "item", "id": "Q6"}, "type": "wikibase-entityid"},
        "datatype": "wikibase-item"}]}}]);
    let mut store = PgLog::new(client);
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "add", "id": "Q6", "claims": {"P7": [statement]}}),
        None,
        Some(6),
    );
    edit.revid = Some(902);
    store.append(LOCAL, edit).await.unwrap();
    let p = pipeline();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let worker = Worker::new()
        .with(Arc::new(RefCountConsumer) as Arc<dyn Consumer<PgBackend>>)
        .unwrap();
    let refs = "SELECT target_id || '|' || roles FROM view.entity_ref
                WHERE tenant = 'librarybase' AND source_id = 'Q6' AND target_id IN ('P7', 'P8', 'P9', 'WDQ42', 'Q6')";
    let counts = "SELECT target_id || '|' || refs FROM view.entity_ref_count
                  WHERE tenant = 'librarybase' AND target_id IN ('P7', 'P8', 'P9', 'WDQ42', 'Q6')";
    {
        let client = store.client();
        // The value is referred to as a main value and a qualifier's; the entity's
        // reference to itself is no row.
        assert_eq!(
            sorted(client, refs).await,
            ["P7|1", "P8|2", "P9|4", "WDQ42|3"]
        );
        // Nothing is counted until the worker folds what composition appended.
        assert!(sorted(client, counts).await.is_empty());
        assert!(
            count(
                client,
                "SELECT count(*) FROM ops.projection_work WHERE projection = 'ref_count'"
            )
            .await
                > 0,
            "the fold is queued"
        );
    }
    worker.run_until_idle(&backend, 100).await.unwrap();
    {
        let client = store.client();
        assert_eq!(
            sorted(client, counts).await,
            ["P7|1", "P8|1", "P9|1", "WDQ42|1"]
        );
        assert_eq!(
            count(client, "SELECT count(*) FROM view.entity_ref_delta").await,
            0
        );
    }

    // The statement removed: its rows go, and the counts with them.
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "remove", "id": "Q6", "statements": [sid]}),
        None,
        Some(6),
    );
    edit.revid = Some(903);
    store.append(LOCAL, edit).await.unwrap();
    p.catch_up(&backend, &store, LOCAL, 2).await.unwrap();
    worker.run_until_idle(&backend, 100).await.unwrap();
    let client = store.into_inner();
    assert!(sorted(&client, refs).await.is_empty());
    assert!(sorted(&client, counts).await.is_empty());
}

// 04 §4.1–§4.3, §4.11 (0004 §1–3, §10): a tenant's own `same-as` and `different-from`
// form clusters; links that would put two members of one namespace into a cluster, or join
// two IDs a `different-from` keeps apart, are held, every one of them; removing the cause
// applies them again.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn tier_one_links_form_clusters_and_hold_conflicts() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let mut store = PgLog::new(seed(client).await);
    let p = pipeline();
    let link = |op: &str, key: &str, a: &str, b: &str| {
        draft(
            PAYLOAD_CHANGESET,
            key,
            &json!({"op": op, "id": key, "ids": [a, b]}),
            None,
            (key == "Q6").then_some(6),
        )
    };
    let unlink = |op: &str, key: &str, a: &str, b: &str| {
        draft(
            PAYLOAD_CHANGESET,
            key,
            &json!({"op": "remove", "id": key, "link": {"op": op, "ids": [a, b]}}),
            None,
            (key == "Q6").then_some(6),
        )
    };

    store
        .append(LOCAL, link("same-as", "Q6", "Q6", "WDQ1"))
        .await
        .unwrap();
    store
        .append(LOCAL, link("same-as", "WDQ1", "WDQ1", "OAW1"))
        .await
        .unwrap();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let client = store.into_inner();
    assert_eq!(
        cluster_state(&client).await,
        (vec!["Q6:OA=OAW1,WD=WDQ1,local=Q6".to_string()], vec![]),
        "the local member is canonical"
    );
    assert_eq!(
        triplespace_projections::identity::members(&client, TENANT, "OAW1")
            .await
            .unwrap(),
        ["Q6", "WDQ1"]
    );

    // A second Wikidata member: the links are held, and the cluster dissolves.
    let client = append_local(
        client,
        &backend,
        &p,
        link("same-as", "WDQ2", "OAW1", "WDQ2"),
    )
    .await;
    let (clusters, held) = cluster_state(&client).await;
    assert!(clusters.is_empty(), "{clusters:?}");
    assert_eq!(held, ["OAW1=WDQ1", "OAW1=WDQ2", "Q6=WDQ1"]);
    assert_eq!(
        count(
            &client,
            "SELECT count(DISTINCT conflict_id) FROM view.link WHERE status = 'held'"
        )
        .await,
        1
    );

    // Removing the link that caused it applies the others again.
    let client = append_local(
        client,
        &backend,
        &p,
        unlink("same-as", "WDQ2", "OAW1", "WDQ2"),
    )
    .await;
    assert_eq!(
        cluster_state(&client).await.0,
        ["Q6:OA=OAW1,WD=WDQ1,local=Q6"]
    );

    // `different-from` keeps two members apart, through the third.
    let client = append_local(
        client,
        &backend,
        &p,
        link("different-from", "Q6", "Q6", "OAW1"),
    )
    .await;
    let (clusters, held) = cluster_state(&client).await;
    assert!(clusters.is_empty());
    assert_eq!(held, ["OAW1=WDQ1", "Q6=WDQ1"]);
    let client = append_local(
        client,
        &backend,
        &p,
        unlink("different-from", "Q6", "Q6", "OAW1"),
    )
    .await;
    assert_eq!(
        cluster_state(&client).await.0,
        ["Q6:OA=OAW1,WD=WDQ1,local=Q6"]
    );
    assert_eq!(
        triplespace_projections::identity::members(&client, TENANT, "Q7")
            .await
            .unwrap(),
        Vec::<String>::new()
    );
}

/// The tenant's clusters, as `canonical:namespace=member,…`, and its held links.
async fn cluster_state(client: &Client) -> (Vec<String>, Vec<String>) {
    let clusters = sorted(
        client,
        "SELECT c.canonical_id || ':' || string_agg(m.namespace || '=' || m.entity_id, ',' ORDER BY m.namespace COLLATE \"C\")
           FROM view.cluster c JOIN view.cluster_member m USING (tenant, cluster_id)
          WHERE c.tenant = 'librarybase' GROUP BY c.canonical_id",
    )
    .await;
    let held = sorted(
        client,
        "SELECT a || '=' || b FROM view.link WHERE tenant = 'librarybase' AND status = 'held'",
    )
    .await;
    (clusters, held)
}

/// Appends a record to the tenant's local graph and projects it.
async fn append_local(
    client: Client,
    backend: &PgBackend,
    p: &Pipeline<PgBackend>,
    record: Draft,
) -> Client {
    let mut store = PgLog::new(client);
    store.append(LOCAL, record).await.unwrap();
    p.catch_up(backend, &store, LOCAL, 2).await.unwrap();
    store.into_inner()
}

// 04 §4.4 (0004 A25): each member of a cluster reads as the cluster's fused body, under its
// own ID, with the cluster's canonical ID in its row; removing the link parts them again.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_clusters_members_share_its_fused_body() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let mut store = PgLog::new(seed(client).await);
    let p = pipeline();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let record =
        |content: serde_json::Value| draft(PAYLOAD_CHANGESET, "Q6", &content, None, Some(6));
    store
        .append(
            LOCAL,
            record(json!({"op": "same-as", "id": "Q6", "ids": ["Q6", "XDQ10"]})),
        )
        .await
        .unwrap();
    p.catch_up(&backend, &store, LOCAL, 2).await.unwrap();
    let client = store.into_inner();
    let order = triplespace_projections::EntityProjection::default();
    let read = |id: &'static str| {
        let (client, order) = (&client, &order);
        async move {
            triplespace_projections::read::current(
                client,
                order,
                TENANT,
                &scatter_wikibase_model::id::EntityId::parse(id).unwrap(),
            )
            .await
            .unwrap()
            .unwrap()
        }
    };
    let xd = read("XDQ10").await;
    assert_eq!(xd.resolved.entity.id.as_str(), "XDQ10", "under its own ID");
    assert_eq!(
        xd.resolved.entity.labels.get("en").map(String::as_str),
        Some("Six"),
        "the local graph comes first"
    );
    let statements: Vec<String> = xd
        .resolved
        .entity
        .all_statements()
        .filter_map(|s| s.id.as_ref().map(ToString::to_string))
        .collect();
    // Q6's statement, read as XDQ10's, is named after XDQ10 and keeps its UUID (04 §4.9);
    // its provenance keeps the GUID as stored.
    let as_member = Q6_P2.replacen("Q6", "XDQ10", 1);
    assert!(statements.contains(&as_member), "{statements:?}");
    assert!(
        xd.resolved
            .statements
            .iter()
            .any(|p| p.members.iter().any(|m| m.as_str() == Q6_P2))
    );
    assert_eq!(
        xd.cluster.as_ref().map(|m| m.canonical.as_str()),
        Some("Q6")
    );
    assert!(xd.local().is_none(), "Q6's local state is not XDQ10's");
    assert_eq!(read("Q6").await.resolved.entity.labels.len(), 2);
    let canonical =
        |id: &str| {
            let client = &client;
            let id = id.to_string();
            async move {
                client
                .query_one(
                    "SELECT canonical_id, cluster_id IS NOT NULL, resolved_kind FROM view.entity
                      WHERE tenant = $1 AND id = $2",
                    &[&TENANT, &id],
                )
                .await
                .map(|r| (r.get::<_, String>(0), r.get::<_, bool>(1), r.get::<_, String>(2)))
                .unwrap()
            }
        };
    assert_eq!(
        canonical("XDQ10").await,
        ("Q6".into(), true, "materialized".into())
    );
    assert_eq!(
        canonical("Q6").await,
        ("Q6".into(), true, "materialized".into())
    );

    // Unlinked, each is itself again.
    let client = append_local(
        client,
        &backend,
        &p,
        record(
            json!({"op": "remove", "id": "Q6", "link": {"op": "same-as", "ids": ["Q6", "XDQ10"]}}),
        ),
    )
    .await;
    let xd = triplespace_projections::read::current(
        &client,
        &order,
        TENANT,
        &scatter_wikibase_model::id::EntityId::parse("XDQ10").unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        xd.resolved.entity.labels.get("en").map(String::as_str),
        Some("Registrar")
    );
    assert_eq!(
        client
            .query_one(
                "SELECT canonical_id, cluster_id IS NULL FROM view.entity WHERE tenant = $1 AND id = 'XDQ10'",
                &[&TENANT],
            )
            .await
            .map(|r| (r.get::<_, String>(0), r.get::<_, bool>(1)))
            .unwrap(),
        ("XDQ10".into(), true)
    );
}

// 04 §4.6–§4.8 (0004 A27): statements that say the same thing through two members of a
// property cluster fuse into one, with both members in its provenance.
#[tokio::test]
async fn statements_fuse_across_a_property_cluster() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let mut store = PgLog::new(seed(client).await);
    let item = json!({"type": "item", "id": "XDQ10", "labels": {"en": label("en", "Registrar")},
        "claims": {"XDP4": [string_statement("XDP4", "x", "XDQ10$00000000-0000-0000-0000-0000000000C4", "string")]}});
    store
        .append(
            MIRROR,
            draft(
                PAYLOAD_CHANGESET,
                "XDQ10",
                &json!({"op": "put", "id": "XDQ10", "entity": item, "upstream": {"revid": 58}, "size": 120}),
                Some(7),
                Some(101),
            ),
        )
        .await
        .unwrap();
    store
        .append(
            LOCAL,
            draft(
                PAYLOAD_CHANGESET,
                "Q6",
                &json!({"op": "same-as", "id": "Q6", "ids": ["Q6", "XDQ10"]}),
                None,
                Some(6),
            ),
        )
        .await
        .unwrap();
    let p = pipeline();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let client = store.into_inner();
    assert_eq!(
        p4_statements(&client).await.len(),
        2,
        "P4 and XDP4 are two properties"
    );
    let client = append_local(
        client,
        &backend,
        &p,
        draft(
            PAYLOAD_CHANGESET,
            "P4",
            &json!({"op": "equivalent-property", "id": "P4", "ids": ["P4", "XDP4"]}),
            None,
            None,
        ),
    )
    .await;
    assert_eq!(
        cluster_state(&client).await.0,
        ["P4:XD=XDP4,local=P4", "Q6:XD=XDQ10,local=Q6"]
    );
    assert_eq!(
        p4_statements(&client).await,
        [vec![
            Q6_P4.to_string(),
            "XDQ10$00000000-0000-0000-0000-0000000000C4".to_string()
        ]],
        "one statement, asserted by both graphs"
    );
}

/// The members of each statement of `Q6`'s fused body that `P4` or `XDP4` makes.
async fn p4_statements(client: &Client) -> Vec<Vec<String>> {
    let order = triplespace_projections::EntityProjection::default();
    let q6 = scatter_wikibase_model::id::EntityId::parse("Q6").unwrap();
    let c = triplespace_projections::read::current(client, &order, TENANT, &q6)
        .await
        .unwrap()
        .unwrap();
    let mut x: Vec<Vec<String>> = c
        .resolved
        .statements
        .iter()
        .filter(|s| {
            s.members
                .iter()
                .any(|m| m.as_str().ends_with("C4") || m.as_str() == Q6_P4)
        })
        .map(|s| s.members.iter().map(ToString::to_string).collect())
        .collect();
    x.sort();
    x
}

// 04 §4.4 (0004 A27): a deleted member leaves its cluster, its links kept and not applied;
// undeleted, it is a member again.
#[tokio::test]
async fn a_deleted_member_leaves_its_cluster() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let mut store = PgLog::new(seed(client).await);
    store
        .append(
            LOCAL,
            draft(
                PAYLOAD_CHANGESET,
                "Q6",
                &json!({"op": "same-as", "id": "Q6", "ids": ["Q6", "XDQ10"]}),
                None,
                Some(6),
            ),
        )
        .await
        .unwrap();
    let p = pipeline();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let acl = |deleted: bool| {
        let mut v = json!({"target": "acl:entity:Q6", "restrictions": {}});
        if deleted {
            v["restrictions"] = json!({"read": {"group": "sysop"}});
            v["read_kind"] = json!("moderation");
        }
        draft(
            triplespace_projections::actors::payload::ACL,
            "acl:entity:Q6",
            &v,
            None,
            None,
        )
    };
    store.append(TENANT_LOG, acl(true)).await.unwrap();
    p.catch_up(&backend, &store, TENANT_LOG, 2).await.unwrap();
    let client = store.into_inner();
    assert!(cluster_state(&client).await.0.is_empty(), "Q6 has left");
    assert_eq!(
        rows(
            &client,
            "SELECT status FROM view.link WHERE tenant = 'librarybase'"
        )
        .await,
        ["inactive"]
    );
    assert_eq!(label_of_xd(&client).await.as_deref(), Some("Registrar"));

    let mut store = PgLog::new(client);
    store.append(TENANT_LOG, acl(false)).await.unwrap();
    p.catch_up(&backend, &store, TENANT_LOG, 2).await.unwrap();
    let client = store.into_inner();
    assert_eq!(cluster_state(&client).await.0, ["Q6:XD=XDQ10,local=Q6"]);
    assert_eq!(label_of_xd(&client).await.as_deref(), Some("Six"));
}

/// `XDQ10`'s English label as the tenant reads it.
async fn label_of_xd(client: &Client) -> Option<String> {
    let order = triplespace_projections::EntityProjection::default();
    triplespace_projections::read::current(
        client,
        &order,
        TENANT,
        &scatter_wikibase_model::id::EntityId::parse("XDQ10").unwrap(),
    )
    .await
    .unwrap()
    .unwrap()
    .resolved
    .entity
    .labels
    .get("en")
    .cloned()
}

// 04 §4.3 (0004 A28): a mirrored statement of a link property the instance names links its
// entity to the entity its value names, as a shared cluster; a tenant's own link builds on
// it, and the link lasts only as long as the statement.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn mirror_derived_links_form_shared_clusters() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    client
        .execute(
            "INSERT INTO view.registry (tenant, kind, code, config, \"offset\")
             VALUES ('', 'reconcile', 'default', $1, 1)",
            &[&json!({"kind": "reconcile", "name": "default",
                "links": {"XD": [{"property": "P9", "target": "WD"}]}})],
        )
        .await
        .unwrap();
    let put = |claims: serde_json::Value, revid: u64| {
        let item = json!({"type": "item", "id": "XDQ10", "labels": {"en": label("en", "Registrar")}, "claims": claims});
        draft(
            PAYLOAD_CHANGESET,
            "XDQ10",
            &json!({"op": "put", "id": "XDQ10", "entity": item, "upstream": {"revid": revid}, "size": 120}),
            Some(7),
            Some(101),
        )
    };
    let mut store = PgLog::new(client);
    store
        .append(
            MIRROR,
            put(
                json!({"XDP9": [string_statement("XDP9", "Q42", "XDQ10$00000000-0000-0000-0000-0000000000D9", "external-id")]}),
                58,
            ),
        )
        .await
        .unwrap();
    let p = pipeline();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let client = store.into_inner();
    assert_eq!(
        shared_clusters(&client).await,
        ["WDQ42:WDQ42,XDQ10"],
        "Wikidata ranks first"
    );
    assert_eq!(
        rows(
            &client,
            "SELECT a || '=' || b || '|' || tier FROM view.link WHERE tenant = ''"
        )
        .await,
        ["WDQ42=XDQ10|2"]
    );
    assert_eq!(
        triplespace_projections::identity::members(&client, TENANT, "XDQ10")
            .await
            .unwrap(),
        ["WDQ42"],
        "a tenant with no links of its own reads the shared cluster"
    );

    // The tenant's own link joins the shared cluster in its view; the shared one is as it was.
    let client = append_local(
        client,
        &backend,
        &p,
        draft(
            PAYLOAD_CHANGESET,
            "Q6",
            &json!({"op": "same-as", "id": "Q6", "ids": ["Q6", "XDQ10"]}),
            None,
            Some(6),
        ),
    )
    .await;
    assert_eq!(
        cluster_state(&client).await.0,
        ["Q6:WD=WDQ42,XD=XDQ10,local=Q6"]
    );
    assert_eq!(shared_clusters(&client).await, ["WDQ42:WDQ42,XDQ10"]);

    // The statement goes upstream: so do the shared link and cluster, in every view.
    let mut store = PgLog::new(client);
    store.append(MIRROR, put(json!({}), 59)).await.unwrap();
    p.catch_up(&backend, &store, MIRROR, 2).await.unwrap();
    let client = store.into_inner();
    assert!(shared_clusters(&client).await.is_empty());
    assert_eq!(cluster_state(&client).await.0, ["Q6:XD=XDQ10,local=Q6"]);
}

/// The shared clusters, as `canonical:member,…`.
async fn shared_clusters(client: &Client) -> Vec<String> {
    sorted(
        client,
        "SELECT c.canonical_id || ':' || string_agg(m.entity_id, ',' ORDER BY m.entity_id COLLATE \"C\")
           FROM view.cluster c JOIN view.cluster_member m USING (tenant, cluster_id)
          WHERE c.tenant = '' GROUP BY c.canonical_id",
    )
    .await
}

const WIKIDATA: u64 = 0x2005;
const TENANT_CONFIG: u64 = 0x2006;

/// Opens a second mirror, Wikidata's, and the tenant's `config`.
async fn more_partitions(client: &Client) {
    for (partition, tenant, name) in [
        (WIKIDATA, None, "mirror/wikidata"),
        (TENANT_CONFIG, Some(TENANT), "config"),
    ] {
        log::create_partition(
            client,
            partition,
            Segments::new(8).unwrap(),
            &PartitionInfo {
                tenant: tenant.map(str::to_owned),
                name: name.into(),
                graph_iri: format!("https://scatter.example/graph/{name}"),
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
}

/// A mirror's `put` of an item with the given claims.
fn mirror_put(id: &str, claims: &serde_json::Value, revid: u64, page_id: u64) -> Draft {
    let item =
        json!({"type": "item", "id": id, "labels": {"en": label("en", id)}, "claims": claims});
    draft(
        PAYLOAD_CHANGESET,
        id,
        &json!({"op": "put", "id": id, "entity": item, "upstream": {"revid": revid}, "size": 120}),
        Some(7),
        Some(page_id),
    )
}

/// A `reconcile:default` record.
fn reconcile(content: &serde_json::Value) -> Draft {
    let mut content = content.clone();
    content["kind"] = json!("reconcile");
    content["name"] = json!("default");
    draft(PAYLOAD_CONFIG, "reconcile:default", &content, None, None)
}

/// Appends a record and projects its partition.
async fn append_to(
    client: Client,
    backend: &PgBackend,
    p: &Pipeline<PgBackend>,
    partition: u64,
    record: Draft,
) -> Client {
    let mut store = PgLog::new(client);
    store.append(partition, record).await.unwrap();
    p.catch_up(backend, &store, partition, 2).await.unwrap();
    store.into_inner()
}

/// The DOI statement the inference tests share.
fn doi(subject: &str, value: &str) -> serde_json::Value {
    json!({"WDP356": [string_statement(
        "WDP356",
        value,
        &format!("{subject}$00000000-0000-0000-0000-000000000356"),
        "external-id",
    )]})
}

// 04 §4.3 (0004 §3 tier 3, A29): two mirrored entities of different namespaces that share a
// value of an inference property the instance names are one shared cluster; the link is no
// exact match; a tenant may decline it; and a value that stops being unique links nothing.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn inferred_links_join_entities_that_share_a_unique_value() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    more_partitions(&client).await;
    let p = pipeline();
    let mut store = PgLog::new(client);
    store
        .append(CONFIG_PARTITION, reconcile(&json!({"infer": ["WDP356"]})))
        .await
        .unwrap();
    store
        .append(
            WIKIDATA,
            mirror_put("WDQ42", &doi("WDQ42", "10.1/x"), 1, 142),
        )
        .await
        .unwrap();
    store
        .append(
            MIRROR,
            mirror_put("XDQ10", &doi("XDQ10", "10.1/x"), 58, 101),
        )
        .await
        .unwrap();
    for partition in [
        CONFIG_PARTITION,
        INSTANCE_LOG,
        WIKIDATA,
        MIRROR,
        LOCAL,
        TENANT_LOG,
    ] {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let client = store.into_inner();
    assert_eq!(shared_clusters(&client).await, ["WDQ42:WDQ42,XDQ10"]);
    assert_eq!(
        rows(
            &client,
            "SELECT a || '=' || b || '|' || tier || '|' || split_part(source, '=', 1)
               FROM view.link WHERE tenant = ''"
        )
        .await,
        ["WDQ42=XDQ10|3|WDP356"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.cluster WHERE tenant = '' AND inferred"
        )
        .await,
        1
    );
    // The tenant reads the shared cluster, and its fused body; but a shared identifier
    // value is no exact match, so no response names one member by the other (04 §4.12).
    assert_eq!(
        triplespace_projections::identity::members(&client, TENANT, "XDQ10")
            .await
            .unwrap(),
        ["WDQ42"]
    );
    assert!(
        triplespace_projections::identity::exact_matches(&client, TENANT, &["XDQ10".to_string()])
            .await
            .unwrap()
            .is_empty()
    );

    // The tenant declines inferred links: in its view the two are apart, each with a row
    // of its own; the shared cluster is as it was.
    let client = append_to(
        client,
        &backend,
        &p,
        TENANT_CONFIG,
        reconcile(&json!({"infer": false})),
    )
    .await;
    assert!(
        triplespace_projections::identity::members(&client, TENANT, "XDQ10")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(shared_clusters(&client).await, ["WDQ42:WDQ42,XDQ10"]);
    assert_eq!(
        sorted(
            &client,
            "SELECT id || '|' || canonical_id FROM view.entity
              WHERE tenant = 'librarybase' AND id IN ('WDQ42', 'XDQ10')"
        )
        .await,
        ["WDQ42|WDQ42", "XDQ10|XDQ10"]
    );
    // It takes them again.
    let client = append_to(
        client,
        &backend,
        &p,
        TENANT_CONFIG,
        reconcile(&json!({"infer": true})),
    )
    .await;
    assert_eq!(
        triplespace_projections::identity::members(&client, TENANT, "XDQ10")
            .await
            .unwrap(),
        ["WDQ42"]
    );

    // A second Wikidata item with the value: it is unique in no namespace but the other's,
    // so nothing is linked. The item loses it, and the link is back.
    let client = append_to(
        client,
        &backend,
        &p,
        WIKIDATA,
        mirror_put("WDQ43", &doi("WDQ43", "10.1/x"), 2, 143),
    )
    .await;
    assert!(shared_clusters(&client).await.is_empty());
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.link WHERE tier = 3").await,
        0
    );
    let client = append_to(
        client,
        &backend,
        &p,
        WIKIDATA,
        mirror_put("WDQ43", &json!({}), 3, 143),
    )
    .await;
    assert_eq!(shared_clusters(&client).await, ["WDQ42:WDQ42,XDQ10"]);

    // A rebuild from the log reproduces the links and the clusters.
    let before = identity_rows(&client).await;
    let store = PgLog::new(client);
    let order = [
        CONFIG_PARTITION,
        INSTANCE_LOG,
        WIKIDATA,
        MIRROR,
        TENANT_CONFIG,
        LOCAL,
        TENANT_LOG,
    ];
    p.rebuild(&backend, &store, &order, 3).await.unwrap();
    assert_eq!(identity_rows(store.client()).await, before);
}

/// Every row of the cluster tables, but for a cluster's version, which counts recomputations.
async fn identity_rows(client: &Client) -> Vec<String> {
    let mut out = Vec::new();
    for sql in [
        "SELECT 'link|' || tenant || '|' || a || '|' || b || '|' || tier || '|' || status FROM view.link",
        "SELECT 'block|' || tenant || '|' || a || '|' || b FROM view.different_from",
        "SELECT 'retired|' || tenant || '|' || entity_id || '|' || coalesce(target, '-') FROM view.retired",
        "SELECT 'cluster|' || tenant || '|' || canonical_id || '|' || inferred FROM view.cluster",
        "SELECT 'member|' || tenant || '|' || namespace || '|' || entity_id FROM view.cluster_member",
    ] {
        out.extend(rows(client, sql).await);
    }
    out.sort();
    out
}

// 04 §4.1, §4.4 (0004 §1, §4, A29): an ID upstream merged into another is an alias of it,
// so what was linked to it is linked to its target; an entity upstream deleted leaves its
// cluster, comes back with its next state, and stays where the tenant retains it.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_retired_member_gives_way() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let mut store = PgLog::new(client);
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    store
        .append(
            LOCAL,
            draft(
                PAYLOAD_CHANGESET,
                "Q6",
                &json!({"op": "same-as", "id": "Q6", "ids": ["Q6", "XDQ10"]}),
                None,
                Some(6),
            ),
        )
        .await
        .unwrap();
    p.catch_up(&backend, &store, LOCAL, 2).await.unwrap();
    let client = store.into_inner();
    assert_eq!(cluster_state(&client).await.0, ["Q6:XD=XDQ10,local=Q6"]);

    // Upstream merges XDQ10 into XDQ11.
    let client = append_to(
        client,
        &backend,
        &p,
        MIRROR,
        mirror_put("XDQ11", &json!({}), 60, 111),
    )
    .await;
    let redirect = draft(
        PAYLOAD_CHANGESET,
        "XDQ10",
        &json!({"op": "redirect", "from": "XDQ10", "to": "XDQ11", "upstream": {"revid": 61}}),
        Some(7),
        Some(101),
    );
    let client = append_to(client, &backend, &p, MIRROR, redirect).await;
    assert_eq!(cluster_state(&client).await.0, ["Q6:XD=XDQ11,local=Q6"]);
    assert_eq!(
        rows(
            &client,
            "SELECT entity_id || '>' || target FROM view.retired"
        )
        .await,
        ["XDQ10>XDQ11"]
    );

    // Upstream deletes XDQ11: under the default policy, `orphan`, it leaves the cluster.
    let tombstone = |revid: u64| {
        draft(
            PAYLOAD_CHANGESET,
            "XDQ11",
            &json!({"op": "tombstone", "id": "XDQ11", "upstream": {"revid": revid}}),
            Some(7),
            Some(111),
        )
    };
    let client = append_to(client, &backend, &p, MIRROR, tombstone(62)).await;
    assert!(cluster_state(&client).await.0.is_empty());
    assert_eq!(
        rows(
            &client,
            "SELECT status FROM view.link WHERE tenant = 'librarybase'"
        )
        .await,
        ["inactive"]
    );
    // It comes back upstream, and into the cluster.
    let client = append_to(
        client,
        &backend,
        &p,
        MIRROR,
        mirror_put("XDQ11", &json!({}), 63, 111),
    )
    .await;
    assert_eq!(cluster_state(&client).await.0, ["Q6:XD=XDQ11,local=Q6"]);

    // The tenant retains it; deleted upstream again, it stays.
    let client = append_local(
        client,
        &backend,
        &p,
        draft(
            PAYLOAD_CHANGESET,
            "XDQ11",
            &json!({"op": "retain", "id": "XDQ11", "policy": "retain"}),
            None,
            Some(111),
        ),
    )
    .await;
    let client = append_to(client, &backend, &p, MIRROR, tombstone(64)).await;
    assert_eq!(cluster_state(&client).await.0, ["Q6:XD=XDQ11,local=Q6"]);
}

// 04 §4.3, §4.4 (0004 §3, §4, A29): the instance's record orders the providers, so the
// canonical member of a shared cluster follows it; and a tenant that deprecates the
// mirrored statement a link comes from no longer takes the link.
#[tokio::test]
async fn the_record_orders_providers_and_a_correction_drops_a_link() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let statement = "XDQ10$00000000-0000-0000-0000-0000000000D9";
    let mut store = PgLog::new(client);
    store
        .append(
            CONFIG_PARTITION,
            reconcile(&json!({"order": ["XD", "WD"],
                "links": {"XD": [{"property": "P9", "target": "WD"}]}})),
        )
        .await
        .unwrap();
    store
        .append(
            MIRROR,
            mirror_put(
                "XDQ10",
                &json!({"XDP9": [string_statement("XDP9", "Q42", statement, "external-id")]}),
                58,
                101,
            ),
        )
        .await
        .unwrap();
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let client = store.into_inner();
    assert_eq!(
        shared_clusters(&client).await,
        ["XDQ10:WDQ42,XDQ10"],
        "the record puts XD before Wikidata"
    );
    assert_eq!(
        triplespace_projections::identity::members(&client, TENANT, "XDQ10")
            .await
            .unwrap(),
        ["WDQ42"]
    );

    let client = append_local(
        client,
        &backend,
        &p,
        draft(
            PAYLOAD_CHANGESET,
            "XDQ10",
            &json!({"op": "override", "id": "XDQ10", "statement": statement, "rank": "deprecated"}),
            None,
            Some(101),
        ),
    )
    .await;
    assert!(
        triplespace_projections::identity::members(&client, TENANT, "XDQ10")
            .await
            .unwrap()
            .is_empty(),
        "the tenant deprecated the statement"
    );
    assert_eq!(shared_clusters(&client).await, ["XDQ10:WDQ42,XDQ10"]);
}

// 04 §4.1 (0004 §1, A29): a local merge, and one a wiki's history brought with it, make the
// retired ID an alias of its target, so what was linked to it is linked to the target; a
// whole state of the ID makes it stand for itself again; a rebuild reproduces the rows; and
// an erased redirect takes its alias with it.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_local_merge_makes_an_alias() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let store = PgLog::new(client);
    for partition in ALL {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let mut client = store.into_inner();
    let local = |key: &str, op: serde_json::Value, page: u64| {
        draft(PAYLOAD_CHANGESET, key, &op, None, Some(page))
    };
    let adopt = |op: serde_json::Value, revid: u64| {
        let mut d = draft(PAYLOAD_CHANGESET, "Q8", &op, None, Some(8));
        d.revid = Some(revid);
        d
    };
    let q8 =
        json!({"type": "item", "id": "Q8", "labels": {"en": label("en", "Eight")}, "claims": {}});
    for record in [
        local(
            "Q7",
            json!({"op": "same-as", "id": "Q7", "ids": ["Q7", "XDQ10"]}),
            7,
        ),
        adopt(
            json!({"op": "adopt", "id": "Q8", "entity": q8, "source_revid": 5, "source_time": "2026-09-20T14:02:11Z", "source_pageid": 8}),
            950,
        ),
        local(
            "Q8",
            json!({"op": "same-as", "id": "Q8", "ids": ["Q8", "OAW1"]}),
            8,
        ),
    ] {
        client = append_local(client, &backend, &p, record).await;
    }
    assert_eq!(
        cluster_state(&client).await.0,
        ["Q7:XD=XDQ10,local=Q7", "Q8:OA=OAW1,local=Q8"]
    );

    // Q7 is merged into Q6, and the adopted history turns Q8 into a redirect to Q6.
    let redirect = local("Q7", json!({"op": "redirect", "from": "Q7", "to": "Q6"}), 7);
    client = append_local(client, &backend, &p, redirect).await;
    assert_eq!(
        cluster_state(&client).await.0,
        ["Q6:XD=XDQ10,local=Q6", "Q8:OA=OAW1,local=Q8"]
    );
    let redirected = adopt(
        json!({"op": "adopt", "id": "Q8", "redirect": "Q6", "source_revid": 6, "source_parentid": 5, "source_time": "2026-09-21T14:02:11Z", "source_pageid": 8}),
        951,
    );
    client = append_local(client, &backend, &p, redirected).await;
    assert_eq!(
        cluster_state(&client).await.0,
        ["Q6:OA=OAW1,XD=XDQ10,local=Q6"]
    );
    assert_eq!(
        sorted(
            &client,
            "SELECT entity_id || '>' || target FROM view.retired"
        )
        .await,
        ["Q7>Q6", "Q8>Q6"]
    );

    // The history goes on: Q8 is an item again, and stands for itself.
    let restored = adopt(
        json!({"op": "adopt", "id": "Q8", "entity": q8, "source_revid": 7, "source_parentid": 6, "source_time": "2026-09-22T14:02:11Z", "source_pageid": 8}),
        952,
    );
    client = append_local(client, &backend, &p, restored).await;
    assert_eq!(
        cluster_state(&client).await.0,
        ["Q6:XD=XDQ10,local=Q6", "Q8:OA=OAW1,local=Q8"]
    );

    // A rebuild reproduces the rows.
    let before = identity_rows(&client).await;
    let mut store = PgLog::new(client);
    p.rebuild(&backend, &store, &ALL, 3).await.unwrap();
    assert_eq!(identity_rows(store.client()).await, before);

    // The redirect of Q7 is erased: its alias goes, and Q7 has its link again.
    let offset: i64 = store
        .client()
        .query_one(
            "SELECT \"offset\" FROM view.retired WHERE entity_id = 'Q7'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    store
        .erase_parts(
            LOCAL,
            u64::try_from(offset).unwrap(),
            &[scatter_log::body::CONTENT],
        )
        .await
        .unwrap();
    p.rebuild(&backend, &store, &ALL, 3).await.unwrap();
    assert_eq!(
        cluster_state(store.client()).await.0,
        ["Q7:XD=XDQ10,local=Q7", "Q8:OA=OAW1,local=Q8"]
    );
}

const SOURCE: u64 = 0x2007;

// 04 §4.3, §4.4 (0004 §3, §9, A29): a tenant's record names a link property for one of its
// own entity sources; a statement of it in the source's graph links the source's entity as
// the tenant's own tier-2 row, in the tenant's clusters and no one else's, and lasts as
// long as the statement.
#[tokio::test]
async fn a_tenants_source_links_by_its_own_properties() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    more_partitions(&client).await;
    log::create_partition(
        &client,
        SOURCE,
        Segments::new(8).unwrap(),
        &PartitionInfo {
            tenant: Some(TENANT.into()),
            name: "source/mhc".into(),
            graph_iri: "https://scatter.example/graph/source/mhc".into(),
            history: History::Full,
            integrity: Integrity::Logged,
            export: Export::Public,
            hash: "sha-256".into(),
            origin: None,
        },
    )
    .await
    .unwrap();
    let p = pipeline();
    let mut store = PgLog::new(client);
    store
        .append(
            TENANT_CONFIG,
            reconcile(&json!({"order": ["mhc"],
                "links": {"mhc": [{"property": "P9", "target": "local"}]}})),
        )
        .await
        .unwrap();
    let statement = "mhc:Q1$00000000-0000-0000-0000-0000000000E9";
    store
        .append(
            SOURCE,
            mirror_put(
                "mhc:Q1",
                &json!({"mhc:P9": [string_statement("mhc:P9", "Q6", statement, "external-id")]}),
                1,
                3001,
            ),
        )
        .await
        .unwrap();
    for partition in [
        CONFIG_PARTITION,
        INSTANCE_LOG,
        MIRROR,
        TENANT_CONFIG,
        SOURCE,
        LOCAL,
        TENANT_LOG,
    ] {
        p.catch_up(&backend, &store, partition, 2).await.unwrap();
    }
    let client = store.into_inner();
    assert_eq!(
        rows(
            &client,
            "SELECT tenant || '|' || a || '=' || b || '|' || tier FROM view.link"
        )
        .await,
        ["librarybase|Q6=mhc:Q1|2"]
    );
    assert_eq!(cluster_state(&client).await.0, ["Q6:local=Q6,mhc=mhc:Q1"]);
    assert!(shared_clusters(&client).await.is_empty());

    // The source drops the statement: the link and the cluster go.
    let client = append_to(
        client,
        &backend,
        &p,
        SOURCE,
        mirror_put("mhc:Q1", &json!({}), 2, 3001),
    )
    .await;
    assert!(cluster_state(&client).await.0.is_empty());
    assert_eq!(count(&client, "SELECT count(*) FROM view.link").await, 0);
}
