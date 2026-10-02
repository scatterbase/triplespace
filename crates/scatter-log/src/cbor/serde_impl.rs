//! Serde ↔ [`Value`]: a payload struct's CBOR is its serde shape.
//!
//! The mapping is the structural one of 0006 §2 with one addition JSON lacks: `serialize_bytes`
//! gives a byte string. Options are present-or-absent at the struct level (a `None` field
//! is skipped only when the struct says so; otherwise it is `null`), unit variants are text,
//! and a variant with data is a one-entry map `{variant: data}`, as serde's JSON convention
//! has it, so the two renderings agree wherever both exist.

use std::fmt;

use serde::de::{self, DeserializeOwned, IntoDeserializer, Visitor};
use serde::ser::{self, Serialize, SerializeMap, SerializeSeq};

use super::Value;

/// A serde error in either direction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SerdeError(String);

impl ser::Error for SerdeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

impl de::Error for SerdeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

/// Serializes a value to the CBOR data model.
pub fn to_value<T: Serialize>(value: &T) -> Result<Value, SerdeError> {
    value.serialize(Serializer)
}

/// Deserializes a value from the CBOR data model.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, SerdeError> {
    T::deserialize(value)
}

// ---------------------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------------------

struct Serializer;

struct SeqSer(Vec<Value>);
struct MapSer {
    pairs: Vec<(Value, Value)>,
    key: Option<Value>,
}
struct VariantSer {
    variant: &'static str,
    inner: Vec<Value>,
}
struct StructVariantSer {
    variant: &'static str,
    pairs: Vec<(Value, Value)>,
}

impl ser::Serializer for Serializer {
    type Ok = Value;
    type Error = SerdeError;
    type SerializeSeq = SeqSer;
    type SerializeTuple = SeqSer;
    type SerializeTupleStruct = SeqSer;
    type SerializeTupleVariant = VariantSer;
    type SerializeMap = MapSer;
    type SerializeStruct = MapSer;
    type SerializeStructVariant = StructVariantSer;

    fn serialize_bool(self, v: bool) -> Result<Value, SerdeError> {
        Ok(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_i16(self, v: i16) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_i32(self, v: i32) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_i64(self, v: i64) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_i128(self, v: i128) -> Result<Value, SerdeError> {
        Ok(Value::Int(v))
    }
    fn serialize_u8(self, v: u8) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_u16(self, v: u16) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_u32(self, v: u32) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_u64(self, v: u64) -> Result<Value, SerdeError> {
        Ok(Value::Int(i128::from(v)))
    }
    fn serialize_u128(self, v: u128) -> Result<Value, SerdeError> {
        i128::try_from(v)
            .map(Value::Int)
            .map_err(|_| SerdeError("u128 out of range".into()))
    }
    fn serialize_f32(self, v: f32) -> Result<Value, SerdeError> {
        Ok(Value::Float(f64::from(v)))
    }
    fn serialize_f64(self, v: f64) -> Result<Value, SerdeError> {
        Ok(Value::Float(v))
    }
    fn serialize_char(self, v: char) -> Result<Value, SerdeError> {
        Ok(Value::Text(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<Value, SerdeError> {
        Ok(Value::Text(v.to_string()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Value, SerdeError> {
        Ok(Value::Bytes(v.to_vec()))
    }
    fn serialize_none(self) -> Result<Value, SerdeError> {
        Ok(Value::Null)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> Result<Value, SerdeError> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value, SerdeError> {
        Ok(Value::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value, SerdeError> {
        Ok(Value::Null)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Value, SerdeError> {
        Ok(Value::Text(variant.to_string()))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        v: &T,
    ) -> Result<Value, SerdeError> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<Value, SerdeError> {
        Ok(Value::Map(vec![(
            Value::Text(variant.to_string()),
            v.serialize(Serializer)?,
        )]))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSer, SerdeError> {
        Ok(SeqSer(Vec::with_capacity(len.unwrap_or(0))))
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSer, SerdeError> {
        Ok(SeqSer(Vec::with_capacity(len)))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<SeqSer, SerdeError> {
        Ok(SeqSer(Vec::with_capacity(len)))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantSer, SerdeError> {
        Ok(VariantSer {
            variant,
            inner: Vec::with_capacity(len),
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<MapSer, SerdeError> {
        Ok(MapSer {
            pairs: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
        })
    }
    fn serialize_struct(self, _: &'static str, len: usize) -> Result<MapSer, SerdeError> {
        Ok(MapSer {
            pairs: Vec::with_capacity(len),
            key: None,
        })
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<StructVariantSer, SerdeError> {
        Ok(StructVariantSer {
            variant,
            pairs: Vec::with_capacity(len),
        })
    }
}

impl ser::SerializeSeq for SeqSer {
    type Ok = Value;
    type Error = SerdeError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), SerdeError> {
        self.0.push(v.serialize(Serializer)?);
        Ok(())
    }
    fn end(self) -> Result<Value, SerdeError> {
        Ok(Value::Array(self.0))
    }
}

impl ser::SerializeTuple for SeqSer {
    type Ok = Value;
    type Error = SerdeError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), SerdeError> {
        SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<Value, SerdeError> {
        SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SeqSer {
    type Ok = Value;
    type Error = SerdeError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), SerdeError> {
        SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<Value, SerdeError> {
        SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleVariant for VariantSer {
    type Ok = Value;
    type Error = SerdeError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), SerdeError> {
        self.inner.push(v.serialize(Serializer)?);
        Ok(())
    }
    fn end(self) -> Result<Value, SerdeError> {
        Ok(Value::Map(vec![(
            Value::Text(self.variant.to_string()),
            Value::Array(self.inner),
        )]))
    }
}

impl ser::SerializeMap for MapSer {
    type Ok = Value;
    type Error = SerdeError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, k: &T) -> Result<(), SerdeError> {
        self.key = Some(k.serialize(Serializer)?);
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), SerdeError> {
        let k = self
            .key
            .take()
            .ok_or_else(|| SerdeError("value before key".into()))?;
        self.pairs.push((k, v.serialize(Serializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value, SerdeError> {
        Ok(Value::map(self.pairs))
    }
}

impl ser::SerializeStruct for MapSer {
    type Ok = Value;
    type Error = SerdeError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        k: &'static str,
        v: &T,
    ) -> Result<(), SerdeError> {
        SerializeMap::serialize_entry(self, k, v)
    }
    fn end(self) -> Result<Value, SerdeError> {
        SerializeMap::end(self)
    }
}

impl ser::SerializeStructVariant for StructVariantSer {
    type Ok = Value;
    type Error = SerdeError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        k: &'static str,
        v: &T,
    ) -> Result<(), SerdeError> {
        self.pairs
            .push((Value::Text(k.to_string()), v.serialize(Serializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value, SerdeError> {
        Ok(Value::Map(vec![(
            Value::Text(self.variant.to_string()),
            Value::Map(self.pairs),
        )]))
    }
}

// ---------------------------------------------------------------------------------------
// Deserializer
// ---------------------------------------------------------------------------------------

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a bool",
        Value::Int(_) => "an integer",
        Value::Float(_) => "a float",
        Value::Bytes(_) => "a byte string",
        Value::Text(_) => "a text string",
        Value::Array(_) => "an array",
        Value::Map(_) => "a map",
    }
}

impl<'de> de::Deserializer<'de> for Value {
    type Error = SerdeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        match self {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::Int(i) => {
                if let Ok(u) = u64::try_from(i) {
                    visitor.visit_u64(u)
                } else if let Ok(s) = i64::try_from(i) {
                    visitor.visit_i64(s)
                } else {
                    visitor.visit_i128(i)
                }
            }
            Value::Float(f) => visitor.visit_f64(f),
            Value::Bytes(b) => visitor.visit_byte_buf(b),
            Value::Text(s) => visitor.visit_string(s),
            Value::Array(a) => visitor.visit_seq(SeqDe(a.into_iter())),
            Value::Map(m) => visitor.visit_map(MapDe {
                iter: m.into_iter(),
                value: None,
            }),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        match self {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        match self {
            Value::Text(s) => visitor.visit_enum(s.into_deserializer()),
            Value::Map(mut m) if m.len() == 1 => {
                let (k, v) = m.remove(0);
                let Value::Text(k) = k else {
                    return Err(SerdeError("enum variant key is not text".into()));
                };
                visitor.visit_enum(EnumDe {
                    variant: k,
                    value: v,
                })
            }
            other => Err(SerdeError(format!(
                "expected an enum as text or a one-entry map, got {}",
                type_name(&other)
            ))),
        }
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        match self {
            Value::Bytes(b) => visitor.visit_byte_buf(b),
            Value::Array(a) => {
                let bytes = a
                    .into_iter()
                    .map(|v| match v {
                        Value::Int(i) => {
                            u8::try_from(i).map_err(|_| SerdeError("byte out of range".into()))
                        }
                        other => Err(SerdeError(format!(
                            "expected a byte, got {}",
                            type_name(&other)
                        ))),
                    })
                    .collect::<Result<Vec<u8>, _>>()?;
                visitor.visit_byte_buf(bytes)
            }
            other => Err(SerdeError(format!(
                "expected bytes, got {}",
                type_name(&other)
            ))),
        }
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        match self {
            Value::Null => visitor.visit_unit(),
            other => Err(SerdeError(format!(
                "expected null, got {}",
                type_name(&other)
            ))),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct SeqDe(std::vec::IntoIter<Value>);

impl<'de> de::SeqAccess<'de> for SeqDe {
    type Error = SerdeError;
    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, SerdeError> {
        match self.0.next() {
            Some(v) => seed.deserialize(v).map(Some),
            None => Ok(None),
        }
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct MapDe {
    iter: std::vec::IntoIter<(Value, Value)>,
    value: Option<Value>,
}

impl<'de> de::MapAccess<'de> for MapDe {
    type Error = SerdeError;
    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, SerdeError> {
        match self.iter.next() {
            Some((k, v)) => {
                self.value = Some(v);
                seed.deserialize(k).map(Some)
            }
            None => Ok(None),
        }
    }
    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, SerdeError> {
        let v = self
            .value
            .take()
            .ok_or_else(|| SerdeError("value before key".into()))?;
        seed.deserialize(v)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct EnumDe {
    variant: String,
    value: Value,
}

impl<'de> de::EnumAccess<'de> for EnumDe {
    type Error = SerdeError;
    type Variant = VariantDe;
    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, VariantDe), SerdeError> {
        let v = seed.deserialize(self.variant.into_deserializer())?;
        Ok((v, VariantDe(self.value)))
    }
}

struct VariantDe(Value);

impl<'de> de::VariantAccess<'de> for VariantDe {
    type Error = SerdeError;
    fn unit_variant(self) -> Result<(), SerdeError> {
        match self.0 {
            Value::Null => Ok(()),
            other => Err(SerdeError(format!(
                "expected a unit variant, got {}",
                type_name(&other)
            ))),
        }
    }
    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, SerdeError> {
        seed.deserialize(self.0)
    }
    fn tuple_variant<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, SerdeError> {
        de::Deserializer::deserialize_any(self.0, visitor)
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        de::Deserializer::deserialize_any(self.0, visitor)
    }
}

impl IntoDeserializer<'_, SerdeError> for Value {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct KeyRecord {
        id: String,
        #[serde(with = "serde_bytes_shim")]
        public: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        valid_until: Option<u64>,
        kind: Kind,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Kind {
        Registered,
        Bot { operator: String },
        Other(u8),
    }

    mod serde_bytes_shim {
        use serde::{Deserializer, Serializer};
        pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
            s.serialize_bytes(v)
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
            struct V;
            impl serde::de::Visitor<'_> for V {
                type Value = Vec<u8>;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("bytes")
                }
                fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                    Ok(v)
                }
                fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                    Ok(v.to_vec())
                }
            }
            d.deserialize_bytes(V)
        }
    }

    #[test]
    fn structs_round_trip_with_bytes_and_enums() {
        let k = KeyRecord {
            id: "abc".into(),
            public: vec![1, 2, 3],
            valid_until: None,
            kind: Kind::Bot {
                operator: "local:42".into(),
            },
            tags: vec![],
        };
        let v = to_value(&k).unwrap();
        assert_eq!(
            v,
            Value::Map(vec![
                (Value::text("id"), Value::text("abc")),
                (
                    Value::text("kind"),
                    Value::Map(vec![(
                        Value::text("bot"),
                        Value::Map(vec![(Value::text("operator"), Value::text("local:42"))])
                    )])
                ),
                (Value::text("public"), Value::Bytes(vec![1, 2, 3])),
            ])
        );
        let bytes = v.encode().unwrap();
        let back: KeyRecord = from_value(super::super::decode(&bytes).unwrap()).unwrap();
        assert_eq!(back, k);
        let unit = KeyRecord {
            kind: Kind::Registered,
            valid_until: Some(7),
            ..k
        };
        let v = to_value(&unit).unwrap();
        let back: KeyRecord = from_value(v).unwrap();
        assert_eq!(back.kind, Kind::Registered);
        assert_eq!(back.valid_until, Some(7));
        let v = to_value(&Kind::Other(5)).unwrap();
        assert_eq!(v, Value::Map(vec![(Value::text("other"), Value::Int(5))]));
        assert_eq!(from_value::<Kind>(v).unwrap(), Kind::Other(5));
        // JSON renders the same shape, bytes aside.
        let j = serde_json::to_value(&back).unwrap();
        assert_eq!(j["kind"], "registered");
        assert_eq!(j["valid_until"], 7);
    }
}
