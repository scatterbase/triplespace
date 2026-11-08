//! Mirrored statements named as their upstream names them (05 §1.5; 0002 A43): the
//! statement node under the provider's concept base, and its reference and value nodes
//! under the provider's base, hashed over the IDs as the provider writes them, so that each
//! node is the one the provider's own RDF has. Every other statement is the instance's.

use std::collections::BTreeSet;

use oxrdf::Triple;
use scatter_wikibase_model::{Entity, Hasher, StatementId};
use scatter_wikibase_rdf::{Flavor, IriScheme, Namespaces, Renderer};

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
    let iris = IriScheme::with_default_registries(Namespaces::triplespace("https://example.org"));
    let hasher = Hasher::local();
    let upstream: BTreeSet<StatementId> = [StatementId::parse(UPSTREAM).unwrap()].into();
    let triples = Renderer::new(&iris, &hasher, Flavor::Dump.produce())
        .with_upstream(&upstream)
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
