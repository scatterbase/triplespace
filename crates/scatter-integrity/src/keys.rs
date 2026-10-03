//! The key chain (0006 §6; 0015 §3; 0018 §2, §10): the `key:` records of a `config`
//! partition, in order. The first is the partition's trust anchor; each later one is a
//! rotation, whose attestation must carry a signature by the key before it over the
//! new record's preimage. A tenant move is a rotation naming the new instance's key and
//! the tenant's final checkpoint.

use scatter_log::record::Record;
use scatter_log::registry::{ALG_ED25519, KeyEntry, PAYLOAD_CONFIG};

use crate::attest::{AttestError, Attestation};
use crate::note::NoteVerifier;

/// One link of the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainKey {
    /// The entry.
    pub entry: KeyEntry,
    /// The partition and offset of the record that introduced it.
    pub partition: u64,
    /// The offset.
    pub offset: u64,
    /// Header field 3 of that record: when the key became current.
    pub since: u64,
}

impl ChainKey {
    /// The public key as 32 bytes, when it is one.
    #[must_use]
    pub fn public_key(&self) -> Option<[u8; 32]> {
        self.entry.public_key.as_slice().try_into().ok()
    }
}

/// Why a record does not extend the chain.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ChainError {
    /// Not a `scatter:v0/config` record with a `key:` key.
    #[error("key chain: offset {0} is not a key record")]
    NotAKey(u64),
    /// The content part is erased or not a key entry.
    #[error("key chain: offset {0}: {1}")]
    Entry(u64, String),
    /// The key ID is not the hash of the public key, or the algorithm is unknown.
    #[error("key chain: offset {0}: key `{1}` is inconsistent")]
    Key(u64, String),
    /// A rotation is not signed by the previous key.
    #[error("key chain: offset {0}: rotation is not signed by the previous key `{1}`")]
    Rotation(u64, String),
    /// The rotation's signature is malformed or wrong.
    #[error("key chain: offset {0}: {1}")]
    Signature(u64, AttestError),
}

/// The key chain of one `config` partition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyChain {
    keys: Vec<ChainKey>,
}

impl KeyChain {
    /// An empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `record` is a `key:` record.
    #[must_use]
    pub fn is_key_record(record: &Record) -> bool {
        record.header().payload_type == PAYLOAD_CONFIG
            && record
                .header()
                .key
                .as_deref()
                .is_some_and(|k| k.starts_with("key:"))
    }

    /// Extends the chain with a `key:` record, checking the rotation signature when
    /// there is a previous key.
    pub fn push(&mut self, record: &Record) -> Result<&ChainKey, ChainError> {
        let h = record.header();
        if !Self::is_key_record(record) {
            return Err(ChainError::NotAKey(h.offset));
        }
        let content = record
            .body()
            .content()
            .value()
            .map_err(|e| ChainError::Entry(h.offset, e.to_string()))?
            .ok_or_else(|| ChainError::Entry(h.offset, "content erased".into()))?;
        let entry = KeyEntry::from_value(&content)
            .map_err(|e| ChainError::Entry(h.offset, e.to_string()))?;
        if h.key.as_deref() != Some(entry.key().as_str()) {
            return Err(ChainError::Entry(
                h.offset,
                "header key does not name the entry".into(),
            ));
        }
        if entry.alg != ALG_ED25519 || !entry.id_matches() || entry.public_key.len() != 32 {
            return Err(ChainError::Key(h.offset, entry.key_id));
        }
        if let Some(previous) = self.keys.last() {
            let attestation =
                Attestation::of(record).map_err(|e| ChainError::Signature(h.offset, e))?;
            let signer = attestation.signature().map(|s| s.key.clone());
            if signer.as_deref() != Some(previous.entry.key_id.as_str()) {
                return Err(ChainError::Rotation(
                    h.offset,
                    previous.entry.key_id.clone(),
                ));
            }
            let public = previous.public_key();
            let verified = attestation
                .verify(record, |_| public)
                .map_err(|e| ChainError::Signature(h.offset, e))?;
            if !verified {
                return Err(ChainError::Rotation(
                    h.offset,
                    previous.entry.key_id.clone(),
                ));
            }
        }
        self.keys.push(ChainKey {
            entry,
            partition: h.partition,
            offset: h.offset,
            since: h.appended_at,
        });
        Ok(self.keys.last().expect("just pushed"))
    }

    /// Builds a chain from the `key:` records among `records`, in offset order; other
    /// records are skipped.
    pub fn from_records<'a>(
        records: impl IntoIterator<Item = &'a Record>,
    ) -> Result<Self, ChainError> {
        let mut chain = Self::new();
        for r in records {
            if Self::is_key_record(r) {
                chain.push(r)?;
            }
        }
        Ok(chain)
    }

    /// The keys, oldest first.
    #[must_use]
    pub fn keys(&self) -> &[ChainKey] {
        &self.keys
    }

    /// The current key: the last one.
    #[must_use]
    pub fn current(&self) -> Option<&ChainKey> {
        self.keys.last()
    }

    /// The key current at a time: the last one whose record was appended at or before
    /// `at`.
    #[must_use]
    pub fn current_at(&self, at: u64) -> Option<&ChainKey> {
        self.keys.iter().rev().find(|k| k.since <= at)
    }

    /// The key with an ID.
    #[must_use]
    pub fn get(&self, key_id: &str) -> Option<&ChainKey> {
        self.keys.iter().find(|k| k.entry.key_id == key_id)
    }

    /// The public key for an ID, in the form [`Attestation::verify`] takes.
    #[must_use]
    pub fn lookup(&self, key_id: &str) -> Option<[u8; 32]> {
        self.get(key_id).and_then(ChainKey::public_key)
    }

    /// Every key as a note verifier under `origin`, so a checkpoint signed by any key
    /// of the chain verifies (0006 §6: checkpoints identify their key by the note's key
    /// hash).
    #[must_use]
    pub fn verifiers(&self, origin: &str) -> Vec<NoteVerifier> {
        self.keys
            .iter()
            .filter_map(|k| NoteVerifier::new(origin, k.public_key()?).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use scatter_log::body::{Body, COMMENT};
    use scatter_log::cbor::{self, Value};
    use scatter_log::header::Header;

    use crate::attest::client_signature;

    fn key_record(offset: u64, entry: &KeyEntry, signer: Option<(&SigningKey, &str)>) -> Record {
        let content = cbor::encode(&entry.to_value()).unwrap();
        let comment = cbor::encode(&Value::text("rotation")).unwrap();
        let mut attestation = vec![(Value::text("actor"), Value::text("instance:scatter"))];
        if let Some((sk, id)) = signer {
            attestation.push((
                Value::text("signature"),
                client_signature(sk, id, &content, &comment),
            ));
        }
        let body = Body::core(
            &entry.to_value(),
            &Value::text("rotation"),
            &Value::map(attestation),
        )
        .unwrap();
        let header = Header {
            version: 1,
            partition: 0,
            offset,
            appended_at: 1000 + offset,
            payload_type: PAYLOAD_CONFIG.into(),
            key: Some(entry.key()),
            commitment: [0; 32],
            revid: None,
            logid: None,
            page_id: None,
        };
        Record::seal(header, body)
    }

    #[test]
    fn rotations_must_be_signed_by_the_previous_key() {
        let a = SigningKey::from_bytes(&[1; 32]);
        let b = SigningKey::from_bytes(&[2; 32]);
        let c = SigningKey::from_bytes(&[3; 32]);
        let ea = KeyEntry::ed25519(a.verifying_key().to_bytes());
        let eb = KeyEntry::ed25519(b.verifying_key().to_bytes());
        let ec = KeyEntry::ed25519(c.verifying_key().to_bytes());

        let r0 = key_record(0, &ea, None);
        let r1 = key_record(1, &eb, Some((&a, &ea.key_id)));
        let r2 = key_record(2, &ec, Some((&b, &eb.key_id)));
        let chain = KeyChain::from_records([&r0, &r1, &r2]).unwrap();
        assert_eq!(chain.keys().len(), 3);
        assert_eq!(chain.current().unwrap().entry, ec);
        assert_eq!(chain.current_at(1001).unwrap().entry, eb);
        assert_eq!(chain.current_at(999), None);
        assert_eq!(chain.lookup(&ea.key_id), Some(a.verifying_key().to_bytes()));
        assert_eq!(chain.verifiers("h/log/config").len(), 3);

        // Signed by the wrong key.
        let bad = key_record(2, &ec, Some((&a, &ea.key_id)));
        assert!(matches!(
            KeyChain::from_records([&r0, &r1, &bad]),
            Err(ChainError::Rotation(2, _))
        ));
        // Unsigned rotation.
        let unsigned = key_record(1, &eb, None);
        assert!(matches!(
            KeyChain::from_records([&r0, &unsigned]),
            Err(ChainError::Rotation(1, _))
        ));
        // Claims the previous key but the signature is wrong.
        let forged = key_record(1, &eb, Some((&c, &ea.key_id)));
        assert!(matches!(
            KeyChain::from_records([&r0, &forged]),
            Err(ChainError::Signature(1, AttestError::BadSignature(_)))
        ));
        // An erased comment makes the rotation unverifiable, which is a chain failure.
        let mut erased = r1.clone();
        erased.body_mut().erase(COMMENT).unwrap();
        assert!(matches!(
            KeyChain::from_records([&r0, &erased]),
            Err(ChainError::Rotation(1, _))
        ));
        // A key ID that does not match its public key.
        let mut wrong = ea.clone();
        wrong.key_id = eb.key_id.clone();
        assert!(matches!(
            KeyChain::from_records([&key_record(0, &wrong, None)]),
            Err(ChainError::Key(0, _))
        ));
    }
}
