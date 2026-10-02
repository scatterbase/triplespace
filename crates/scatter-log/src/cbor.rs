//! Canonical CBOR (0006 §2; 0033 §6): the core deterministic encoding of RFC 8949 §4.2.1.
//!
//! Everything the log hashes is encoded here, by hand, to these rules:
//!
//! - integers and lengths in their shortest form;
//! - definite lengths only;
//! - map keys sorted by their encoded bytes, with no duplicates;
//! - floats in the shortest width that round-trips (16, 32 or 64 bits), NaN as the
//!   canonical `0xf97e00`;
//! - no tags, no `undefined`, no simple values beyond `false`, `true` and `null`.
//!
//! [`Value`] is the data model, [`encode`] the only encoder, and [`decode`] the strict
//! decoder: it parses, re-encodes, and rejects the input if the bytes differ, so a
//! non-canonical record can never be read as if it were canonical.
//!
//! JSON maps onto this structurally (0006 §2): [`Value::from_json`] and [`Value::to_json`]
//! convert; byte strings have no JSON form and convert to an error. Serde types encode
//! through [`to_value`] and decode through [`from_value`], with `serialize_bytes` giving
//! a byte string, so a payload struct's CBOR is its serde shape and nothing else.

use std::cmp::Ordering;
use std::fmt;

use minicbor::data::{Int, Type};

mod serde_impl;

pub use serde_impl::{SerdeError, from_value, to_value};

/// A CBOR data item.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer in `[-2^64, 2^64 - 1]`.
    Int(i128),
    /// A float.
    Float(f64),
    /// A byte string.
    Bytes(Vec<u8>),
    /// A text string.
    Text(String),
    /// An array.
    Array(Vec<Value>),
    /// A map. Pairs may be given in any order and encoding sorts them, but equality is
    /// positional: build with [`Value::map`] to compare with decoded values.
    Map(Vec<(Value, Value)>),
}

/// Why bytes are not canonical CBOR, or a value cannot be encoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CborError {
    /// Not well-formed CBOR.
    #[error("CBOR: {0}")]
    Malformed(String),
    /// Well-formed, but not the canonical encoding of what it decodes to.
    #[error("CBOR is not canonical at byte {0}")]
    NotCanonical(usize),
    /// A tag, `undefined`, an unassigned simple value, or an indefinite length.
    #[error("CBOR: {0} is not allowed")]
    NotAllowed(&'static str),
    /// A map with two equal keys.
    #[error("CBOR: duplicate map key")]
    DuplicateKey,
    /// An integer outside `[-2^64, 2^64 - 1]`.
    #[error("CBOR: integer {0} is out of range")]
    IntRange(i128),
    /// Bytes after the item.
    #[error("CBOR: {0} trailing bytes")]
    Trailing(usize),
    /// A value JSON cannot carry: a byte string, a non-finite float, a non-text map key.
    #[error("JSON cannot carry {0}")]
    NotJson(&'static str),
}

impl Value {
    /// A text value.
    #[must_use]
    pub fn text(s: &str) -> Self {
        Self::Text(s.to_string())
    }

    /// Whether this is the text `s`.
    #[must_use]
    pub fn text_eq(&self, s: &str) -> bool {
        matches!(self, Self::Text(t) if t == s)
    }

    /// A map with its pairs in canonical key order. The encoder sorts regardless; this
    /// makes values built here compare equal to decoded ones.
    #[must_use]
    pub fn map(mut pairs: Vec<(Self, Self)>) -> Self {
        pairs.sort_by(|(a, _), (b, _)| key_order(a, b));
        Self::Map(pairs)
    }

    /// The canonical bytes.
    pub fn encode(&self) -> Result<Vec<u8>, CborError> {
        encode(self)
    }

    /// From JSON, structurally (0006 §2).
    #[must_use]
    pub fn from_json(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Int(i128::from(i))
                } else if let Some(u) = n.as_u64() {
                    Self::Int(i128::from(u))
                } else {
                    Self::Float(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            serde_json::Value::String(s) => Self::Text(s.clone()),
            serde_json::Value::Array(a) => Self::Array(a.iter().map(Self::from_json).collect()),
            serde_json::Value::Object(o) => Self::map(
                o.iter()
                    .map(|(k, v)| (Self::Text(k.clone()), Self::from_json(v)))
                    .collect(),
            ),
        }
    }

    /// To JSON, structurally; fails on what JSON cannot carry.
    pub fn to_json(&self) -> Result<serde_json::Value, CborError> {
        Ok(match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(b) => serde_json::Value::Bool(*b),
            Self::Int(i) => {
                if let Ok(u) = u64::try_from(*i) {
                    serde_json::Value::from(u)
                } else if let Ok(s) = i64::try_from(*i) {
                    serde_json::Value::from(s)
                } else {
                    return Err(CborError::NotJson("an integer beyond 64 bits"));
                }
            }
            Self::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .ok_or(CborError::NotJson("a non-finite float"))?,
            Self::Bytes(_) => return Err(CborError::NotJson("a byte string")),
            Self::Text(s) => serde_json::Value::String(s.clone()),
            Self::Array(a) => {
                serde_json::Value::Array(a.iter().map(Self::to_json).collect::<Result<_, _>>()?)
            }
            Self::Map(m) => {
                let mut o = serde_json::Map::new();
                for (k, v) in m {
                    let Self::Text(k) = k else {
                        return Err(CborError::NotJson("a non-text map key"));
                    };
                    o.insert(k.clone(), v.to_json()?);
                }
                serde_json::Value::Object(o)
            }
        })
    }
}

impl fmt::Display for Value {
    /// Diagnostic notation, for messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(x) => write!(f, "{x:?}"),
            Self::Bytes(b) => {
                f.write_str("h'")?;
                for byte in b {
                    write!(f, "{byte:02x}")?;
                }
                f.write_str("'")
            }
            Self::Text(s) => write!(f, "{s:?}"),
            Self::Array(a) => {
                f.write_str("[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Self::Map(m) => {
                f.write_str("{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str("}")
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------------------

/// The canonical encoding of a value.
pub fn encode(value: &Value) -> Result<Vec<u8>, CborError> {
    let mut out = Vec::new();
    write(value, &mut out)?;
    Ok(out)
}

fn head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let m = major << 5;
    if arg < 24 {
        out.push(m | u8::try_from(arg).expect("< 24"));
    } else if arg <= 0xff {
        out.push(m | 0x18);
        out.push(u8::try_from(arg).expect("<= 0xff"));
    } else if arg <= 0xffff {
        out.push(m | 0x19);
        out.extend_from_slice(&u16::try_from(arg).expect("<= 0xffff").to_be_bytes());
    } else if arg <= 0xffff_ffff {
        out.push(m | 0x1a);
        out.extend_from_slice(&u32::try_from(arg).expect("<= 0xffffffff").to_be_bytes());
    } else {
        out.push(m | 0x1b);
        out.extend_from_slice(&arg.to_be_bytes());
    }
}

fn write(value: &Value, out: &mut Vec<u8>) -> Result<(), CborError> {
    match value {
        Value::Null => out.push(0xf6),
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::Int(i) => {
            if let Ok(u) = u64::try_from(*i) {
                head(out, 0, u);
            } else if *i < 0 {
                let n = u64::try_from(-1 - *i).map_err(|_| CborError::IntRange(*i))?;
                head(out, 1, n);
            } else {
                return Err(CborError::IntRange(*i));
            }
        }
        Value::Float(x) => write_float(*x, out),
        Value::Bytes(b) => {
            head(out, 2, b.len() as u64);
            out.extend_from_slice(b);
        }
        Value::Text(s) => {
            head(out, 3, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(a) => {
            head(out, 4, a.len() as u64);
            for v in a {
                write(v, out)?;
            }
        }
        Value::Map(m) => {
            let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = m
                .iter()
                .map(|(k, v)| Ok((encode(k)?, encode(v)?)))
                .collect::<Result<_, CborError>>()?;
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            if pairs.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(CborError::DuplicateKey);
            }
            head(out, 5, pairs.len() as u64);
            for (k, v) in pairs {
                out.extend_from_slice(&k);
                out.extend_from_slice(&v);
            }
        }
    }
    Ok(())
}

/// The shortest float encoding that round-trips (RFC 8949 §4.2.1, preferred
/// serialization): half, single, or double precision.
fn write_float(x: f64, out: &mut Vec<u8>) {
    if x.is_nan() {
        out.extend_from_slice(&[0xf9, 0x7e, 0x00]);
        return;
    }
    if let Some(h) = f16_bits(x) {
        out.push(0xf9);
        out.extend_from_slice(&h.to_be_bytes());
        return;
    }
    #[allow(clippy::cast_possible_truncation)]
    let single = x as f32;
    if f64::from(single).to_bits() == x.to_bits() {
        out.push(0xfa);
        out.extend_from_slice(&single.to_bits().to_be_bytes());
        return;
    }
    out.push(0xfb);
    out.extend_from_slice(&x.to_bits().to_be_bytes());
}

/// The IEEE 754 half-precision bits of `x`, if `x` is exactly representable.
fn f16_bits(x: f64) -> Option<u16> {
    if x.is_infinite() {
        return Some(if x > 0.0 { 0x7c00 } else { 0xfc00 });
    }
    if x == 0.0 {
        return Some(if x.is_sign_negative() { 0x8000 } else { 0 });
    }
    let bits = x.to_bits();
    let sign = u16::try_from(bits >> 63).expect("one bit") << 15;
    let exp = i32::try_from((bits >> 52) & 0x7ff).expect("11 bits") - 1023;
    let mantissa = bits & 0x000f_ffff_ffff_ffff;
    if (-14..=15).contains(&exp) {
        // Normal: ten mantissa bits must hold it.
        if mantissa & ((1 << 42) - 1) != 0 {
            return None;
        }
        let e = u16::try_from(exp + 15).expect("5 bits");
        let m = u16::try_from(mantissa >> 42).expect("10 bits");
        Some(sign | (e << 10) | m)
    } else if (-24..-14).contains(&exp) {
        // Subnormal: the value is m · 2^-24 for an integer m < 1024.
        let full = (1u64 << 52) | mantissa;
        let shift = u32::try_from(-exp - 14 + 42).expect("positive");
        if full & ((1u64 << shift) - 1) != 0 {
            return None;
        }
        let m = u16::try_from(full >> shift).expect("10 bits");
        Some(sign | m)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------------------

/// Decodes one canonical item, consuming the whole input. Non-canonical input is
/// rejected: the item is re-encoded and compared with the bytes.
pub fn decode(bytes: &[u8]) -> Result<Value, CborError> {
    let (value, consumed) = decode_prefix(bytes)?;
    if consumed != bytes.len() {
        return Err(CborError::Trailing(bytes.len() - consumed));
    }
    Ok(value)
}

/// Strictly decodes the first data item of a CBOR sequence (RFC 8742), returning it
/// with the number of bytes it took.
pub fn decode_prefix(bytes: &[u8]) -> Result<(Value, usize), CborError> {
    let mut d = minicbor::Decoder::new(bytes);
    let value = read(&mut d)?;
    let consumed = d.position();
    let again = encode(&value)?;
    if again != bytes[..consumed] {
        let at = again
            .iter()
            .zip(bytes)
            .position(|(a, b)| a != b)
            .unwrap_or(again.len().min(consumed));
        return Err(CborError::NotCanonical(at));
    }
    Ok((value, consumed))
}

fn malformed<E: fmt::Display>(e: E) -> CborError {
    CborError::Malformed(e.to_string())
}

fn read(d: &mut minicbor::Decoder<'_>) -> Result<Value, CborError> {
    Ok(match d.datatype().map_err(malformed)? {
        Type::Null => {
            d.null().map_err(malformed)?;
            Value::Null
        }
        Type::Bool => Value::Bool(d.bool().map_err(malformed)?),
        Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::Int => {
            let i: Int = d.int().map_err(malformed)?;
            Value::Int(i128::from(i))
        }
        Type::F16 => Value::Float(f64::from(d.f16().map_err(malformed)?)),
        Type::F32 => Value::Float(f64::from(d.f32().map_err(malformed)?)),
        Type::F64 => Value::Float(d.f64().map_err(malformed)?),
        Type::Bytes => Value::Bytes(d.bytes().map_err(malformed)?.to_vec()),
        Type::String => Value::Text(d.str().map_err(malformed)?.to_string()),
        Type::Array => {
            let n = d
                .array()
                .map_err(malformed)?
                .ok_or(CborError::NotAllowed("an indefinite-length array"))?;
            let mut items = Vec::with_capacity(usize::try_from(n).unwrap_or(0).min(1 << 16));
            for _ in 0..n {
                items.push(read(d)?);
            }
            Value::Array(items)
        }
        Type::Map => {
            let n = d
                .map()
                .map_err(malformed)?
                .ok_or(CborError::NotAllowed("an indefinite-length map"))?;
            let mut pairs = Vec::with_capacity(usize::try_from(n).unwrap_or(0).min(1 << 16));
            for _ in 0..n {
                let k = read(d)?;
                let v = read(d)?;
                pairs.push((k, v));
            }
            Value::Map(pairs)
        }
        Type::BytesIndef | Type::StringIndef | Type::ArrayIndef | Type::MapIndef => {
            return Err(CborError::NotAllowed("an indefinite length"));
        }
        Type::Tag => return Err(CborError::NotAllowed("a tag")),
        Type::Undefined => return Err(CborError::NotAllowed("undefined")),
        Type::Simple => return Err(CborError::NotAllowed("a simple value")),
        Type::Break => return Err(CborError::NotAllowed("a break")),
        Type::Unknown(b) => return Err(CborError::Malformed(format!("unknown head byte {b:#x}"))),
    })
}

/// The canonical order of two keys: by their encoded bytes.
#[must_use]
pub fn key_order(a: &Value, b: &Value) -> Ordering {
    match (encode(a), encode(b)) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::hash::hex;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn rfc8949_appendix_a_vectors() {
        // (value, hex) from RFC 8949 Appendix A, the canonical ones.
        let cases: Vec<(Value, &str)> = vec![
            (Value::Int(0), "00"),
            (Value::Int(1), "01"),
            (Value::Int(10), "0a"),
            (Value::Int(23), "17"),
            (Value::Int(24), "1818"),
            (Value::Int(25), "1819"),
            (Value::Int(100), "1864"),
            (Value::Int(1000), "1903e8"),
            (Value::Int(1_000_000), "1a000f4240"),
            (Value::Int(1_000_000_000_000), "1b000000e8d4a51000"),
            (Value::Int(18_446_744_073_709_551_615), "1bffffffffffffffff"),
            (
                Value::Int(-18_446_744_073_709_551_616),
                "3bffffffffffffffff",
            ),
            (Value::Int(-1), "20"),
            (Value::Int(-10), "29"),
            (Value::Int(-100), "3863"),
            (Value::Int(-1000), "3903e7"),
            (Value::Float(0.0), "f90000"),
            (Value::Float(-0.0), "f98000"),
            (Value::Float(1.0), "f93c00"),
            (Value::Float(1.1), "fb3ff199999999999a"),
            (Value::Float(1.5), "f93e00"),
            (Value::Float(65504.0), "f97bff"),
            (Value::Float(100_000.0), "fa47c35000"),
            (Value::Float(3.402_823_466_385_288_6e38), "fa7f7fffff"),
            (Value::Float(1.0e300), "fb7e37e43c8800759c"),
            (Value::Float(5.960_464_477_539_063e-8), "f90001"),
            (Value::Float(0.000_061_035_156_25), "f90400"),
            (Value::Float(-4.0), "f9c400"),
            (Value::Float(-4.1), "fbc010666666666666"),
            (Value::Float(f64::INFINITY), "f97c00"),
            (Value::Float(f64::NAN), "f97e00"),
            (Value::Float(f64::NEG_INFINITY), "f9fc00"),
            (Value::Bool(false), "f4"),
            (Value::Bool(true), "f5"),
            (Value::Null, "f6"),
            (Value::Bytes(vec![]), "40"),
            (Value::Bytes(vec![1, 2, 3, 4]), "4401020304"),
            (Value::text(""), "60"),
            (Value::text("a"), "6161"),
            (Value::text("IETF"), "6449455446"),
            (Value::text("\"\\"), "62225c"),
            (Value::text("ü"), "62c3bc"),
            (Value::text("水"), "63e6b0b4"),
            (Value::text("𐅑"), "64f0908591"),
            (Value::Array(vec![]), "80"),
            (
                Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
                "83010203",
            ),
            (
                Value::Array(vec![
                    Value::Int(1),
                    Value::Array(vec![Value::Int(2), Value::Int(3)]),
                    Value::Array(vec![Value::Int(4), Value::Int(5)]),
                ]),
                "8301820203820405",
            ),
            (Value::Map(vec![]), "a0"),
            (
                Value::Map(vec![
                    (Value::Int(1), Value::Int(2)),
                    (Value::Int(3), Value::Int(4)),
                ]),
                "a201020304",
            ),
            (
                Value::Map(vec![
                    (Value::text("a"), Value::Int(1)),
                    (
                        Value::text("b"),
                        Value::Array(vec![Value::Int(2), Value::Int(3)]),
                    ),
                ]),
                "a26161016162820203",
            ),
            (
                Value::Map(vec![
                    (Value::text("a"), Value::text("A")),
                    (Value::text("b"), Value::text("B")),
                    (Value::text("c"), Value::text("C")),
                    (Value::text("d"), Value::text("D")),
                    (Value::text("e"), Value::text("E")),
                ]),
                "a56161614161626142616361436164614461656145",
            ),
        ];
        for (v, want) in cases {
            let bytes = encode(&v).unwrap();
            assert_eq!(hex(&bytes), want, "{v}");
            let back = decode(&bytes).unwrap();
            if let Value::Float(f) = v {
                assert!(
                    matches!(back, Value::Float(g) if g.to_bits() == f.to_bits() || (f.is_nan() && g.is_nan()))
                );
            } else {
                assert_eq!(back, v);
            }
        }
    }

    #[test]
    fn map_keys_sort_by_encoded_bytes() {
        // RFC 8949 §4.2.1: 10 < 100 < -1 < "z" < "aa" < [100] < [-1] < false, by bytes.
        let m = Value::Map(vec![
            (Value::Bool(false), Value::Int(0)),
            (Value::Array(vec![Value::Int(-1)]), Value::Int(0)),
            (Value::text("aa"), Value::Int(0)),
            (Value::Int(-1), Value::Int(0)),
            (Value::Array(vec![Value::Int(100)]), Value::Int(0)),
            (Value::text("z"), Value::Int(0)),
            (Value::Int(100), Value::Int(0)),
            (Value::Int(10), Value::Int(0)),
        ]);
        assert_eq!(
            hex(&encode(&m).unwrap()),
            "a8" // 8 pairs
                .to_string()
                + "0a00"
                + "186400"
                + "2000"
                + "617a00"
                + "62616100"
                + "81186400"
                + "812000"
                + "f400"
        );
        // Text keys: shorter first, then bytewise, which is also the model's key order.
        let m = Value::Map(vec![
            (Value::text("labels"), Value::Null),
            (Value::text("id"), Value::Null),
            (Value::text("P10"), Value::Null),
            (Value::text("P2"), Value::Null),
        ]);
        let bytes = encode(&m).unwrap();
        let Value::Map(pairs) = decode(&bytes).unwrap() else {
            panic!()
        };
        let keys: Vec<String> = pairs.iter().map(|(k, _)| k.to_string()).collect();
        assert_eq!(keys, ["\"P2\"", "\"id\"", "\"P10\"", "\"labels\""]);
        assert!(matches!(
            encode(&Value::Map(vec![
                (Value::text("a"), Value::Int(1)),
                (Value::text("a"), Value::Int(2))
            ])),
            Err(CborError::DuplicateKey)
        ));
    }

    #[test]
    fn strict_decoding_rejects_non_canonical_input() {
        let bad: Vec<(&str, &str)> = vec![
            ("1800", "0 in two bytes"),
            ("1900ff", "255 in three bytes"),
            ("fb3ff0000000000000", "1.0 as a double"),
            ("fa3f800000", "1.0 as a single"),
            ("a26162006161 00", "unsorted keys"),
            ("9f0102ff", "indefinite array"),
            ("5f4101ff", "indefinite bytes"),
            ("c074323031332d30332d32315432303a30343a30305a", "a tag"),
            ("f7", "undefined"),
            ("f0", "a simple value"),
            ("0100", "trailing byte"),
            ("f97e01", "a non-canonical NaN"),
            ("a2616100616100", "duplicate keys"),
        ];
        for (h, why) in bad {
            let bytes: Vec<u8> = (0..h.replace(' ', "").len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&h.replace(' ', "")[i..i + 2], 16).unwrap())
                .collect();
            assert!(decode(&bytes).is_err(), "{why}: {h}");
        }
        assert!(decode(&[]).is_err());
        assert!(decode(&[0x82, 0x01]).is_err(), "truncated");
    }

    #[test]
    fn json_round_trip_is_structural() {
        let j: serde_json::Value = serde_json::from_str(
            r#"{"id":"Q8","n":[1,-2,1.5,51.5,-0.1275,null,true],"labels":{"en":{"language":"en","value":"Douglas Adams"}},"big":18446744073709551615}"#,
        )
        .unwrap();
        let v = Value::from_json(&j);
        let bytes = encode(&v).unwrap();
        let back = decode(&bytes).unwrap().to_json().unwrap();
        assert_eq!(back, j);
        assert!(Value::Bytes(vec![1]).to_json().is_err());
        assert!(
            Value::Map(vec![(Value::Int(1), Value::Null)])
                .to_json()
                .is_err()
        );
        // 51.5 and -0.1275: the first fits a half, the second needs a double.
        assert_eq!(hex(&encode(&Value::Float(51.5)).unwrap()), "f95270");
        assert_eq!(
            hex(&encode(&Value::Float(-0.1275)).unwrap()),
            "fbbfc051eb851eb852"
        );
    }

    #[test]
    fn half_precision_edges() {
        assert_eq!(f16_bits(65504.0), Some(0x7bff));
        assert_eq!(f16_bits(65520.0), None, "rounds up to infinity: not exact");
        assert_eq!(
            f16_bits(5.960_464_477_539_063e-8),
            Some(0x0001),
            "smallest subnormal"
        );
        assert_eq!(
            f16_bits(2.980_232_238_769_531_3e-8),
            None,
            "half of it: not representable"
        );
        assert_eq!(
            f16_bits(0.000_061_035_156_25),
            Some(0x0400),
            "smallest normal"
        );
        assert_eq!(f16_bits(0.1), None);
        assert_eq!(f16_bits(-2.5), Some(0xc100));
        assert_eq!(f16_bits(2048.0), Some(0x6800));
        assert_eq!(f16_bits(2049.0), None, "needs 11 mantissa bits");
    }

    #[test]
    fn integer_range() {
        assert!(matches!(
            encode(&Value::Int(18_446_744_073_709_551_616)),
            Err(CborError::IntRange(_))
        ));
        assert!(matches!(
            encode(&Value::Int(-18_446_744_073_709_551_617)),
            Err(CborError::IntRange(_))
        ));
    }
}
