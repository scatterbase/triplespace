//! Statement keys (0004 §7–8; 0002 §8.5): what the resolved view fuses statements by.
//!
//! A statement's **key** is a hash of its main snak and its qualifiers, in any order,
//! each in canonical form: the property and every entity value rewritten to the canonical
//! ID of its cluster (0004 §4), and every literal value reduced to its normalized key
//! (0004 §7, [`scatter_normalize::ValueKey`]). Statements with the same key fuse into one
//! in the resolved view; references are combined, with duplicates removed by the
//! reference hash ([`crate::hash::Hasher::reference`]).
//!
//! What "canonical" means is the resolved view's business, not this crate's: cluster
//! membership, a property's resolver binding and the tenant's URL schemes are passed in
//! through [`Canon`]. [`Identity`] is the trivial one, for a graph read on its own.
//!
//! The hash is SHA-256 over a text preimage ([`statement_key_input`]), in hex. It is a
//! view key, not a log hash, so it carries no 0006 §2 domain tag; the preimage starts with
//! a version marker instead, so that a change to the canonical form changes every key.

use std::fmt::Write as _;

use scatter_normalize::{Resolver, ValueKey};
use sha2::Digest as _;

use crate::id::EntityId;
use crate::statement::{Snak, SnakKind, Statement};
use crate::value::{DataType, DataValue};

/// Separates the parts of a snak key.
const UNIT: char = '\u{1F}';
/// Separates the snak keys of a statement key.
const RECORD: char = '\u{1E}';
/// Changes when the canonical form changes.
const VERSION: &str = "scatter:statement-key/1";

/// What the resolved view knows that the canonical form needs.
pub trait Canon {
    /// The canonical ID of the cluster `id` belongs to; `id` itself when it is canonical
    /// or in no cluster.
    fn canonical_id(&self, id: &EntityId) -> EntityId;

    /// The resolver bound to the property's role, for `external-id` values (0029 §1).
    fn resolver_for(&self, _property: &EntityId) -> Option<&Resolver> {
        None
    }

    /// The URL schemes the tenant allows, for `url` values (0026 §1).
    fn url_schemes(&self) -> &[&str] {
        &["http", "https"]
    }
}

/// The identity canon: every ID is its own canonical ID, no resolvers.
#[derive(Debug, Clone, Copy, Default)]
pub struct Identity;

impl Canon for Identity {
    fn canonical_id(&self, id: &EntityId) -> EntityId {
        id.clone()
    }
}

/// The normalized key of a value under its data type (0004 §7). The data type is the
/// one the snak carries; without one, the value type decides, which gives the right key
/// for everything but `external-id` and `url`, which are strings to the value type.
#[must_use]
pub fn value_key(
    value: &DataValue,
    datatype: Option<&DataType>,
    property: &EntityId,
    canon: &dyn Canon,
) -> ValueKey {
    match value {
        DataValue::EntityId(e) => ValueKey::exact("entity", canon.canonical_id(&e.id).as_str()),
        DataValue::String(s) => match datatype {
            Some(DataType::ExternalId) => ValueKey::external_id(s, canon.resolver_for(property))
                .unwrap_or_else(|_| ValueKey::external_id(s, None).expect("no resolver")),
            Some(DataType::Url) => {
                ValueKey::url(s, canon.url_schemes()).unwrap_or_else(|_| ValueKey::exact("url", s))
            }
            Some(DataType::String) | None => ValueKey::string(s),
            Some(other) => ValueKey::exact(other.id(), s),
        },
        DataValue::MonolingualText(m) => ValueKey::monolingualtext(&m.text, &m.language),
        DataValue::Time(t) => ValueKey::time(&t.time, t.precision, &t.calendarmodel),
        DataValue::Quantity(q) => ValueKey::quantity(
            &q.amount,
            &q.unit,
            q.lower_bound.as_deref(),
            q.upper_bound.as_deref(),
        ),
        DataValue::GlobeCoordinate(_) | DataValue::Unknown { .. } => {
            // Everything else: the exact value, as its canonical JSON.
            ValueKey::exact(
                value.value_type_name(),
                &crate::json::canonical(&value.to_wire(false)),
            )
        }
    }
}

/// A snak in canonical form: the canonical property, the snak type, and the value key.
#[must_use]
pub fn snak_key(snak: &Snak, canon: &dyn Canon) -> String {
    let property = canon.canonical_id(&snak.property);
    let mut s = format!("{property}{UNIT}{}", snak.kind.snaktype());
    if let SnakKind::Value(v) = &snak.kind {
        s.push(UNIT);
        s.push_str(value_key(v, snak.datatype.as_ref(), &snak.property, canon).as_str());
    }
    s
}

/// The preimage of a statement's key: the version marker, the main snak key, and the
/// qualifier snak keys sorted, so that qualifier order does not matter (0004 §8).
#[must_use]
pub fn statement_key_input(statement: &Statement, canon: &dyn Canon) -> String {
    let mut qualifiers: Vec<String> = statement
        .qualifiers
        .values()
        .flatten()
        .map(|q| snak_key(q, canon))
        .collect();
    qualifiers.sort();
    let mut s = format!("{VERSION}{RECORD}{}", snak_key(&statement.mainsnak, canon));
    for q in qualifiers {
        s.push(RECORD);
        s.push_str(&q);
    }
    s
}

/// A statement's key (0004 §8): SHA-256 of [`statement_key_input`], in hex.
#[must_use]
pub fn statement_key(statement: &Statement, canon: &dyn Canon) -> String {
    let digest = sha2::Sha256::digest(statement_key_input(statement, canon).as_bytes());
    digest.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statement(json: &str) -> Statement {
        serde_json::from_str(json).unwrap()
    }

    /// `LBQ1` is a non-canonical member of `Q7`'s cluster; `WDP31` of `P1`'s.
    struct Clusters;
    impl Canon for Clusters {
        fn canonical_id(&self, id: &EntityId) -> EntityId {
            match id.as_str() {
                "LBQ1" => EntityId::parse("Q7").unwrap(),
                "WDP31" => EntityId::parse("P1").unwrap(),
                _ => id.clone(),
            }
        }
    }

    #[test]
    fn qualifier_order_does_not_matter_and_values_are_normalized() {
        // `Noe\u{308}l` (decomposed) and `No\u{eb}l` (composed) are one value under NFC.
        let decomposed = "Noe\u{308}l";
        let a = statement(&format!(
            r#"{{"mainsnak":{{"snaktype":"value","property":"P1","datatype":"string","datavalue":{{"value":"{decomposed}","type":"string"}}}},
                "qualifiers":{{"P2":[{{"snaktype":"novalue","property":"P2"}}],"P3":[{{"snaktype":"somevalue","property":"P3"}}]}},
                "qualifiers-order":["P2","P3"],"rank":"normal"}}"#
        ));
        let b = statement(
            r#"{"mainsnak":{"snaktype":"value","property":"P1","datatype":"string","datavalue":{"value":"Noël","type":"string"}},
                "qualifiers":{"P3":[{"snaktype":"somevalue","property":"P3"}],"P2":[{"snaktype":"novalue","property":"P2"}]},
                "qualifiers-order":["P3","P2"],"rank":"preferred",
                "references":[{"snaks":{"P8":[{"snaktype":"novalue","property":"P8"}]},"snaks-order":["P8"]}]}"#,
        );
        assert_eq!(statement_key(&a, &Identity), statement_key(&b, &Identity));
        assert_eq!(statement_key(&a, &Identity).len(), 64);
        // A qualifier more, and the key changes.
        let mut c = a.clone();
        c.qualifiers
            .entry(EntityId::parse("P9").unwrap())
            .or_default()
            .push(a.mainsnak.clone());
        assert_ne!(statement_key(&a, &Identity), statement_key(&c, &Identity));
    }

    #[test]
    fn canonical_ids_fuse_across_graphs() {
        let local = statement(
            r#"{"mainsnak":{"snaktype":"value","property":"P1","datatype":"wikibase-item","datavalue":{"value":{"entity-type":"item","id":"Q7"},"type":"wikibase-entityid"}},"rank":"normal"}"#,
        );
        let mirrored = statement(
            r#"{"mainsnak":{"snaktype":"value","property":"WDP31","datatype":"wikibase-item","datavalue":{"value":{"entity-type":"item","id":"LBQ1"},"type":"wikibase-entityid"}},"rank":"normal"}"#,
        );
        assert_ne!(
            statement_key(&local, &Identity),
            statement_key(&mirrored, &Identity)
        );
        assert_eq!(
            statement_key(&local, &Clusters),
            statement_key(&mirrored, &Clusters)
        );
        assert_eq!(
            snak_key(&mirrored.mainsnak, &Clusters),
            "P1\u{1F}value\u{1F}entity\u{1F}Q7"
        );
    }

    #[test]
    fn data_types_pick_the_normalizer() {
        let url = statement(
            r#"{"mainsnak":{"snaktype":"value","property":"P8","datatype":"url","datavalue":{"value":"HTTP://Example.org/a/../b","type":"string"}},"rank":"normal"}"#,
        );
        assert!(
            snak_key(&url.mainsnak, &Identity).ends_with("url\u{1F}http://example.org/b"),
            "{}",
            snak_key(&url.mainsnak, &Identity)
        );
        // Time keeps its precision and calendar; a coarser time is another value (0004 §7).
        let t = |p: u8| {
            statement(&format!(
                r#"{{"mainsnak":{{"snaktype":"value","property":"P4","datatype":"time","datavalue":{{"value":{{"time":"+2019-03-04T00:00:00Z","timezone":0,"before":0,"after":0,"precision":{p},"calendarmodel":"http://www.wikidata.org/entity/Q1985727"}},"type":"time"}}}},"rank":"normal"}}"#
            ))
        };
        assert_ne!(
            statement_key(&t(9), &Identity),
            statement_key(&t(11), &Identity)
        );
        // A globe coordinate is the exact value.
        let g = statement(
            r#"{"mainsnak":{"snaktype":"value","property":"P6","datatype":"globe-coordinate","datavalue":{"value":{"latitude":51.5,"longitude":-0.1275,"altitude":null,"precision":0.0001,"globe":"http://www.wikidata.org/entity/Q2"},"type":"globecoordinate"}},"rank":"normal"}"#,
        );
        let key = snak_key(&g.mainsnak, &Identity);
        assert!(
            key.contains("globecoordinate\u{1F}{\"type\"") && key.contains(r#""latitude":51.5"#),
            "{key}"
        );
    }
}
