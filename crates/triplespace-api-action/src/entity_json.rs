//! An entity as `wbgetentities` and `Special:EntityData` serve it (wikibase-compat §3.1;
//! 0009 §4–5; 0017 §1): the Wikibase form with `pageid`, `ns`, `title`, `lastrevid` and
//! `modified`, a Domain's derived `mul` label, and the `props` and `languages` filters.

use scatter_normalize::keyed::KeyedRegistry;
use scatter_providers::Registry;
use scatter_wikibase_model::entity::{Entity, EntityType, PageInfo};
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde_json::{Map, Value, json};
use triplespace_projections::read::Current;

/// The namespace number and prefixed title of an entity (namespaces.toml; 0017 §1).
#[must_use]
pub fn title_of(id: &EntityId, entity_type: &EntityType) -> (i64, String) {
    if let Some((t, key)) = id.keyed_parts() {
        return match t {
            "domain" => (210, format!("Domain:{key}")),
            "keyword" => (212, format!("Keyword:{key}")),
            "notation" => (216, format!("Notation:{key}")),
            _ => (120, id.as_str().to_string()),
        };
    }
    match entity_type {
        EntityType::Property => (122, format!("Property:{}", id.as_str())),
        _ => (120, format!("Item:{}", id.as_str())),
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` from microseconds since the epoch.
#[must_use]
pub fn iso8601(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// The hasher for an entity's snak and reference hashes: the provider's for a foreign
/// ID, the local one otherwise.
#[must_use]
pub fn hasher_for(id: &EntityId, registry: &Registry) -> Hasher {
    if id.form() == IdForm::Foreign
        && let Ok(f) = registry.parse_foreign_id(id.as_str())
    {
        return Hasher::mirrored_from(&f.provider().code);
    }
    Hasher::local()
}

/// Adds a keyed entity's derived label and alias (0009 §5) where the resolved state has
/// no `mul` label of its own.
pub fn add_derived_terms(entity: &mut Entity) {
    let Some((t, key)) = entity
        .id
        .keyed_parts()
        .map(|(t, k)| (t.to_string(), k.to_string()))
    else {
        return;
    };
    let Some(kt) = KeyedRegistry::default_registry().by_name(&t) else {
        return;
    };
    let Some(label) = kt.derived_label(&key) else {
        return;
    };
    if !entity.labels.contains_key("mul") {
        entity.labels.insert("mul".into(), label.clone());
    }
    if label != key {
        let aliases = entity.aliases.entry("mul".into()).or_default();
        if !aliases.contains(&key) {
            aliases.push(key);
        }
    }
}

/// The served JSON of a present entity.
#[must_use]
pub fn present(current: &Current, registry: &Registry) -> Value {
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
    let text = entity.to_wikibase_json(&hasher_for(&entity.id, registry), Some(&page));
    serde_json::from_str(&text).unwrap_or(Value::Null)
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
            "type" | "id" => true,
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
