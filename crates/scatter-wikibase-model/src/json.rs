//! Canonical JSON and the two output forms (0006 §2; wikibase-compat §3).
//!
//! **Canonical JSON** is compact, with no whitespace, strings escaped as `serde_json`
//! escapes them (non-ASCII left as UTF-8), numbers as `serde_json` prints them, and object
//! keys sorted the way the core deterministic CBOR encoding sorts map keys: by the bytes
//! of their encoding, which for text strings means shorter keys first and then bytewise.
//! `P1 < P2 < P10`, and `claims < labels < aliases`. JSON → CBOR → JSON reproduces the
//! bytes because both sides sort the same way.
//!
//! **Forms.** [`Form::Storage`] is what the log stores: no page metadata, no `numeric-id`,
//! and `hash` only where the guard kept upstream's (0006 §2, as amended 2026-09-28).
//! [`Form::Wikibase`] is what the API serves: `numeric-id` recomputed, and hashes as the
//! next slice of this crate computes them; until then a stored hash is emitted and an
//! absent one is left out.

use std::fmt::Write as _;

use serde_json::Value;

use crate::entity::{Entity, PageInfo, ParsedEntity};

/// Which JSON form to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The stored form: model data only.
    Storage,
    /// The served form: with `numeric-id` and page metadata where given.
    Wikibase,
}

/// Why JSON could not be read as an entity.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JsonError {
    /// Not JSON, or not the entity shape.
    #[error("entity JSON: {0}")]
    Parse(#[from] serde_json::Error),
    /// A `wbgetentities` response reported the entity missing.
    #[error("entity `{0}` is missing")]
    Missing(String),
}

impl Entity {
    /// Parses one entity from Wikibase JSON, with any page metadata beside it.
    pub fn from_json(text: &str) -> Result<ParsedEntity, JsonError> {
        Ok(serde_json::from_str(text)?)
    }

    /// Parses one entity from a JSON value.
    pub fn from_value(value: Value) -> Result<ParsedEntity, JsonError> {
        Ok(serde_json::from_value(value)?)
    }

    /// The entity as a JSON value in the given form, with no page metadata.
    #[must_use]
    pub fn to_value(&self, form: Form) -> Value {
        self.to_wire(form == Form::Wikibase, None)
    }

    /// The canonical JSON of the storage form: what the content part of a record holds
    /// (0006 §2), and what the content hash `0x03` is computed over.
    #[must_use]
    pub fn to_canonical_json(&self) -> String {
        canonical(&self.to_value(Form::Storage))
    }

    /// The canonical JSON of the Wikibase form, with page metadata if given.
    #[must_use]
    pub fn to_wikibase_json(&self, page: Option<&PageInfo>) -> String {
        canonical(&self.to_wire(true, page))
    }
}

impl ParsedEntity {
    /// The Wikibase form with the page metadata that was parsed.
    #[must_use]
    pub fn to_wikibase_json(&self) -> String {
        self.entity.to_wikibase_json(self.page.as_ref())
    }
}

/// Parses a `wbgetentities` response, `{"entities": {"Q8": {…}, "Q9": {"missing": ""}}}`,
/// into its entities in key order, or the first missing ID.
pub fn from_wbgetentities(text: &str) -> Result<Vec<ParsedEntity>, JsonError> {
    #[derive(serde::Deserialize)]
    struct Response {
        entities: serde_json::Map<String, Value>,
    }
    let r: Response = serde_json::from_str(text)?;
    r.entities
        .into_iter()
        .map(|(id, v)| {
            if v.get("missing").is_some() {
                return Err(JsonError::Missing(id));
            }
            Entity::from_value(v)
        })
        .collect()
}

/// Serializes a JSON value canonically (module docs).
#[must_use]
pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

/// The CBOR core-deterministic order of two text-string keys: by encoded bytes, which is
/// by length first (the length is in the head byte) and then bytewise.
#[must_use]
pub fn key_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.len()
        .cmp(&b.len())
        .then_with(|| a.as_bytes().cmp(b.as_bytes()))
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let _ = write!(out, "{n}");
        }
        Value::String(s) => {
            out.push_str(&serde_json::to_string(s).expect("a string serializes"));
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| key_order(a, b));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).expect("a string serializes"));
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_sort_by_length_then_bytes() {
        let v = json!({"P10": 1, "P2": 2, "P1": 3, "labels": {}, "id": "Q1", "claims": {}});
        assert_eq!(
            canonical(&v),
            // Two-byte keys first, bytewise within a length: `P1` (0x50…) before `id` (0x69…).
            r#"{"P1":3,"P2":2,"id":"Q1","P10":1,"claims":{},"labels":{}}"#
        );
    }

    #[test]
    fn compact_and_utf8() {
        let v = json!({"value": "Douglas Noël Adams", "n": [1.5, -0.1275, 0.0001, null, true]});
        assert_eq!(
            canonical(&v),
            r#"{"n":[1.5,-0.1275,0.0001,null,true],"value":"Douglas Noël Adams"}"#
        );
    }

    #[test]
    fn canonical_is_a_fixed_point() {
        let v = json!({"b": {"y": [3, {"k": "v", "a": 1}], "x": 1}, "a": "z"});
        let once = canonical(&v);
        let again = canonical(&serde_json::from_str::<Value>(&once).unwrap());
        assert_eq!(once, again);
    }

    #[test]
    fn wbgetentities_wrapper() {
        let text = r#"{"entities": {"Q8": {"type": "item", "id": "Q8"}, "Q9": {"missing": ""}}}"#;
        assert!(matches!(from_wbgetentities(text), Err(JsonError::Missing(id)) if id == "Q9"));
        let text = r#"{"entities": {"Q8": {"type": "item", "id": "Q8", "lastrevid": 26}}}"#;
        let es = from_wbgetentities(text).unwrap();
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].page.as_ref().unwrap().lastrevid, Some(26));
        assert_eq!(
            es[0].entity.to_canonical_json(),
            r#"{"id":"Q8","type":"item","claims":{},"labels":{},"aliases":{},"sitelinks":{},"descriptions":{}}"#
        );
        assert!(es[0].to_wikibase_json().contains(r#""lastrevid":26"#));
    }
}
