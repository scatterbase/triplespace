//! The write path, bulk batches, mirror syncs and adoption against the in-memory store.

use std::collections::BTreeMap;

use scatter_ingest::adopt::{Adopted, Adoption, Floors, run_adoption};
use scatter_ingest::memory::{MemoryCx, MemoryIngest};
use scatter_ingest::{
    Attestation, IngestError, IngestStore, Request, Sequence, SyncItem, run_batch, run_sync,
    write_operation,
};
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::Draft;
use scatter_projection::{Applied, Backend, BoxFuture, Pipeline, Projection, Step};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{JobHeader, Mode, Operation, ProviderOrder, Upstream};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::DataType;
use serde_json::json;

const TENANT: &str = "librarybase";
const LOCAL: u64 = 0x11;
const PAGES: u64 = 0x12;
const TENANT_LOG: u64 = 0x13;
const ACTORS: u64 = 0x14;
const INSTANCE_LOG: u64 = 0x21;
const MIRROR_XD: u64 = 0x22;
const NOW: u64 = 1_790_000_000_000_000;

/// A projection that records what it saw, so the inline run is observable.
struct Recorder;

impl Projection<MemoryIngest> for Recorder {
    fn name(&self) -> &'static str {
        "recorder"
    }
    fn step(&self) -> Step {
        Step::Sources
    }
    fn apply<'a>(
        &'a self,
        cx: &'a mut MemoryCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            cx.put(
                "seen",
                &format!("{:#x}:{}", h.partition, h.offset),
                h.key.as_deref().unwrap_or("-"),
            );
            Ok(Applied::rows(1))
        })
    }
    fn reset<'a>(
        &'a self,
        _cx: &'a mut MemoryCx,
        _partition: u64,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

fn setup() -> (MemoryIngest, Pipeline<MemoryIngest>) {
    let store = MemoryIngest::new();
    store.create_partition(LOCAL, TENANT, "local");
    store.create_partition(PAGES, TENANT, "pages");
    store.create_partition(TENANT_LOG, TENANT, "log");
    store.create_partition(ACTORS, TENANT, "actors");
    store.create_partition(INSTANCE_LOG, "", "log");
    store.create_partition(MIRROR_XD, "", "mirror/internetdomains");
    store.set_datatype(TENANT, "P356", DataType::ExternalId);
    store.set_datatype(TENANT, "P31", DataType::WikibaseItem);
    let pipeline = Pipeline::new().with(Recorder).unwrap();
    (store, pipeline)
}

fn registry() -> &'static Registry {
    Registry::default_registry()
}

fn entity(v: serde_json::Value) -> Entity {
    Entity::from_value(v).unwrap().entity
}

fn string_statement(p: &str, v: &str, datatype: &str) -> serde_json::Value {
    json!({"mainsnak": {"snaktype": "value", "property": p, "datavalue": {"value": v, "type": "string"}, "datatype": datatype},
        "type": "statement", "rank": "normal"})
}

fn item(id: &str, label: &str, claims: &serde_json::Value) -> serde_json::Value {
    json!({"type": "item", "id": id, "labels": {"en": {"language": "en", "value": label}}, "claims": claims})
}

fn op(v: serde_json::Value) -> Operation {
    serde_json::from_value(v).unwrap()
}

fn operation_of(record: &Record) -> Operation {
    scatter_log::cbor::from_value(record.body().content().value().unwrap().unwrap()).unwrap()
}

fn events(store: &MemoryIngest, partition: u64) -> Vec<(String, String)> {
    store
        .records(partition)
        .iter()
        .filter(|r| r.header().payload_type == "scatter:v0/logevent")
        .map(|r| {
            let v = r.body().content().value().unwrap().unwrap();
            (
                v.get("action")
                    .and_then(Value::as_text)
                    .unwrap()
                    .to_string(),
                r.header().key.clone().unwrap(),
            )
        })
        .collect()
}

fn params_of(record: &Record) -> serde_json::Value {
    record
        .body()
        .content()
        .value()
        .unwrap()
        .unwrap()
        .get("params")
        .unwrap()
        .to_json()
        .unwrap()
}

// --- the write path ----------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines)]
fn writes_take_ids_keys_and_project_inline() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        let request = Request::new(TENANT, Attestation::by("librarybase:1"), NOW);
        let mut cx = store.begin().await.unwrap();
        let create =
            op(json!({"op": "create", "id": "Q1", "entity": item("Q1", "One", &json!({}))}));
        let w = write_operation(&store, &pipeline, &mut cx, &request, &create, registry())
            .await
            .unwrap();
        assert_eq!(w.partition, LOCAL);
        let h: &Header = w.record.header();
        assert_eq!(
            (h.offset, h.revid, h.page_id, h.key.as_deref()),
            (0, Some(1), Some(1), Some("Q1"))
        );
        assert_eq!(h.payload_type, "scatter:v0/changeset");
        assert_eq!(w.inline.rows, 1, "the recorder ran inline");
        // A second write to the same key keeps the page ID, takes the next revision.
        let add = op(json!({"op": "add", "id": "Q1", "labels": {"fr": "Un"}}));
        let w2 = write_operation(&store, &pipeline, &mut cx, &request, &add, registry())
            .await
            .unwrap();
        assert_eq!(
            (
                w2.offset(),
                w2.record.header().revid,
                w2.record.header().page_id
            ),
            (1, Some(2), Some(1))
        );
        // A stale base offset is a conflict (0006 §8).
        let mut stale = request.clone();
        stale.base_offset = Some(0);
        let err = write_operation(&store, &pipeline, &mut cx, &stale, &add, registry())
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                IngestError::Conflict {
                    base: 0,
                    latest: 1,
                    ..
                }
            ),
            "{err}"
        );
        let mut fresh = request.clone();
        fresh.base_offset = Some(1);
        write_operation(&store, &pipeline, &mut cx, &fresh, &add, registry())
            .await
            .unwrap();
        // A page's statements go to `pages`.
        let page = op(
            json!({"op": "add", "page": 77, "claims": {"P31": [{"mainsnak": {"snaktype": "somevalue", "property": "P31", "datatype": "wikibase-item"}, "type": "statement", "rank": "normal"}]}}),
        );
        let wp = write_operation(&store, &pipeline, &mut cx, &request, &page, registry())
            .await
            .unwrap();
        assert_eq!(wp.partition, PAGES);
        assert_eq!(wp.record.header().key.as_deref(), Some("77"));
        assert_eq!(
            wp.record.header().page_id,
            Some(2),
            "a fresh page ID for a new key"
        );
        // A keyed subject is keyed by a surrogate, minted and recorded in the instance log.
        let domain =
            op(json!({"op": "add", "id": "domain:example.org", "labels": {"en": "Example"}}));
        let wd = write_operation(&store, &pipeline, &mut cx, &request, &domain, registry())
            .await
            .unwrap();
        assert_eq!(wd.record.header().key.as_deref(), Some("domain#1"));
        let again = write_operation(&store, &pipeline, &mut cx, &request, &domain, registry())
            .await
            .unwrap();
        assert_eq!(
            again.record.header().key.as_deref(),
            Some("domain#1"),
            "reused"
        );
        assert_eq!(again.record.header().page_id, wd.record.header().page_id);
        // A mirror put goes to the provider's partition with a provider-ranged revid.
        let put = op(
            json!({"op": "put", "id": "XDQ5", "entity": item("XDQ5", "Five", &json!({})), "upstream": {"revid": 900}}),
        );
        let wm = write_operation(&store, &pipeline, &mut cx, &request, &put, registry())
            .await
            .unwrap();
        assert_eq!(wm.partition, MIRROR_XD);
        let xd = registry().by_slug("internetdomains").unwrap().number;
        assert_eq!(
            wm.record.header().revid,
            Some((u64::from(xd) << 40) | 0x384)
        );
        // Not record-shaped: refused.
        let bulk = op(json!({"op": "retain", "ids": ["Q1"], "policy": "retain"}));
        assert!(
            write_operation(&store, &pipeline, &mut cx, &request, &bulk, registry())
                .await
                .is_err()
        );
        store.commit(cx).await.unwrap();

        let instance_log = store.records(INSTANCE_LOG);
        assert_eq!(instance_log.len(), 1);
        assert_eq!(
            instance_log[0].header().payload_type,
            "scatter:v0/keyed-surrogate"
        );
        assert_eq!(instance_log[0].header().key.as_deref(), Some("domain#1"));
        let mapping = instance_log[0].body().content().value().unwrap().unwrap();
        assert_eq!(
            mapping.get("key").and_then(Value::as_text),
            Some("example.org")
        );
        assert_eq!(store.records(LOCAL).len(), 5);
        assert_eq!(
            store.state().rows.len(),
            8,
            "every record was projected inline"
        );
        // Rollback leaves nothing.
        let mut cx = store.begin().await.unwrap();
        write_operation(&store, &pipeline, &mut cx, &request, &add, registry())
            .await
            .unwrap();
        store.rollback(cx).await.unwrap();
        assert_eq!(store.records(LOCAL).len(), 5);
    });
}

// --- bulk batches ------------------------------------------------------------------------

const BATCH: &str = r#"{"job":{"source":"citation-batch","graph":"local"}}
{"op":"create","ref":"$w1","entity":{"type":"item","id":"$w1","labels":{"en":{"language":"en","value":"A paper"}},"claims":{"P356":[{"mainsnak":{"snaktype":"value","property":"P356","datavalue":{"value":"10.1/new","type":"string"},"datatype":"external-id"},"type":"statement","rank":"normal"}]}}}
{"op":"create-or-add","ref":"$w2","match":{"P356":"10.1/known"},"entity":{"type":"item","id":"$w2","labels":{"fr":{"language":"fr","value":"Connu"}},"claims":{}}}
{"op":"create-or-add","match":{"P356":"10.1/unknown"},"entity":{"type":"item","id":"Q0","labels":{"en":{"language":"en","value":"Unknown"}},"claims":{}}}
{"op":"add","id":"$w2","claims":{"P31":[{"mainsnak":{"snaktype":"value","property":"P31","datavalue":{"value":{"entity-type":"item","id":"$w1"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","rank":"normal"}]}}
{"op":"retain","ids":["XDQ1","XDQ2"],"policy":"retain"}
{"op":"add","id":"Q9","page":3,"labels":{"en":"both"}}
{"op":"override","statement":"XDQ1$2F1C0000-0000-0000-0000-000000000000","rank":"deprecated"}
"#;

async fn seed_known(store: &MemoryIngest, pipeline: &Pipeline<MemoryIngest>) {
    let request = Request::new(TENANT, Attestation::by("librarybase:1"), NOW);
    let mut cx = store.begin().await.unwrap();
    let known = op(
        json!({"op": "create", "id": "Q5", "entity": item("Q5", "Known", &json!({"P356": [string_statement("P356", "10.1/known", "external-id")]}))}),
    );
    write_operation(store, pipeline, &mut cx, &request, &known, registry())
        .await
        .unwrap();
    store.commit(cx).await.unwrap();
    // Q5 took item number 1 from the sequence; floor it so minted IDs start at Q6.
    let mut cx = store.begin().await.unwrap();
    store
        .floor(&mut cx, TENANT, &Sequence::Entity("item".into()), 5)
        .await
        .unwrap();
    store.commit(cx).await.unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_streamed_batch_mints_matches_and_records_rejects() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        seed_known(&store, &pipeline).await;
        let out = run_batch(
            &store,
            &pipeline,
            TENANT,
            Attestation::by("librarybase:2"),
            BATCH,
            &ProviderOrder::default(),
            registry(),
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(out.ids.get("$w1").map(EntityId::as_str), Some("Q6"));
        assert_eq!(
            out.ids.get("$w2").map(EntityId::as_str),
            Some("Q5"),
            "matched by DOI"
        );
        assert_eq!(out.counts.created, 2, "$w1 and the unknown create-or-add");
        assert_eq!(
            out.counts.merged, 2,
            "the matched create-or-add and the add"
        );
        assert_eq!(out.counts.other, 3, "two retains and the override");
        assert_eq!(out.counts.rejected, 1);
        assert_eq!(out.rejects.len(), 1);
        assert_eq!(out.rejects[0].line, 7);
        assert!(
            out.rejects[0]
                .reason
                .contains("exactly one of `id` and `page`"),
            "{}",
            out.rejects[0].reason
        );

        let ops = store.operations(LOCAL);
        assert_eq!(ops.len(), 1 + 7);
        match &ops[1] {
            Operation::Create {
                id: Some(id),
                entity,
                ..
            } => {
                assert_eq!(id.as_str(), "Q6");
                assert_eq!(entity.id.as_str(), "Q6");
            }
            other => panic!("{other:?}"),
        }
        match &ops[2] {
            Operation::Add {
                id: Some(id),
                labels,
                via: Some(_),
                match_key: Some(_),
                ..
            } => {
                assert_eq!(id.as_str(), "Q5");
                assert_eq!(labels["fr"], "Connu");
            }
            other => panic!("{other:?}"),
        }
        match &ops[3] {
            Operation::Create {
                id: Some(id),
                via: Some(_),
                ..
            } => assert_eq!(id.as_str(), "Q7"),
            other => panic!("{other:?}"),
        }
        match &ops[4] {
            Operation::Add {
                id: Some(id),
                claims,
                ..
            } => {
                assert_eq!(id.as_str(), "Q5");
                let s = &claims.first().unwrap().1[0];
                assert_eq!(
                    s.mainsnak
                        .data_value()
                        .unwrap()
                        .entity_id()
                        .unwrap()
                        .as_str(),
                    "Q6"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ops[5].key().as_deref(), Some("XDQ1"));
        assert_eq!(ops[6].key().as_deref(), Some("XDQ2"));
        assert_eq!(
            ops[7].key().as_deref(),
            Some("XDQ1"),
            "the override completed from its statement"
        );
        // Every record names the job and the subsidiary.
        for r in store.records(LOCAL).iter().skip(1) {
            let a = r.body().attestation().value().unwrap().unwrap();
            assert_eq!(
                a.get("actor").and_then(Value::as_text),
                Some("librarybase:2")
            );
            assert_eq!(a.get("job").and_then(Value::as_u64), Some(out.job_id));
        }
        // The job's records in the tenant log.
        assert_eq!(
            events(&store, TENANT_LOG),
            vec![
                ("start".to_string(), "1".to_string()),
                ("finish".to_string(), "1".to_string())
            ]
        );
        let finish = params_of(&store.records(TENANT_LOG)[1]);
        assert_eq!(finish["counts"]["created"], 2);
        assert_eq!(finish["counts"]["rejected"], 1);
        assert_eq!(finish["rejects"][0]["line"], 7);
        let start = params_of(&store.records(TENANT_LOG)[0]);
        assert_eq!(start["source"], "citation-batch");
        assert_eq!(start["graph"], "local");
        assert!(
            store
                .records(TENANT_LOG)
                .iter()
                .all(|r| r.header().logid.is_some())
        );
    });
}

#[test]
fn an_atomic_batch_fails_as_a_whole() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        let text = BATCH.replacen(r#""graph":"local""#, r#""graph":"local","atomic":true"#, 1);
        let err = run_batch(
            &store,
            &pipeline,
            TENANT,
            Attestation::by("librarybase:2"),
            &text,
            &ProviderOrder::default(),
            registry(),
            NOW,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("line 7"), "{err}");
        assert!(store.records(LOCAL).is_empty(), "nothing landed");
        assert_eq!(
            events(&store, TENANT_LOG),
            vec![
                ("start".to_string(), "1".to_string()),
                ("fail".to_string(), "1".to_string())
            ]
        );
        // Without the bad line it goes through in one unit of work.
        let good: String = text.lines().filter(|l| !l.contains(r#""page":3"#)).fold(
            String::new(),
            |mut acc, l| {
                acc.push_str(l);
                acc.push('\n');
                acc
            },
        );
        let out = run_batch(
            &store,
            &pipeline,
            TENANT,
            Attestation::by("librarybase:2"),
            &good,
            &ProviderOrder::default(),
            registry(),
            NOW,
        )
        .await
        .unwrap();
        assert_eq!(out.counts.rejected, 0);
        assert_eq!(store.operations(LOCAL).len(), 7);
        assert_eq!(events(&store, TENANT_LOG).len(), 4);
        // A mirror graph or a mode is not a bulk job's.
        let bad = r#"{"job":{"source":"x","graph":"mirror/internetdomains"}}"#;
        assert!(
            run_batch(
                &store,
                &pipeline,
                TENANT,
                Attestation::by("librarybase:2"),
                bad,
                &ProviderOrder::default(),
                registry(),
                NOW
            )
            .await
            .is_err()
        );
    });
}

// --- mirror sync -------------------------------------------------------------------------

fn xd_state(id: &str, label: &str, revid: u64) -> SyncItem {
    SyncItem::State {
        entity: Box::new(entity(item(
            id,
            label,
            &json!({"P1": [string_statement("P1", "a", "string")]}),
        ))),
        upstream: Upstream::revision(revid, Some("2026-10-01T00:00:00Z")),
        upstream_id: None,
    }
}

fn sync_header(mode: Mode) -> JobHeader {
    JobHeader::mirror("internetdomains", "2026-10-01", mode)
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_sync_keeps_the_cursor_and_sweeps_in_snapshot_mode() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        let actor = Attestation::by("instance:scatter");
        // A domain mapped from Q9, with a bogus upstream hash that the guard counts.
        let mut domain = entity(json!({"type": "domain", "id": "domain:example.org",
            "labels": {"en": {"language": "en", "value": "example.org"}},
            "claims": {"P1": [{"mainsnak": {"snaktype": "value", "property": "P1", "hash": "0000000000000000000000000000000000000000",
                "datavalue": {"value": "a", "type": "string"}, "datatype": "string"}, "type": "statement", "rank": "normal"}]}}));
        domain.id = EntityId::parse("domain:example.org").unwrap();
        let items = vec![
            xd_state("XDQ1", "One", 10),
            xd_state("XDQ2", "Two", 20),
            SyncItem::State {
                entity: Box::new(domain.clone()),
                upstream: Upstream::revision(30, None),
                upstream_id: Some("Q9".into()),
            },
            SyncItem::Deleted {
                id: EntityId::parse("XDQ99").unwrap(),
                upstream: Upstream::revision(31, None),
            },
        ];
        let out = run_sync(
            &store,
            &pipeline,
            sync_header(Mode::Upsert),
            actor.clone(),
            items,
            100,
            registry(),
            NOW,
            2,
        )
        .await
        .unwrap();
        assert_eq!(out.counts.created, 3);
        assert_eq!(out.counts.unchanged, 1, "a deletion of nothing");
        let ops = store.operations(MIRROR_XD);
        assert_eq!(ops.len(), 3);
        match &ops[0] {
            Operation::Put {
                prev_upstream,
                first_seen,
                size,
                changes,
                ..
            } => {
                assert!(prev_upstream.is_none());
                assert_eq!(*first_seen, Some(NOW));
                assert!(size.is_some());
                assert_eq!(changes.as_ref().unwrap().labels["en"].added, 1);
            }
            other => panic!("{other:?}"),
        }
        match &ops[2] {
            Operation::Put {
                id,
                upstream_id,
                entity,
                ..
            } => {
                assert_eq!(id.as_str(), "domain:example.org");
                assert_eq!(upstream_id.as_deref(), Some("Q9"));
                assert!(
                    entity
                        .all_statements()
                        .next()
                        .unwrap()
                        .mainsnak
                        .hash
                        .is_some(),
                    "a differing hash is kept"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            store.records(MIRROR_XD)[2].header().key.as_deref(),
            Some("domain#1")
        );
        let log = store.records(INSTANCE_LOG);
        let kinds: Vec<&str> = log
            .iter()
            .map(|r| r.header().payload_type.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "scatter:v0/logevent",
                "scatter:v0/keyed-surrogate",
                "scatter:v0/logevent"
            ]
        );
        let finish = params_of(&log[2]);
        assert_eq!(finish["hash_mismatches"]["string"], 1);
        assert_eq!(finish["counts"]["created"], 3);
        assert_eq!(
            finish["mode"],
            serde_json::Value::Null,
            "the finish carries counts; the start carries the mode"
        );
        assert_eq!(params_of(&log[0])["mode"], "upsert");

        // Re-running with the same states changes nothing; a newer state is merged with
        // the previous version and a summary; an older one is skipped.
        let mut two_new = entity(item(
            "XDQ2",
            "Two renamed",
            &json!({"P1": [string_statement("P1", "a", "string"), string_statement("P1", "b", "string")]}),
        ));
        two_new.id = EntityId::parse("XDQ2").unwrap();
        let items = vec![
            xd_state("XDQ1", "One", 10),
            SyncItem::State {
                entity: Box::new(two_new),
                upstream: Upstream::revision(25, None),
                upstream_id: None,
            },
            xd_state("XDQ1", "Older", 5),
        ];
        let out = run_sync(
            &store,
            &pipeline,
            sync_header(Mode::Upsert),
            actor.clone(),
            items,
            100,
            registry(),
            NOW + 10,
            50,
        )
        .await
        .unwrap();
        assert_eq!(
            (out.counts.created, out.counts.merged, out.counts.unchanged),
            (0, 1, 2)
        );
        let ops = store.operations(MIRROR_XD);
        match &ops[3] {
            Operation::Put {
                id,
                prev_upstream,
                first_seen,
                prev_size,
                changes,
                upstream,
                ..
            } => {
                assert_eq!(id.as_str(), "XDQ2");
                assert_eq!(prev_upstream.as_ref().unwrap().revid, Some(20));
                assert_eq!(upstream.revid, Some(25));
                assert_eq!(*first_seen, Some(NOW), "carried from the first put");
                assert!(prev_size.is_some());
                let c = changes.as_ref().unwrap();
                assert_eq!(c.labels["en"].changed, 1);
                assert_eq!(c.claims[&EntityId::parse("P1").unwrap()].added, 1);
            }
            other => panic!("{other:?}"),
        }

        // Snapshot mode: what the job did not see is swept, within the threshold.
        let out = run_sync(
            &store,
            &pipeline,
            sync_header(Mode::Snapshot),
            actor.clone(),
            vec![xd_state("XDQ1", "One", 10)],
            5,
            registry(),
            NOW + 20,
            50,
        )
        .await
        .unwrap();
        assert_eq!(out.swept, 2, "XDQ2 and the domain");
        assert_eq!(out.counts.tombstoned, 2);
        let ops = store.operations(MIRROR_XD);
        assert!(matches!(ops[ops.len() - 1], Operation::Tombstone { .. }));
        let mut cx = store.begin().await.unwrap();
        let remaining = store
            .entities_of_graph(&mut cx, "", "mirror/internetdomains")
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        store.rollback(cx).await.unwrap();
        let finish = params_of(store.records(INSTANCE_LOG).last().unwrap());
        assert_eq!(finish["sweep"]["count"], 2);

        // Over the threshold, the sweep refuses and the job fails.
        let before = store.records(MIRROR_XD).len();
        let err = run_sync(
            &store,
            &pipeline,
            sync_header(Mode::Snapshot),
            actor,
            Vec::<SyncItem>::new(),
            0,
            registry(),
            NOW + 30,
            50,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                err,
                IngestError::SweepThreshold {
                    count: 1,
                    threshold: 0
                }
            ),
            "{err}"
        );
        assert_eq!(store.records(MIRROR_XD).len(), before);
        let last = events(&store, INSTANCE_LOG).pop().unwrap();
        assert_eq!(last.0, "fail");
    });
}

// --- adoption ----------------------------------------------------------------------------

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

fn adoption(frozen: bool) -> Adoption {
    Adoption {
        source: "https://librarybase.org/".into(),
        version: "librarybase-20260928.json.gz".into(),
        adapter_version: Some("scatter-adapter-wikidata 0.0.1".into()),
        frozen,
        floors: Floors {
            entity: BTreeMap::from([("item".to_string(), 1000), ("property".to_string(), 50)]),
            page: 2000,
            revision: 50_000,
            log: 700,
            user: 40,
        },
        accounts: vec![
            account("librarybase:1", "Owner"),
            account("librarybase:7", "Editor"),
        ],
    }
}

fn adopted(id: &str, label: &str, revid: u64, pageid: u64) -> Adopted {
    Adopted {
        entity: entity(item(id, label, &json!({}))),
        source_revid: revid,
        source_time: "2026-09-20T14:02:11Z".into(),
        source_pageid: pageid,
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn adoption_checks_floors_resumes_and_rejects_conflicts() {
    pollster::block_on(async {
        let (store, pipeline) = setup();
        let actor = Attestation::by("librarybase:2");
        // The owner is already there, from `instance create --adopt --owner`.
        let mut cx = store.begin().await.unwrap();
        store
            .append(&mut cx, ACTORS, account("librarybase:1", "Owner"))
            .await
            .unwrap();
        store.commit(cx).await.unwrap();

        let err = run_adoption(
            &store,
            &pipeline,
            TENANT,
            actor.clone(),
            adoption(false),
            Vec::<Adopted>::new(),
            registry(),
            NOW,
            10,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("frozen"), "{err}");

        let items = vec![
            adopted("Q6", "Six", 41_877, 12),
            adopted("P12", "prop", 903, 40),
            adopted("Q8", "Eight", 42_000, 13),
        ];
        let mut items = items;
        items[1].entity = entity(
            json!({"type": "property", "id": "P12", "datatype": "string", "labels": {"en": {"language": "en", "value": "prop"}}, "claims": {}}),
        );
        let out = run_adoption(
            &store,
            &pipeline,
            TENANT,
            actor.clone(),
            adoption(true),
            items.clone(),
            registry(),
            NOW,
            2,
        )
        .await
        .unwrap();
        assert_eq!(out.counts.adopted, 3);
        assert_eq!(
            out.accounts, 1,
            "the owner was present; the editor was written"
        );
        assert_eq!(store.records(ACTORS).len(), 2);
        let local = store.records(LOCAL);
        assert_eq!(local.len(), 3);
        assert_eq!(local[0].header().page_id, Some(12), "the source's page ID");
        assert_eq!(local[1].header().page_id, Some(40));
        assert!(
            local[0].header().revid.unwrap() > 50_000,
            "above the revision floor"
        );
        assert!(matches!(
            operation_of(&local[0]),
            Operation::Adopt {
                source_revid: 41_877,
                ..
            }
        ));
        // The floors moved the sequences.
        let mut cx = store.begin().await.unwrap();
        assert_eq!(
            store
                .next_id(&mut cx, TENANT, &Sequence::Entity("item".into()))
                .await
                .unwrap(),
            1001
        );
        assert_eq!(
            store
                .next_id(&mut cx, TENANT, &Sequence::Page)
                .await
                .unwrap(),
            2001
        );
        assert_eq!(
            store
                .next_id(&mut cx, TENANT, &Sequence::User)
                .await
                .unwrap(),
            41
        );
        store.rollback(cx).await.unwrap();
        let log = store.records(TENANT_LOG);
        let start = params_of(&log[0]);
        assert_eq!(start["mode"], "adopt");
        assert_eq!(start["args"]["floors"]["page"], 2000);
        let finish = params_of(&log[1]);
        assert_eq!(finish["counts"]["adopted"], 3);
        assert_eq!(finish["floors"]["revision"], 50_000);
        assert_eq!(finish["accounts"], 1);

        // Re-running skips what is present and rejects a changed entity.
        let mut again = items.clone();
        again[2].entity = entity(item("Q8", "Eight, changed", &json!({})));
        again.push(adopted("Q9", "Nine", 42_100, 14));
        let out = run_adoption(
            &store,
            &pipeline,
            TENANT,
            actor.clone(),
            adoption(true),
            again,
            registry(),
            NOW + 1,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            (
                out.counts.adopted,
                out.counts.unchanged,
                out.counts.rejected
            ),
            (1, 2, 1)
        );
        assert!(out.rejects[0].reason.contains("ts-adopt-conflict"));
        assert_eq!(store.records(LOCAL).len(), 4);

        // Another source is refused; so is a partition with an ordinary edit.
        let mut other = adoption(true);
        other.source = "https://elsewhere.example/".into();
        let err = run_adoption(
            &store,
            &pipeline,
            TENANT,
            actor.clone(),
            other,
            Vec::<Adopted>::new(),
            registry(),
            NOW,
            10,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("adopted from"), "{err}");
        let request = Request::new(TENANT, actor.clone(), NOW);
        let mut cx = store.begin().await.unwrap();
        write_operation(
            &store,
            &pipeline,
            &mut cx,
            &request,
            &op(json!({"op": "add", "id": "Q6", "labels": {"fr": "Six"}})),
            registry(),
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
        let err = run_adoption(
            &store,
            &pipeline,
            TENANT,
            actor,
            adoption(true),
            Vec::<Adopted>::new(),
            registry(),
            NOW,
            10,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("not adoptions"), "{err}");
    });
}
