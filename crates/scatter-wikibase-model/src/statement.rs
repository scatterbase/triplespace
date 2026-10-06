//! Statements, snaks and references (wikibase-compat §2 and §3.2).
//!
//! Property order inside `qualifiers` and a reference's `snaks` is data: Wikibase carries
//! it in the `qualifiers-order` and `snaks-order` arrays, which are authoritative over the
//! object's own key order. The model keeps the maps ordered and regenerates the arrays.

use indexmap::IndexMap;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::id::{EntityId, StatementId};
use crate::json::Wire;
use crate::value::{DataType, DataValue};

/// A statement's rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rank {
    /// Preferred.
    Preferred,
    /// Normal, the default.
    #[default]
    Normal,
    /// Deprecated.
    Deprecated,
}

/// What a snak says about its property.
#[derive(Debug, Clone, PartialEq)]
pub enum SnakKind {
    /// A value.
    Value(DataValue),
    /// Some unknown value.
    SomeValue,
    /// No value.
    NoValue,
}

impl SnakKind {
    /// The `snaktype` field.
    #[must_use]
    pub const fn snaktype(&self) -> &'static str {
        match self {
            Self::Value(_) => "value",
            Self::SomeValue => "somevalue",
            Self::NoValue => "novalue",
        }
    }
}

/// A snak: a property and what is said about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Snak {
    /// The property, in whatever ID form the source wrote it.
    pub property: EntityId,
    /// Value, some value or no value.
    pub kind: SnakKind,
    /// The property's data type, as the source wrote it beside the snak.
    pub datatype: Option<DataType>,
    /// Upstream's snak hash, kept only where the recomputation differed (0006 §2, as
    /// amended 2026-09-28); `None` means "recompute on output".
    pub hash: Option<String>,
}

impl Snak {
    /// A value snak.
    #[must_use]
    pub fn value(property: EntityId, value: DataValue, datatype: Option<DataType>) -> Self {
        Self {
            property,
            kind: SnakKind::Value(value),
            datatype,
            hash: None,
        }
    }

    /// The data value, for a value snak.
    #[must_use]
    pub fn data_value(&self) -> Option<&DataValue> {
        match &self.kind {
            SnakKind::Value(v) => Some(v),
            _ => None,
        }
    }
}

/// Wire shape of a snak.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnakWire {
    snaktype: String,
    property: EntityId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    datavalue: Option<DataValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    datatype: Option<DataType>,
}

impl<'de> Deserialize<'de> for Snak {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = SnakWire::deserialize(d)?;
        let kind = match (w.snaktype.as_str(), w.datavalue) {
            ("value", Some(v)) => SnakKind::Value(v),
            ("value", None) => return Err(D::Error::custom("value snak without datavalue")),
            ("somevalue", None) => SnakKind::SomeValue,
            ("novalue", None) => SnakKind::NoValue,
            (t @ ("somevalue" | "novalue"), Some(_)) => {
                return Err(D::Error::custom(format!("{t} snak with a datavalue")));
            }
            (other, _) => return Err(D::Error::custom(format!("unknown snaktype `{other}`"))),
        };
        Ok(Self {
            property: w.property,
            kind,
            datatype: w.datatype,
            hash: w.hash,
        })
    }
}

impl Serialize for Snak {
    /// The storage form; the Wikibase form is produced by [`crate::json`].
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_wire(Wire::STORAGE).serialize(s)
    }
}

impl Snak {
    pub(crate) fn to_wire(&self, wire: Wire<'_>) -> serde_json::Value {
        use serde_json::{Map, Value};
        let mut m = Map::new();
        m.insert(
            "snaktype".into(),
            Value::String(self.kind.snaktype().into()),
        );
        m.insert(
            "property".into(),
            Value::String(self.property.as_str().into()),
        );
        // A kept upstream hash as stored; otherwise recomputed, in the Wikibase form.
        match (&self.hash, wire.hasher) {
            (Some(h), _) => {
                m.insert("hash".into(), Value::String(h.clone()));
            }
            (None, Some(hasher)) => {
                m.insert("hash".into(), Value::String(hasher.snak(self)));
            }
            (None, None) => {}
        }
        if let SnakKind::Value(v) = &self.kind {
            m.insert("datavalue".into(), v.to_wire(wire.numeric_ids));
        }
        if let Some(dt) = &self.datatype {
            m.insert("datatype".into(), Value::String(dt.id().into()));
        }
        Value::Object(m)
    }
}

/// Snaks grouped by property, in order.
pub type SnakGroups = IndexMap<EntityId, Vec<Snak>>;

/// A reference: snaks grouped by property, in order, and upstream's hash where kept.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reference {
    /// The snaks, grouped by property in `snaks-order`.
    pub snaks: SnakGroups,
    /// Upstream's reference hash, kept only where the recomputation differed.
    pub hash: Option<String>,
}

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    /// `<EntityId>$<UUID>`; absent on a statement not yet saved.
    pub id: Option<StatementId>,
    /// The main snak.
    pub mainsnak: Snak,
    /// The rank.
    pub rank: Rank,
    /// Qualifiers, grouped by property in `qualifiers-order`.
    pub qualifiers: SnakGroups,
    /// References, in order.
    pub references: Vec<Reference>,
}

impl Statement {
    /// A statement with a main snak and nothing else.
    #[must_use]
    pub fn new(mainsnak: Snak) -> Self {
        Self {
            id: None,
            mainsnak,
            rank: Rank::Normal,
            qualifiers: SnakGroups::new(),
            references: Vec::new(),
        }
    }

    /// The property of the main snak.
    #[must_use]
    pub fn property(&self) -> &EntityId {
        &self.mainsnak.property
    }

    /// Every snak: main, qualifiers, then references, in order.
    pub fn snaks(&self) -> impl Iterator<Item = &Snak> {
        std::iter::once(&self.mainsnak)
            .chain(self.qualifiers.values().flatten())
            .chain(
                self.references
                    .iter()
                    .flat_map(|r| r.snaks.values().flatten()),
            )
    }

    /// Every snak, mutably: main, qualifiers, then references, in order.
    pub fn snaks_mut(&mut self) -> impl Iterator<Item = &mut Snak> {
        std::iter::once(&mut self.mainsnak)
            .chain(self.qualifiers.values_mut().flatten())
            .chain(
                self.references
                    .iter_mut()
                    .flat_map(|r| r.snaks.values_mut().flatten()),
            )
    }
}

// ---------------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------------

/// Applies an `…-order` array to a group map: the named properties first, in that order,
/// then any the array does not name, in map order.
fn apply_order<E: serde::de::Error>(
    groups: &mut SnakGroups,
    order: Option<Vec<EntityId>>,
) -> Result<(), E> {
    let Some(order) = order else { return Ok(()) };
    for p in &order {
        if !groups.contains_key(p) {
            return Err(E::custom(format!("order names `{p}`, which has no snaks")));
        }
    }
    let mut ordered = SnakGroups::with_capacity(groups.len());
    for p in order {
        if let Some(v) = groups.shift_remove(&p) {
            ordered.insert(p, v);
        }
    }
    for (p, v) in groups.drain(..) {
        ordered.insert(p, v);
    }
    *groups = ordered;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceWire {
    #[serde(default)]
    hash: Option<String>,
    #[serde(deserialize_with = "crate::php::empty_array_as_map")]
    snaks: SnakGroups,
    #[serde(rename = "snaks-order", default)]
    snaks_order: Option<Vec<EntityId>>,
}

impl<'de> Deserialize<'de> for Reference {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut w = ReferenceWire::deserialize(d)?;
        apply_order::<D::Error>(&mut w.snaks, w.snaks_order)?;
        Ok(Self {
            snaks: w.snaks,
            hash: w.hash,
        })
    }
}

fn groups_to_wire(groups: &SnakGroups, wire: Wire<'_>) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (p, snaks) in groups {
        m.insert(
            p.as_str().into(),
            serde_json::Value::Array(snaks.iter().map(|s| s.to_wire(wire)).collect()),
        );
    }
    serde_json::Value::Object(m)
}

fn order_to_wire(groups: &SnakGroups) -> serde_json::Value {
    serde_json::Value::Array(
        groups
            .keys()
            .map(|p| serde_json::Value::String(p.as_str().into()))
            .collect(),
    )
}

impl Reference {
    pub(crate) fn to_wire(&self, wire: Wire<'_>) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        match (&self.hash, wire.hasher) {
            (Some(h), _) => {
                m.insert("hash".into(), serde_json::Value::String(h.clone()));
            }
            (None, Some(hasher)) => {
                m.insert(
                    "hash".into(),
                    serde_json::Value::String(hasher.reference(self)),
                );
            }
            (None, None) => {}
        }
        m.insert("snaks".into(), groups_to_wire(&self.snaks, wire));
        m.insert("snaks-order".into(), order_to_wire(&self.snaks));
        serde_json::Value::Object(m)
    }
}

impl Serialize for Reference {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_wire(Wire::STORAGE).serialize(s)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatementWire {
    #[serde(default)]
    id: Option<StatementId>,
    mainsnak: Snak,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    rank: Rank,
    #[serde(default, deserialize_with = "crate::php::empty_array_as_map")]
    qualifiers: SnakGroups,
    #[serde(rename = "qualifiers-order", default)]
    qualifiers_order: Option<Vec<EntityId>>,
    #[serde(default)]
    references: Vec<Reference>,
}

impl<'de> Deserialize<'de> for Statement {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut w = StatementWire::deserialize(d)?;
        match w.kind.as_deref() {
            None | Some("statement" | "claim") => {}
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "unknown statement type `{other}`"
                )));
            }
        }
        apply_order::<D::Error>(&mut w.qualifiers, w.qualifiers_order)?;
        Ok(Self {
            id: w.id,
            mainsnak: w.mainsnak,
            rank: w.rank,
            qualifiers: w.qualifiers,
            references: w.references,
        })
    }
}

impl Statement {
    pub(crate) fn to_wire(&self, wire: Wire<'_>) -> serde_json::Value {
        use serde_json::Value;
        let mut m = serde_json::Map::new();
        m.insert("mainsnak".into(), self.mainsnak.to_wire(wire));
        m.insert("type".into(), Value::String("statement".into()));
        if !self.qualifiers.is_empty() {
            m.insert("qualifiers".into(), groups_to_wire(&self.qualifiers, wire));
            m.insert("qualifiers-order".into(), order_to_wire(&self.qualifiers));
        }
        if let Some(id) = &self.id {
            m.insert("id".into(), Value::String(id.as_str().into()));
        }
        m.insert(
            "rank".into(),
            serde_json::to_value(self.rank).expect("serializable"),
        );
        if !self.references.is_empty() {
            m.insert(
                "references".into(),
                Value::Array(self.references.iter().map(|r| r.to_wire(wire)).collect()),
            );
        }
        Value::Object(m)
    }
}

impl Serialize for Statement {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_wire(Wire::STORAGE).serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snak(p: &str, v: &serde_json::Value) -> serde_json::Value {
        json!({"snaktype": "value", "property": p, "datavalue": {"value": v, "type": "string"}, "datatype": "string"})
    }

    #[test]
    fn order_arrays_are_authoritative() {
        let s: Statement = serde_json::from_value(json!({
            "mainsnak": snak("P1", &json!("x")), "type": "statement", "rank": "normal",
            "qualifiers": {"P4": [snak("P4", &json!("a"))], "P2": [snak("P2", &json!("b"))]},
            "qualifiers-order": ["P2", "P4"],
            "references": [{"snaks": {"P8": [snak("P8", &json!("u"))], "P4": [snak("P4", &json!("t"))]}, "snaks-order": ["P4", "P8"]}]
        }))
        .unwrap();
        let q: Vec<_> = s.qualifiers.keys().map(EntityId::as_str).collect();
        assert_eq!(q, ["P2", "P4"]);
        let r: Vec<_> = s.references[0].snaks.keys().map(EntityId::as_str).collect();
        assert_eq!(r, ["P4", "P8"]);
        let out = s.to_wire(Wire::STORAGE.with_numeric_ids());
        assert_eq!(out["qualifiers-order"], json!(["P2", "P4"]));
        assert_eq!(out["references"][0]["snaks-order"], json!(["P4", "P8"]));
        assert_eq!(s.snaks().count(), 5);
    }

    #[test]
    fn empty_groups_are_omitted_and_ids_optional() {
        let s = Statement::new(Snak::value(
            "P1".parse().unwrap(),
            DataValue::String("x".into()),
            Some(DataType::String),
        ));
        let out = s.to_wire(Wire::STORAGE.with_numeric_ids());
        assert_eq!(
            out,
            json!({"mainsnak": snak("P1", &json!("x")), "type": "statement", "rank": "normal"})
        );
    }

    #[test]
    fn snak_shapes() {
        let nv: Snak = serde_json::from_value(
            json!({"snaktype": "novalue", "property": "p2", "datatype": "string"}),
        )
        .unwrap();
        assert_eq!(nv.kind, SnakKind::NoValue);
        assert_eq!(nv.property.as_str(), "P2", "IDs are canonical on entry");
        assert!(
            serde_json::from_value::<Snak>(json!({"snaktype": "value", "property": "P2"})).is_err()
        );
        assert!(serde_json::from_value::<Snak>(json!({"snaktype": "somevalue", "property": "P2", "datavalue": {"value": "x", "type": "string"}})).is_err());
        assert!(serde_json::from_value::<Snak>(json!({"snaktype": "value", "property": "P2", "datavalue": {"value": "x", "type": "string"}, "extra": 1})).is_err());
    }

    #[test]
    fn bad_order_is_an_error() {
        assert!(
            serde_json::from_value::<Reference>(json!({
                "snaks": {"P8": [snak("P8", &json!("u"))]}, "snaks-order": ["P9"]
            }))
            .is_err()
        );
    }
}
