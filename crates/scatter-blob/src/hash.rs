//! The two hashes the store computes: SHA-256, which addresses every object (0039 §3), and
//! SHA-1, which an `upload` record carries for MediaWiki compatibility (0039 §2) and a
//! `link`-mode foreign repository reports (0039 §11). Both are computed in one streaming pass
//! over the bytes as they are stored (0039 §3: "streams into `stash/` while the server computes
//! SHA-256 and SHA-1").
//!
//! Each hash type has exactly one canonical encoding, lowercase hex (22 §1.2 rule 4): it is
//! the `{h}` of the key layout and the form `Display` writes and `FromStr` reads.

use std::fmt;
use std::str::FromStr;

use sha1::Digest as _;

/// A SHA-256 digest: the address of an object's bytes (0039 §3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha256(pub [u8; 32]);

/// A SHA-1 digest, kept beside the SHA-256 for MediaWiki compatibility (0039 §2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha1(pub [u8; 20]);

/// A hex string did not decode to a digest of the expected length.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a {expected}-character hex digest: {input:?}")]
pub struct HashParseError {
    /// The hex length a digest of this type has.
    pub expected: usize,
    /// What was given.
    pub input: String,
}

macro_rules! hash_type {
    ($name:ident, $len:expr, $field:ident) => {
        impl $name {
            /// The digest's length in bytes.
            pub const LEN: usize = $len;

            /// The digest of `bytes`, in one pass. For streamed bytes use [`Hasher`].
            #[must_use]
            pub fn of(bytes: &[u8]) -> Self {
                let mut h = Hasher::new();
                h.update(bytes);
                h.finish().$field
            }

            /// The canonical encoding: lowercase hex, the `{h}` of the key layout (0039 §3).
            #[must_use]
            pub fn hex(&self) -> String {
                encode_hex(&self.0)
            }

            /// The raw bytes.
            #[must_use]
            pub fn as_bytes(&self) -> &[u8; $len] {
                &self.0
            }

            /// Decodes the canonical encoding; uppercase hex is accepted and normalized.
            pub fn parse_hex(s: &str) -> Result<Self, HashParseError> {
                let err = || HashParseError {
                    expected: $len * 2,
                    input: s.to_owned(),
                };
                if s.len() != $len * 2 {
                    return Err(err());
                }
                let mut out = [0u8; $len];
                for (i, pair) in s.as_bytes().chunks(2).enumerate() {
                    let hi = hex_nibble(pair[0]).ok_or_else(err)?;
                    let lo = hex_nibble(pair[1]).ok_or_else(err)?;
                    out[i] = (hi << 4) | lo;
                }
                Ok(Self(out))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.hex())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.hex())
            }
        }

        impl FromStr for $name {
            type Err = HashParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::parse_hex(s)
            }
        }

        impl From<[u8; $len]> for $name {
            fn from(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }
        }
    };
}

hash_type!(Sha256, 32, sha256);
hash_type!(Sha1, 20, sha1);

/// What one pass over an object's bytes yields: both hashes and the size (0039 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Digest {
    /// The address of the bytes.
    pub sha256: Sha256,
    /// The MediaWiki-compatible hash.
    pub sha1: Sha1,
    /// The size in bytes.
    pub size: u64,
}

/// Computes SHA-256 and SHA-1 and counts bytes in one streaming pass (0039 §3).
#[derive(Clone)]
pub struct Hasher {
    sha256: sha2::Sha256,
    sha1: sha1::Sha1,
    size: u64,
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Hasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The two running states have no useful rendering; the size is what a reader wants.
        f.debug_struct("Hasher")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Hasher {
    /// An empty hasher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sha256: sha2::Sha256::new(),
            sha1: sha1::Sha1::new(),
            size: 0,
        }
    }

    /// Feeds the next bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.sha256.update(bytes);
        self.sha1.update(bytes);
        self.size += bytes.len() as u64;
    }

    /// The bytes fed so far.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Both digests and the size.
    #[must_use]
    pub fn finish(self) -> Digest {
        Digest {
            sha256: Sha256(self.sha256.finalize().into()),
            sha1: Sha1(self.sha1.finalize().into()),
            size: self.size,
        }
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[usize::from(b >> 4)] as char);
        s.push(HEX[usize::from(b & 0x0f)] as char);
    }
    s
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // FIPS 180-2 / RFC 3174 test vectors for "abc"; the SHA-1 is also what MediaWiki stores
    // for the same bytes (0039 §2).
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    #[test]
    fn one_pass_gives_both_hashes_and_the_size() {
        let mut h = Hasher::new();
        h.update(b"a");
        h.update(b"bc");
        let d = h.finish();
        assert_eq!(d.sha256.hex(), ABC_SHA256);
        assert_eq!(d.sha1.hex(), ABC_SHA1);
        assert_eq!(d.size, 3);
        assert_eq!(Sha256::of(b"abc"), d.sha256);
        assert_eq!(Sha1::of(b"abc"), d.sha1);
    }

    #[test]
    fn hex_round_trips_and_normalizes_case() {
        let h: Sha256 = ABC_SHA256.parse().unwrap();
        assert_eq!(h.to_string(), ABC_SHA256);
        assert_eq!(
            Sha256::parse_hex(&ABC_SHA256.to_uppercase()).unwrap(),
            h,
            "uppercase decodes to the same digest; the canonical form stays lowercase"
        );
        assert_eq!(format!("{h:?}"), format!("Sha256({ABC_SHA256})"));
        let s: Sha1 = ABC_SHA1.parse().unwrap();
        assert_eq!(s.hex(), ABC_SHA1);
    }

    #[test]
    fn malformed_hex_is_refused() {
        assert!(Sha256::parse_hex(ABC_SHA1).is_err(), "wrong length");
        assert!(Sha1::parse_hex(ABC_SHA256).is_err(), "wrong length");
        let bad = format!("zz{}", &ABC_SHA256[2..]);
        let e = Sha256::parse_hex(&bad).unwrap_err();
        assert_eq!(e.expected, 64);
        assert_eq!(e.input, bad);
    }
}
