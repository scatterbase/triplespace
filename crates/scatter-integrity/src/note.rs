//! [C2SP signed notes](https://c2sp.org/signed-note): the envelope every checkpoint and
//! segment manifest is signed in (0006 §6).
//!
//! ```text
//! <text, one or more lines>
//!
//! — <name> <base64(key hash[0..4] ‖ signature)>
//! ```
//!
//! The signature is Ed25519 over the text, trailing newline included. The key hash is
//! `SHA-256(name ‖ "\n" ‖ 0x01 ‖ public key)`; its first four bytes let a verifier pick
//! the key without trying every one. A verifier's text form is
//! `<name>+<hash as 8 hex digits>+<base64(0x01 ‖ public key)>`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use sha2::{Digest as _, Sha256};

/// The signed-note algorithm byte for Ed25519.
pub const ALG_ED25519: u8 = 0x01;

/// The signature line prefix: an em dash and a space.
pub const SIGNATURE_PREFIX: &str = "\u{2014} ";

/// Why bytes are not a signed note, or a note does not verify.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NoteError {
    /// Not UTF-8, no blank line, or a malformed line.
    #[error("signed note: {0}")]
    Malformed(&'static str),
    /// A name with whitespace or `+`, or empty.
    #[error("signed note: `{0}` is not a valid key name")]
    Name(String),
    /// Not base64, or the wrong length.
    #[error("signed note: bad signature encoding")]
    Encoding,
    /// A key string is not `name+hash+key`.
    #[error("signed note: bad verifier `{0}`")]
    Verifier(String),
    /// The public key is not a valid Ed25519 point.
    #[error("signed note: bad public key")]
    PublicKey,
    /// No signature by a known key verified.
    #[error("signed note: no signature verifies")]
    Unverified,
}

/// Whether a name may sign a note: non-empty, no whitespace, no `+`.
#[must_use]
pub fn is_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('+') && !name.chars().any(char::is_whitespace)
}

/// The key hash of a name and an Ed25519 public key.
#[must_use]
pub fn key_hash(name: &str, public_key: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    h.update(b"\n");
    h.update([ALG_ED25519]);
    h.update(public_key);
    h.finalize().into()
}

/// A key that signs notes, with the name it signs under.
#[derive(Debug)]
pub struct NoteSigner {
    name: String,
    key: SigningKey,
}

impl NoteSigner {
    /// A signer for `name`.
    pub fn new(name: &str, key: SigningKey) -> Result<Self, NoteError> {
        if !is_name(name) {
            return Err(NoteError::Name(name.to_string()));
        }
        Ok(Self {
            name: name.to_string(),
            key,
        })
    }

    /// The name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The matching verifier.
    #[must_use]
    pub fn verifier(&self) -> NoteVerifier {
        NoteVerifier::new(&self.name, self.key.verifying_key().to_bytes())
            .expect("the name was checked")
    }

    /// Signs the text, which must end with a newline, and returns the complete note.
    pub fn sign(&self, text: &str) -> Result<Note, NoteError> {
        Note::check_text(text)?;
        let sig = self.key.sign(text.as_bytes());
        let hash = key_hash(&self.name, &self.key.verifying_key().to_bytes());
        let mut bytes = hash[..4].to_vec();
        bytes.extend_from_slice(&sig.to_bytes());
        Ok(Note {
            text: text.to_string(),
            signatures: vec![NoteSignature {
                name: self.name.clone(),
                bytes,
            }],
        })
    }
}

/// A key that verifies notes signed under a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteVerifier {
    name: String,
    public_key: [u8; 32],
    hash: [u8; 32],
}

impl NoteVerifier {
    /// A verifier for `name` and an Ed25519 public key.
    pub fn new(name: &str, public_key: [u8; 32]) -> Result<Self, NoteError> {
        if !is_name(name) {
            return Err(NoteError::Name(name.to_string()));
        }
        VerifyingKey::from_bytes(&public_key).map_err(|_| NoteError::PublicKey)?;
        Ok(Self {
            name: name.to_string(),
            public_key,
            hash: key_hash(name, &public_key),
        })
    }

    /// Parses `name+hash+base64(0x01 ‖ key)`.
    pub fn parse(s: &str) -> Result<Self, NoteError> {
        let bad = || NoteError::Verifier(s.to_string());
        let mut parts = s.splitn(3, '+');
        let (name, hash, key) = (
            parts.next().ok_or_else(bad)?,
            parts.next().ok_or_else(bad)?,
            parts.next().ok_or_else(bad)?,
        );
        let key = BASE64.decode(key).map_err(|_| bad())?;
        let (&alg, pk) = key.split_first().ok_or_else(bad)?;
        if alg != ALG_ED25519 {
            return Err(bad());
        }
        let public_key: [u8; 32] = pk.try_into().map_err(|_| bad())?;
        let v = Self::new(name, public_key)?;
        if hash != v.hash_hex() {
            return Err(bad());
        }
        Ok(v)
    }

    /// The name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The public key.
    #[must_use]
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    /// The first four bytes of the key hash, as eight hex digits.
    #[must_use]
    pub fn hash_hex(&self) -> String {
        scatter_log::hash::hex(&self.hash[..4])
    }

    /// The text form, `name+hash+base64(0x01 ‖ key)`.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut key = vec![ALG_ED25519];
        key.extend_from_slice(&self.public_key);
        format!("{}+{}+{}", self.name, self.hash_hex(), BASE64.encode(key))
    }

    /// Whether a signature line's bytes are by this key over `text`.
    #[must_use]
    pub fn verifies(&self, text: &str, sig: &NoteSignature) -> bool {
        if sig.name != self.name || sig.bytes.len() != 4 + 64 || sig.bytes[..4] != self.hash[..4] {
            return false;
        }
        let Ok(signature) = Signature::from_slice(&sig.bytes[4..]) else {
            return false;
        };
        VerifyingKey::from_bytes(&self.public_key)
            .is_ok_and(|k| k.verify(text.as_bytes(), &signature).is_ok())
    }
}

/// One signature line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSignature {
    /// The key name.
    pub name: String,
    /// Four bytes of key hash, then the signature.
    pub bytes: Vec<u8>,
}

/// A note: text and the signatures on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The signed text, trailing newline included.
    pub text: String,
    /// The signatures, in order.
    pub signatures: Vec<NoteSignature>,
}

impl Note {
    fn check_text(text: &str) -> Result<(), NoteError> {
        if text.is_empty() || !text.ends_with('\n') {
            return Err(NoteError::Malformed("the text ends with a newline"));
        }
        if text.contains("\n\n") || text.starts_with('\n') {
            return Err(NoteError::Malformed("the text has no blank line"));
        }
        if text.lines().any(|l| l.starts_with(SIGNATURE_PREFIX)) {
            return Err(NoteError::Malformed("a text line begins like a signature"));
        }
        Ok(())
    }

    /// Parses a note's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, NoteError> {
        let s = std::str::from_utf8(bytes).map_err(|_| NoteError::Malformed("not UTF-8"))?;
        let (text, sigs) = s
            .split_once("\n\n")
            .ok_or(NoteError::Malformed("no blank line"))?;
        let text = format!("{text}\n");
        Self::check_text(&text)?;
        if !sigs.ends_with('\n') {
            return Err(NoteError::Malformed("the note ends with a newline"));
        }
        let mut signatures = Vec::new();
        for line in sigs.split_terminator('\n') {
            let rest = line
                .strip_prefix(SIGNATURE_PREFIX)
                .ok_or(NoteError::Malformed("a signature line"))?;
            let (name, b64) = rest
                .split_once(' ')
                .ok_or(NoteError::Malformed("a signature line"))?;
            if !is_name(name) {
                return Err(NoteError::Name(name.to_string()));
            }
            let bytes = BASE64.decode(b64).map_err(|_| NoteError::Encoding)?;
            if bytes.len() < 5 {
                return Err(NoteError::Encoding);
            }
            signatures.push(NoteSignature {
                name: name.to_string(),
                bytes,
            });
        }
        if signatures.is_empty() {
            return Err(NoteError::Malformed("no signature"));
        }
        Ok(Self { text, signatures })
    }

    /// The note's bytes.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = self.text.clone();
        out.push('\n');
        for s in &self.signatures {
            out.push_str(SIGNATURE_PREFIX);
            out.push_str(&s.name);
            out.push(' ');
            out.push_str(&BASE64.encode(&s.bytes));
            out.push('\n');
        }
        out
    }

    /// Adds a signature by `signer`.
    pub fn add_signature(&mut self, signer: &NoteSigner) -> Result<(), NoteError> {
        let fresh = signer.sign(&self.text)?;
        self.signatures.extend(fresh.signatures);
        Ok(())
    }

    /// The verifiers among `verifiers` whose signature on this note checks out.
    #[must_use]
    pub fn verified_by<'a>(&self, verifiers: &'a [NoteVerifier]) -> Vec<&'a NoteVerifier> {
        verifiers
            .iter()
            .filter(|v| self.signatures.iter().any(|s| v.verifies(&self.text, s)))
            .collect()
    }

    /// Checks that at least one of `verifiers` signed the note.
    pub fn verify(&self, verifiers: &[NoteVerifier]) -> Result<(), NoteError> {
        if self.verified_by(verifiers).is_empty() {
            Err(NoteError::Unverified)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer(seed: u8, name: &str) -> NoteSigner {
        NoteSigner::new(name, SigningKey::from_bytes(&[seed; 32])).unwrap()
    }

    #[test]
    fn sign_parse_verify() {
        let s = signer(1, "example.org/log/local");
        let note = s.sign("example.org/log/local\n42\nAAAA\n").unwrap();
        let text = note.to_text();
        assert!(
            text.starts_with("example.org/log/local\n42\nAAAA\n\n\u{2014} example.org/log/local ")
        );
        assert!(text.ends_with('\n'));
        let back = Note::parse(text.as_bytes()).unwrap();
        assert_eq!(back, note);
        back.verify(&[s.verifier()]).unwrap();
        let other = signer(2, "example.org/log/local");
        assert_eq!(back.verify(&[other.verifier()]), Err(NoteError::Unverified));
        // A tampered text fails.
        let mut tampered = back.clone();
        tampered.text = "example.org/log/local\n43\nAAAA\n".into();
        assert_eq!(tampered.verify(&[s.verifier()]), Err(NoteError::Unverified));
        // Two signatures: either verifier accepts.
        let mut both = back;
        both.add_signature(&other).unwrap();
        assert_eq!(both.verified_by(&[s.verifier(), other.verifier()]).len(), 2);
        assert_eq!(Note::parse(both.to_text().as_bytes()).unwrap(), both);
    }

    #[test]
    fn verifier_text_form_round_trips() {
        let s = signer(3, "witness.example");
        let v = s.verifier();
        let text = v.to_text();
        assert!(text.starts_with("witness.example+"));
        assert_eq!(text.split('+').nth(1).unwrap().len(), 8);
        assert_eq!(NoteVerifier::parse(&text).unwrap(), v);
        let (name, rest) = text.split_once('+').unwrap();
        let (hash, key) = rest.split_once('+').unwrap();
        let flipped = if hash.starts_with('0') { "1" } else { "0" };
        let wrong = format!("{name}+{flipped}{}+{key}", &hash[1..]);
        assert!(matches!(
            NoteVerifier::parse(&wrong),
            Err(NoteError::Verifier(_))
        ));
        assert!(matches!(
            NoteVerifier::parse("nohash"),
            Err(NoteError::Verifier(_))
        ));
        assert!(matches!(
            NoteVerifier::new("bad name", [0; 32]),
            Err(NoteError::Name(_))
        ));
        assert!(matches!(
            NoteVerifier::new("a+b", [0; 32]),
            Err(NoteError::Name(_))
        ));
    }

    #[test]
    fn malformed_notes_are_refused() {
        let s = signer(1, "n");
        assert!(matches!(s.sign("no newline"), Err(NoteError::Malformed(_))));
        assert!(matches!(s.sign("a\n\nb\n"), Err(NoteError::Malformed(_))));
        assert!(matches!(
            s.sign("\u{2014} x\n"),
            Err(NoteError::Malformed(_))
        ));
        assert!(matches!(
            Note::parse(b"text\n"),
            Err(NoteError::Malformed(_))
        ));
        assert!(matches!(
            Note::parse(b"text\n\n"),
            Err(NoteError::Malformed(_))
        ));
        assert!(matches!(
            Note::parse(b"text\n\n- n AAAA\n"),
            Err(NoteError::Malformed(_))
        ));
        assert!(matches!(
            Note::parse("text\n\n\u{2014} n !!!\n".as_bytes()),
            Err(NoteError::Encoding)
        ));
        assert!(matches!(
            Note::parse("text\n\n\u{2014} n AAAA".as_bytes()),
            Err(NoteError::Malformed(_))
        ));
    }
}
