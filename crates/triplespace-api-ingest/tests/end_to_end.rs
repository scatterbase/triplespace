//! The whole write side over Postgres: adoption, a bulk batch, a mirror sync and an
//! interactive edit go through `scatter-ingest` on `PgIngest`, and the `view` tables come
//! out as the projections describe them. Named by `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use ed25519_dalek::SigningKey;
use scatter_actors::issuer::IssuerRegistry;
use scatter_ingest::adopt::{
    Adopted, AdoptedItem, AdoptedPage, Adoption, Author, Floors, run_adoption,
};
use scatter_ingest::{
    Attestation, BlockEntry, BlockPayload, IngestError, Request, SyncItem, run_batch, run_batch_in,
    run_sync, write_many, write_operation,
};
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
use scatter_projection::{Backend, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{Graph, JobHeader, Mode, Operation, ProviderOrder, Upstream};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use serde_json::json;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::PgIngest;
use triplespace_projections::{Farm, PgBackend, milestone_pipeline};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const TENANT: &str = "librarybase";
const LOCAL: u64 = 0x3001;
const PAGES: u64 = 0x3002;
const TENANT_LOG: u64 = 0x3003;
const ACTORS: u64 = 0x3004;
const INSTANCE_LOG: u64 = 0x3011;
const MIRROR: u64 = 0x3012;
const NOW: u64 = 1_790_000_000_000_000;

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
        "tsingest_test_{}_{}",
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

fn pipeline() -> Pipeline<PgIngest> {
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

/// Genesis, the tenant's registry entry, the partitions and the sequences.
async fn seed(client: Client) -> Client {
    let key = SigningKey::from_bytes(&[29; 32]);
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
            Draft {
                appended_at: 2,
                payload_type: PAYLOAD_CONFIG.into(),
                key: Some(format!("tenant:{TENANT}")),
                revid: None,
                logid: None,
                page_id: None,
                body: Body::core(
                    &Value::from_json(&json!({"kind": "tenant", "slug": TENANT, "base": "https://librarybase.org", "issuer": TENANT})),
                    &Value::Null,
                    &Value::map(vec![(Value::text("actor"), Value::text("instance:scatter"))]),
                )
                .unwrap(),
            },
        )
        .await
        .unwrap();
    let client = store.into_inner();
    for (partition, tenant, name) in [
        (LOCAL, Some(TENANT), "local"),
        (PAGES, Some(TENANT), "pages"),
        (TENANT_LOG, Some(TENANT), "log"),
        (ACTORS, Some(TENANT), "actors"),
        (INSTANCE_LOG, None, "log"),
        (MIRROR, None, "mirror/internetdomains"),
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
    PgIngest::create_sequences(&client, TENANT, &["item", "property"], &["domain"])
        .await
        .unwrap();
    client
}

fn entity(v: serde_json::Value) -> Entity {
    Entity::from_value(v).unwrap().entity
}

fn string_statement(p: &str, v: &str, datatype: &str, id: &str) -> serde_json::Value {
    json!({"mainsnak": {"snaktype": "value", "property": p, "datavalue": {"value": v, "type": "string"}, "datatype": datatype},
        "type": "statement", "rank": "normal", "id": id})
}

fn item(id: &str, label: &str, claims: &serde_json::Value) -> serde_json::Value {
    json!({"type": "item", "id": id, "labels": {"en": {"language": "en", "value": label}}, "claims": claims})
}

fn account(key: &str, name: &str) -> Draft {
    Draft {
        appended_at: NOW,
        payload_type: "scatter:v0/actor".into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::from_json(&json!({"kind": "registered", "name": name, "status": "active"})),
            &Value::Null,
            &Value::map(vec![(Value::text("actor"), Value::text("librarybase:2"))]),
        )
        .unwrap(),
    }
}

async fn rows(client: &Client, sql: &str) -> Vec<String> {
    let mut out: Vec<String> = client
        .query(sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect();
    out.sort();
    out
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn adoption_batch_sync_and_edit_end_to_end() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let store = PgIngest::new(backend);
    let p = pipeline();
    let registry = Registry::default_registry();
    // The config partition's projections (the tenant entry) first.
    let log_store = PgLog::new(client);
    p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let client = log_store.into_inner();

    // 1. Adoption: two items and a property, with an account and the floors.
    let adoption = Adoption {
        issuer: TENANT.into(),
        source: "https://librarybase.org/".into(),
        version: "librarybase-20260928.json.gz".into(),
        adapter_version: Some("scatter-adapter-wikibase 0.0.1".into()),
        frozen: true,
        history: false,
        current_state_dump: false,
        floors: Floors {
            entity: BTreeMap::from([("item".to_string(), 1000), ("property".to_string(), 50)]),
            page: 2000,
            revision: 50_000,
            log: 700,
            user: 40,
        },
        accounts: vec![account("librarybase:7", "Editor")],
        property_types: BTreeMap::new(),
    };
    let items = vec![
        Adopted::current(
            entity(item(
                "Q6",
                "Six",
                &json!({"P356": [string_statement("P356", "10.1/known", "external-id", "Q6$00000000-0000-0000-0000-000000000001")]}),
            )),
            41_877,
            "2026-09-20T14:02:11Z",
            12,
        ),
        Adopted::current(
            entity(item("Q8", "Eight", &json!({}))),
            42_000,
            "2026-09-21T00:00:00Z",
            13,
        ),
        Adopted::current(
            entity(
                json!({"type": "property", "id": "P356", "datatype": "external-id",
                "labels": {"en": {"language": "en", "value": "DOI"}}, "claims": {}}),
            ),
            903,
            "2024-01-03T09:15:40Z",
            40,
        ),
    ];
    let out = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        adoption,
        items,
        registry,
        NOW,
        2,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.adopted, 3);
    assert_eq!(out.accounts, 1);
    assert_eq!(
        rows(&client, "SELECT id || '|' || type || '|' || page_id::text || '|' || resolved_kind || '|' || local_revid::text FROM view.entity WHERE tenant = 'librarybase'").await,
        vec!["P356|property|40|source|50003", "Q6|item|12|source|50001", "Q8|item|13|source|50002"]
    );
    assert_eq!(
        rows(&client, "SELECT status || '|' || mode || '|' || source || '|' || (params->'floors'->>'page') || '|' || (counts->>'adopted') FROM view.job WHERE tenant = 'librarybase'").await,
        vec!["finished|adopt|https://librarybase.org/|2000|3"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.actor WHERE tenant = 'librarybase'"
        )
        .await,
        1
    );
    assert_eq!(count(&client, "SELECT count(*) FROM view.activity WHERE tenant = 'librarybase' AND kind = 'edit' AND new").await, 3);
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.identifier WHERE entity_id = 'Q6'"
        )
        .await,
        1
    );
    // Idempotent: the same dump again changes nothing.
    let again = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        Adoption {
            issuer: TENANT.into(),
            source: "https://librarybase.org/".into(),
            version: "librarybase-20260928.json.gz".into(),
            adapter_version: None,
            frozen: true,
            history: false,
            current_state_dump: false,
            floors: Floors::default(),
            accounts: vec![],
            property_types: BTreeMap::new(),
        },
        vec![Adopted::current(
            entity(item("Q8", "Eight", &json!({}))),
            42_000,
            "2026-09-21T00:00:00Z",
            13,
        )],
        registry,
        NOW + 1,
        2,
    )
    .await
    .unwrap();
    assert_eq!((again.counts.adopted, again.counts.unchanged), (0, 1));
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM log.record WHERE partition = 12289"
        )
        .await,
        3
    );

    // 2. A streamed batch: a new item with a DOI, a create-or-add that matches Q6's DOI,
    //    an add that points at the new item, and one bad line.
    let batch = r#"{"job":{"source":"citation-batch","graph":"local"}}
{"op":"create","ref":"$w1","entity":{"type":"item","id":"$w1","labels":{"en":{"language":"en","value":"A paper"}},"claims":{"P356":[{"mainsnak":{"snaktype":"value","property":"P356","datavalue":{"value":"10.1/new","type":"string"},"datatype":"external-id"},"type":"statement","rank":"normal"}]}}}
{"op":"create-or-add","ref":"$w2","match":{"P356":"10.1/known"},"entity":{"type":"item","id":"$w2","labels":{"fr":{"language":"fr","value":"Six"}},"claims":{}}}
{"op":"add","id":"$w2","claims":{"P31":[{"mainsnak":{"snaktype":"value","property":"P31","datavalue":{"value":{"entity-type":"item","id":"$w1"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","rank":"normal"}]}}
{"op":"add","id":"Q8","page":3,"labels":{"en":"both"}}
"#;
    let out = run_batch(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:7"),
        batch,
        &ProviderOrder::default(),
        registry,
        NOW + 2,
    )
    .await
    .unwrap();
    assert_eq!(
        out.ids.get("$w1").map(EntityId::as_str),
        Some("Q1001"),
        "above the item floor"
    );
    assert_eq!(
        out.ids.get("$w2").map(EntityId::as_str),
        Some("Q6"),
        "matched through view.identifier"
    );
    assert_eq!(
        (out.counts.created, out.counts.merged, out.counts.rejected),
        (1, 2, 1)
    );
    assert_eq!(
        rows(&client, "SELECT entity_id || '|' || kind::text || '|' || lang || '|' || text FROM view.term WHERE tenant = 'librarybase' AND entity_id IN ('Q6', 'Q1001')").await,
        vec!["Q1001|1|en|A paper", "Q6|1|en|Six", "Q6|1|fr|Six"]
    );
    assert_eq!(
        rows(&client, "SELECT resolved_kind || '|' || resolved_version::text FROM view.entity WHERE tenant = 'librarybase' AND id = 'Q6'").await,
        vec!["materialized|3"],
        "two adds re-resolved the adopted entity"
    );
    assert_eq!(
        rows(&client, "SELECT status || '|' || (counts->>'rejected') || '|' || (counts->>'created') FROM view.job WHERE tenant = 'librarybase' AND source = 'citation-batch'").await,
        vec!["finished|1|1"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.job_reject WHERE tenant = 'librarybase'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.identifier WHERE entity_id = 'Q1001' AND graph = 'local'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &client,
            &format!(
                "SELECT count(*) FROM view.activity WHERE tenant = 'librarybase' AND job_id = {}",
                out.job_id
            )
        )
        .await,
        3,
        "the create and two adds name the job"
    );

    // 3. A mirror sync of a key-mapped Domain and an ordinary item: an instance job. An
    // earlier load of the same mirror died after its start record; this one marks it
    // interrupted (0011 A36).
    let dead = {
        let mut cx = store.begin().await.unwrap();
        let job = scatter_ingest::Job::start(
            &store,
            &p,
            &mut cx,
            "",
            JobHeader::mirror("internetdomains", "2026-10-01", Mode::Upsert),
            Attestation::by("instance:scatter"),
            NOW + 2,
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
        job.id
    };
    // A follower's job for the day, open between its batches, is left alone (0092 A6).
    let following = {
        let mut header = JobHeader::mirror("internetdomains", "follow-2026-10-01", Mode::Upsert);
        header
            .params
            .insert("follow".into(), serde_json::Value::Bool(true));
        let mut cx = store.begin().await.unwrap();
        let job = scatter_ingest::Job::start(
            &store,
            &p,
            &mut cx,
            "",
            header,
            Attestation::by("instance:scatter"),
            NOW + 2,
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
        job.id
    };
    let domain = entity(json!({"type": "domain", "id": "domain:example.org",
        "labels": {"en": {"language": "en", "value": "example.org"}},
        "claims": {"P1": [string_statement("P1", "a", "string", "domain:example.org$00000000-0000-0000-0000-0000000000A1")]}}));
    let out = run_sync(
        &store,
        &p,
        JobHeader::mirror("internetdomains", "2026-10-01", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            SyncItem::State {
                entity: Box::new(domain),
                upstream: Upstream::revision(55, None),
                upstream_id: Some("Q9".into()),
                upstream_aliases: Vec::new(),
            },
            SyncItem::State {
                entity: Box::new(entity(item("XDQ10", "Registrar", &json!({})))),
                upstream: Upstream::revision(56, None),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
        ],
        100,
        registry,
        NOW + 3,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.created, 2);
    assert_eq!(
        rows(
            &client,
            "SELECT keyed_type || '|' || surrogate::text || '|' || key FROM view.keyed_surrogate"
        )
        .await,
        vec!["domain|1|example.org"]
    );
    assert_eq!(
        rows(
            &client,
            "SELECT provider || '|' || upstream_id || '|' || key FROM view.keyed_map"
        )
        .await,
        vec!["internetdomains|Q9|example.org"]
    );
    assert_eq!(
        rows(&client, "SELECT id || '|' || resolved_kind || '|' || coalesce(surrogate::text, '-') || '|' || page_id::text FROM view.entity WHERE tenant = ''").await,
        vec!["XDQ10|source|-|2", "domain:example.org|source|1|1"],
        "mirror page IDs come from the instance sequence (0013 A29)"
    );
    assert_eq!(
        rows(&client, "SELECT key FROM log.record WHERE partition = 12306 AND payload_type = 'scatter:v0/changeset'").await,
        vec!["XDQ10", "domain#1"]
    );
    assert_eq!(count(&client, "SELECT count(*) FROM view.job WHERE tenant = '' AND status = 'finished' AND graph = 'mirror/internetdomains'").await, 1);
    assert_eq!(
        rows(&client, &format!("SELECT status || '|' || (counts->>'by') || '|' || (finish_offset IS NOT NULL)::text FROM view.job WHERE tenant = '' AND job_id = {dead}")).await,
        vec![format!("interrupted|{}|true", out.job_id)],
        "the dead load is closed by the next load of the same mirror"
    );
    assert_eq!(
        rows(
            &client,
            &format!("SELECT status FROM view.job WHERE tenant = '' AND job_id = {following}")
        )
        .await,
        vec!["running"],
        "a follower's open job is not a dead load"
    );
    {
        use scatter_ingest::IngestStore;
        let mut cx = store.begin().await.unwrap();
        for (followers, want) in [(false, vec![]), (true, vec![following])] {
            assert_eq!(
                store
                    .running_jobs(
                        &mut cx,
                        "",
                        "internetdomains",
                        "mirror/internetdomains",
                        followers
                    )
                    .await
                    .unwrap(),
                want
            );
        }
        store.rollback(cx).await.unwrap();
    }
    // One load of a source and graph at a time (0011 A37): a second is refused while the
    // first holds the lock, a load of another graph is not, and dropping the lock frees it.
    {
        use scatter_ingest::IngestStore;
        let wait = Some(std::time::Duration::from_millis(200));
        let held = store
            .lock_load("", "internetdomains", "mirror/internetdomains", wait)
            .await
            .unwrap();
        let refused = store
            .lock_load("", "internetdomains", "mirror/internetdomains", wait)
            .await;
        assert!(
            matches!(refused, Err(IngestError::LoadRunning { .. })),
            "{:?}",
            refused.err()
        );
        let other = store
            .lock_load("", "internetdomains", "mirror/elsewhere", wait)
            .await
            .unwrap();
        drop((held, other));
        let again = store
            .lock_load("", "internetdomains", "mirror/internetdomains", wait)
            .await;
        assert!(again.is_ok(), "{:?}", again.err());
    }
    let xd = u64::from(registry.by_slug("internetdomains").unwrap().number);
    assert_eq!(
        count(&client, &format!("SELECT count(*) FROM log.record WHERE payload_type = 'scatter:v0/changeset' AND revid = {}", (xd << 40) | 0x37)).await,
        1,
        "the mirror record carries the upstream revision ID"
    );
    // The sync is a load: its records are no activity rows (03 §4.6; 0013 §5.5).
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.activity WHERE source = 'mirror/internetdomains' AND kind = 'sync'").await,
        0
    );

    // 4. An interactive edit asserts about the Domain in the tenant's local graph: a
    //    tenant overlay row appears, materialized, with the local label winning.
    let request = Request::new(TENANT, Attestation::by("librarybase:7"), NOW + 4);
    let mut cx = store.begin().await.unwrap();
    let add: Operation = serde_json::from_value(
        json!({"op": "add", "id": "domain:example.org", "labels": {"en": "Example"}}),
    )
    .unwrap();
    let w = write_operation(&store, &p, &mut cx, &request, &add, registry)
        .await
        .unwrap();
    store.commit(cx).await.unwrap();
    assert_eq!(
        w.record.header().key.as_deref(),
        Some("domain#1"),
        "the surrogate the sync minted"
    );
    assert_eq!(
        w.record.header().page_id,
        Some(1),
        "the key's page ID, across partitions"
    );
    assert!(w.inline.rows > 0);
    assert_eq!(
        rows(
            &client,
            "SELECT tenant || '|' || resolved_kind FROM view.entity WHERE id = 'domain:example.org'"
        )
        .await,
        vec!["librarybase|materialized", "|source"]
    );
    assert_eq!(
        rows(&client, "SELECT tenant || '|' || text FROM view.term WHERE entity_id = 'domain:example.org' AND lang = 'en'").await,
        vec!["librarybase|Example", "|example.org"]
    );
    // A stale base offset is refused.
    let mut stale = request.clone();
    stale.base_offset = Some(0);
    let mut cx = store.begin().await.unwrap();
    let err = write_operation(&store, &p, &mut cx, &stale, &add, registry)
        .await
        .unwrap_err();
    store.rollback(cx).await.unwrap();
    assert!(matches!(err, IngestError::Conflict { .. }), "{err}");

    // 5. Keys move (0009 §9, A13): Q20 owns b.example with Q21 merged into it, beside a
    //    foreign XDQ22; then Q20's value moves to c.example and Q22 becomes d.example.
    //    b.example is cleared, XDQ22 retired, and keyed_map follows the newest puts.
    let domain_of = |key: &str, owner: &str, aliases: &[&str], revid: u64| {
        let mut e = entity(json!({"type": "domain", "id": format!("domain:{key}"),
            "labels": {"en": {"language": "en", "value": key}}}));
        e.id = scatter_wikibase_model::id::EntityId::parse(&format!("domain:{key}")).unwrap();
        SyncItem::State {
            entity: Box::new(e),
            upstream: Upstream::revision(revid, None),
            upstream_id: Some(owner.into()),
            upstream_aliases: aliases.iter().map(|a| (*a).to_string()).collect(),
        }
    };
    let keyed_map = "SELECT upstream_id || '|' || key || '|' || alias::text FROM view.keyed_map WHERE provider = 'internetdomains' AND upstream_id <> 'Q9' ORDER BY upstream_id";
    let out = run_sync(
        &store,
        &p,
        JobHeader::mirror("internetdomains", "2026-10-02", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            domain_of("b.example", "Q20", &["Q21"], 60),
            SyncItem::State {
                entity: Box::new(entity(item("XDQ22", "Not yet a domain", &json!({})))),
                upstream: Upstream::revision(61, None),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
        ],
        100,
        registry,
        NOW + 6,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.created, 2);
    assert_eq!(
        rows(&client, keyed_map).await,
        vec!["Q20|b.example|false", "Q21|b.example|true"]
    );
    let out = run_sync(
        &store,
        &p,
        JobHeader::mirror("internetdomains", "2026-10-03", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            domain_of("c.example", "Q20", &["Q21"], 62),
            domain_of("d.example", "Q22", &[], 63),
        ],
        100,
        registry,
        NOW + 7,
        10,
    )
    .await
    .unwrap();
    assert_eq!(
        (
            out.counts.created,
            out.counts.cleared,
            out.counts.tombstoned
        ),
        (2, 1, 1)
    );
    assert_eq!(
        rows(&client, keyed_map).await,
        vec![
            "Q20|c.example|false",
            "Q21|c.example|true",
            "Q22|d.example|false"
        ]
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.entity_source WHERE graph = 'mirror/internetdomains' AND entity_id = 'XDQ22'").await,
        0,
        "the foreign form of a newly mapped item is retired"
    );

    // Everything is consistent with a rebuild from the log.
    let keyed_before = rows(&client, keyed_map).await;
    let before = rows(&client, "SELECT (tenant, id, type, page_id, surrogate, resolved_kind, convert_from(resolved, 'UTF8'))::text FROM view.entity").await;
    let log_store = PgLog::new(client);
    p.rebuild(
        &store,
        &log_store,
        &[
            CONFIG_PARTITION,
            INSTANCE_LOG,
            MIRROR,
            ACTORS,
            LOCAL,
            PAGES,
            TENANT_LOG,
        ],
        5,
    )
    .await
    .unwrap();
    let client = log_store.into_inner();
    let after = rows(&client, "SELECT (tenant, id, type, page_id, surrogate, resolved_kind, convert_from(resolved, 'UTF8'))::text FROM view.entity").await;
    assert_eq!(before, after);
    assert_eq!(keyed_before, rows(&client, keyed_map).await);
}

/// One revision of a history adoption (0035 A12).
/// A snapshot sweep writes its operations in blocks (0013 A86), and over Postgres, where
/// a block is a binary `COPY` and its projections run over the block, it writes what one
/// operation at a time writes (`batch` 1): the same keys at the same offsets with the same
/// revision IDs, page IDs and body sizes (a body's hashes are salted, so its leaf differs
/// from one database to the next), and the same entity rows. The keyed Domain's clearing `put`, which no
/// block takes, is written by itself between the tombstones' blocks.
#[tokio::test]
async fn a_sweep_in_blocks_writes_what_one_operation_at_a_time_writes() {
    if skipped() {
        return;
    }
    let mut runs = Vec::new();
    for batch in [1usize, 3, 100] {
        let (client, backend) = fresh().await;
        let client = seed(client).await;
        let store = PgIngest::new(backend);
        let p = pipeline();
        let registry = Registry::default_registry();
        let log_store = PgLog::new(client);
        p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
            .await
            .unwrap();
        let client = log_store.into_inner();
        let state = |n: u64| SyncItem::State {
            entity: Box::new(entity(item(&format!("XDQ{n}"), "An item", &json!({})))),
            upstream: Upstream::revision(10 + n, None),
            upstream_id: None,
            upstream_aliases: Vec::new(),
        };
        let domain = entity(json!({"type": "domain", "id": "domain:example.org",
            "labels": {"en": {"language": "en", "value": "example.org"}}}));
        let mut items: Vec<SyncItem> = (1..=6).map(state).collect();
        items.push(SyncItem::State {
            entity: Box::new(domain),
            upstream: Upstream::revision(30, None),
            upstream_id: Some("Q9".into()),
            upstream_aliases: Vec::new(),
        });
        run_sync(
            &store,
            &p,
            JobHeader::mirror("internetdomains", "2026-10-01", Mode::Upsert),
            Attestation::by("instance:scatter"),
            items,
            100,
            registry,
            NOW,
            100,
        )
        .await
        .unwrap();
        // The snapshot sees XDQ1 alone: five tombstones and the Domain's clearing.
        let out = run_sync(
            &store,
            &p,
            JobHeader::mirror("internetdomains", "2026-10-02", Mode::Snapshot),
            Attestation::by("instance:scatter"),
            vec![state(1)],
            100,
            registry,
            NOW + 10,
            batch,
        )
        .await
        .unwrap();
        assert_eq!(out.swept, 6, "batch {batch}");
        assert_eq!(
            (out.counts.tombstoned, out.counts.cleared),
            (5, 1),
            "batch {batch}"
        );
        let records = rows(
            &client,
            "SELECT lpad(\"offset\"::text, 6, '0') || '|' || key || '|' || coalesce(revid::text, '-')
                    || '|' || coalesce(page_id::text, '-') || '|' || octet_length(body)::text
             FROM log.record WHERE partition = 12306",
        )
        .await;
        let entities = rows(
            &client,
            "SELECT id || '|' || resolved_kind || '|' || page_id::text FROM view.entity WHERE tenant = ''",
        )
        .await;
        runs.push((batch, records, entities));
    }
    let (_, records_one, entities_one) = &runs[0];
    assert_eq!(
        records_one.len(),
        13,
        "seven puts, five tombstones, one clearing"
    );
    for (batch, records, entities) in &runs[1..] {
        assert_eq!(records, records_one, "batch {batch}: the same records");
        assert_eq!(
            entities, entities_one,
            "batch {batch}: the same entity rows"
        );
    }
}

/// Every row of every `view` table, as text, by table: what the projections left.
async fn views(client: &Client) -> BTreeMap<String, Vec<String>> {
    let tables = rows(
        client,
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'view' AND c.relkind = 'r'",
    )
    .await;
    let mut out = BTreeMap::new();
    for table in tables {
        let found = rows(
            client,
            &format!("SELECT row_to_json(t)::text FROM view.\"{table}\" t"),
        )
        .await;
        out.insert(table, found);
    }
    out
}

/// A block (`write_many`; 0013 A87) over Postgres, where it is a binary `COPY` and the
/// real projections run over its stretches, leaves what one record at a time leaves: the
/// same keys at the same offsets with the same revision IDs, page IDs and body sizes,
/// and the same rows in every `view` table. The block is a tenant's `local` graph by
/// several authors: a property and items that use it, an item naming one created later
/// in the block, a key written three times, and an adopted entity's history under its
/// own page and revision IDs.
///
/// The projections say where a key may repeat in a stretch (0013 A89): that block is one
/// stretch, and two more blocks are the cases the entity projection keeps apart, an
/// entity that goes and comes back and records appended at two times, which leave the
/// same rows because their stretches end there.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_block_of_local_records_leaves_what_one_record_at_a_time_leaves() {
    if skipped() {
        return;
    }
    let by = |actor: &str, comment: Option<&str>, revid: Option<u64>| {
        let mut r = Request::new(TENANT, Attestation::by(actor), NOW);
        r.comment = comment.map(str::to_string);
        r.revid = revid;
        r.budget = scatter_projection::Budget::NONE;
        r
    };
    let op = |v: serde_json::Value| -> Operation { serde_json::from_value(v).unwrap() };
    let adopt = |id: &str, label: &str, revid: u64, pageid: u64| Operation::Adopt {
        id: EntityId::parse(id).unwrap(),
        entity: Some(entity(item(id, label, &json!({})))),
        redirect: None,
        source_revid: revid,
        source_parentid: None,
        source_time: "2026-09-20T14:02:11Z".into(),
        source_pageid: pageid,
        minor: false,
        withheld: Vec::new(),
    };
    let names_q3 = json!({"P1": [{"mainsnak": {"snaktype": "value", "property": "P1",
        "datavalue": {"value": {"entity-type": "item", "numeric-id": 3, "id": "Q3"}, "type": "wikibase-entityid"},
        "datatype": "wikibase-item"}, "type": "statement", "rank": "normal",
        "id": "Q2$00000000-0000-0000-0000-0000000000B1"}]});
    let block: Vec<(Request, Operation)> = vec![
        (
            by("librarybase:1", Some("a property"), None),
            op(
                json!({"op": "create", "id": "P1", "entity": {"type": "property", "id": "P1",
                "datatype": "wikibase-item", "labels": {"en": {"language": "en", "value": "related to"}}}}),
            ),
        ),
        (
            by("librarybase:1", Some("first"), None),
            op(json!({"op": "create", "id": "Q1", "entity": item("Q1", "One", &json!({}))})),
        ),
        // Names Q3, which the block creates two records later.
        (
            by("librarybase:2", None, None),
            op(json!({"op": "create", "id": "Q2", "entity": item("Q2", "Two", &names_q3)})),
        ),
        (
            by("librarybase:2", None, None),
            op(json!({"op": "add", "id": "Q1", "labels": {"fr": "Un"}})),
        ),
        (
            by("librarybase:1", None, None),
            op(json!({"op": "create", "id": "Q3", "entity": item("Q3", "Three", &json!({}))})),
        ),
        (
            by("librarybase:3", Some("again"), None),
            op(
                json!({"op": "add", "id": "Q1", "descriptions": {"en": "the first"}, "aliases": {"en": ["Uno"]}}),
            ),
        ),
        (
            by("librarybase:7", Some("r40"), Some(40)),
            adopt("Q7", "Seven", 40, 700),
        ),
        (
            by("librarybase:8", None, Some(41)),
            adopt("Q7", "Seven, again", 41, 700),
        ),
        (
            by("librarybase:7", None, Some(42)),
            adopt("Q7", "Seven, thrice", 42, 700),
        ),
    ];
    // A key repeats in a stretch only where the entity projection has shown it leaves the
    // same rows (0013 A89): not past a revision that takes the entity away, and not when
    // its records were appended at different times.
    let redirect = Operation::Adopt {
        id: EntityId::parse("Q7").unwrap(),
        entity: None,
        redirect: Some(EntityId::parse("Q1").unwrap()),
        source_revid: 41,
        source_parentid: None,
        source_time: "2026-09-20T14:02:11Z".into(),
        source_pageid: 700,
        minor: false,
        withheld: Vec::new(),
    };
    let gone_and_back: Vec<(Request, Operation)> = vec![
        (
            by("librarybase:1", None, None),
            op(json!({"op": "create", "id": "Q1", "entity": item("Q1", "One", &json!({}))})),
        ),
        (
            by("librarybase:7", None, Some(40)),
            adopt("Q7", "Seven", 40, 700),
        ),
        (by("librarybase:8", None, Some(41)), redirect),
        (
            by("librarybase:7", None, Some(42)),
            adopt("Q7", "Seven, back", 42, 700),
        ),
        (
            by("librarybase:1", None, None),
            op(json!({"op": "create", "id": "Q2", "entity": item("Q2", "Two", &json!({}))})),
        ),
        (
            by("librarybase:1", None, None),
            op(json!({"op": "create", "id": "Q3", "entity": item("Q3", "Three", &json!({}))})),
        ),
    ];
    let mut later = by("librarybase:2", None, None);
    later.now = NOW + 5_000_000;
    let mut two_times: Vec<(Request, Operation)> = (1..=5)
        .map(|n| {
            let id = format!("Q{n}");
            (
                by("librarybase:1", None, None),
                op(json!({"op": "create", "id": id, "entity": item(&id, "An item", &json!({}))})),
            )
        })
        .collect();
    two_times.push((
        later,
        op(json!({"op": "add", "id": "Q1", "labels": {"fr": "Un"}})),
    ));
    // More keys than the projections send at once (0013 A93), some of them again: seventy
    // items created, the first ten then edited, in one stretch.
    let mut wide: Vec<(Request, Operation)> = (101..=170)
        .map(|n| {
            let id = format!("Q{n}");
            (
                by("librarybase:1", None, None),
                op(json!({"op": "create", "id": id, "entity": item(&id, "An item", &json!({}))})),
            )
        })
        .collect();
    wide.extend((101..=110).map(|n| {
        (
            by("librarybase:2", None, None),
            op(json!({"op": "add", "id": format!("Q{n}"), "labels": {"fr": "Un article"}})),
        )
    }));
    // Each case has more records than `SMALL_BLOCK`, so that it is written as a block.
    let cases = [
        ("every key repeats in one stretch", &block, vec![9]),
        ("more keys than are sent at once", &wide, vec![80]),
        (
            "an entity that goes and comes back",
            &gone_and_back,
            vec![2, 3, 6],
        ),
        ("records appended at two times", &two_times, vec![5, 6]),
    ];
    for (case, block, stretches) in cases {
        let mut runs = Vec::new();
        for in_block in [false, true] {
            let (client, backend) = fresh().await;
            let client = seed(client).await;
            let store = PgIngest::new(backend);
            let p = pipeline();
            let registry = Registry::default_registry();
            let log_store = PgLog::new(client);
            p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
                .await
                .unwrap();
            let client = log_store.into_inner();
            let mut cx = store.begin().await.unwrap();
            if in_block {
                let entries: Vec<_> = block
                    .iter()
                    .map(|(request, op)| BlockEntry {
                        request,
                        payload: BlockPayload::Operation(op),
                    })
                    .collect();
                let written = write_many(
                    &store,
                    &p,
                    &mut cx,
                    LOCAL,
                    &Graph::Local,
                    &entries,
                    scatter_projection::Budget::NONE,
                    registry,
                )
                .await
                .unwrap();
                assert_eq!(p.stretches(&written), stretches, "{case}: its stretches");
            } else {
                for (request, op) in block {
                    write_operation(&store, &p, &mut cx, request, op, registry)
                        .await
                        .unwrap();
                }
            }
            store.commit(cx).await.unwrap();
            let records = rows(
                &client,
                "SELECT lpad(\"offset\"::text, 6, '0') || '|' || key || '|' || coalesce(revid::text, '-')
                        || '|' || coalesce(page_id::text, '-') || '|' || octet_length(body)::text
                 FROM log.record WHERE partition = 12289",
            )
            .await;
            runs.push((records, views(&client).await));
        }
        let (records_one, views_one) = &runs[0];
        let (records_block, views_block) = &runs[1];
        assert_eq!(records_one.len(), block.len(), "{case}");
        assert_eq!(records_block, records_one, "{case}: the same records");
        assert!(
            views_one.values().map(Vec::len).sum::<usize>() > 5,
            "{case}: the projections wrote their rows"
        );
        for (table, rows_one) in views_one {
            assert_eq!(
                &views_block[table], rows_one,
                "{case}: the same rows in view.{table}"
            );
        }
    }
}

fn revision(id: &str, revid: u64, parent: Option<u64>, time: &str, author: Author) -> Adopted {
    Adopted {
        id: EntityId::parse(id).unwrap(),
        entity: Some(entity(item(id, &format!("{id} at {revid}"), &json!({})))),
        redirect: None,
        source_revid: revid,
        source_parentid: parent,
        source_time: time.into(),
        source_pageid: if id == "Q6" { 12 } else { 13 },
        minor: false,
        withheld: Vec::new(),
        comment: Some(format!("edit {revid}")),
        author,
    }
}

/// The batch of the block test: two created items, one of them edited twice more, a
/// `create-or-add` that finds nothing, a statement naming an item of the batch, a bulk
/// `retain`, and (streamed only) a line with problems between the operations.
fn block_batch(atomic: bool) -> String {
    let job = if atomic {
        r#"{"job":{"source":"block-batch","graph":"local","atomic":true}}"#
    } else {
        r#"{"job":{"source":"block-batch","graph":"local"}}"#
    };
    let mut lines = vec![
        job,
        r#"{"op":"create","ref":"$a","entity":{"type":"item","id":"$a","labels":{"en":{"language":"en","value":"A paper"}},"claims":{"P356":[{"mainsnak":{"snaktype":"value","property":"P356","datavalue":{"value":"10.1/a","type":"string"},"datatype":"external-id"},"type":"statement","rank":"normal"}]}}}"#,
        r#"{"op":"create","ref":"$b","entity":{"type":"item","id":"$b","labels":{"en":{"language":"en","value":"Another"}},"claims":{}}}"#,
        r#"{"op":"add","id":"$a","labels":{"fr":"Un article"}}"#,
        r#"{"op":"create-or-add","match":{"P356":"10.1/unknown"},"entity":{"type":"item","id":"Q0","labels":{"en":{"language":"en","value":"Unknown"}},"claims":{}}}"#,
        r#"{"op":"add","id":"$b","claims":{"P31":[{"mainsnak":{"snaktype":"value","property":"P31","datavalue":{"value":{"entity-type":"item","id":"$a"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","rank":"normal"}]}}"#,
        r#"{"op":"retain","ids":["XDQ1","XDQ2"],"policy":"retain"}"#,
    ];
    if !atomic {
        lines.push(r#"{"op":"add","id":"Q8","page":3,"labels":{"en":"both"}}"#);
    }
    lines.push(r#"{"op":"add","id":"$a","descriptions":{"en":"the first"}}"#);
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// A batch writes its operations in blocks (0013 A90), and over Postgres, with the real
/// projections, a streamed and an atomic batch leave what they leave an operation at a
/// time (a block of one): the same records, counts, IDs and rejects, and the same rows
/// in every `view` table.
#[tokio::test]
async fn a_batch_in_blocks_leaves_what_one_operation_at_a_time_leaves() {
    if skipped() {
        return;
    }
    for atomic in [false, true] {
        let text = block_batch(atomic);
        let mut runs = Vec::new();
        for block in [1usize, 3, 200] {
            let (client, backend) = fresh().await;
            let client = seed(client).await;
            let store = PgIngest::new(backend);
            let p = pipeline();
            let registry = Registry::default_registry();
            let log_store = PgLog::new(client);
            p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
                .await
                .unwrap();
            let client = log_store.into_inner();
            let out = run_batch_in(
                &store,
                &p,
                TENANT,
                Attestation::by("librarybase:7"),
                &text,
                &ProviderOrder::default(),
                registry,
                NOW,
                block,
            )
            .await
            .unwrap();
            let records = rows(
                &client,
                "SELECT lpad(\"offset\"::text, 6, '0') || '|' || key || '|' || coalesce(revid::text, '-')
                        || '|' || coalesce(page_id::text, '-') || '|' || octet_length(body)::text
                 FROM log.record WHERE partition = 12289",
            )
            .await;
            runs.push((
                block,
                (out.counts, out.ids, out.rejects),
                records,
                views(&client).await,
            ));
        }
        let (_, outcome_one, records_one, views_one) = &runs[0];
        assert_eq!(
            (records_one.len(), outcome_one.0.rejected),
            (8, u64::from(!atomic)),
            "atomic {atomic}: eight records, and the line with problems rejected when streamed"
        );
        for (block, outcome, records, views) in &runs[1..] {
            assert_eq!(outcome, outcome_one, "atomic {atomic}, blocks of {block}");
            assert_eq!(records, records_one, "atomic {atomic}, blocks of {block}");
            for (table, rows_one) in views_one {
                assert_eq!(
                    &views[table], rows_one,
                    "atomic {atomic}, blocks of {block}: the same rows in view.{table}"
                );
            }
        }
    }
}

/// One revision of a page in an export, on day `day` of January 2026.
fn import_revision(
    id: u64,
    day: u64,
    text: Option<&str>,
    author: Author,
) -> scatter_ingest::import::ImportRevision {
    scatter_ingest::import::ImportRevision {
        source_revid: id,
        source_parentid: (id > 1).then(|| id - 1),
        source_time: format!("2026-01-{day:02}T00:00:00Z"),
        time: 1_767_225_600_000_000 + (day - 1) * 86_400_000_000,
        model: "wikitext".into(),
        text: text.map(str::to_string),
        sha1: text
            .map(scatter_mediawiki_xml::sha1_base36)
            .unwrap_or_default(),
        minor: id.is_multiple_of(2),
        comment: Some(format!("edit {id}")),
        withheld: if text.is_none() {
            vec!["text".into()]
        } else {
            Vec::new()
        },
        author,
    }
}

/// An import writes each page's revisions as one block (0013 A91), over Postgres with the
/// real projections: a page of six revisions (more than `SMALL_BLOCK`, so one block) by a registered author, an imported one
/// and a hidden one, one with its text withheld, lands under one minted page ID with the
/// tenant's revision IDs in timestamp order; a second run writes nothing twice; and
/// older revisions imported onto the title go behind the page's own newest, which is
/// restored on top.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn an_import_writes_each_page_as_a_block() {
    use scatter_ingest::import::{Import, ImportPage, run_import};
    use triplespace_api_ingest::import::PgImportHost;
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let store = PgIngest::new(backend);
    let p = pipeline();
    let log_store = PgLog::new(client);
    p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let client = log_store.into_inner();
    let imported = |raw: &str| Author::Surrogate {
        kind: "imported".into(),
        raw: raw.into(),
    };
    let guide = ImportPage {
        ns: 4,
        title: "Guide".into(),
        revisions: vec![
            // Out of order in the file: the job sorts by timestamp.
            import_revision(3, 5, Some("Third text."), Author::Hidden),
            import_revision(
                1,
                3,
                Some("First text."),
                Author::Actor("librarybase:7".into()),
            ),
            import_revision(2, 4, None, imported("Old Editor")),
            import_revision(4, 6, Some("Fourth text."), imported("Old Editor")),
            import_revision(5, 7, Some("Fifth text."), Author::Hidden),
            import_revision(
                6,
                8,
                Some("Sixth text."),
                Author::Actor("librarybase:7".into()),
            ),
        ],
    };
    let single = ImportPage {
        ns: 4,
        title: "Single".into(),
        revisions: vec![import_revision(
            9,
            3,
            Some("Only text."),
            imported("Another"),
        )],
    };
    let older = ImportPage {
        ns: 4,
        title: "Guide".into(),
        revisions: vec![
            import_revision(20, 1, Some("An older text."), imported("Another")),
            import_revision(21, 2, Some("A second older text."), imported("Third")),
        ],
    };
    let import = || Import {
        source: "upload".into(),
        version: Some("pages.xml".into()),
        history: true,
        summary: None,
        job_id: None,
    };
    let mut outcomes = Vec::new();
    for pages in [
        vec![guide.clone(), single.clone()],
        vec![guide, single],
        vec![older],
    ] {
        let out = run_import(
            &store,
            &p,
            &PgImportHost,
            TENANT,
            Attestation::by("librarybase:2"),
            import(),
            pages,
            NOW,
        )
        .await
        .unwrap();
        assert!(out.rejects.is_empty(), "{:?}", out.rejects);
        outcomes.push((out.counts, out.pages));
    }
    let c = |i: usize| {
        let c: &scatter_ingest::import::ImportCounts = &outcomes[i].0;
        (c.created, c.merged, c.revisions, c.present, c.restored)
    };
    assert_eq!(c(0), (2, 0, 7, 0, 0), "two pages created, seven revisions");
    assert_eq!(
        c(1),
        (0, 2, 0, 7, 0),
        "the second run finds them all present"
    );
    assert_eq!(
        c(2),
        (0, 1, 2, 0, 1),
        "two older revisions, and the newest restored"
    );
    let records = rows(
        &client,
        "SELECT lpad(\"offset\"::text, 6, '0') || '|' || key || '|' || revid::text || '|' || page_id::text
         FROM log.record WHERE partition = 12290",
    )
    .await;
    let guide_id = outcomes[0].1[0].page_id;
    let single_id = outcomes[0].1[1].page_id;
    assert_ne!(guide_id, single_id);
    let of = |page: u64| -> Vec<u64> {
        records
            .iter()
            .filter_map(|r| {
                let f: Vec<&str> = r.split('|').collect();
                (f[1] == page.to_string() && f[3] == page.to_string())
                    .then(|| f[2].parse().unwrap())
            })
            .collect()
    };
    let guide_revids = of(guide_id);
    assert_eq!(
        guide_revids.len(),
        6 + 2 + 1,
        "six, two older, the restoring edit"
    );
    assert!(
        guide_revids.windows(2).all(|w| w[0] < w[1]),
        "revision IDs in the order written: {guide_revids:?}"
    );
    assert_eq!(of(single_id).len(), 1);
    assert_eq!(records.len(), 10);
    // The page's current text is its own newest revision, restored on top.
    assert_eq!(
        rows(&client, "SELECT title || '|' || latest_revid::text FROM view.page WHERE tenant = 'librarybase' AND ns = 4").await,
        vec![
            format!("Guide|{}", guide_revids.last().unwrap()),
            format!("Single|{}", of(single_id)[0]),
        ]
    );
}

/// The passes of the mixed-unit test over a key-mapped provider: items a block takes
/// among items it does not (states mapped onto keys, a state whose upstream item owned a
/// key, a keyed deletion, a subject named twice), a mapping that moves, and a snapshot.
fn mixed_passes() -> Vec<(Mode, Vec<SyncItem>)> {
    let xd = |id: &str| EntityId::parse(id).unwrap();
    let state = |id: &str, label: &str, revid: u64| SyncItem::State {
        entity: Box::new(entity(item(id, label, &json!({})))),
        upstream: Upstream::revision(revid, Some("2026-10-01T00:00:00Z")),
        upstream_id: None,
        upstream_aliases: Vec::new(),
    };
    let domain = |key: &str, owner: &str, aliases: &[&str], label: &str, revid: u64| {
        let mut d = entity(json!({"type": "domain", "id": format!("domain:{key}"),
            "labels": {"en": {"language": "en", "value": label}}}));
        d.id = EntityId::parse(&format!("domain:{key}")).unwrap();
        SyncItem::State {
            entity: Box::new(d),
            upstream: Upstream::revision(revid, Some("2026-10-01T00:00:00Z")),
            upstream_id: Some(owner.into()),
            upstream_aliases: aliases.iter().map(|a| (*a).to_string()).collect(),
        }
    };
    let deleted = |id: &str, revid: u64| SyncItem::Deleted {
        id: xd(id),
        upstream: Upstream::revision(revid, None),
    };
    let redirect = |from: &str, to: &str, revid: u64| SyncItem::Redirected {
        from: xd(from),
        to: xd(to),
        upstream: Upstream::revision(revid, Some("2026-10-01T00:00:00Z")),
    };
    let rejected = |id: &str| SyncItem::Rejected {
        upstream_id: id.into(),
        reason: "not a valid domain key".into(),
    };
    vec![
        (
            Mode::Upsert,
            vec![
                state("XDQ1", "One", 10),
                state("XDQ2", "Two", 11),
                domain("a.example", "Q9", &[], "A", 12),
                state("XDQ3", "Three", 13),
                state("XDQ4", "Four", 14),
                state("XDQ5", "Five", 15),
                state("XDQ6", "Six", 16),
                domain("d.example", "Q20", &[], "D", 17),
                state("XDQ7", "Seven", 18),
                redirect("XDQ8", "XDQ7", 19),
                deleted("XDQ99", 20),
                rejected("Q30"),
            ],
        ),
        (
            Mode::Upsert,
            vec![
                state("XDQ1", "One, changed", 30),
                state("XDQ2", "Two", 11),
                domain("b.example", "Q9", &[], "B", 31),
                domain("c.example", "Q5", &["Q6"], "C", 32),
                state("XDQ3", "Three, changed", 33),
                deleted("XDQ4", 34),
                state("XDQ20", "Twenty", 35),
                state("XDQ7", "Seven, changed", 36),
                rejected("Q31"),
                state("XDQ7", "Seven, changed again", 37),
                redirect("XDQ8", "XDQ7", 19),
                state("XDQ10", "Ten", 38),
                state("XDQ11", "Eleven", 39),
                deleted("domain:d.example", 40),
                state("XDQ12", "Twelve", 41),
            ],
        ),
        (
            Mode::Snapshot,
            vec![
                state("XDQ1", "One, changed", 30),
                domain("b.example", "Q9", &[], "B", 31),
                state("XDQ10", "Ten, changed", 50),
                state("XDQ12", "Twelve", 41),
            ],
        ),
    ]
}

/// A unit's items that a block takes are written in blocks where they come among items
/// it does not (0013 A92), for a provider that maps keys too, and over Postgres, with the
/// real projections, the sync leaves what it leaves one item to a unit (`batch` 1, every
/// item by itself): the same records, counts and rows in every `view` table.
#[tokio::test]
async fn a_mixed_unit_leaves_what_one_item_at_a_time_leaves() {
    if skipped() {
        return;
    }
    let mut runs = Vec::new();
    for batch in [1usize, 6, 100] {
        let (client, backend) = fresh().await;
        let client = seed(client).await;
        let store = PgIngest::new(backend);
        let p = pipeline();
        let registry = Registry::default_registry();
        let log_store = PgLog::new(client);
        p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
            .await
            .unwrap();
        let client = log_store.into_inner();
        let mut outcomes = Vec::new();
        for (i, (mode, items)) in mixed_passes().into_iter().enumerate() {
            let out = run_sync(
                &store,
                &p,
                JobHeader::mirror("internetdomains", &format!("2026-10-0{}", i + 1), mode),
                Attestation::by("instance:scatter"),
                items,
                100,
                registry,
                NOW + 10 * i as u64,
                batch,
            )
            .await
            .unwrap();
            outcomes.push((out.counts, out.swept));
        }
        let records = rows(
            &client,
            "SELECT partition::text || '|' || lpad(\"offset\"::text, 6, '0') || '|' || coalesce(key, '-')
                    || '|' || coalesce(revid::text, '-') || '|' || coalesce(page_id::text, '-')
                    || '|' || octet_length(body)::text
             FROM log.record WHERE partition IN (12305, 12306)",
        )
        .await;
        runs.push((batch, outcomes, records, views(&client).await));
    }
    let (_, outcomes_one, records_one, views_one) = &runs[0];
    let second = &outcomes_one[1].0;
    assert_eq!(
        (
            second.created,
            second.merged,
            second.cleared,
            second.tombstoned
        ),
        (6, 4, 2, 3),
        "the second pass, as in memory"
    );
    assert!(outcomes_one[2].1 > 0, "the snapshot sweeps");
    for (batch, outcomes, records, views) in &runs[1..] {
        assert_eq!(outcomes, outcomes_one, "batch {batch}: the same counts");
        assert_eq!(records, records_one, "batch {batch}: the same records");
        for (table, rows_one) in views_one {
            assert_eq!(
                &views[table], rows_one,
                "batch {batch}: the same rows in view.{table}"
            );
        }
    }
}

/// One revision of an adopted wikitext page: its `create`, or an `edit` after it.
fn page_revision(
    page_id: u64,
    title: &str,
    revid: u64,
    first: bool,
    author: Author,
) -> AdoptedPage {
    use scatter_pages::{PAYLOAD_PAGE, PageOp, Source};
    let source = Some(Source {
        source_revid: revid,
        source_parentid: None,
        source_time: "2026-09-05T00:00:00Z".into(),
        withheld: Vec::new(),
    });
    let op = if first {
        PageOp::Create {
            ns: 4,
            title: title.into(),
            model: "wikitext".into(),
            minor: false,
            source,
        }
    } else {
        PageOp::Edit {
            model: None,
            minor: false,
            source,
        }
    };
    AdoptedPage {
        page_id,
        payload_type: PAYLOAD_PAGE.into(),
        content: op.to_json(),
        extra: vec![Value::text(&format!("Text of {title} at {revid}."))],
        source_revid: revid,
        comment: Some(format!("page edit {revid}")),
        author,
    }
}

/// An adoption writes each unit's revisions as blocks (0013 A88), and what it leaves is
/// what it leaves writing one revision per unit (`batch` 1, a block of one each): the
/// same records in `local`, `pages` and `actors`, the same counts, and the same rows in
/// every `view` table, over a first run and a re-run that finds everything present. The
/// history has entities of several revisions, authors that need a surrogate, a hidden
/// author, a redirect, an entity the dump comes back to, and pages between the entities.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn an_adoption_in_blocks_leaves_what_one_revision_at_a_time_leaves() {
    if skipped() {
        return;
    }
    let ip = |raw: &str| Author::Surrogate {
        kind: "anonymous".into(),
        raw: raw.into(),
    };
    let editor = || Author::Actor("librarybase:7".into());
    let mut merged = revision(
        "Q7",
        44_000,
        Some(43_000),
        "2026-09-21T00:00:00Z",
        Author::Hidden,
    );
    merged.entity = None;
    merged.redirect = Some(EntityId::parse("Q6").unwrap());
    merged.withheld = vec!["attestation".into()];
    let items: Vec<AdoptedItem> = vec![
        revision("Q6", 41_000, None, "2026-09-01T00:00:00Z", editor()).into(),
        revision(
            "Q6",
            41_500,
            Some(41_000),
            "2026-09-10T00:00:00Z",
            ip("192.0.2.1"),
        )
        .into(),
        revision(
            "Q6",
            41_877,
            Some(41_500),
            "2026-09-20T14:02:11Z",
            ip("192.0.2.1"),
        )
        .into(),
        revision("Q7", 43_000, None, "2026-09-02T00:00:00Z", editor()).into(),
        page_revision(300, "About", 45_000, true, editor()).into(),
        page_revision(300, "About", 45_001, false, ip("192.0.2.9")).into(),
        page_revision(301, "Help", 45_002, true, editor()).into(),
        merged.into(),
        // The dump comes back to Q6, and repeats a revision already adopted.
        revision(
            "Q6",
            41_877,
            Some(41_500),
            "2026-09-20T14:02:11Z",
            ip("192.0.2.1"),
        )
        .into(),
        revision(
            "Q6",
            46_000,
            Some(41_877),
            "2026-09-25T00:00:00Z",
            ip("192.0.2.2"),
        )
        .into(),
        page_revision(301, "Help", 46_001, false, editor()).into(),
    ];
    let adoption = Adoption {
        issuer: TENANT.into(),
        source: "https://librarybase.org/".into(),
        version: "librarybase-20261019-history.xml.gz".into(),
        adapter_version: None,
        frozen: true,
        history: true,
        current_state_dump: false,
        floors: Floors {
            entity: BTreeMap::from([("item".to_string(), 1000)]),
            page: 2000,
            revision: 50_000,
            log: 700,
            user: 40,
        },
        accounts: vec![account("librarybase:7", "Editor")],
        property_types: BTreeMap::new(),
    };
    let mut runs = Vec::new();
    for batch in [1usize, 4, 100] {
        let (client, backend) = fresh().await;
        let client = seed(client).await;
        let store = PgIngest::new(backend);
        let p = pipeline();
        let registry = Registry::default_registry();
        let log_store = PgLog::new(client);
        p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
            .await
            .unwrap();
        let client = log_store.into_inner();
        let mut counts = Vec::new();
        for _ in 0..2 {
            let out = run_adoption(
                &store,
                &p,
                TENANT,
                Attestation::by("librarybase:2"),
                adoption.clone(),
                items.clone(),
                registry,
                NOW,
                batch,
            )
            .await
            .unwrap();
            assert!(out.rejects.is_empty(), "batch {batch}: {:?}", out.rejects);
            counts.push(out.counts);
        }
        let records = rows(
            &client,
            "SELECT partition::text || '|' || lpad(\"offset\"::text, 6, '0') || '|' || coalesce(key, '-')
                    || '|' || coalesce(revid::text, '-') || '|' || coalesce(page_id::text, '-')
                    || '|' || octet_length(body)::text
             FROM log.record WHERE partition IN (12289, 12290, 12292)",
        )
        .await;
        runs.push((batch, counts, records, views(&client).await));
    }
    let (_, counts_one, records_one, views_one) = &runs[0];
    assert_eq!(
        (counts_one[0].adopted, counts_one[0].unchanged),
        (10, 1),
        "the first run adopts all but the repeated revision"
    );
    assert_eq!(
        (counts_one[1].adopted, counts_one[1].unchanged),
        (0, 11),
        "the re-run finds everything present"
    );
    for (batch, counts, records, views) in &runs[1..] {
        assert_eq!(counts, counts_one, "batch {batch}: the same counts");
        assert_eq!(records, records_one, "batch {batch}: the same records");
        for (table, rows_one) in views_one {
            assert_eq!(
                &views[table], rows_one,
                "batch {batch}: the same rows in view.{table}"
            );
        }
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_history_adoption_over_postgres() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let store = PgIngest::new(backend);
    let p = pipeline();
    let registry = Registry::default_registry();
    let log_store = PgLog::new(client);
    p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let client = log_store.into_inner();

    let ip = || Author::Surrogate {
        kind: "anonymous".into(),
        raw: "192.0.2.1".into(),
    };
    let mut hidden = revision("Q6", 41_500, Some(41_000), "2026-09-10T00:00:00Z", ip());
    hidden.comment = None;
    hidden.withheld = vec!["comment".into()];
    hidden.minor = true;
    let mut merged = revision(
        "Q7",
        44_000,
        Some(43_000),
        "2026-09-21T00:00:00Z",
        Author::Hidden,
    );
    merged.entity = None;
    merged.redirect = Some(EntityId::parse("Q6").unwrap());
    merged.withheld = vec!["attestation".into()];
    let history = vec![
        revision(
            "Q6",
            41_000,
            None,
            "2026-09-01T00:00:00Z",
            Author::Actor("librarybase:7".into()),
        ),
        hidden,
        revision("Q6", 41_877, Some(41_500), "2026-09-20T14:02:11Z", ip()),
        revision(
            "Q7",
            43_000,
            None,
            "2026-09-02T00:00:00Z",
            Author::Actor("librarybase:7".into()),
        ),
        merged,
    ];
    let adoption = Adoption {
        issuer: TENANT.into(),
        source: "https://librarybase.org/".into(),
        version: "librarybase-20261019-history.xml.gz".into(),
        adapter_version: None,
        frozen: true,
        history: true,
        current_state_dump: false,
        floors: Floors {
            entity: BTreeMap::from([("item".to_string(), 1000)]),
            page: 2000,
            revision: 50_000,
            log: 700,
            user: 40,
        },
        accounts: vec![account("librarybase:7", "Editor")],
        property_types: BTreeMap::new(),
    };
    let out = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        adoption.clone(),
        history.clone(),
        registry,
        NOW,
        2,
    )
    .await
    .unwrap();
    assert_eq!(
        (out.counts.adopted, out.counts.rejected),
        (5, 0),
        "{:?}",
        out.rejects
    );
    // The current state is the source's newest revision, under its own ID; the merged
    // item has no state of its own.
    assert_eq!(
        rows(&client, "SELECT id || '|' || page_id::text || '|' || local_revid::text FROM view.entity WHERE tenant = 'librarybase'").await,
        vec!["Q6|12|41877"]
    );
    // Each revision is an activity row at its source time, under its author.
    assert_eq!(
        rows(&client, "SELECT revid::text || '|' || to_char(time AT TIME ZONE 'UTC', 'YYYY-MM-DD') || '|' || coalesce(actor_key, '-') || '|' || coalesce(comment, '-') || '|' || new::text || '|' || minor::text || '|' || bot::text || '|' || visibility::text FROM view.activity WHERE tenant = 'librarybase' AND kind = 'edit' ORDER BY revid").await,
        vec![
            "41000|2026-09-01|librarybase:7|edit 41000|true|false|true|0",
            "41500|2026-09-10|actor:1|-|false|true|true|10",
            "41877|2026-09-20|actor:1|edit 41877|false|false|true|0",
            "43000|2026-09-02|librarybase:7|edit 43000|true|false|true|0",
            "44000|2026-09-21|-|edit 44000|false|false|true|12",
        ]
    );
    // A `record` ACL's bits join the ones the source withheld, and retiring it leaves
    // the source's (0023 §5; 0035 A14).
    let row = client
        .query_one(
            "SELECT partition, \"offset\" FROM view.activity WHERE tenant = 'librarybase' AND revid = 41500",
            &[],
        )
        .await
        .unwrap();
    let target = scatter_actors::Target::Record {
        partition: scatter_log_postgres::ids::partition_from_db(row.get(0)),
        offset: u64::try_from(row.get::<_, i64>(1)).unwrap(),
    };
    let who = scatter_actors::evaluate::evaluate(
        &scatter_actors::evaluate::Principal::registered(["sysop".to_string()]),
        scatter_actors::GroupRegistry::default_registry(),
        scatter_actors::GrantRegistry::default_registry(),
    );
    let mut hide = scatter_actors::Acl::read(
        target.clone(),
        scatter_actors::ReadKind::Moderation,
        "sysop",
        None,
    );
    hide.parts = vec!["content".into()];
    for (acl, want) in [(Some(hide), "11"), (None, "10")] {
        let mut cx = store.begin().await.unwrap();
        triplespace_api_ingest::acl::write(
            &store,
            &p,
            &mut cx,
            TENANT,
            "librarybase:7",
            &who,
            triplespace_api_ingest::acl::AclWrite {
                target: target.clone(),
                acl,
                reason: "test".into(),
                suppress_group: "suppress".into(),
                reserve: None,
                enclosing: Vec::new(),
                job: None,
            },
            NOW + 2,
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
        assert_eq!(
            rows(&client, "SELECT visibility::text FROM view.activity WHERE tenant = 'librarybase' AND revid = 41500").await,
            vec![want]
        );
    }
    // The IP is one surrogate, `actor:1`, with its IRI under the tenant's base.
    assert_eq!(
        rows(&client, "SELECT actor_key || '|' || kind || '|' || raw || '|' || iri FROM view.actor WHERE tenant = 'librarybase' AND issuer = 'actor'").await,
        vec!["actor:1|anonymous|192.0.2.1|https://librarybase.org/actor/1"]
    );
    // A re-run changes nothing.
    let again = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        adoption,
        history,
        registry,
        NOW + 1,
        10,
    )
    .await
    .unwrap();
    assert_eq!((again.counts.adopted, again.counts.unchanged), (0, 5));
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM log.record WHERE partition = 12289"
        )
        .await,
        5
    );
}

/// A tenant declares an entity source (0078 §1), its partition opens, a sync loads it
/// under the tenant with IDs in the colon form (0078 §2, §6.4), and a deletion tombstones.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_tenant_declares_and_loads_an_entity_source() {
    use scatter_actors::evaluate::Effective;
    use scatter_ingest::sync::run_sync_in;
    use scatter_providers::{EntitySource, SourceType};
    use triplespace_api_ingest::sources::{SourceWriteError, write as write_source};
    use triplespace_projections::entity_sources;

    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    // The tenant's `config` partition, where its declarations go.
    log::create_partition(
        &client,
        0x3005,
        Segments::new(8).unwrap(),
        &PartitionInfo {
            tenant: Some(TENANT.into()),
            name: "config".into(),
            graph_iri: "https://librarybase.org/graph/config".into(),
            history: History::Full,
            integrity: Integrity::Logged,
            export: Export::Public,
            hash: "sha-256".into(),
            origin: None,
        },
    )
    .await
    .unwrap();
    let store = PgIngest::new(backend);
    let p = pipeline();
    let log_store = PgLog::new(client);
    p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let client = log_store.into_inner();
    let who = Effective {
        permissions: scatter_actors::group::GroupRegistry::default_registry()
            .permissions()
            .clone(),
        ..Effective::default()
    };
    let source = |name: &str| EntitySource {
        name: name.into(),
        label: "A source".into(),
        types: vec![
            SourceType {
                upstream_prefix: "Q".into(),
                entity_type: "item".into(),
                id_grammar: "digits".into(),
                iri: "https://src.example/entity/{upstream_id}".into(),
            },
            SourceType {
                upstream_prefix: "P".into(),
                entity_type: "property".into(),
                id_grammar: "digits".into(),
                iri: "https://src.example/entity/{upstream_id}".into(),
            },
        ],
        api: Some("https://src.example/w/api.php".into()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
        number: None,
        promoted_to: None,
    };

    // 1. A registry name is refused (0078 §3 rule 1); a fresh name is numbered and opens
    //    its partition.
    for taken in ["wikidata", "domain"] {
        let mut cx = store.begin().await.unwrap();
        let err = write_source(
            &store,
            &p,
            &mut cx,
            TENANT,
            "https://librarybase.org",
            "instance:scatter",
            &who,
            taken,
            Some(source(taken)),
            "",
            NOW,
        )
        .await
        .unwrap_err();
        store.rollback(cx).await.unwrap();
        assert!(
            matches!(
                err,
                SourceWriteError::NameTaken(_)
                    | SourceWriteError::Invalid(scatter_providers::SourceError::NameTaken(_))
            ),
            "{taken}: {err}"
        );
    }
    let mut cx = store.begin().await.unwrap();
    let written = write_source(
        &store,
        &p,
        &mut cx,
        TENANT,
        "https://librarybase.org",
        "instance:scatter",
        &who,
        "src",
        Some(source("src")),
        "declared",
        NOW,
    )
    .await
    .unwrap()
    .unwrap();
    store.commit(cx).await.unwrap();
    assert_eq!(written.number, Some(4096));
    assert_eq!(
        rows(
            &client,
            "SELECT kind || ':' || code FROM view.registry WHERE tenant = 'librarybase' AND (kind = 'entity-source' OR code = 'source/src') ORDER BY 1"
        )
        .await,
        ["entity-source:src", "graph:source/src"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM log.partition WHERE tenant = 'librarybase' AND name = 'source/src'"
        )
        .await,
        1
    );
    let registry = entity_sources::registry_for(&client, TENANT).await.unwrap();
    let src = registry.by_slug("src").unwrap();
    assert!(src.source);
    assert_eq!(
        registry.parse_foreign_id("SRC:q7").unwrap().to_string(),
        "src:Q7"
    );
    let registry: &'static Registry = Box::leak(Box::new(registry));

    // 2. A sync into the source: tenant rows, colon-form IDs, no surrogates.
    let q7 = entity(json!({"type": "item", "id": "src:Q7",
        "labels": {"en": {"language": "en", "value": "Seven"}},
        "claims": {"src:P1": [string_statement("src:P1", "a", "string", "src:Q7$00000000-0000-0000-0000-0000000000B1")]}}));
    let q8 = entity(json!({"type": "item", "id": "src:Q8",
        "labels": {"en": {"language": "en", "value": "Eight"}}, "claims": {}}));
    let out = run_sync_in(
        &store,
        &p,
        TENANT,
        JobHeader::source("src", "dump-1", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            SyncItem::State {
                entity: Box::new(q7),
                upstream: Upstream::revision(5, Some("2026-10-01T00:00:00Z")),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
            SyncItem::State {
                entity: Box::new(q8),
                upstream: Upstream::revision(6, Some("2026-10-01T00:00:00Z")),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
        ],
        100,
        registry,
        NOW + 1,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.created, 2, "{out:?}");
    assert_eq!(
        rows(
            &client,
            "SELECT tenant || '|' || graph || '|' || entity_id || '|' || upstream_version FROM view.entity_source WHERE graph = 'source/src' ORDER BY entity_id"
        )
        .await,
        [
            "librarybase|source/src|src:Q7|5",
            "librarybase|source/src|src:Q8|6"
        ]
    );
    assert_eq!(
        rows(
            &client,
            "SELECT tenant || '|' || id || '|' || type || '|' || coalesce(surrogate::text, '-') FROM view.entity WHERE id LIKE 'src:%' ORDER BY id"
        )
        .await,
        ["librarybase|src:Q7|item|-", "librarybase|src:Q8|item|-"]
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.keyed_surrogate").await,
        0,
        "a source's entity needs no surrogate (0078 §2)"
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.keyed_map").await,
        0
    );
    // The job is the tenant's (0078 §4).
    assert_eq!(
        count(
            &client,
            &format!(
                "SELECT count(*) FROM view.job WHERE tenant = 'librarybase' AND job_id = {}",
                out.job_id
            )
        )
        .await,
        1
    );

    // 3. An upstream deletion tombstones (a source's entity is foreign, not keyed).
    let out = run_sync_in(
        &store,
        &p,
        TENANT,
        JobHeader::source("src", "follow-1", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![SyncItem::Deleted {
            id: EntityId::parse("src:Q8").unwrap(),
            upstream: Upstream::revision(7, None),
        }],
        100,
        registry,
        NOW + 2,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.tombstoned, 1, "{out:?}");
    assert_eq!(
        rows(
            &client,
            "SELECT entity_id FROM view.entity_source WHERE graph = 'source/src' ORDER BY 1"
        )
        .await,
        ["src:Q7"]
    );

    // 4. The instance cannot write a source graph, and the record retires.
    let err = scatter_ingest::run_sync(
        &store,
        &p,
        JobHeader::source("src", "x", Mode::Upsert),
        Attestation::by("instance:scatter"),
        Vec::<SyncItem>::new(),
        100,
        registry,
        NOW + 3,
        10,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, IngestError::Adapter(_)), "{err}");
    let mut cx = store.begin().await.unwrap();
    write_source(
        &store,
        &p,
        &mut cx,
        TENANT,
        "https://librarybase.org",
        "instance:scatter",
        &who,
        "src",
        None,
        "retired",
        NOW + 4,
    )
    .await
    .unwrap();
    store.commit(cx).await.unwrap();
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.registry WHERE tenant = 'librarybase' AND kind = 'entity-source'"
        )
        .await,
        0
    );
}
