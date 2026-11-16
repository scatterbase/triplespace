//! Signing keys (0015 §1; 0024 §4): the `scatter:v0/key` payload, the current key of an
//! actor, and signature verification.
//!
//! A subsidiary may hold Ed25519 signing keys whose public half is registered as a
//! `scatter:v0/key` record in the tenant's `actors` partition, keyed by the actor. A later
//! record under the same key ID rotates or revokes it. A write submitted with a signature
//! is verified against the actor's current key before the append; the signature is over
//! `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`, the two inner hashes being the
//! content hashes of 0006 §2 that `scatter-log` computes. A signature never grants
//! anything.
//!
//! The **key ID** is the Base32z (z-base-32) encoding of the SHA-256 of the public key,
//! the text form 0006 §2 gives identifiers.

use std::fmt;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::time::Timestamp;

/// The one algorithm (0033 §6).
pub const ED25519: &str = "ed25519";

/// The domain tag of a client signature's preimage (0006 §2, as extended by 0015 §1).
pub const SIGNATURE_TAG: u8 = 0x05;

/// A 32-byte public key, serialized as bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey(pub [u8; 32]);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", zbase32(&self.0))
    }
}

impl Serialize for PublicKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = PublicKey;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("32 bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                <[u8; 32]>::try_from(v)
                    .map(PublicKey)
                    .map_err(|_| E::invalid_length(v.len(), &self))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut v = Vec::with_capacity(32);
                while let Some(b) = seq.next_element::<u8>()? {
                    v.push(b);
                }
                self.visit_bytes(&v)
            }
        }
        d.deserialize_bytes(V)
    }
}

/// The content part of a `scatter:v0/key` record, keyed by the actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRecord {
    /// The key ID: Base32z of SHA-256 of the public key.
    pub key_id: String,
    /// `ed25519`.
    pub alg: String,
    /// The public key.
    pub public_key: PublicKey,
    /// Valid from this instant.
    pub valid_from: Timestamp,
    /// Valid until this instant, if the key expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<Timestamp>,
    /// Revoked: a later record for the same key ID with this set ends its validity at
    /// once. A revoked record keeps the public key so that older signatures stay checkable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub revoked: bool,
}

/// Why a key record or a signature is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SigningError {
    /// The key ID is not the hash of the public key.
    #[error("key ID `{0}` does not match the public key (expected `{1}`)")]
    KeyId(String, String),
    /// An algorithm other than `ed25519`.
    #[error("unsupported signature algorithm `{0}`")]
    Algorithm(String),
    /// The public key bytes are not a valid Ed25519 point.
    #[error("the public key is not a valid Ed25519 key")]
    PublicKey,
    /// The actor has no current key, or none with the signature's key ID.
    #[error("no current signing key `{0}` for this actor")]
    NoKey(String),
    /// The key is outside its validity, or revoked.
    #[error("signing key `{0}` is not valid at {1}")]
    NotValid(String, Timestamp),
    /// The signature does not verify (`ts-bad-signature`).
    #[error("the signature does not verify against key `{0}`")]
    BadSignature(String),
}

impl KeyRecord {
    /// A registration of `public_key`, valid from `valid_from`.
    pub fn register(
        public_key: PublicKey,
        valid_from: Timestamp,
        valid_until: Option<Timestamp>,
    ) -> Result<Self, SigningError> {
        VerifyingKey::from_bytes(&public_key.0).map_err(|_| SigningError::PublicKey)?;
        Ok(Self {
            key_id: key_id(&public_key),
            alg: ED25519.to_string(),
            public_key,
            valid_from,
            valid_until,
            revoked: false,
        })
    }

    /// The record that revokes this key.
    #[must_use]
    pub fn revoke(&self) -> Self {
        Self {
            revoked: true,
            ..self.clone()
        }
    }

    /// Checks the record's own consistency: the algorithm, the key ID and the key.
    pub fn validate(&self) -> Result<(), SigningError> {
        if self.alg != ED25519 {
            return Err(SigningError::Algorithm(self.alg.clone()));
        }
        VerifyingKey::from_bytes(&self.public_key.0).map_err(|_| SigningError::PublicKey)?;
        let expected = key_id(&self.public_key);
        if self.key_id != expected {
            return Err(SigningError::KeyId(self.key_id.clone(), expected));
        }
        Ok(())
    }

    /// Whether the key is valid at `at`: registered, not expired, not revoked.
    #[must_use]
    pub fn is_valid_at(&self, at: Timestamp) -> bool {
        !self.revoked && self.valid_from <= at && self.valid_until.is_none_or(|u| at < u)
    }

    /// Verifies `signature` over `message` with this key, which must be valid at `at`.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &[u8; 64],
        at: Timestamp,
    ) -> Result<(), SigningError> {
        self.validate()?;
        if !self.is_valid_at(at) {
            return Err(SigningError::NotValid(self.key_id.clone(), at));
        }
        let vk =
            VerifyingKey::from_bytes(&self.public_key.0).map_err(|_| SigningError::PublicKey)?;
        vk.verify_strict(message, &Signature::from_bytes(signature))
            .map_err(|_| SigningError::BadSignature(self.key_id.clone()))
    }
}

/// The key ID of a public key.
#[must_use]
pub fn key_id(public_key: &PublicKey) -> String {
    zbase32(&sha2::Sha256::digest(public_key.0))
}

/// The current key records of an actor, from its `scatter:v0/key` records in log order:
/// the latest record per key ID wins, and a revoked one is dropped. Keys that have expired
/// are kept, since [`KeyRecord::is_valid_at`] decides by time.
#[must_use]
pub fn current_keys(records: &[KeyRecord]) -> Vec<&KeyRecord> {
    let mut latest: Vec<&KeyRecord> = Vec::new();
    for r in records {
        latest.retain(|k| k.key_id != r.key_id);
        latest.push(r);
    }
    latest.into_iter().filter(|k| !k.revoked).collect()
}

/// Verifies a submitted signature against the actor's key records (0015 §1): the key named
/// by `key_id` must be current and valid at `at`, and the signature must verify over
/// `message`, which is [`signature_preimage`] of the record's parts.
pub fn verify_submitted(
    records: &[KeyRecord],
    key_id: &str,
    message: &[u8],
    signature: &[u8; 64],
    at: Timestamp,
) -> Result<(), SigningError> {
    let key = current_keys(records)
        .into_iter()
        .find(|k| k.key_id == key_id)
        .ok_or_else(|| SigningError::NoKey(key_id.to_string()))?;
    key.verify(message, signature, at)
}

/// The preimage a client signs: `H(0x05 ‖ content_hash ‖ comment_hash)`, where the two
/// hashes are `H(0x03 ‖ part)` of the canonical content and comment parts (0006 §2).
#[must_use]
pub fn signature_preimage(content_hash: &[u8; 32], comment_hash: &[u8; 32]) -> [u8; 32] {
    let mut h = sha2::Sha256::new();
    h.update([SIGNATURE_TAG]);
    h.update(content_hash);
    h.update(comment_hash);
    h.finalize().into()
}

/// z-base-32 (0006 §2: the text form of identifiers), as Scatterbase writes it: the
/// alphabet `ybndrfg8ejkmcpqxot1uwisza345h769`, no padding.
#[must_use]
pub fn zbase32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ybndrfg8ejkmcpqxot1uwisza345h769";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u64 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        buffer = (buffer << 8) | u64::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn keypair(seed: u8) -> (SigningKey, PublicKey) {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let pk = PublicKey(sk.verifying_key().to_bytes());
        (sk, pk)
    }

    #[test]
    fn zbase32_vectors() {
        // From the z-base-32 specification and common test vectors.
        assert_eq!(zbase32(b""), "");
        assert_eq!(zbase32(&[0x00]), "yy");
        assert_eq!(zbase32(&[0xff]), "9h");
        assert_eq!(zbase32(b"hello"), "pb1sa5dx");
        // Byte-oriented: three bytes are 24 bits, five characters.
        assert_eq!(zbase32(&[0x10, 0x11, 0x10]), "nyety");
    }

    #[test]
    fn register_rotate_revoke_and_verify() {
        let (sk, pk) = keypair(1);
        let reg = KeyRecord::register(pk, Timestamp(100), None).unwrap();
        assert_eq!(reg.key_id, key_id(&pk));
        assert_eq!(reg.key_id.len(), 52, "256 bits in z-base-32");
        reg.validate().unwrap();
        let msg = signature_preimage(&[1; 32], &[2; 32]);
        let sig = sk.sign(&msg).to_bytes();
        assert!(
            verify_submitted(
                std::slice::from_ref(&reg),
                &reg.key_id,
                &msg,
                &sig,
                Timestamp(150)
            )
            .is_ok()
        );
        assert!(matches!(
            verify_submitted(
                std::slice::from_ref(&reg),
                &reg.key_id,
                &msg,
                &sig,
                Timestamp(50)
            ),
            Err(SigningError::NotValid(..))
        ));
        let mut tampered = msg;
        tampered[0] ^= 1;
        assert!(matches!(
            verify_submitted(
                std::slice::from_ref(&reg),
                &reg.key_id,
                &tampered,
                &sig,
                Timestamp(150)
            ),
            Err(SigningError::BadSignature(_))
        ));
        // Rotation: a second key; both current until the first is revoked.
        let (sk2, pk2) = keypair(2);
        let reg2 = KeyRecord::register(pk2, Timestamp(200), Some(Timestamp(300))).unwrap();
        let records = vec![reg.clone(), reg2.clone()];
        assert_eq!(current_keys(&records).len(), 2);
        let sig2 = sk2.sign(&msg).to_bytes();
        assert!(verify_submitted(&records, &reg2.key_id, &msg, &sig2, Timestamp(250)).is_ok());
        assert!(
            matches!(
                verify_submitted(&records, &reg2.key_id, &msg, &sig2, Timestamp(300)),
                Err(SigningError::NotValid(..))
            ),
            "expired at valid_until"
        );
        let records = vec![reg.clone(), reg2, reg.revoke()];
        assert_eq!(current_keys(&records).len(), 1);
        assert!(matches!(
            verify_submitted(&records, &reg.key_id, &msg, &sig, Timestamp(150)),
            Err(SigningError::NoKey(_))
        ));
        // A key signed with another key's ID does not verify.
        assert!(matches!(
            verify_submitted(&records, "nope", &msg, &sig, Timestamp(150)),
            Err(SigningError::NoKey(_))
        ));
    }

    #[test]
    fn record_consistency() {
        let (_, pk) = keypair(3);
        let mut r = KeyRecord::register(pk, Timestamp(0), None).unwrap();
        r.key_id = "wrong".into();
        assert!(matches!(r.validate(), Err(SigningError::KeyId(..))));
        let mut r = KeyRecord::register(pk, Timestamp(0), None).unwrap();
        r.alg = "rsa".into();
        assert!(matches!(r.validate(), Err(SigningError::Algorithm(_))));
        // Serde: the public key is bytes, which JSON renders as an array.
        let r = KeyRecord::register(pk, Timestamp(7), None).unwrap();
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"alg\":\"ed25519\"") && !json.contains("revoked"));
        let back: KeyRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
