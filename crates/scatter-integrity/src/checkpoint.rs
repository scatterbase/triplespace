//! [C2SP tlog-checkpoints](https://c2sp.org/tlog-checkpoint) (0006 §6): the origin, the
//! tree size and the root, signed as a note by the instance key.
//!
//! ```text
//! example.org/log/local
//! 42
//! <base64 root>
//!
//! — example.org/log/local <base64 signature>
//! ```
//!
//! No extension lines (0006 §6). The signature name is the origin. A segment manifest in
//! a `hashed` partition is a checkpoint whose origin is `{origin}/segment/{n}`, whose size
//! is the segment's record count and whose root is the segment's own tree head.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use scatter_log::hash::Hash;

use crate::note::{Note, NoteError, NoteSigner, NoteVerifier};

/// The origin line of a tenant partition: `{host}/log/{name}` (0006 §6; 0015 §5).
#[must_use]
pub fn tenant_origin(host: &str, partition_name: &str) -> String {
    format!("{host}/log/{partition_name}")
}

/// The origin line of an instance partition: `{farm host}/instance/log/{name}` (0046 §7).
#[must_use]
pub fn instance_origin(farm_host: &str, partition_name: &str) -> String {
    format!("{farm_host}/instance/log/{partition_name}")
}

/// The origin line of segment `n`'s manifest in a `hashed` partition.
#[must_use]
pub fn segment_origin(origin: &str, n: u64) -> String {
    format!("{origin}/segment/{n}")
}

/// Why text is not a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckpointError {
    /// The note is wrong.
    #[error(transparent)]
    Note(#[from] NoteError),
    /// The body is not three lines of the right shapes.
    #[error("checkpoint: {0}")]
    Body(&'static str),
    /// The origin is not the one expected.
    #[error("checkpoint: origin `{found}`, expected `{expected}`")]
    Origin {
        /// What the checkpoint says.
        found: String,
        /// What the verifier wanted.
        expected: String,
    },
}

/// A tree head: what a checkpoint says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// The origin line.
    pub origin: String,
    /// The tree size.
    pub size: u64,
    /// The root hash.
    pub root: Hash,
}

impl Checkpoint {
    /// The checkpoint's body: the note text.
    #[must_use]
    pub fn body(&self) -> String {
        format!(
            "{}\n{}\n{}\n",
            self.origin,
            self.size,
            BASE64.encode(self.root)
        )
    }

    /// Parses a body, the three lines before the blank line.
    pub fn parse_body(text: &str) -> Result<Self, CheckpointError> {
        let mut lines = text.split_terminator('\n');
        let origin = lines
            .next()
            .ok_or(CheckpointError::Body("no origin line"))?;
        if origin.is_empty() || origin.chars().any(char::is_whitespace) {
            return Err(CheckpointError::Body("origin"));
        }
        let size = lines.next().ok_or(CheckpointError::Body("no size line"))?;
        if size.is_empty()
            || !size.bytes().all(|b| b.is_ascii_digit())
            || (size.len() > 1 && size.starts_with('0'))
        {
            return Err(CheckpointError::Body("size"));
        }
        let size: u64 = size.parse().map_err(|_| CheckpointError::Body("size"))?;
        let root = lines.next().ok_or(CheckpointError::Body("no root line"))?;
        let root: Hash = BASE64
            .decode(root)
            .ok()
            .and_then(|r| r.try_into().ok())
            .ok_or(CheckpointError::Body("root"))?;
        if lines.next().is_some() {
            return Err(CheckpointError::Body("extension lines are not used"));
        }
        if !text.ends_with('\n') {
            return Err(CheckpointError::Body("the body ends with a newline"));
        }
        Ok(Self {
            origin: origin.to_string(),
            size,
            root,
        })
    }

    /// Signs the checkpoint with the instance key, under the origin as the key name.
    pub fn sign(
        &self,
        key: &ed25519_dalek::SigningKey,
    ) -> Result<SignedCheckpoint, CheckpointError> {
        let signer = NoteSigner::new(&self.origin, key.clone())?;
        Ok(SignedCheckpoint {
            checkpoint: self.clone(),
            note: signer.sign(&self.body())?,
        })
    }
}

/// A checkpoint with its signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedCheckpoint {
    /// What it says.
    pub checkpoint: Checkpoint,
    /// The note it is signed in.
    pub note: Note,
}

impl SignedCheckpoint {
    /// Parses a signed checkpoint's bytes without verifying any signature.
    pub fn parse(bytes: &[u8]) -> Result<Self, CheckpointError> {
        let note = Note::parse(bytes)?;
        let checkpoint = Checkpoint::parse_body(&note.text)?;
        Ok(Self { checkpoint, note })
    }

    /// The bytes.
    #[must_use]
    pub fn to_text(&self) -> String {
        self.note.to_text()
    }

    /// Checks the origin and that one of `verifiers` signed it.
    pub fn verify(&self, origin: &str, verifiers: &[NoteVerifier]) -> Result<(), CheckpointError> {
        if self.checkpoint.origin != origin {
            return Err(CheckpointError::Origin {
                found: self.checkpoint.origin.clone(),
                expected: origin.to_string(),
            });
        }
        self.note.verify(verifiers)?;
        Ok(())
    }

    /// Adds a cosignature, as a witness would.
    pub fn cosign(&mut self, signer: &NoteSigner) -> Result<(), CheckpointError> {
        self.note.add_signature(signer)?;
        Ok(())
    }
}

/// The verifier for an origin and an instance public key.
pub fn verifier_for(origin: &str, public_key: [u8; 32]) -> Result<NoteVerifier, NoteError> {
    NoteVerifier::new(origin, public_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn origins() {
        assert_eq!(
            tenant_origin("librarybase.org", "local"),
            "librarybase.org/log/local"
        );
        assert_eq!(
            instance_origin("scatter.example", "mirror/wikidata"),
            "scatter.example/instance/log/mirror/wikidata"
        );
        assert_eq!(
            segment_origin("scatter.example/instance/log/mirror/wikidata", 7),
            "scatter.example/instance/log/mirror/wikidata/segment/7"
        );
    }

    #[test]
    fn sign_parse_verify() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let cp = Checkpoint {
            origin: "librarybase.org/log/local".into(),
            size: 42,
            root: [0xab; 32],
        };
        let signed = cp.sign(&key).unwrap();
        let text = signed.to_text();
        assert!(text.starts_with("librarybase.org/log/local\n42\nq6urq6urq6urq6urq6urq6urq6urq6urq6urq6urq6s=\n\n\u{2014} librarybase.org/log/local "));
        let back = SignedCheckpoint::parse(text.as_bytes()).unwrap();
        assert_eq!(back, signed);
        let v = verifier_for(&cp.origin, key.verifying_key().to_bytes()).unwrap();
        back.verify(&cp.origin, std::slice::from_ref(&v)).unwrap();
        assert!(matches!(
            back.verify("other.org/log/local", std::slice::from_ref(&v)),
            Err(CheckpointError::Origin { .. })
        ));
        let other = verifier_for(
            &cp.origin,
            SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes(),
        )
        .unwrap();
        assert!(matches!(
            back.verify(&cp.origin, &[other]),
            Err(CheckpointError::Note(NoteError::Unverified))
        ));
    }

    #[test]
    fn bodies_are_strict() {
        assert!(
            Checkpoint::parse_body("o\n1\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n").is_ok()
        );
        assert!(
            Checkpoint::parse_body("o\n01\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n")
                .is_err()
        );
        assert!(
            Checkpoint::parse_body("o\n0\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n").is_ok()
        );
        assert!(
            Checkpoint::parse_body("o o\n1\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n")
                .is_err()
        );
        assert!(Checkpoint::parse_body("o\n1\nAAAA\n").is_err());
        assert!(
            Checkpoint::parse_body("o\n1\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\next\n")
                .is_err()
        );
        assert!(Checkpoint::parse_body("o\n1\n").is_err());
    }
}
