//! An entity as `wbgetentities` and `Special:EntityData` serve it (wikibase-compat §3.1;
//! 0009 §4–5; 0017 §1): the Wikibase form with `pageid`, `ns`, `title`, `lastrevid` and
//! `modified`, a Domain's derived `mul` label, and the `props` and `languages` filters.

use scatter_providers::Registry;
use scatter_wikibase_model::entity::{Entity, EntityType, PageInfo};
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde_json::{Map, Value, json};
use triplespace_projections::read::Current;

/// The namespace number and prefixed title of an entity (namespaces.toml; 0017 §1). A
/// source's entity (`mhc:Q1`, 0078 §2) is in the namespace of its entity type, as a
/// foreign one is: `Item:mhc:Q1`.
#[must_use]
pub fn title_of(id: &EntityId, entity_type: &EntityType) -> (i64, String) {
    if let Some((t, key)) = id.keyed_parts() {
        match t {
            "domain" => return (210, format!("Domain:{key}")),
            "keyword" => return (212, format!("Keyword:{key}")),
            "notation" => return (216, format!("Notation:{key}")),
            _ => {}
        }
    }
    match entity_type {
        EntityType::Property => (122, format!("Property:{}", id.as_str())),
        _ => (120, format!("Item:{}", id.as_str())),
    }
}

pub use triplespace_projections::read::iso8601;

/// The hasher for an entity's snak and reference hashes: the provider's for a foreign
/// ID, the local one otherwise.
#[must_use]
pub fn hasher_for(id: &EntityId, registry: &Registry) -> Hasher {
    if id.form() != IdForm::Local
        && let Ok(f) = registry.parse_foreign_id(id.as_str())
    {
        return Hasher::mirrored_from(&f.provider().code);
    }
    Hasher::local()
}

/// Adds a keyed entity's derived label and alias (0009 §5); now in the projections, so
/// that `Special:EntityData` and the dumps serve the same terms.
pub use triplespace_projections::read::add_derived_terms;

/// The served JSON of a present entity.
#[must_use]
pub fn present(current: &Current, registry: &Registry) -> Value {
    let (entity, page) = served(current);
    let text = entity.to_wikibase_json(&hasher_for(&entity.id, registry), Some(&page));
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// The entity as it is served, with its derived terms, and its page information: what
/// [`present`] writes as JSON and `Special:EntityData` renders as RDF.
#[must_use]
pub fn served(current: &Current) -> (Entity, PageInfo) {
    let mut entity = current.resolved.entity.clone();
    add_derived_terms(&mut entity);
    let (ns, title) = title_of(&entity.id, &entity.entity_type);
    let page = PageInfo {
        pageid: Some(current.page_id),
        ns: Some(ns),
        title: Some(title),
        lastrevid: current.lastrevid,
        modified: Some(iso8601(current.modified)),
    };
    (entity, page)
}

/// The statements of the resolved view named as their upstream names them (0002 A43).
#[must_use]
pub fn upstream_statements(
    current: &Current,
    registry: &Registry,
) -> std::collections::BTreeSet<scatter_wikibase_model::StatementId> {
    current.upstream_statements(registry)
}

/// The ID as the tenant's table of names canonicalizes it (0078 §2): a source ID's rest
/// in its type's grammar (`opencredit:q8` → `opencredit:Q8`); any other ID unchanged.
#[must_use]
pub fn canonical_in(id: EntityId, registry: &Registry) -> EntityId {
    if id.form() == IdForm::Keyed
        && let Ok(f) = registry.parse_foreign_id(id.as_str())
        && let Ok(c) = EntityId::parse(&f.to_string())
    {
        return c;
    }
    id
}

/// Whether the ID is an entity source's (0078 §2) in this table of names.
#[must_use]
pub fn is_source_id(id: &EntityId, registry: &Registry) -> bool {
    id.form() == IdForm::Keyed
        && registry
            .parse_foreign_id(id.as_str())
            .is_ok_and(|f| f.provider().source)
}

/// The served JSON of a valid keyed ID no graph holds (0009 §4): its derived label and
/// nothing else.
#[must_use]
pub fn empty_keyed(id: &EntityId) -> Value {
    let t = id.keyed_parts().map_or("domain", |(t, _)| t);
    let mut entity = Entity::new(id.clone(), EntityType::parse(t));
    add_derived_terms(&mut entity);
    let (ns, title) = title_of(&entity.id, &entity.entity_type);
    let mut v: Value = serde_json::from_str(&entity.to_wikibase_json(&Hasher::local(), None))
        .unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("ns".into(), json!(ns));
        m.insert("title".into(), json!(title));
    }
    v
}

/// Keeps only the requested `props` and `languages` (compat §4.1). `info` keeps the page
/// fields; `type` and `id` always stay.
pub fn filter(value: &mut Value, props: &[String], languages: &[String]) {
    let Value::Object(m) = value else { return };
    if !props.is_empty() {
        let keep = |k: &str| match k {
            "type" | "id" | "redirects" => true,
            "pageid" | "ns" | "title" | "lastrevid" | "modified" | "missing" => {
                props.iter().any(|p| p == "info")
            }
            "claims" => props.iter().any(|p| p == "claims"),
            "datatype" => props.iter().any(|p| p == "datatype"),
            other => props.iter().any(|p| p == other),
        };
        m.retain(|k, _| keep(k));
    }
    if !languages.is_empty() {
        for key in ["labels", "descriptions", "aliases"] {
            if let Some(Value::Object(terms)) = m.get_mut(key) {
                terms.retain(|lang, _| languages.iter().any(|l| l == lang));
            }
        }
    }
}

/// The `entities` object of a response, keyed by the ID as given.
#[must_use]
pub fn entities_object(entries: Vec<(String, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in entries {
        m.insert(k, v);
    }
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which statements are named as their upstream names them (0002 A43): a statement only
    /// a mirror asserts is; one the local graph also asserts, one the local graph alone
    /// asserts, and a mirrored one under a local rank override are the instance's.
    #[test]
    fn upstream_statements_are_the_mirror_only_and_uncorrected() {
        use scatter_wikibase_model::{Rank, StatementId};
        use scatter_wikibase_resolve::{Contribution, Correction, resolve};
        use std::collections::BTreeMap;

        let statement = |guid: &str, value: &str| {
            json!({"mainsnak": {"snaktype": "value", "property": "WDP1", "datatype": "string",
                "datavalue": {"value": value, "type": "string"}},
                "type": "statement", "rank": "normal", "id": guid})
        };
        let entity = |claims: Vec<Value>| {
            Entity::from_value(json!({"type": "item", "id": "WDQ42", "claims": {"WDP1": claims}}))
                .unwrap()
                .entity
        };
        let only_mirror = "WDQ42$00000000-0000-0000-0000-000000000001";
        let fused_mirror = "WDQ42$00000000-0000-0000-0000-000000000002";
        let fused_local = "WDQ42$00000000-0000-0000-0000-000000000003";
        let only_local = "WDQ42$00000000-0000-0000-0000-000000000004";
        let corrected = "WDQ42$00000000-0000-0000-0000-000000000005";
        let contributions = vec![
            Contribution {
                graph: "local".into(),
                entity: entity(vec![
                    statement(fused_local, "both"),
                    statement(only_local, "ours"),
                ]),
            },
            Contribution {
                graph: "mirror/wikidata".into(),
                entity: entity(vec![
                    statement(only_mirror, "theirs"),
                    statement(fused_mirror, "both"),
                    statement(corrected, "ranked here"),
                ]),
            },
        ];
        let corrections: BTreeMap<StatementId, Correction> = BTreeMap::from([(
            StatementId::parse(corrected).unwrap(),
            Correction::Rank(Rank::Preferred),
        )]);
        let current = Current {
            resolved: resolve(&contributions, &corrections).unwrap(),
            contributions,
            corrections,
            page_id: 1,
            lastrevid: None,
            modified: 0,
            local_offset: None,
            local_revid: None,
        };
        let upstream: Vec<String> = upstream_statements(&current, Registry::default_registry())
            .iter()
            .map(|s| s.as_str().to_string())
            .collect();
        assert_eq!(upstream, vec![only_mirror.to_string()]);
    }

    #[test]
    fn times_and_titles() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_790_000_000_000_000), "2026-09-21T14:13:20Z");
        let q = EntityId::parse("Q6").unwrap();
        assert_eq!(title_of(&q, &EntityType::Item), (120, "Item:Q6".into()));
        let d = EntityId::parse("domain:xn--bcher-kva.example").unwrap();
        assert_eq!(
            title_of(&d, &EntityType::Domain),
            (210, "Domain:xn--bcher-kva.example".into())
        );
        let v = empty_keyed(&d);
        assert_eq!(v["labels"]["mul"]["value"], "bücher.example");
        assert_eq!(v["aliases"]["mul"][0]["value"], "xn--bcher-kva.example");
        assert_eq!(v["title"], "Domain:xn--bcher-kva.example");
        assert!(v.get("pageid").is_none());
    }

    #[test]
    fn filters() {
        let mut v = json!({"type": "item", "id": "Q6", "pageid": 1, "lastrevid": 2,
            "labels": {"en": {"language": "en", "value": "a"}, "de": {"language": "de", "value": "b"}},
            "claims": {}});
        filter(&mut v, &["labels".into()], &["de".into()]);
        assert_eq!(
            v,
            json!({"type": "item", "id": "Q6", "labels": {"de": {"language": "de", "value": "b"}}})
        );
    }
}
