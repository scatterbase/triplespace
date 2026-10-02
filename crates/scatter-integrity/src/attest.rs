//! Signatures in the attestation part (0015 §1; 0040 §3): a client signature by the
//! actor's key over `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`, and an instance
//! attestation's signature by the instance key over
//! `H(0x06 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment) ‖ authority)`.
//!
//! Both have the shape `{"key": <key ID>, "alg": "ed25519", "sig": <64 bytes>}`; the
//! `authority` field tells them apart. Erasing the content, the comment or the
//! attestation makes a signature unverifiable, which 0006 §7 accepts: that is reported
//! as [`Attestation::Unverifiable`], not as a failure.

use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use scatter_log::cbor::{self, Value};
use scatter_log::hash::{self, Hash};
use scatter_log::record::Record;
use scatter_log::registry::ALG_ED25519;

/// What an attestation carries, as far as signatures go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attestation {
    /// No signature field.
    Unsigned,
    /// A part the signature covers was erased.
    Unverifiable,
    /// A client signature (0015 §1).
    Client(SignatureField),
    /// An instance attestation (0040 §3), with the canonical bytes of its `authority`.
    Instance {
        /// The signature.
        signature: SignatureField,
        /// The canonical CBOR of the `authority` map.
        authority: Vec<u8>,
    },
}

/// A `signature` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureField {
    /// The key ID.
    pub key: String,
    /// The algorithm, `ed25519`.
    pub alg: String,
    /// The signature.
    pub sig: Vec<u8>,
}

/// Why a signature is not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AttestError {
    /// The attestation part is not canonical CBOR, or not a map.
    #[error("attestation: {0}")]
    Shape(&'static str),
    /// A `signature` field without `key`, `alg` and 64-byte `sig`.
    #[error("attestation: malformed signature field")]
    Field,
    /// An algorithm this version does not verify.
    #[error("attestation: unsupported algorithm `{0}`")]
    Algorithm(String),
    /// No key with that ID is known to the verifier.
    #[error("attestation: unknown key `{0}`")]
    UnknownKey(String),
    /// The key is not a valid Ed25519 point.
    #[error("attestation: key `{0}` is not a valid public key")]
    PublicKey(String),
    /// The signature does not verify.
    #[error("attestation: signature by `{0}` does not verify")]
    BadSignature(String),
}

impl Attestation {
    /// Reads a record's attestation part.
    pub fn of(record: &Record) -> Result<Self, AttestError> {
        let body = record.body();
        let Some(bytes) = body.attestation().bytes() else {
            return Ok(Self::Unverifiable);
        };
        let v = cbor::decode(bytes).map_err(|_| AttestError::Shape("not canonical CBOR"))?;
        if !matches!(v, Value::Map(_)) {
            return Err(AttestError::Shape("not a map"));
        }
        let Some(sig) = v.get("signature") else {
            return Ok(Self::Unsigned);
        };
        let field = SignatureField {
            key: sig
                .get("key")
                .and_then(Value::as_text)
                .ok_or(AttestError::Field)?
                .to_string(),
            alg: sig
                .get("alg")
                .and_then(Value::as_text)
                .ok_or(AttestError::Field)?
                .to_string(),
            sig: sig
                .get("sig")
                .and_then(Value::as_bytes)
                .ok_or(AttestError::Field)?
                .to_vec(),
        };
        if field.sig.len() != 64 {
            return Err(AttestError::Field);
        }
        if body.content().is_erased() || body.comment().is_erased() {
            return Ok(Self::Unverifiable);
        }
        Ok(match v.get("authority") {
            Some(authority) => Self::Instance {
                signature: field,
                authority: cbor::encode(authority)
                    .map_err(|_| AttestError::Shape("authority does not encode"))?,
            },
            None => Self::Client(field),
        })
    }

    /// The signature field, if any.
    #[must_use]
    pub fn signature(&self) -> Option<&SignatureField> {
        match self {
            Self::Client(s) | Self::Instance { signature: s, .. } => Some(s),
            Self::Unsigned | Self::Unverifiable => None,
        }
    }

    /// The preimage the signature is over, for a record whose parts are present.
    #[must_use]
    pub fn preimage(&self, record: &Record) -> Option<Hash> {
        let body = record.body();
        let content = body.content().bytes()?;
        let comment = body.comment().bytes()?;
        match self {
            Self::Client(_) => Some(hash::signature_preimage(content, comment)),
            Self::Instance { authority, .. } => {
                Some(hash::instance_preimage(content, comment, authority))
            }
            Self::Unsigned | Self::Unverifiable => None,
        }
    }

    /// Checks the signature against the public key `lookup` gives for its key ID.
    ///
    /// `Ok(false)` means there was nothing to check (unsigned, or unverifiable after an
    /// erasure); `Ok(true)` that the signature verified.
    pub fn verify(
        &self,
        record: &Record,
        lookup: impl Fn(&str) -> Option<[u8; 32]>,
    ) -> Result<bool, AttestError> {
        let Some(field) = self.signature() else {
            return Ok(false);
        };
        let Some(preimage) = self.preimage(record) else {
            return Ok(false);
        };
        if field.alg != ALG_ED25519 {
            return Err(AttestError::Algorithm(field.alg.clone()));
        }
        let public =
            lookup(&field.key).ok_or_else(|| AttestError::UnknownKey(field.key.clone()))?;
        let key = VerifyingKey::from_bytes(&public)
            .map_err(|_| AttestError::PublicKey(field.key.clone()))?;
        let sig = Signature::from_slice(&field.sig).map_err(|_| AttestError::Field)?;
        key.verify(&preimage, &sig)
            .map_err(|_| AttestError::BadSignature(field.key.clone()))?;
        Ok(true)
    }
}

/// Signs a record's content and comment as a client (0015 §1): the `signature` map to
/// put in the attestation.
#[must_use]
pub fn client_signature(
    key: &ed25519_dalek::SigningKey,
    key_id: &str,
    content: &[u8],
    comment: &[u8],
) -> Value {
    use ed25519_dalek::Signer as _;
    let sig = key.sign(&hash::signature_preimage(content, comment));
    signature_value(key_id, &sig.to_bytes())
}

/// Signs an instance act (0040 §3): the `signature` map for an attestation that carries
/// `authority`, whose canonical bytes are `authority`.
#[must_use]
pub fn instance_signature(
    key: &ed25519_dalek::SigningKey,
    key_id: &str,
    content: &[u8],
    comment: &[u8],
    authority: &[u8],
) -> Value {
    use ed25519_dalek::Signer as _;
    let sig = key.sign(&hash::instance_preimage(content, comment, authority));
    signature_value(key_id, &sig.to_bytes())
}

fn signature_value(key_id: &str, sig: &[u8; 64]) -> Value {
    Value::map(vec![
        (Value::text("key"), Value::text(key_id)),
        (Value::text("alg"), Value::text(ALG_ED25519)),
        (Value::text("sig"), Value::Bytes(sig.to_vec())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use scatter_log::body::{ATTESTATION, Body, COMMENT, Part};
    use scatter_log::header::Header;
    use scatter_log::registry::key_id;

    fn header() -> Header {
        Header {
            version: 1,
            partition: 7,
            offset: 0,
            appended_at: 1,
            payload_type: "scatter:v0/changeset".into(),
            key: None,
            commitment: [0; 32],
            revid: None,
            logid: None,
            page_id: None,
        }
    }

    fn record(attestation: &Value) -> Record {
        let content =
            cbor::encode(&Value::map(vec![(Value::text("op"), Value::text("put"))])).unwrap();
        let comment = cbor::encode(&Value::text("hi")).unwrap();
        let body = Body::new(vec![
            Part::new(content),
            Part::new(comment),
            Part::of(attestation).unwrap(),
        ])
        .unwrap();
        Record::seal(header(), body)
    }

    #[test]
    fn client_signatures_verify() {
        let sk = SigningKey::from_bytes(&[1; 32]);
        let pk = sk.verifying_key().to_bytes();
        let id = key_id(&pk);
        let content =
            cbor::encode(&Value::map(vec![(Value::text("op"), Value::text("put"))])).unwrap();
        let comment = cbor::encode(&Value::text("hi")).unwrap();
        let sig = client_signature(&sk, &id, &content, &comment);
        let r = record(&Value::map(vec![
            (Value::text("actor"), Value::text("local:42")),
            (Value::text("signature"), sig),
        ]));
        let a = Attestation::of(&r).unwrap();
        assert!(matches!(a, Attestation::Client(_)));
        let lookup = |k: &str| (k == id).then_some(pk);
        assert_eq!(a.verify(&r, lookup), Ok(true));
        assert!(matches!(
            a.verify(&r, |_| None),
            Err(AttestError::UnknownKey(_))
        ));
        let other = SigningKey::from_bytes(&[2; 32]).verifying_key().to_bytes();
        assert!(matches!(
            a.verify(&r, |_| Some(other)),
            Err(AttestError::BadSignature(_))
        ));

        // Unsigned records have nothing to check.
        let plain = record(&Value::map(vec![(
            Value::text("actor"),
            Value::text("local:42"),
        )]));
        let a = Attestation::of(&plain).unwrap();
        assert_eq!(a, Attestation::Unsigned);
        assert_eq!(a.verify(&plain, lookup), Ok(false));

        // Erasing the comment makes it unverifiable, not wrong.
        let mut erased = r.clone();
        erased.body_mut().erase(COMMENT).unwrap();
        assert_eq!(Attestation::of(&erased).unwrap(), Attestation::Unverifiable);
        let mut erased = r;
        erased.body_mut().erase(ATTESTATION).unwrap();
        assert_eq!(Attestation::of(&erased).unwrap(), Attestation::Unverifiable);
    }

    #[test]
    fn instance_attestations_bind_the_authority() {
        let sk = SigningKey::from_bytes(&[3; 32]);
        let pk = sk.verifying_key().to_bytes();
        let id = key_id(&pk);
        let content =
            cbor::encode(&Value::map(vec![(Value::text("op"), Value::text("put"))])).unwrap();
        let comment = cbor::encode(&Value::text("hi")).unwrap();
        let authority = Value::map(vec![
            (Value::text("partition"), Value::Int(7)),
            (Value::text("offset"), Value::Int(3)),
            (Value::text("leaf"), Value::Bytes(vec![5; 32])),
        ]);
        let authority_bytes = cbor::encode(&authority).unwrap();
        let sig = instance_signature(&sk, &id, &content, &comment, &authority_bytes);
        let r = record(&Value::map(vec![
            (Value::text("actor"), Value::text("instance:scatter")),
            (Value::text("authority"), authority.clone()),
            (Value::text("binding"), Value::text("prerogative")),
            (Value::text("signature"), sig.clone()),
        ]));
        let a = Attestation::of(&r).unwrap();
        assert!(matches!(a, Attestation::Instance { .. }));
        assert_eq!(a.verify(&r, |_| Some(pk)), Ok(true));

        // The same signature under a different authority fails.
        let mut other_authority = authority;
        if let Value::Map(p) = &mut other_authority {
            p.iter_mut().find(|(k, _)| k.text_eq("offset")).unwrap().1 = Value::Int(4);
        }
        let r2 = record(&Value::map(vec![
            (Value::text("actor"), Value::text("instance:scatter")),
            (Value::text("authority"), other_authority),
            (Value::text("signature"), sig),
        ]));
        let a2 = Attestation::of(&r2).unwrap();
        assert!(matches!(
            a2.verify(&r2, |_| Some(pk)),
            Err(AttestError::BadSignature(_))
        ));
    }

    #[test]
    fn malformed_fields() {
        let r = record(&Value::map(vec![(
            Value::text("signature"),
            Value::map(vec![(Value::text("key"), Value::text("k"))]),
        )]));
        assert_eq!(Attestation::of(&r), Err(AttestError::Field));
        let r = record(&Value::map(vec![(
            Value::text("signature"),
            Value::map(vec![
                (Value::text("key"), Value::text("k")),
                (Value::text("alg"), Value::text("rsa")),
                (Value::text("sig"), Value::Bytes(vec![0; 64])),
            ]),
        )]));
        let a = Attestation::of(&r).unwrap();
        assert!(matches!(
            a.verify(&r, |_| Some([0; 32])),
            Err(AttestError::Algorithm(_))
        ));
        let r = record(&Value::Array(vec![]));
        assert_eq!(Attestation::of(&r), Err(AttestError::Shape("not a map")));
    }
}
