//! The record header (0006 §3; 0015 §2; 0018 §2): a CBOR array of ten fields that the
//! Merkle tree commits to and that is never erased. It must not contain personal data.

use crate::cbor::{self, CborError, Value};

/// The format version this crate writes (header field 0).
pub const FORMAT_VERSION: u64 = 1;

/// The partition ID of the instance `config` partition (0015 §3).
pub const CONFIG_PARTITION: u64 = 0;

/// A record header.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Header {
    /// Field 0: the format version.
    pub version: u64,
    /// Field 1: the partition, 64 random bits, or 0 for the instance `config` partition.
    pub partition: u64,
    /// Field 2: the position in the partition, from 0 with no gaps.
    pub offset: u64,
    /// Field 3: microseconds since the Unix epoch, UTC.
    pub appended_at: u64,
    /// Field 4: the payload type, `scatter:v0/changeset`.
    pub payload_type: String,
    /// Field 5: the key, an identifier and never content, or none.
    pub key: Option<String>,
    /// Field 6: the body commitment.
    pub commitment: [u8; 32],
    /// Field 7: the global revision ID, or none.
    pub revid: Option<u64>,
    /// Field 8: the global log ID, or none.
    pub logid: Option<u64>,
    /// Field 9: the page ID of the record's key, or none.
    pub page_id: Option<u64>,
}

/// Why bytes are not a header.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HeaderError {
    /// Not canonical CBOR.
    #[error(transparent)]
    Cbor(#[from] CborError),
    /// Not an array of ten fields of the right types.
    #[error("header: {0}")]
    Shape(&'static str),
    /// A format version this crate does not read.
    #[error("header: format version {0} is not supported")]
    Version(u64),
}

fn opt_u64(v: &Value, what: &'static str) -> Result<Option<u64>, HeaderError> {
    match v {
        Value::Null => Ok(None),
        Value::Int(i) => u64::try_from(*i)
            .map(Some)
            .map_err(|_| HeaderError::Shape(what)),
        _ => Err(HeaderError::Shape(what)),
    }
}

fn req_u64(v: &Value, what: &'static str) -> Result<u64, HeaderError> {
    match v {
        Value::Int(i) => u64::try_from(*i).map_err(|_| HeaderError::Shape(what)),
        _ => Err(HeaderError::Shape(what)),
    }
}

impl Header {
    /// The header as a CBOR value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let opt = |o: Option<u64>| o.map_or(Value::Null, |n| Value::Int(i128::from(n)));
        Value::Array(vec![
            Value::Int(i128::from(self.version)),
            Value::Int(i128::from(self.partition)),
            Value::Int(i128::from(self.offset)),
            Value::Int(i128::from(self.appended_at)),
            Value::Text(self.payload_type.clone()),
            self.key.clone().map_or(Value::Null, Value::Text),
            Value::Bytes(self.commitment.to_vec()),
            opt(self.revid),
            opt(self.logid),
            opt(self.page_id),
        ])
    }

    /// The canonical bytes: what the Merkle leaf hashes.
    ///
    /// # Panics
    ///
    /// Never: every field encodes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_value()).expect("a header encodes")
    }

    /// Parses canonical bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, HeaderError> {
        Self::from_value(&cbor::decode(bytes)?)
    }

    /// Reads a header from a CBOR value.
    pub fn from_value(v: &Value) -> Result<Self, HeaderError> {
        let Value::Array(f) = v else {
            return Err(HeaderError::Shape("not an array"));
        };
        if f.len() != 10 {
            return Err(HeaderError::Shape("not ten fields"));
        }
        let version = req_u64(&f[0], "version")?;
        if version != FORMAT_VERSION {
            return Err(HeaderError::Version(version));
        }
        let Value::Text(payload_type) = &f[4] else {
            return Err(HeaderError::Shape("payload type"));
        };
        let key = match &f[5] {
            Value::Null => None,
            Value::Text(k) => Some(k.clone()),
            _ => return Err(HeaderError::Shape("key")),
        };
        let Value::Bytes(c) = &f[6] else {
            return Err(HeaderError::Shape("commitment"));
        };
        let commitment: [u8; 32] = c
            .as_slice()
            .try_into()
            .map_err(|_| HeaderError::Shape("commitment is not 32 bytes"))?;
        Ok(Self {
            version,
            partition: req_u64(&f[1], "partition")?,
            offset: req_u64(&f[2], "offset")?,
            appended_at: req_u64(&f[3], "appended_at")?,
            payload_type: payload_type.clone(),
            key,
            commitment,
            revid: opt_u64(&f[7], "revid")?,
            logid: opt_u64(&f[8], "logid")?,
            page_id: opt_u64(&f[9], "page_id")?,
        })
    }

    /// The Merkle leaf of this header.
    #[must_use]
    pub fn leaf(&self) -> [u8; 32] {
        crate::hash::merkle_leaf(&self.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_fields_round_trip() {
        let h = Header {
            version: 1,
            partition: 12_345_678_901_234_567_890,
            offset: 41,
            appended_at: 1_790_000_000_000_042,
            payload_type: "scatter:v0/changeset".into(),
            key: Some("Q6".into()),
            commitment: [7; 32],
            revid: Some(900),
            logid: None,
            page_id: Some(12),
        };
        let bytes = h.encode();
        assert_eq!(bytes[0], 0x8a, "an array of ten");
        assert_eq!(Header::decode(&bytes).unwrap(), h);
        let mut v2 = h.clone();
        v2.version = 2;
        assert!(matches!(
            Header::decode(&v2.encode()),
            Err(HeaderError::Version(2))
        ));
        assert!(matches!(
            Header::from_value(&Value::Array(vec![Value::Int(1)])),
            Err(HeaderError::Shape(_))
        ));
        let mut short = h.to_value();
        if let Value::Array(f) = &mut short {
            f[6] = Value::Bytes(vec![1, 2]);
        }
        assert!(matches!(
            Header::from_value(&short),
            Err(HeaderError::Shape(_))
        ));
    }
}
