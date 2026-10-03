//! The operations of payloads.md §3, the wire flow of 0002 §8.5–8.7, eager validation
//! and the Wikibase adapter.

use std::collections::BTreeMap;

use scatter_wikibase_changeset::adapter::{Adapter, KeyMap, assign_statement_ids};
use scatter_wikibase_changeset::{
    Batch, Changes, Context, Graph, JobHeader, MatchKey, Mode, NoProperties, Operation, Problem,
    ProviderOrder, Resolution, Retention, Upstream, Via, WikibaseAdapter, WireError,
    deterministic_statement_id, validate, validate_all,
};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, StatementId};
use scatter_wikibase_model::value::DataType;
use serde_json::{Value, json};

fn id(s: &str) -> EntityId {
    EntityId::parse(s).unwrap()
}

fn statement(p: &str, v: &Value, id: Option<&str>) -> Value {
    let mut s = json!({"mainsnak": {"snaktype": "value", "property": p, "datavalue": v,
        "datatype": if v_is_entity(v) { "wikibase-item" } else { "string" }},
        "type": "statement", "rank": "normal"});
    if let Some(id) = id {
        s["id"] = json!(id);
    }
    s
}

fn v_is_entity(v: &Value) -> bool {
    v["type"] == "wikibase-entityid"
}

fn string(v: &str) -> Value {
    json!({"value": v, "type": "string"})
}

fn item(q: &str) -> Value {
    json!({"value": {"entity-type": "item", "id": q}, "type": "wikibase-entityid"})
}

fn item_entity(id: &str, claims: &Value) -> Value {
    json!({"type": "item", "id": id,
        "labels": {"en": {"language": "en", "value": format!("label of {id}")}},
        "claims": claims})
}

fn types() -> BTreeMap<EntityId, DataType> {
    [
        ("P31", DataType::WikibaseItem),
        ("P50", DataType::WikibaseItem),
        ("P356", DataType::ExternalId),
        ("P40", DataType::String),
        ("P248", DataType::WikibaseItem),
    ]
    .into_iter()
    .map(|(p, t)| (id(p), t))
    .collect()
}

fn local_context<'a>(
    order: &'a ProviderOrder,
    types: &'a dyn scatter_wikibase_changeset::PropertyTypes,
) -> Context<'a> {
    Context::for_job(&JobHeader::local("test"), order, types).unwrap()
}

fn problems(op: &Operation, graph: &str, mode: Option<Mode>) -> Vec<Problem> {
    let order = ProviderOrder::default();
    let t = types();
    let mut job = JobHeader::local("test");
    job.graph = graph.to_string();
    job.mode = mode;
    let mut op = op.clone();
    op.complete(&order);
    validate(&op, &Context::for_job(&job, &order, &t).unwrap())
}

// --- payloads.md §3 ---------------------------------------------------------------------

#[test]
fn the_payloads_examples_round_trip() {
    let examples = [
        json!({"op": "put", "id": "WDQ42", "entity": item_entity("WDQ42", &json!({})),
            "upstream": {"revid": 2_148_573_921_u64, "time": "2026-09-30T12:00:00Z"},
            "prev_upstream": {"revid": 2_148_570_000_u64}, "first_seen": 1_780_000_000_000_000_u64,
            "size": 48211, "prev_size": 47990,
            "changes": {"claims": {"P31": {"added": 1, "removed": 0, "changed": 0}},
                        "labels": {"de": {"added": 0, "removed": 0, "changed": 1}}},
            "delta": {"anything": true}}),
        json!({"op": "put", "id": "OAW123", "entity": item_entity("OAW123", &json!({})), "upstream": {"version": "2026-08-30"}, "size": 2210}),
        json!({"op": "tombstone", "id": "WDQ77", "upstream": {"revid": 2_148_580_000_u64, "time": "x"}}),
        json!({"op": "redirect", "from": "WDQ99", "to": "WDQ42", "upstream": {"revid": 2_148_581_000_u64}}),
        json!({"op": "create", "id": "Q900001", "entity": item_entity("Q900001", &json!({}))}),
        json!({"op": "create", "id": "Q900002", "entity": item_entity("Q900002", &json!({})), "match": {"P356": "10.1234/x"}, "via": "create-or-add"}),
        json!({"op": "add", "id": "Q5", "claims": {"P50": [statement("P50", &item("Q7"), None)]}, "match": {"P356": "10.1234/y"}, "via": "create-or-add"}),
        json!({"op": "add", "id": "Q5", "entity": item_entity("Q5", &json!({})), "match": {"P356": "10.1234/y"}, "via": "create-or-add", "overwrite": true}),
        json!({"op": "adopt", "id": "Q6", "entity": item_entity("Q6", &json!({})), "source_revid": 41877, "source_time": "2026-09-20T14:02:11Z", "source_pageid": 12}),
        json!({"op": "add", "id": "Q5", "claims": {"P50": [statement("P50", &item("Q7"), None)]}, "labels": {"fr": "Exemple"}, "aliases": {"en": ["Ex"]}}),
        json!({"op": "add", "id": "WDQ42", "references": {"WDQ42$C13E7A23-0000-0000-0000-000000000000": [{"snaks": {"P248": [{"snaktype": "value", "property": "P248", "datavalue": item("Q8"), "datatype": "wikibase-item"}]}, "snaks-order": ["P248"]}]}}),
        json!({"op": "add", "page": 1234, "claims": {"P40": [statement("P40", &string("x"), None)]}}),
        json!({"op": "remove", "id": "Q5", "statements": ["Q5$A8C2D6D3-0000-0000-0000-000000000000"], "labels": ["fr"], "aliases": {"en": ["Ex"]}, "sitelinks": ["enwiki"]}),
        json!({"op": "remove", "id": "WDQ42", "references": {"WDQ42$C13E7A23-0000-0000-0000-000000000000": ["eaf0a11b92f297234266b31f9331c3ebcfe09c1e"]}}),
        json!({"op": "remove", "id": "Q456", "link": {"op": "same-as", "ids": ["Q456", "OAW123"]}}),
        json!({"op": "override", "id": "OAW123", "statement": "OAW123$2F1C0000-0000-0000-0000-000000000000", "rank": "deprecated"}),
        json!({"op": "override", "id": "OAW123", "statement": "OAW123$2F1C0000-0000-0000-0000-000000000000", "suppress": true}),
        json!({"op": "retain", "id": "WDQ123", "policy": "retain"}),
        json!({"op": "convert", "id": "WDQ123", "local": "Q456"}),
        json!({"op": "same-as", "id": "Q456", "ids": ["Q456", "OAW123"]}),
        json!({"op": "different-from", "id": "Q1", "ids": ["Q1", "OAW7"]}),
        json!({"op": "equivalent-property", "id": "P12", "ids": ["P12", "WDP585"]}),
        json!({"op": "redirect", "from": "P12", "to": "WDP585"}),
    ];
    let expected_keys = [
        "WDQ42", "OAW123", "WDQ77", "WDQ99", "Q900001", "Q900002", "Q5", "Q5", "Q6", "Q5", "WDQ42",
        "1234", "Q5", "WDQ42", "Q456", "OAW123", "OAW123", "WDQ123", "WDQ123", "Q456", "Q1", "P12",
        "P12",
    ];
    for (example, key) in examples.iter().zip(expected_keys) {
        let op: Operation =
            serde_json::from_value(example.clone()).unwrap_or_else(|e| panic!("{example}: {e}"));
        assert_eq!(op.key().as_deref(), Some(key), "{}", op.name());
        assert!(op.is_record_shape(), "{}", op.name());
        let back: Value = serde_json::to_value(&op).unwrap();
        let again: Operation = serde_json::from_value(back.clone()).unwrap();
        assert_eq!(again, op);
        // Every input field survives: compare as canonical JSON of the re-parsed input.
        let normalized: Value =
            serde_json::to_value(serde_json::from_value::<Operation>(example.clone()).unwrap())
                .unwrap();
        assert_eq!(back, normalized);
    }
    // And they all pass validation in their graphs.
    let order = ProviderOrder::default();
    let t = types();
    for (i, example) in examples.iter().enumerate() {
        let op: Operation = serde_json::from_value(example.clone()).unwrap();
        let (graph, mode) = match i {
            0..=3 => (
                if i == 1 {
                    "mirror/openalex"
                } else {
                    "mirror/wikidata"
                },
                Some(Mode::Upsert),
            ),
            8 => ("local", Some(Mode::Adopt)),
            11 => ("pages", None),
            _ => ("local", None),
        };
        let mut job = JobHeader::local("t");
        job.graph = graph.into();
        job.mode = mode;
        let p = validate(&op, &Context::for_job(&job, &order, &t).unwrap());
        assert!(p.is_empty(), "{}: {p:?}", op.name());
    }
}

#[test]
fn unknown_fields_and_bad_shapes_are_rejected_at_parse() {
    for bad in [
        json!({"op": "retain", "id": "Q1", "policy": "forever"}),
        json!({"op": "create", "id": "Q1", "entity": item_entity("Q1", &json!({})), "extra": 1}),
        json!({"op": "frobnicate", "id": "Q1"}),
        json!({"op": "add", "id": "Q1", "labels": {"en": {"language": "en", "value": "x"}}}),
        json!({"op": "put", "id": "WDQ1", "entity": item_entity("WDQ1", &json!({})), "upstream": {"revid": "x"}}),
    ] {
        assert!(
            serde_json::from_value::<Operation>(bad.clone()).is_err(),
            "{bad}"
        );
    }
}

// --- the wire flow ----------------------------------------------------------------------

const BATCH: &str = r#"{"job":{"source":"citation-batch","graph":"local","atomic":true}}
{"op":"create","ref":"$w1","entity":{"type":"item","id":"$w1","labels":{"en":{"language":"en","value":"A paper"}},"claims":{}}}
{"op":"create","ref":"$p1","entity":{"type":"property","id":"$p1","datatype":"wikibase-item","labels":{"en":{"language":"en","value":"cites"}},"claims":{}}}

{"op":"add","id":"Q5","claims":{"$p1":[{"mainsnak":{"snaktype":"value","property":"$p1","datavalue":{"value":{"entity-type":"item","id":"$w1"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","rank":"normal"}]},"labels":{"fr":"$w1 is not a handle here"}}
{"op":"add","id":"WDQ42","claims":{"P50":[{"mainsnak":{"snaktype":"value","property":"P50","datavalue":{"value":{"entity-type":"item","id":"$w1"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","rank":"normal"}]}}
{"op":"override","statement":"OAW123$2F1C0000-0000-0000-0000-000000000000","rank":"deprecated"}
{"op":"retain","ids":["WDQ123","WDQ124"],"policy":"retain"}
{"op":"same-as","ids":["OAW123","$w1"]}
"#;

#[test]
fn a_batch_is_read_resolved_typed_completed_and_validated() {
    let mut batch = Batch::parse(BATCH).unwrap();
    assert_eq!(batch.job.source, "citation-batch");
    assert!(batch.job.atomic);
    assert_eq!(batch.ops.len(), 7);
    assert_eq!(batch.ops[2].line, 5, "blank lines count");
    assert_eq!(batch.refs().unwrap(), vec!["$w1", "$p1"]);
    assert!(matches!(
        batch.ops[2].typed(),
        Err(WireError::Unresolved { .. })
    ));

    let mut ids = BTreeMap::new();
    ids.insert("$w1".to_string(), id("Q900001"));
    ids.insert("$p1".to_string(), id("P900"));
    batch.resolve_refs(&ids).unwrap();
    let mut ops = batch.operations().unwrap();
    let order = ProviderOrder::default();
    let mut t = types();
    t.insert(id("P900"), DataType::WikibaseItem);
    let cx = local_context(&order, &t);
    let problems = validate_all(&mut ops, &cx);
    assert!(problems.is_empty(), "{problems:?}");

    match &ops[0] {
        Operation::Create {
            id: Some(i),
            temporary_ref: None,
            entity,
            ..
        } => {
            assert_eq!(i.as_str(), "Q900001");
            assert_eq!(entity.id.as_str(), "Q900001");
        }
        other => panic!("{other:?}"),
    }
    match &ops[2] {
        Operation::Add { claims, labels, .. } => {
            let (p, ss) = claims.first().unwrap();
            assert_eq!(p.as_str(), "P900");
            assert_eq!(ss[0].mainsnak.property.as_str(), "P900");
            assert_eq!(
                ss[0]
                    .mainsnak
                    .data_value()
                    .unwrap()
                    .entity_id()
                    .unwrap()
                    .as_str(),
                "Q900001"
            );
            assert_eq!(
                labels["fr"], "$w1 is not a handle here",
                "labels are not ID positions"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        ops[4].key().as_deref(),
        Some("OAW123"),
        "override completed from its statement"
    );
    assert_eq!(
        ops[6].key().as_deref(),
        Some("Q900001"),
        "the local member outranks OpenAlex"
    );
    let fanned: Vec<Operation> = ops
        .clone()
        .into_iter()
        .flat_map(Operation::fan_out)
        .collect();
    assert_eq!(fanned.len(), 8);
    assert!(fanned.iter().all(Operation::is_record_shape));
    assert_eq!(fanned[5].key().as_deref(), Some("WDQ123"));
    assert_eq!(fanned[6].key().as_deref(), Some("WDQ124"));

    // Written out again, the batch reads back to the same operations.
    let mut out = Vec::new();
    Batch::write(&mut out, &batch.job, &fanned).unwrap();
    let again = Batch::read(&out[..]).unwrap();
    assert_eq!(again.job, batch.job);
    assert_eq!(again.operations().unwrap(), fanned);
}

#[test]
fn wire_errors_name_their_line() {
    assert!(matches!(Batch::parse(""), Err(WireError::Empty)));
    assert!(matches!(
        Batch::parse("{\"op\":\"retain\"}"),
        Err(WireError::NoJob)
    ));
    assert!(matches!(Batch::parse("[1]"), Err(WireError::NoJob)));
    assert!(matches!(
        Batch::parse("{\"job\":{\"source\":\"x\",\"graph\":\"local\"}}\n{\"id\":\"Q1\"}"),
        Err(WireError::NotAnOperation { line: 2 })
    ));
    assert!(matches!(
        Batch::parse("{\"job\":{\"source\":\"x\",\"graph\":\"local\"}}\nnot json"),
        Err(WireError::Json { line: 2, .. })
    ));
    let dup = "{\"job\":{\"source\":\"x\",\"graph\":\"local\"}}\n{\"op\":\"create\",\"ref\":\"$w1\",\"entity\":{}}\n{\"op\":\"create\",\"ref\":\"$w1\",\"entity\":{}}";
    assert!(matches!(
        Batch::parse(dup).unwrap().refs(),
        Err(WireError::DuplicateRef { line: 3, .. })
    ));
    let undeclared = "{\"job\":{\"source\":\"x\",\"graph\":\"local\"}}\n{\"op\":\"retain\",\"ids\":[\"$w9\"],\"policy\":\"retain\"}";
    assert!(matches!(
        Batch::parse(undeclared).unwrap().refs(),
        Err(WireError::UndeclaredRef { line: 2, handle }) if handle == "$w9"
    ));
    let bad = "{\"job\":{\"source\":\"x\",\"graph\":\"local\"}}\n{\"op\":\"create\",\"ref\":\"$w 1\",\"entity\":{}}";
    assert!(matches!(
        Batch::parse(bad).unwrap().refs(),
        Err(WireError::BadHandle { line: 2, .. })
    ));
    let mut b = Batch::parse(dup).unwrap();
    assert!(matches!(
        b.resolve_refs(&BTreeMap::new()),
        Err(WireError::Unresolved { line: 2, .. })
    ));
}

// --- create-or-add ----------------------------------------------------------------------

#[test]
fn create_or_add_resolves_to_what_happened() {
    let op: Operation = serde_json::from_value(json!({"op": "create-or-add", "ref": "$w1",
        "match": {"P356": "10.1234/x"},
        "entity": item_entity("Q0", &json!({"P31": [statement("P31", &item("Q5"), None)]}))}))
    .unwrap();
    assert!(!op.is_record_shape());
    assert_eq!(op.key(), None);
    let key = MatchKey::Identifier(
        [(id("P356"), "10.1234/x".to_string())]
            .into_iter()
            .collect(),
    );

    let created = op
        .clone()
        .resolve_create_or_add(Resolution::Minted(id("Q900002")));
    match &created {
        Operation::Create {
            id: Some(i),
            entity,
            match_key: Some(m),
            via: Some(Via::CreateOrAdd),
            temporary_ref: None,
        } => {
            assert_eq!(i.as_str(), "Q900002");
            assert_eq!(entity.id.as_str(), "Q900002");
            assert_eq!(*m, key);
        }
        other => panic!("{other:?}"),
    }
    let merged = op
        .clone()
        .resolve_create_or_add(Resolution::Matched(id("Q5")));
    match &merged {
        Operation::Add {
            id: Some(i),
            claims,
            labels,
            entity: None,
            match_key: Some(_),
            via: Some(Via::CreateOrAdd),
            overwrite: false,
            ..
        } => {
            assert_eq!(i.as_str(), "Q5");
            assert_eq!(claims.len(), 1);
            assert_eq!(labels["en"], "label of Q0");
        }
        other => panic!("{other:?}"),
    }
    let mut over = op;
    if let Operation::CreateOrAdd { overwrite, .. } = &mut over {
        *overwrite = true;
    }
    let replaced = over.resolve_create_or_add(Resolution::Matched(id("Q5")));
    match &replaced {
        Operation::Add {
            entity: Some(e),
            overwrite: true,
            claims,
            ..
        } => {
            assert_eq!(e.id.as_str(), "Q5");
            assert!(claims.is_empty());
        }
        other => panic!("{other:?}"),
    }
    for r in [&created, &merged, &replaced] {
        assert!(r.is_record_shape());
        let text = serde_json::to_string(r).unwrap();
        assert!(
            text.contains(r#""via":"create-or-add""#)
                && text.contains(r#""match":{"P356":"10.1234/x"}"#)
        );
    }
    // A match key may be a foreign entity ID.
    let op: Operation = serde_json::from_value(json!({"op": "create-or-add", "match": "WDQ42",
        "entity": item_entity("Q0", &json!({}))}))
    .unwrap();
    assert!(
        matches!(op, Operation::CreateOrAdd { match_key: MatchKey::Entity(ref e), .. } if e.as_str() == "WDQ42")
    );
}

// --- validation -------------------------------------------------------------------------

fn op(v: Value) -> Operation {
    serde_json::from_value(v).unwrap()
}

#[test]
fn graph_and_mode_problems() {
    let put = op(
        json!({"op": "put", "id": "WDQ1", "entity": item_entity("WDQ1", &json!({})), "upstream": {"revid": 1}}),
    );
    assert!(
        problems(&put, "local", None).contains(&Problem::WrongGraph {
            op: "put",
            graph: "local".into()
        })
    );
    assert!(problems(&put, "mirror/wikidata", Some(Mode::Upsert)).is_empty());
    assert!(
        problems(&put, "mirror/openalex", Some(Mode::Upsert)).contains(&Problem::WrongProvider {
            id: id("WDQ1"),
            code: "OA".into()
        })
    );
    let create = op(json!({"op": "create", "id": "Q1", "entity": item_entity("Q1", &json!({}))}));
    assert!(
        problems(&create, "mirror/wikidata", Some(Mode::Upsert)).contains(&Problem::WrongGraph {
            op: "create",
            graph: "mirror/wikidata".into()
        })
    );
    assert!(
        problems(&create, "local", Some(Mode::Adopt))
            .contains(&Problem::WrongMode { op: "create" })
    );
    let adopt = op(
        json!({"op": "adopt", "id": "Q6", "entity": item_entity("Q6", &json!({})), "source_revid": 1, "source_time": "t", "source_pageid": 2}),
    );
    assert!(problems(&adopt, "local", None).contains(&Problem::WrongMode { op: "adopt" }));
    assert!(problems(&adopt, "local", Some(Mode::Adopt)).is_empty());
    let bad_adopt = op(
        json!({"op": "adopt", "id": "WDQ6", "entity": item_entity("WDQ6", &json!({})), "source_revid": 1, "source_time": "t", "source_pageid": 2}),
    );
    assert!(
        problems(&bad_adopt, "local", Some(Mode::Adopt)).contains(&Problem::NotLocal(id("WDQ6")))
    );
    let page_add = op(
        json!({"op": "add", "page": 12, "claims": {"P40": [statement("P40", &string("x"), None)]}}),
    );
    assert!(problems(&page_add, "pages", None).is_empty());
    assert!(
        problems(&page_add, "local", None).contains(&Problem::WrongGraph {
            op: "add",
            graph: "local".into()
        })
    );
    let entity_add = op(json!({"op": "add", "id": "Q1", "labels": {"en": "x"}}));
    assert!(
        problems(&entity_add, "pages", None).contains(&Problem::WrongGraph {
            op: "add",
            graph: "pages".into()
        })
    );
    let mirror_redirect =
        op(json!({"op": "redirect", "from": "WDQ1", "to": "WDQ2", "upstream": {"revid": 1}}));
    assert!(problems(&mirror_redirect, "mirror/wikidata", None).is_empty());
    assert!(
        problems(&mirror_redirect, "local", None).contains(&Problem::WrongGraph {
            op: "redirect",
            graph: "local".into()
        })
    );
    let mut job = JobHeader::local("x");
    job.graph = "mirror/nowhere".into();
    let order = ProviderOrder::default();
    let t = types();
    let cx = Context::for_job(&job, &order, &t).unwrap();
    assert_eq!(
        validate_all(&mut [], &cx),
        vec![(0, Problem::UnknownProvider("nowhere".into()))]
    );
    job.graph = "elsewhere".into();
    assert!(Context::for_job(&job, &order, &t).is_none());
}

#[test]
#[allow(clippy::too_many_lines)]
fn shape_problems() {
    let both = op(json!({"op": "add", "id": "Q1", "page": 2, "labels": {"en": "x"}}));
    assert!(problems(&both, "local", None).contains(&Problem::Subject));
    assert_eq!(both.key(), None);
    let neither = op(json!({"op": "remove", "statements": []}));
    assert_eq!(problems(&neither, "local", None), vec![Problem::Subject]);
    let page_terms = op(json!({"op": "add", "page": 2, "labels": {"en": "x"}}));
    assert!(problems(&page_terms, "pages", None).contains(&Problem::PageTerms));
    let empty = op(json!({"op": "add", "id": "Q1"}));
    assert!(problems(&empty, "local", None).contains(&Problem::Empty));
    let empty = op(json!({"op": "remove", "id": "Q1"}));
    assert!(problems(&empty, "local", None).contains(&Problem::Empty));

    let keyed = op(
        json!({"op": "create", "id": "Q1", "entity": {"type": "domain", "id": "domain:example.org", "claims": {}}}),
    );
    let p = problems(&keyed, "local", None);
    assert!(p.contains(&Problem::KeyedCreate(id("domain:example.org"))));
    assert!(p.contains(&Problem::IdMismatch {
        id: id("Q1"),
        entity: id("domain:example.org")
    }));
    let keyed_add = op(
        json!({"op": "add", "id": "domain:example.org", "claims": {"P40": [statement("P40", &string("x"), None)]}}),
    );
    assert!(
        problems(&keyed_add, "local", None).is_empty(),
        "a keyed entity takes add"
    );
    let wrong_type =
        op(json!({"op": "create", "id": "P1", "entity": item_entity("P1", &json!({}))}));
    assert!(
        problems(&wrong_type, "local", None).contains(&Problem::TypeMismatch {
            id: id("P1"),
            entity_type: "item".into()
        })
    );
    let unminted =
        op(json!({"op": "create", "ref": "$w1", "entity": item_entity("Q1", &json!({}))}));
    assert!(problems(&unminted, "local", None).contains(&Problem::WireShape));
    assert!(!unminted.is_record_shape());

    let tomb = op(json!({"op": "tombstone", "id": "domain:example.org", "upstream": {"revid": 1}}));
    assert!(
        problems(&tomb, "mirror/internetdomains", Some(Mode::Upsert))
            .contains(&Problem::KeyedOperation(id("domain:example.org")))
    );
    let keyed_put = op(
        json!({"op": "put", "id": "domain:example.org", "entity": {"type": "domain", "id": "domain:example.org", "claims": {}}, "upstream": {"revid": 1}}),
    );
    assert!(
        problems(&keyed_put, "mirror/internetdomains", Some(Mode::Upsert)).is_empty(),
        "an empty put clears a keyed mirror state"
    );
    let no_upstream = op(json!({"op": "tombstone", "id": "WDQ1", "upstream": {}}));
    assert!(
        problems(&no_upstream, "mirror/wikidata", Some(Mode::Upsert))
            .contains(&Problem::NoUpstream)
    );

    let over = op(
        json!({"op": "override", "id": "OAW1", "statement": "OAW1$2F1C0000-0000-0000-0000-000000000000", "rank": "normal", "suppress": true}),
    );
    assert!(problems(&over, "local", None).contains(&Problem::OverrideShape));
    let over = op(
        json!({"op": "override", "statement": "OAW1$2F1C0000-0000-0000-0000-000000000000", "suppress": false}),
    );
    assert!(problems(&over, "local", None).contains(&Problem::OverrideShape));
    let over = op(
        json!({"op": "override", "id": "Q1", "statement": "OAW1$2F1C0000-0000-0000-0000-000000000000", "suppress": true}),
    );
    assert!(matches!(
        problems(&over, "local", None)[..],
        [Problem::StatementSubject { .. }]
    ));

    let foreign_statement = op(
        json!({"op": "remove", "id": "Q5", "statements": ["Q6$A8C2D6D3-0000-0000-0000-000000000000"]}),
    );
    assert!(matches!(
        problems(&foreign_statement, "local", None)[..],
        [Problem::StatementSubject { .. }]
    ));
    let misfiled = op(
        json!({"op": "add", "id": "Q5", "claims": {"P31": [statement("P50", &item("Q7"), None)]}}),
    );
    assert!(
        problems(&misfiled, "local", None).contains(&Problem::PropertyGroup {
            group: id("P31"),
            property: id("P50")
        })
    );

    let bulk_retain = op(json!({"op": "retain", "ids": ["WDQ1"], "policy": "orphan"}));
    assert!(
        problems(&bulk_retain, "local", None).is_empty(),
        "bulk retain fans out later"
    );
    let no_retain = op(json!({"op": "retain", "policy": "orphan"}));
    assert!(problems(&no_retain, "local", None).contains(&Problem::WireShape));
    let convert = op(json!({"op": "convert", "id": "Q1", "local": "WDQ2"}));
    let p = problems(&convert, "local", None);
    assert!(
        p.contains(&Problem::NotForeign(id("Q1"))) && p.contains(&Problem::NotLocal(id("WDQ2")))
    );
    let convert = op(json!({"op": "convert", "id": "WDP1", "local": "Q2"}));
    assert!(
        problems(&convert, "local", None).contains(&Problem::TypeMismatch {
            id: id("Q2"),
            entity_type: "property".into()
        })
    );
}

#[test]
fn link_problems() {
    let ok = op(json!({"op": "same-as", "ids": ["OAW123", "Q456"]}));
    let mut c = ok.clone();
    c.complete(&ProviderOrder::default());
    assert_eq!(c.key().as_deref(), Some("Q456"));
    assert!(problems(&ok, "local", None).is_empty());
    let wrong_key = op(json!({"op": "same-as", "id": "OAW123", "ids": ["OAW123", "Q456"]}));
    assert_eq!(
        problems(&wrong_key, "local", None),
        vec![Problem::LinkKey {
            id: id("OAW123"),
            highest: id("Q456")
        }]
    );
    let not_member = op(json!({"op": "same-as", "id": "Q1", "ids": ["OAW123", "Q456"]}));
    assert_eq!(
        problems(&not_member, "local", None),
        vec![Problem::LinkShape]
    );
    let three = op(json!({"op": "same-as", "ids": ["OAW123", "Q456", "Q1"]}));
    assert!(problems(&three, "local", None).contains(&Problem::LinkShape));
    let same_ns = op(json!({"op": "same-as", "ids": ["Q1", "Q2"]}));
    assert!(problems(&same_ns, "local", None).contains(&Problem::LinkNamespace));
    let two_keyed = op(json!({"op": "same-as", "ids": ["domain:a.org", "domain:b.org"]}));
    assert!(problems(&two_keyed, "local", None).contains(&Problem::LinkNamespace));
    let keyed_and_local = op(json!({"op": "same-as", "ids": ["Q77", "domain:en.wikipedia.org"]}));
    let mut k = keyed_and_local.clone();
    k.complete(&ProviderOrder::default());
    assert_eq!(
        k.key().as_deref(),
        Some("domain:en.wikipedia.org"),
        "keyed ranks first"
    );
    assert!(problems(&keyed_and_local, "local", None).is_empty());
    let props_as_items = op(json!({"op": "same-as", "ids": ["P1", "WDP2"]}));
    assert!(problems(&props_as_items, "local", None).contains(&Problem::LinkTypes));
    let items_as_props = op(json!({"op": "equivalent-property", "ids": ["Q1", "WDQ2"]}));
    assert!(problems(&items_as_props, "local", None).contains(&Problem::LinkTypes));
    let removed = op(
        json!({"op": "remove", "id": "OAW123", "link": {"op": "same-as", "ids": ["Q456", "OAW123"]}}),
    );
    assert!(
        problems(&removed, "local", None).contains(&Problem::LinkKey {
            id: id("OAW123"),
            highest: id("Q456")
        })
    );

    let self_redirect = op(json!({"op": "redirect", "from": "Q1", "to": "Q1"}));
    assert!(problems(&self_redirect, "local", None).contains(&Problem::RedirectShape));
    let item_across = op(json!({"op": "redirect", "from": "Q1", "to": "WDQ2"}));
    assert!(problems(&item_across, "local", None).contains(&Problem::RedirectShape));
    let property_across = op(json!({"op": "redirect", "from": "P12", "to": "WDP585"}));
    assert!(problems(&property_across, "local", None).is_empty());
    let type_change = op(json!({"op": "redirect", "from": "P12", "to": "Q5"}));
    assert!(problems(&type_change, "local", None).contains(&Problem::RedirectShape));
}

#[test]
fn property_type_problems() {
    // P31 is an item property: a string value is the wrong value type.
    let wrong_value = op(
        json!({"op": "add", "id": "Q1", "claims": {"P31": [statement("P31", &string("x"), None)]}}),
    );
    let p = problems(&wrong_value, "local", None);
    assert!(
        p.contains(&Problem::Datatype {
            property: id("P31"),
            expected: DataType::WikibaseItem,
            found: DataType::String
        }),
        "{p:?}"
    );
    assert!(
        p.contains(&Problem::ValueType {
            property: id("P31"),
            datatype: DataType::WikibaseItem,
            value_type: "string".into()
        }),
        "{p:?}"
    );
    // The right value type but a property entity where an item is wanted.
    let wrong_entity = op(
        json!({"op": "add", "id": "Q1", "claims": {"P31": [statement("P31",
        &json!({"value": {"entity-type": "property", "id": "P5"}, "type": "wikibase-entityid"}), None)]}}),
    );
    assert!(
        problems(&wrong_entity, "local", None).contains(&Problem::EntityType {
            property: id("P31"),
            datatype: DataType::WikibaseItem,
            entity_type: "property".into()
        })
    );
    // An unknown property is fine unless strict; its own datatype is still checked against the value.
    let unknown = op(
        json!({"op": "add", "id": "Q1", "claims": {"P999": [statement("P999", &item("Q2"), None)]}}),
    );
    assert!(problems(&unknown, "local", None).is_empty());
    let order = ProviderOrder::default();
    let t = types();
    let mut cx = local_context(&order, &t);
    cx.strict_properties = true;
    assert_eq!(
        validate(&unknown, &cx),
        vec![Problem::UnknownProperty(id("P999"))]
    );
    let self_inconsistent = op(
        json!({"op": "add", "id": "Q1", "claims": {"P999": [{"mainsnak": {"snaktype": "value", "property": "P999", "datavalue": string("x"), "datatype": "quantity"}, "type": "statement", "rank": "normal"}]}}),
    );
    let cx = Context::for_job(&JobHeader::local("x"), &order, &NoProperties).unwrap();
    assert_eq!(
        validate(&self_inconsistent, &cx),
        vec![Problem::ValueType {
            property: id("P999"),
            datatype: DataType::Quantity,
            value_type: "string".into()
        }]
    );
    // Qualifiers and references are checked too, inside statements and when added by GUID.
    let qual = op(
        json!({"op": "add", "id": "Q1", "qualifiers": {"Q1$A8C2D6D3-0000-0000-0000-000000000000": {"P31": [{"snaktype": "value", "property": "P31", "datavalue": string("x"), "datatype": "string"}]}}}),
    );
    assert!(
        problems(&qual, "local", None)
            .iter()
            .any(|p| matches!(p, Problem::ValueType { .. }))
    );
    let somevalue = op(
        json!({"op": "add", "id": "Q1", "claims": {"P31": [{"mainsnak": {"snaktype": "somevalue", "property": "P31", "datatype": "wikibase-item"}, "type": "statement", "rank": "normal"}]}}),
    );
    assert!(problems(&somevalue, "local", None).is_empty());
    assert_eq!(Problem::Empty.to_string(), "the operation does nothing");
}

// --- the Wikibase adapter ---------------------------------------------------------------

#[test]
fn the_wikibase_adapter_rewrites_ids() {
    let wd = WikibaseAdapter::new("wikidata").unwrap();
    assert_eq!(wd.provider().code, "WD");
    assert_eq!(wd.rewrite_id("Q42").unwrap().as_str(), "WDQ42");
    assert_eq!(wd.rewrite_id("P31").unwrap().as_str(), "WDP31");
    assert_eq!(
        wd.rewrite_id("WDQ42").unwrap().as_str(),
        "WDQ42",
        "already stored"
    );
    assert_eq!(
        wd.rewrite_id("domain:example.org").unwrap().as_str(),
        "domain:example.org"
    );
    assert!(wd.rewrite_id("42").is_err());
    assert_eq!(
        wd.canonical_iri(&id("WDQ42")).as_deref(),
        Some("http://www.wikidata.org/entity/Q42")
    );
    assert_eq!(
        wd.canonical_iri(&id("domain:example.org")).as_deref(),
        Some("https://scatter.red/domain/example.org")
    );
    assert_eq!(wd.canonical_iri(&id("Q42")), None);
    assert!(wd.key_map().is_none());

    let mut e: Entity = serde_json::from_value(json!({"type": "item", "id": "Q42",
        "labels": {"en": {"language": "en", "value": "Douglas Adams"}},
        "claims": {"P31": [{"mainsnak": {"snaktype": "value", "property": "P31",
            "datavalue": item("Q5"), "datatype": "wikibase-item"},
            "type": "statement", "rank": "normal", "id": "Q42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9",
            "qualifiers": {"P50": [{"snaktype": "value", "property": "P50", "datavalue": item("Q7"), "datatype": "wikibase-item"}]},
            "qualifiers-order": ["P50"],
            "references": [{"snaks": {"P248": [{"snaktype": "value", "property": "P248", "datavalue": item("Q8"), "datatype": "wikibase-item"}]}, "snaks-order": ["P248"]}]}]},
        "sitelinks": {"enwiki": {"site": "enwiki", "title": "Douglas Adams", "badges": ["Q17437798"]}}}))
    .unwrap();
    wd.rewrite_entity(&mut e).unwrap();
    assert_eq!(e.id.as_str(), "WDQ42");
    let (p, ss) = e.statements.first().unwrap();
    assert_eq!(p.as_str(), "WDP31");
    let s = &ss[0];
    assert_eq!(
        s.id.as_ref().unwrap().as_str(),
        "WDQ42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"
    );
    assert_eq!(s.mainsnak.property.as_str(), "WDP31");
    assert_eq!(
        s.mainsnak
            .data_value()
            .unwrap()
            .entity_id()
            .unwrap()
            .as_str(),
        "WDQ5"
    );
    let (qp, qs) = s.qualifiers.first().unwrap();
    assert_eq!(qp.as_str(), "WDP50");
    assert_eq!(
        qs[0].data_value().unwrap().entity_id().unwrap().as_str(),
        "WDQ7"
    );
    let (rp, rs) = s.references[0].snaks.first().unwrap();
    assert_eq!(rp.as_str(), "WDP248");
    assert_eq!(
        rs[0].data_value().unwrap().entity_id().unwrap().as_str(),
        "WDQ8"
    );
    assert_eq!(e.sitelinks["enwiki"].badges[0].as_str(), "WDQ17437798");
    assert_eq!(e.labels["en"], "Douglas Adams");
    // Idempotent.
    let again = e.clone();
    wd.rewrite_entity(&mut e).unwrap();
    assert_eq!(e, again);
}

#[test]
fn adoption_mode_keeps_local_ids_and_maps_entity_sources() {
    let lb = WikibaseAdapter::new("librarybase")
        .unwrap()
        .home()
        .with_source("wikidata", "wd");
    assert_eq!(lb.rewrite_id("Q6").unwrap().as_str(), "Q6");
    assert_eq!(lb.rewrite_id("P12").unwrap().as_str(), "P12");
    assert_eq!(lb.rewrite_id("wikidata:Q42").unwrap().as_str(), "WDQ42");
    assert_eq!(lb.rewrite_id("wikidata:P31").unwrap().as_str(), "WDP31");
    assert_eq!(
        lb.rewrite_id("domain:example.org").unwrap().as_str(),
        "domain:example.org",
        "a keyed ID is not a source prefix"
    );
    let mut e: Entity = serde_json::from_value(json!({"type": "item", "id": "Q6",
        "claims": {"P12": [{"mainsnak": {"snaktype": "value", "property": "P12",
            "datavalue": item("wikidata:Q42"), "datatype": "wikibase-item"},
            "type": "statement", "rank": "normal", "id": "Q6$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"}]}}))
    .unwrap();
    lb.rewrite_entity(&mut e).unwrap();
    assert_eq!(e.id.as_str(), "Q6");
    let s = &e.statements.first().unwrap().1[0];
    assert_eq!(
        s.id.as_ref().unwrap().as_str(),
        "Q6$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"
    );
    assert_eq!(
        s.mainsnak
            .data_value()
            .unwrap()
            .entity_id()
            .unwrap()
            .as_str(),
        "WDQ42"
    );
}

#[test]
fn deterministic_statement_ids_are_stable_uuids() {
    let hasher = Hasher::mirrored_from("OA");
    let mut e: Entity = serde_json::from_value(json!({"type": "item", "id": "OAW123",
        "claims": {"P50": [statement("P50", &item("OAA1"), None), statement("P50", &item("OAA2"), None)],
                   "P356": [statement("P356", &string("10.1234/x"), None)]}}))
    .unwrap();
    assign_statement_ids(&mut e, &hasher);
    let ids: Vec<StatementId> = e.all_statements().map(|s| s.id.clone().unwrap()).collect();
    assert_eq!(ids.len(), 3);
    for sid in &ids {
        assert!(sid.as_str().starts_with("OAW123$"));
        let guid = sid.uuid();
        assert_eq!(guid.len(), 36);
        assert_eq!(&guid[14..15], "5", "version 5");
        assert!(
            matches!(&guid[19..20], "8" | "9" | "A" | "B"),
            "RFC 4122 variant: {guid}"
        );
        assert_eq!(guid, guid.to_uppercase());
    }
    assert_ne!(ids[0], ids[1], "different values, different IDs");
    // The same statement gets the same ID on a re-import, and an existing ID is kept.
    let first = e.all_statements().next().unwrap().clone();
    assert_eq!(deterministic_statement_id(&e.id, &first, &hasher), ids[0]);
    let before = ids.clone();
    assign_statement_ids(&mut e, &hasher);
    let after: Vec<StatementId> = e.all_statements().map(|s| s.id.clone().unwrap()).collect();
    assert_eq!(before, after);
    // Another subject, another ID.
    assert_ne!(
        deterministic_statement_id(&id("OAW124"), &first, &hasher),
        ids[0]
    );
}

struct Domains(EntityId);

impl KeyMap for Domains {
    fn keyed_type(&self) -> &'static str {
        "domain"
    }
    fn identity_property(&self) -> &EntityId {
        &self.0
    }
    fn key_of(&self, value: &str) -> Option<String> {
        let v = value.trim().to_ascii_lowercase();
        (!v.is_empty() && !v.contains(' ') && v.contains('.')).then_some(v)
    }
}

#[test]
fn a_key_map_maps_by_the_identity_property() {
    let map = Domains(id("P1"));
    let e = |values: &[&str]| -> Entity {
        let ss: Vec<Value> = values
            .iter()
            .map(|v| statement("P1", &string(v), None))
            .collect();
        serde_json::from_value(json!({"type": "item", "id": "Q9", "claims": {"P1": ss}})).unwrap()
    };
    assert_eq!(
        map.map(&e(&["Example.ORG"])).unwrap().unwrap().as_str(),
        "domain:example.org"
    );
    assert_eq!(
        map.map(&e(&[])).unwrap(),
        None,
        "an item without an identity value stays foreign"
    );
    assert!(matches!(
        map.map(&e(&["not a domain"])),
        Err(scatter_wikibase_changeset::AdapterError::InvalidKey { .. })
    ));
    assert!(matches!(
        map.map(&e(&["a.org", "b.org"])),
        Err(scatter_wikibase_changeset::AdapterError::MultipleKeys { count: 2 })
    ));
}

// --- changes and upstream ---------------------------------------------------------------

#[test]
fn a_put_carries_its_summary() {
    let old: Entity = serde_json::from_value(item_entity(
        "WDQ1",
        &json!({"P31": [statement("P31", &item("Q5"), None)]}),
    ))
    .unwrap();
    let mut new = old.clone();
    new.labels.insert("de".into(), "Beispiel".into());
    let changes = Changes::between(Some(&old), &new);
    let put = Operation::Put {
        id: id("WDQ1"),
        entity: new,
        upstream: Upstream::revision(7, Some("2026-09-30T12:00:00Z")),
        prev_upstream: Some(Upstream::revision(6, None)),
        first_seen: Some(1),
        size: Some(10),
        prev_size: Some(9),
        changes: Some(changes),
        delta: None,
    };
    let text = serde_json::to_string(&put).unwrap();
    assert!(text.contains(r#""changes":{"labels":{"de":{"added":1,"removed":0,"changed":0}}}"#));
    assert!(text.contains(r#""upstream":{"revid":7,"time":"2026-09-30T12:00:00Z"}"#));
    assert!(!text.contains("delta"));
    assert!(put.is_mirror());
    assert!(Upstream::version("2026-08-30").revid.is_none());
    assert!(Upstream::default().is_empty());
    let graph = Graph::parse("mirror/wikidata").unwrap();
    assert_eq!(graph.provider_slug(), Some("wikidata"));
    assert_eq!(
        serde_json::from_str::<Operation>(r#"{"op":"retain","id":"WDQ1","policy":"cascade"}"#)
            .unwrap(),
        Operation::Retain {
            id: Some(id("WDQ1")),
            ids: vec![],
            policy: Retention::Cascade
        }
    );
}
