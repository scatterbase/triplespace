//! Mirrored statements named as their upstream names them (05 §1.5; 0002 A43): the
//! statement node under the provider's concept base, and its reference and value nodes
//! under the provider's base, hashed over the IDs as the provider writes them, so that each
//! node is the one the provider's own RDF has. Every other statement is the instance's.
//! Rendered by `scatter-wikibase-rdf` with the nodes `rdf::upstream_nodes` resolves (0115 §15).

use std::collections::BTreeMap;

use oxrdf::Triple;
use scatter_providers::{Registry, UpstreamRef};
use scatter_wikibase_ids::TriplespaceIris;
use scatter_wikibase_ids::rdf::upstream_nodes;
use scatter_wikibase_model::{Entity, Hasher, StatementId};
use scatter_wikibase_rdf::{Flavor, IriScheme, Namespaces, Renderer};

fn scheme(base: &str) -> IriScheme<'static> {
    IriScheme::new(
        Namespaces::with_wiki_path(base),
        TriplespaceIris::embedded(),
    )
}

const UPSTREAM: &str = "WDQ42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9";
const LOCAL: &str = "WDQ42$0D1A2B3C-0000-4000-8000-000000000001";

/// A statement with a qualifier carrying a time, and a reference naming another entity.
fn statement(guid: &str, p: &str, q: &str, prefix: &str) -> serde_json::Value {
    serde_json::json!({
        "mainsnak": {"snaktype": "value", "property": format!("{prefix}P31"), "datatype": "wikibase-item",
            "datavalue": {"value": {"entity-type": "item", "id": format!("{prefix}Q5")}, "type": "wikibase-entityid"}},
        "qualifiers": {format!("{prefix}P580"): [{"snaktype": "value", "property": format!("{prefix}P580"), "datatype": "time",
            "datavalue": {"value": {"time": "+1952-03-11T00:00:00Z", "timezone": 0, "before": 0, "after": 0,
                "precision": 11, "calendarmodel": "http://www.wikidata.org/entity/Q1985727"}, "type": "time"}}]},
        "references": [{"snaks": {p: [{"snaktype": "value", "property": p, "datatype": "wikibase-item",
            "datavalue": {"value": {"entity-type": "item", "id": q}, "type": "wikibase-entityid"}}]}}],
        "type": "statement", "rank": "normal", "id": guid,
    })
}

fn objects<'t>(triples: &'t [Triple], subject: &str, predicate_suffix: &str) -> Vec<&'t str> {
    triples
        .iter()
        .filter(|t| {
            t.subject.to_string() == format!("<{subject}>")
                && t.predicate.as_str().ends_with(predicate_suffix)
        })
        .filter_map(|t| match &t.object {
            oxrdf::Term::NamedNode(n) => Some(n.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn mirrored_statements_keep_upstream_nodes() {
    let entity = Entity::from_value(serde_json::json!({
        "type": "item", "id": "WDQ42",
        "claims": {"WDP31": [
            statement(UPSTREAM, "WDP248", "WDQ36578", "WD"),
            statement(LOCAL, "WDP248", "WDQ36579", "WD"),
        ]},
    }))
    .unwrap()
    .entity;
    let iris = scheme("https://example.org");
    let hasher = Hasher::local();
    let registry = Registry::default_registry();
    let upstream: BTreeMap<StatementId, UpstreamRef> = [(
        StatementId::parse(UPSTREAM).unwrap(),
        UpstreamRef::of(&registry.parse_foreign_id("WDQ42").unwrap()),
    )]
    .into();
    let triples = Renderer::new(&iris, &hasher, Flavor::Dump.produce())
        .with_upstream(&upstream_nodes(registry, &upstream))
        .entity(&entity, None);
    let concept = "http://www.wikidata.org/entity/Q42";

    // The statement node Wikidata's own RDF has.
    let claims = objects(&triples, concept, "/prop/P31");
    let wd_statement =
        "http://www.wikidata.org/entity/statement/Q42-F078E5B3-F9A8-480E-B7AC-D97778CBBEF9";
    assert!(claims.contains(&wd_statement), "{claims:?}");
    let local_statement =
        "https://example.org/entity/statement/WDQ42-0D1A2B3C-0000-4000-8000-000000000001";
    assert!(claims.contains(&local_statement), "{claims:?}");

    // Its reference node is Wikidata's: named by the hash of the reference as Wikidata
    // writes it (P248 → Q36578), under Wikidata's base.
    let bare = Entity::from_value(serde_json::json!({
        "type": "item", "id": "Q42",
        "claims": {"P31": [statement("Q42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9", "P248", "Q36578", "")]},
    }))
    .unwrap()
    .entity;
    let bare_ref = &bare.all_statements().next().unwrap().references[0];
    let expected_ref = format!(
        "http://www.wikidata.org/reference/{}",
        Hasher::local().reference(bare_ref)
    );
    assert_eq!(
        objects(&triples, wd_statement, "#wasDerivedFrom"),
        vec![expected_ref.as_str()]
    );
    // And its qualifier's value node is under Wikidata's base.
    let value = objects(&triples, wd_statement, "/prop/qualifier/value/P580");
    assert!(
        value[0].starts_with("http://www.wikidata.org/value/"),
        "{value:?}"
    );

    // The instance's statement keeps the instance's nodes.
    let local_ref = objects(&triples, local_statement, "#wasDerivedFrom");
    assert!(
        local_ref[0].starts_with("https://example.org/reference/"),
        "{local_ref:?}"
    );
    let local_value = objects(&triples, local_statement, "/prop/qualifier/value/P580");
    assert!(
        local_value[0].starts_with("https://example.org/value/"),
        "{local_value:?}"
    );
    // Upstream's predicates for both: the property is Wikidata's either way.
    assert!(objects(&triples, concept, "/prop/P31").len() == 2);
}

/// A key-mapped provider's statement is stored under the keyed entity's ID with the
/// upstream statement's UUID; given the item that was mapped onto the entity, it is named
/// as the provider names it, statement node and reference node alike (05 §1.5; 0002 A44).
#[test]
fn key_mapped_statements_keep_the_items_upstream_nodes() {
    const MAPPED: &str = "domain:wikipedia.org$00000000-0000-0000-0000-0000000000B2";
    const UNMAPPED: &str = "domain:example.net$00000000-0000-0000-0000-0000000000C1";
    let keyed = |id: &str, guid: &str| {
        Entity::from_value(serde_json::json!({
            "type": "domain", "id": id,
            "claims": {"XDP31": [statement(guid, "XDP3", "XDQ7", "XD")]},
        }))
        .unwrap()
        .entity
    };
    let iris = scheme("https://example.org");
    let hasher = Hasher::local();
    let registry = Registry::default_registry();
    let xd = registry.by_code("XD").unwrap();
    // The statement of `domain:wikipedia.org` came from the provider's Q11; the entity's
    // ID says nothing of that, so the caller says it.
    let upstream: BTreeMap<StatementId, UpstreamRef> = [(
        StatementId::parse(MAPPED).unwrap(),
        UpstreamRef::mapped(xd, "Q11").unwrap(),
    )]
    .into();
    let nodes = upstream_nodes(registry, &upstream);
    let renderer = Renderer::new(&iris, &hasher, Flavor::Dump.produce()).with_upstream(&nodes);

    let triples = renderer.entity(&keyed("domain:wikipedia.org", MAPPED), None);
    let concept = "https://scatter.red/domain/wikipedia.org";
    let xd_statement =
        "https://internetdomains.wiki/entity/statement/Q11-00000000-0000-0000-0000-0000000000B2";
    assert_eq!(
        objects(&triples, concept, "/prop/P31"),
        vec![xd_statement],
        "the statement node the provider's own RDF has"
    );
    let reference = objects(&triples, xd_statement, "#wasDerivedFrom");
    assert!(
        reference[0].starts_with("https://internetdomains.wiki/reference/"),
        "{reference:?}"
    );
    let value = objects(&triples, xd_statement, "/prop/qualifier/value/P580");
    assert!(
        value[0].starts_with("https://internetdomains.wiki/value/"),
        "{value:?}"
    );

    // A keyed entity's statement the caller gives no item for is the instance's.
    let triples = renderer.entity(&keyed("domain:example.net", UNMAPPED), None);
    assert_eq!(
        objects(
            &triples,
            "https://scatter.red/domain/example.net",
            "/prop/P31"
        ),
        vec![
            "https://example.org/entity/statement/domain:example.net-00000000-0000-0000-0000-0000000000C1"
        ]
    );
}

/// A mirrored entity as a dump renders it for the tenant at `base`: the tenant's
/// namespaces, and value hashing over the tenant's entity base.
fn shared_lines(base: &str) -> Vec<String> {
    let entity = Entity::from_value(serde_json::json!({
        "type": "item", "id": "WDQ42",
        "labels": {"en": {"language": "en", "value": "Douglas Adams"}},
        "sitelinks": {"enwiki": {"site": "enwiki", "title": "Douglas Adams", "badges": ["WDQ17437796"]}},
        "claims": {"WDP31": [statement(UPSTREAM, "WDP248", "WDQ36578", "WD")]},
    }))
    .unwrap()
    .entity;
    let registry = Registry::default_registry();
    let iris = scheme(base);
    let hasher = Hasher::local().with_iris(std::sync::Arc::new(
        scatter_wikibase_ids::value_iri::ValueIris::new(
            registry,
            Some(&iris.namespaces().entity()),
        ),
    ));
    let upstream: BTreeMap<StatementId, UpstreamRef> = [(
        StatementId::parse(UPSTREAM).unwrap(),
        UpstreamRef::of(&registry.parse_foreign_id("WDQ42").unwrap()),
    )]
    .into();
    let page = scatter_wikibase_model::PageInfo {
        lastrevid: Some(7),
        modified: Some("2026-11-22T00:00:00Z".into()),
        ..Default::default()
    };
    let triples = Renderer::new(&iris, &hasher, Flavor::Dump.produce())
        .with_upstream(&upstream_nodes(registry, &upstream))
        .entity(&entity, Some(&page));
    scatter_wikibase_rdf::canonical_lines(&triples)
}

// 08 §10.5: a shared-row delta is stored once and read by every tenant that reads its
// provider. That holds only if a mirrored entity's rendering depends on the reading tenant
// through its base alone: rendered under the reserved base and given a tenant's base in
// its place, it is byte for byte what that tenant's dump holds.
#[test]
fn a_mirrored_entity_differs_between_tenants_by_base_only() {
    const RESERVED: &str = "https://tenant.invalid/";
    let shared = shared_lines(RESERVED);
    assert!(
        shared.iter().any(|l| l.contains(RESERVED)),
        "the document node is the tenant's"
    );
    for base in ["https://librarybase.org/", "https://other.example/"] {
        let mut substituted: Vec<String> = shared
            .iter()
            .map(|l| l.replace(&format!("<{RESERVED}"), &format!("<{base}")))
            .collect();
        substituted.sort_unstable();
        assert_eq!(substituted, shared_lines(base), "{base}");
    }
}
