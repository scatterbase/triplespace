//! Inferred links (0004 §3, tier 3; chapter 04 §4.3): two entities of different namespaces
//! that share a value of an identifier property the instance's `reconcile:default` record
//! names under `infer` are linked, where the value is unique in each namespace.
//!
//! The values are those of `view.identifier`'s shared rows, the normalized keys of each
//! mirrored entity's external identifiers (04 §4.7). The identity projection runs before
//! the entity projection writes them, so for the entities of the block it is applying it
//! keeps the values itself ([`Values`]): what each held as the block began, replaced by
//! what its record says as the record is applied. A record recomputes the links of every
//! value its entity holds or held: the namespaces with exactly one holder are linked to
//! each other, and the shared rows of `view.link` for the value (tier 3, `source`
//! `{property}={value key}`) are replaced where they differ.
//!
//! The registry ships no inference properties, so nothing is read until an instance names
//! one. A tenant may decline the links in its own view (0004 A29).

use std::collections::{BTreeMap, BTreeSet};

use scatter_log::record::Record;
use scatter_log_postgres::PgClient as _;
use scatter_wikibase_fusion::key::value_key;
use scatter_wikibase_ids::TriplespaceId as _;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::statement::SnakKind;
use scatter_wikibase_model::value::{DataType, DataValue};

use crate::backend::PgCx;
use crate::common::offset_db;
use crate::identity::namespace;

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// A value of an inference property: the property, as stored, and the normalized key.
pub(crate) type Value = (String, String);

/// The inference values of a block's entities, by entity: as `view.identifier` had them
/// when the block began, then as each record leaves them.
pub(crate) type Values = BTreeMap<String, BTreeSet<Value>>;

/// The values of the inference properties `infer` in a state, keyed as `view.identifier`
/// keys them.
#[must_use]
pub(crate) fn values_of(entity: &Entity, infer: &[String]) -> BTreeSet<Value> {
    entity
        .all_statements()
        .filter_map(|s| {
            let (Some(DataType::ExternalId), SnakKind::Value(v @ DataValue::String(_))) =
                (s.mainsnak.datatype.as_ref(), &s.mainsnak.kind)
            else {
                return None;
            };
            let property = s.mainsnak.property.as_str();
            infer.iter().any(|p| p == property).then(|| {
                let key = value_key(
                    v,
                    Some(&DataType::ExternalId),
                    &s.mainsnak.property,
                    crate::resolvers::bound(),
                );
                (property.to_string(), key.as_str().to_string())
            })
        })
        .collect()
}

/// What the entities `subjects` hold of the inference properties, from the shared rows of
/// `view.identifier`; every subject has an entry.
pub(crate) async fn held(
    cx: &PgCx,
    subjects: &[String],
    infer: &[String],
) -> Result<Values, String> {
    let mut out: Values = subjects
        .iter()
        .map(|s| (s.clone(), BTreeSet::new()))
        .collect();
    for r in cx
        .query(
            "SELECT entity_id, property, value_key FROM view.identifier
              WHERE tenant = '' AND entity_id = ANY($1) AND property = ANY($2)",
            &[&subjects, &infer],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        out.entry(r.get(0))
            .or_default()
            .insert((r.get(1), r.get(2)));
    }
    Ok(out)
}

/// `subject`'s record leaves it holding `new`. The inferred links of every value it holds
/// or held are recomputed; the IDs whose links changed are returned, for the clusters
/// around them to be recomputed.
pub(crate) async fn relink(
    cx: &PgCx,
    subject: &str,
    new: BTreeSet<Value>,
    values: &mut Values,
    record: &Record,
) -> Result<Vec<String>, String> {
    let old = values
        .insert(subject.to_string(), new.clone())
        .unwrap_or_default();
    let partition = scatter_log_postgres::ids::partition_to_db(record.header().partition);
    let offset = offset_db(record)?;
    let known: Vec<&String> = values.keys().collect();
    let mut seeds = Vec::new();
    for (property, key) in old.union(&new) {
        // The holders: the stored rows of the entities outside the block, and the
        // block's own as they stand.
        let mut holders: Vec<String> = cx
            .query(
                "SELECT DISTINCT entity_id FROM view.identifier
                  WHERE tenant = '' AND property = $1 AND value_key = $2 AND NOT (entity_id = ANY($3))",
                &[property, key, &known],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| r.get(0))
            .collect();
        let value = (property.clone(), key.clone());
        holders.extend(
            values
                .iter()
                .filter(|(_, held)| held.contains(&value))
                .map(|(id, _)| id.clone()),
        );
        let source = format!("{property}={key}");
        let links = links_of_value(&holders);
        let before: Vec<(String, String)> = cx
            .query(
                "SELECT a, b FROM view.link
                  WHERE tenant = '' AND tier = 3 AND source = $1 ORDER BY a, b",
                &[&source],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect();
        if before == links {
            continue;
        }
        cx.execute(
            "DELETE FROM view.link WHERE tenant = '' AND tier = 3 AND source = $1",
            &[&source],
        )
        .await
        .map_err(|e| sql(&e))?;
        for (a, b) in &links {
            cx.execute(
                "INSERT INTO view.link (tenant, a, b, tier, source, status, \"partition\", \"offset\")
                 VALUES ('', $1, $2, 3, $3, 'applied', $4, $5)
                 ON CONFLICT (tenant, a, b, tier, source) DO NOTHING",
                &[a, b, &source, &partition, &offset],
            )
            .await
            .map_err(|e| sql(&e))?;
        }
        seeds.extend(before.into_iter().chain(links).flat_map(|(a, b)| [a, b]));
    }
    Ok(seeds)
}

/// The links a value's holders make: each namespace with exactly one holder is eligible,
/// and every eligible holder is linked to the least of them, `(a, b)` with `a` the lesser
/// ID. A value that appears on more than one entity of a namespace links none of that
/// namespace's (04 §4.3), and a keyword or a notation is never linked (04 §4.1).
#[must_use]
pub fn links_of_value(holders: &[String]) -> Vec<(String, String)> {
    let mut by_namespace: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for id in holders.iter().collect::<BTreeSet<_>>() {
        by_namespace.entry(namespace(id)).or_default().push(id);
    }
    let names = |id: &str| {
        scatter_wikibase_model::id::EntityId::parse(id)
            .ok()
            .and_then(|e| {
                e.keyed_parts().and_then(|(name, _)| {
                    scatter_keyed::KeyedRegistry::default_registry()
                        .by_name(name)
                        .map(|t| !t.clusters)
                })
            })
            .unwrap_or(false)
    };
    let eligible: Vec<&String> = by_namespace
        .values()
        .filter(|ids| ids.len() == 1)
        .map(|ids| ids[0])
        .filter(|id| !names(id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let Some((first, rest)) = eligible.split_first() else {
        return Vec::new();
    };
    rest.iter()
        .map(|other| ((*first).clone(), (*other).clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    // 04 §4.3: a value that is unique in each namespace links its holders; one that
    // appears on two entities of a namespace links none of that namespace's.
    #[test]
    fn a_unique_value_links_its_holders() {
        assert_eq!(
            links_of_value(&ids(&["WDQ1", "OAW1"])),
            [("OAW1".to_string(), "WDQ1".to_string())]
        );
        assert!(links_of_value(&ids(&["WDQ1", "WDQ2", "OAW1"])).is_empty());
        assert_eq!(
            links_of_value(&ids(&["WDQ1", "WDQ2", "OAW1", "XDQ3"])),
            [("OAW1".to_string(), "XDQ3".to_string())]
        );
        assert!(links_of_value(&ids(&["WDQ1"])).is_empty());
        // Two entities of one namespace are a duplicate, never a link (04 §4.2).
        assert!(links_of_value(&ids(&["OAW1", "OAA1"])).is_empty());
    }
}
