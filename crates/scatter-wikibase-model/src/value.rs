//! Data types and data values (wikibase-compat §2 and §3.3; 0009 §3; 0017 §5; 0036 §3).
//!
//! A **data type** is a property-level concept backed by a **value type**, the kind of
//! `datavalue` stored. Values keep the strings Wikibase gives them: quantity amounts and
//! bounds, time strings and coordinate decimals are never reinterpreted (0006 §2).

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::id::EntityId;

/// The kind of `datavalue` a data type stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// `{"value": "…", "type": "string"}`
    String,
    /// `{"value": {"entity-type": …, "id": …}, "type": "wikibase-entityid"}`
    EntityId,
    /// `globecoordinate`
    GlobeCoordinate,
    /// `quantity`
    Quantity,
    /// `monolingualtext`
    MonolingualText,
    /// `time`
    Time,
}

impl ValueType {
    /// The `type` field of a `datavalue`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::EntityId => "wikibase-entityid",
            Self::GlobeCoordinate => "globecoordinate",
            Self::Quantity => "quantity",
            Self::MonolingualText => "monolingualtext",
            Self::Time => "time",
        }
    }
}

/// A property's data type. The registered ones are listed; any other ID is kept as
/// [`DataType::Other`], since a Wikibase may register more.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DataType {
    /// `string`
    String,
    /// `external-id`
    ExternalId,
    /// `url`
    Url,
    /// `commonsMedia`
    CommonsMedia,
    /// `localMedia`: a file in the tenant's own File namespace (0039 §12; WikibaseLocalMedia)
    LocalMedia,
    /// `geo-shape`
    GeoShape,
    /// `tabular-data`
    TabularData,
    /// `math`
    Math,
    /// `musical-notation`
    MusicalNotation,
    /// `edtf` (WikibaseEDTF; wikibase-compat §6)
    Edtf,
    /// `globe-coordinate`
    GlobeCoordinate,
    /// `monolingualtext`
    MonolingualText,
    /// `quantity`
    Quantity,
    /// `time`
    Time,
    /// `wikibase-item`
    WikibaseItem,
    /// `wikibase-property`
    WikibaseProperty,
    /// `wikibase-lexeme`
    WikibaseLexeme,
    /// `wikibase-form`
    WikibaseForm,
    /// `wikibase-sense`
    WikibaseSense,
    /// `wikibase-domain` (0009 §3)
    WikibaseDomain,
    /// `wikibase-keyword` (0017 §5)
    WikibaseKeyword,
    /// `wikibase-notation` (0048 §1)
    WikibaseNotation,
    /// A data type this crate does not know; its value type is unknown too.
    Other(String),
}

impl DataType {
    /// Parses a data type ID.
    #[must_use]
    pub fn parse(id: &str) -> Self {
        match id {
            "string" => Self::String,
            "external-id" => Self::ExternalId,
            "url" => Self::Url,
            "commonsMedia" => Self::CommonsMedia,
            "localMedia" => Self::LocalMedia,
            "geo-shape" => Self::GeoShape,
            "tabular-data" => Self::TabularData,
            "math" => Self::Math,
            "musical-notation" => Self::MusicalNotation,
            "edtf" => Self::Edtf,
            "globe-coordinate" => Self::GlobeCoordinate,
            "monolingualtext" => Self::MonolingualText,
            "quantity" => Self::Quantity,
            "time" => Self::Time,
            "wikibase-item" => Self::WikibaseItem,
            "wikibase-property" => Self::WikibaseProperty,
            "wikibase-lexeme" => Self::WikibaseLexeme,
            "wikibase-form" => Self::WikibaseForm,
            "wikibase-sense" => Self::WikibaseSense,
            "wikibase-domain" => Self::WikibaseDomain,
            "wikibase-keyword" => Self::WikibaseKeyword,
            "wikibase-notation" => Self::WikibaseNotation,
            other => Self::Other(other.to_string()),
        }
    }

    /// The data type ID.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::String => "string",
            Self::ExternalId => "external-id",
            Self::Url => "url",
            Self::CommonsMedia => "commonsMedia",
            Self::LocalMedia => "localMedia",
            Self::GeoShape => "geo-shape",
            Self::TabularData => "tabular-data",
            Self::Math => "math",
            Self::MusicalNotation => "musical-notation",
            Self::Edtf => "edtf",
            Self::GlobeCoordinate => "globe-coordinate",
            Self::MonolingualText => "monolingualtext",
            Self::Quantity => "quantity",
            Self::Time => "time",
            Self::WikibaseItem => "wikibase-item",
            Self::WikibaseProperty => "wikibase-property",
            Self::WikibaseLexeme => "wikibase-lexeme",
            Self::WikibaseForm => "wikibase-form",
            Self::WikibaseSense => "wikibase-sense",
            Self::WikibaseDomain => "wikibase-domain",
            Self::WikibaseKeyword => "wikibase-keyword",
            Self::WikibaseNotation => "wikibase-notation",
            Self::Other(s) => s,
        }
    }

    /// The value type behind the data type, where known.
    #[must_use]
    pub fn value_type(&self) -> Option<ValueType> {
        Some(match self {
            Self::String
            | Self::ExternalId
            | Self::Url
            | Self::CommonsMedia
            | Self::LocalMedia
            | Self::GeoShape
            | Self::TabularData
            | Self::Math
            | Self::MusicalNotation
            | Self::Edtf => ValueType::String,
            Self::GlobeCoordinate => ValueType::GlobeCoordinate,
            Self::MonolingualText => ValueType::MonolingualText,
            Self::Quantity => ValueType::Quantity,
            Self::Time => ValueType::Time,
            Self::WikibaseItem
            | Self::WikibaseProperty
            | Self::WikibaseLexeme
            | Self::WikibaseForm
            | Self::WikibaseSense
            | Self::WikibaseDomain
            | Self::WikibaseKeyword
            | Self::WikibaseNotation => ValueType::EntityId,
            Self::Other(_) => return None,
        })
    }

    /// For an entity-valued data type, the `entity-type` its values carry.
    #[must_use]
    pub fn entity_type(&self) -> Option<&'static str> {
        Some(match self {
            Self::WikibaseItem => "item",
            Self::WikibaseProperty => "property",
            Self::WikibaseLexeme => "lexeme",
            Self::WikibaseForm => "form",
            Self::WikibaseSense => "sense",
            Self::WikibaseDomain => "domain",
            Self::WikibaseKeyword => "keyword",
            Self::WikibaseNotation => "notation",
            _ => return None,
        })
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl Serialize for DataType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for DataType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::parse(&String::deserialize(d)?))
    }
}

/// An entity value: `{"entity-type": "item", "id": "Q3"}`. The `numeric-id` Wikibase
/// writes beside `id` is derived and is dropped at parse (0006 §2); [`crate::json`]
/// re-emits it in the Wikibase form where [`EntityId::numeric_id`] gives one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityIdValue {
    /// The Wikibase entity type (`item`, `property`, `domain`, …).
    pub entity_type: String,
    /// The ID, in whatever form the source wrote it.
    pub id: EntityId,
}

/// A globe coordinate. Numbers are JSON numbers as Wikibase typed them (0006 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobeCoordinate {
    /// Degrees.
    pub latitude: f64,
    /// Degrees.
    pub longitude: f64,
    /// Always `null` in current Wikibase, and kept so.
    #[serde(default)]
    pub altitude: Option<f64>,
    /// Degrees; `null` when unknown.
    #[serde(default)]
    pub precision: Option<f64>,
    /// The globe's IRI (Earth is `http://www.wikidata.org/entity/Q2` on every Wikibase).
    pub globe: String,
}

/// A quantity: decimal strings with a sign, never numbers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantity {
    /// `"+1.96"`
    pub amount: String,
    /// A unit IRI, or `"1"` for a unitless quantity.
    pub unit: String,
    /// Optional; written before `lowerBound` by Wikibase.
    #[serde(
        rename = "upperBound",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub upper_bound: Option<String>,
    /// Optional.
    #[serde(
        rename = "lowerBound",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub lower_bound: Option<String>,
}

/// A monolingual text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonolingualText {
    /// The text.
    pub text: String,
    /// The language code.
    pub language: String,
}

/// A time value (wikibase-compat §3.3). The `time` string uses historical year numbering
/// and `00` for an unknown month or day; nothing here reinterprets it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Time {
    /// `"+1952-03-11T00:00:00Z"`
    pub time: String,
    /// Minutes offset from UTC; always 0 in current Wikibase.
    pub timezone: i64,
    /// Uncertainty in units of `precision`; unused.
    pub before: i64,
    /// Uncertainty in units of `precision`; unused.
    pub after: i64,
    /// 0 (gigayear) to 14 (second); 9 is year, 10 month, 11 day.
    pub precision: u8,
    /// The calendar model IRI: Gregorian `…/Q1985727` or Julian `…/Q1985786`.
    pub calendarmodel: String,
}

/// A data value: the `datavalue` of a `value` snak.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DataValue {
    /// A string, for every string-backed data type.
    String(String),
    /// An entity reference.
    EntityId(EntityIdValue),
    /// A globe coordinate.
    GlobeCoordinate(GlobeCoordinate),
    /// A quantity.
    Quantity(Quantity),
    /// A monolingual text.
    MonolingualText(MonolingualText),
    /// A time.
    Time(Time),
    /// A value of a type this crate does not know, kept as JSON so nothing is lost.
    Unknown {
        /// The `type` field.
        value_type: String,
        /// The `value` field.
        value: serde_json::Value,
    },
}

impl DataValue {
    /// The `type` field this value serializes with.
    #[must_use]
    pub fn value_type_name(&self) -> &str {
        match self {
            Self::String(_) => ValueType::String.name(),
            Self::EntityId(_) => ValueType::EntityId.name(),
            Self::GlobeCoordinate(_) => ValueType::GlobeCoordinate.name(),
            Self::Quantity(_) => ValueType::Quantity.name(),
            Self::MonolingualText(_) => ValueType::MonolingualText.name(),
            Self::Time(_) => ValueType::Time.name(),
            Self::Unknown { value_type, .. } => value_type,
        }
    }

    /// The entity an entity value refers to.
    #[must_use]
    pub fn entity_id(&self) -> Option<&EntityId> {
        match self {
            Self::EntityId(v) => Some(&v.id),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------------

/// The wire shape `{"value": …, "type": "…"}`.
#[derive(Serialize, Deserialize)]
struct Wire<'a> {
    value: serde_json::Value,
    #[serde(rename = "type")]
    value_type: std::borrow::Cow<'a, str>,
}

/// The entity-value wire shape, with the derived `numeric-id` accepted and dropped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityIdWire {
    #[serde(rename = "entity-type")]
    entity_type: String,
    #[serde(rename = "numeric-id", default)]
    #[allow(dead_code)]
    numeric_id: Option<u64>,
    id: EntityId,
}

impl<'de> Deserialize<'de> for DataValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(d)?;
        let v = wire.value;
        let conv = |e: serde_json::Error| D::Error::custom(e.to_string());
        Ok(match wire.value_type.as_ref() {
            "string" => match v {
                serde_json::Value::String(s) => Self::String(s),
                other => {
                    return Err(D::Error::custom(format!(
                        "string value is not a string: {other}"
                    )));
                }
            },
            "wikibase-entityid" => {
                let w: EntityIdWire = serde_json::from_value(v).map_err(conv)?;
                Self::EntityId(EntityIdValue {
                    entity_type: w.entity_type,
                    id: w.id,
                })
            }
            "globecoordinate" => Self::GlobeCoordinate(serde_json::from_value(v).map_err(conv)?),
            "quantity" => Self::Quantity(serde_json::from_value(v).map_err(conv)?),
            "monolingualtext" => Self::MonolingualText(serde_json::from_value(v).map_err(conv)?),
            "time" => Self::Time(serde_json::from_value(v).map_err(conv)?),
            other => Self::Unknown {
                value_type: other.to_string(),
                value: v,
            },
        })
    }
}

impl DataValue {
    /// Serializes to the wire shape. `numeric_ids` adds `numeric-id` to entity values
    /// where the ID has one (the Wikibase form); the storage form leaves it out.
    pub(crate) fn to_wire(&self, numeric_ids: bool) -> serde_json::Value {
        use serde_json::{Map, Value, json};
        let value = match self {
            Self::String(s) => Value::String(s.clone()),
            Self::EntityId(e) => {
                let mut m = Map::new();
                m.insert("entity-type".into(), Value::String(e.entity_type.clone()));
                if numeric_ids && let Some(n) = e.id.numeric_id() {
                    m.insert("numeric-id".into(), json!(n));
                }
                m.insert("id".into(), Value::String(e.id.as_str().to_string()));
                Value::Object(m)
            }
            Self::GlobeCoordinate(g) => serde_json::to_value(g).expect("serializable"),
            Self::Quantity(q) => serde_json::to_value(q).expect("serializable"),
            Self::MonolingualText(m) => serde_json::to_value(m).expect("serializable"),
            Self::Time(t) => serde_json::to_value(t).expect("serializable"),
            Self::Unknown { value, .. } => value.clone(),
        };
        json!({"value": value, "type": self.value_type_name()})
    }
}

impl Serialize for DataValue {
    /// The storage form (no `numeric-id`). The Wikibase form goes through [`crate::json`].
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_wire(false).serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn data_types_and_value_types() {
        assert_eq!(
            DataType::parse("edtf").value_type(),
            Some(ValueType::String)
        );
        assert_eq!(
            DataType::parse("wikibase-domain").entity_type(),
            Some("domain")
        );
        assert_eq!(
            DataType::parse("wikibase-domain").value_type(),
            Some(ValueType::EntityId)
        );
        let other = DataType::parse("some-extension-type");
        assert_eq!(other.id(), "some-extension-type");
        assert_eq!(other.value_type(), None);
        assert_eq!(
            serde_json::to_value(DataType::CommonsMedia).unwrap(),
            json!("commonsMedia")
        );
        // 0039 §12: the WikibaseLocalMedia type, a string naming a local file.
        assert_eq!(DataType::parse("localMedia"), DataType::LocalMedia);
        assert_eq!(DataType::LocalMedia.value_type(), Some(ValueType::String));
    }

    #[test]
    fn entity_value_drops_and_recomputes_numeric_id() {
        let v: DataValue = serde_json::from_value(json!({
            "value": {"entity-type": "item", "numeric-id": 3, "id": "Q3"}, "type": "wikibase-entityid"
        }))
        .unwrap();
        assert_eq!(v.entity_id().unwrap().as_str(), "Q3");
        assert_eq!(
            v.to_wire(false),
            json!({"value": {"entity-type": "item", "id": "Q3"}, "type": "wikibase-entityid"})
        );
        assert_eq!(
            v.to_wire(true),
            json!({"value": {"entity-type": "item", "numeric-id": 3, "id": "Q3"}, "type": "wikibase-entityid"})
        );
        let d: DataValue = serde_json::from_value(json!({
            "value": {"entity-type": "domain", "id": "domain:doi.org"}, "type": "wikibase-entityid"
        }))
        .unwrap();
        assert_eq!(
            d.to_wire(true),
            json!({"value": {"entity-type": "domain", "id": "domain:doi.org"}, "type": "wikibase-entityid"}),
            "a keyed ID has no numeric-id (0009 §3)"
        );
    }

    #[test]
    fn typed_values_round_trip_and_keep_json_types() {
        let cases = [
            json!({"value": "Douglas Noël Adams", "type": "string"}),
            json!({"value": {"latitude": 51.5, "longitude": -0.1275, "altitude": null, "precision": 0.0001, "globe": "http://www.wikidata.org/entity/Q2"}, "type": "globecoordinate"}),
            json!({"value": {"amount": "+1.96", "unit": "http://localhost:8080/entity/Q3", "upperBound": "+1.97", "lowerBound": "+1.95"}, "type": "quantity"}),
            json!({"value": {"amount": "+42", "unit": "1"}, "type": "quantity"}),
            json!({"value": {"text": "Douglas Adams", "language": "en"}, "type": "monolingualtext"}),
            json!({"value": {"time": "+1500-00-00T00:00:00Z", "timezone": 0, "before": 0, "after": 0, "precision": 9, "calendarmodel": "http://www.wikidata.org/entity/Q1985786"}, "type": "time"}),
            json!({"value": {"x": 1}, "type": "future-type"}),
        ];
        for c in cases {
            let v: DataValue = serde_json::from_value(c.clone()).unwrap();
            assert_eq!(v.to_wire(true), c, "{c}");
            assert_eq!(serde_json::to_value(&v).unwrap(), c);
        }
        let q: DataValue = serde_json::from_value(
            json!({"value": {"amount": "+1", "unit": "1"}, "type": "quantity"}),
        )
        .unwrap();
        match q {
            DataValue::Quantity(q) => assert_eq!(q.amount, "+1", "amounts stay strings"),
            _ => panic!(),
        }
    }

    #[test]
    fn strict_about_unknown_fields_in_known_types() {
        assert!(
            serde_json::from_value::<DataValue>(json!({
                "value": {"text": "x", "language": "en", "extra": 1}, "type": "monolingualtext"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DataValue>(json!({"value": 5, "type": "string"})).is_err()
        );
    }
}
