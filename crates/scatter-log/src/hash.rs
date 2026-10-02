//! The hashes of the log (0006 §2–3, §5; 0015 §1; 0040 §3): SHA-256 everywhere, each use
//! prefixed with a one-byte domain tag so that no hash can be mistaken for another kind.
//!
//! | Tag | Use |
//! |---|---|
//! | `0x00` | Merkle leaf: the hash of a record header |
//! | `0x01` | Merkle interior node: the hash of two child hashes |
//! | `0x02` | Body commitment: over the part leaves in order |
//! | `0x03` | Content hash of a part alone |
//! | `0x04` | Leaf of one body part: `H(0x04 ‖ i ‖ salt ‖ bytes)` |
//! | `0x05` | Preimage of a client signature |
//! | `0x06` | Preimage of an instance attestation's signature |
//!
//! Tags `0x00` and `0x01` are RFC 6962's, so the tree is the RFC 6962 tree unchanged.

use sha2::{Digest as _, Sha256};

/// A SHA-256 digest.
pub type Hash = [u8; 32];

/// Merkle leaf (RFC 6962).
pub const TAG_LEAF: u8 = 0x00;
/// Merkle interior node (RFC 6962).
pub const TAG_NODE: u8 = 0x01;
/// Body commitment.
pub const TAG_COMMITMENT: u8 = 0x02;
/// Content hash of a part.
pub const TAG_CONTENT: u8 = 0x03;
/// Leaf of a body part.
pub const TAG_PART: u8 = 0x04;
/// Client signature preimage.
pub const TAG_SIGNATURE: u8 = 0x05;
/// Instance attestation preimage.
pub const TAG_INSTANCE: u8 = 0x06;

fn tagged(tag: u8, parts: &[&[u8]]) -> Hash {
    let mut h = Sha256::new();
    h.update([tag]);
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// `H(0x00 ‖ header)`: the Merkle leaf of a record.
#[must_use]
pub fn merkle_leaf(header_bytes: &[u8]) -> Hash {
    tagged(TAG_LEAF, &[header_bytes])
}

/// `H(0x01 ‖ left ‖ right)`: a Merkle interior node.
#[must_use]
pub fn merkle_node(left: &Hash, right: &Hash) -> Hash {
    tagged(TAG_NODE, &[left, right])
}

/// `H(0x04 ‖ i ‖ salt ‖ bytes)`: the leaf of body part `i`.
#[must_use]
pub fn part_leaf(index: u8, salt: &[u8; 16], bytes: &[u8]) -> Hash {
    tagged(TAG_PART, &[&[index], salt, bytes])
}

/// `H(0x02 ‖ leaf_0 ‖ … ‖ leaf_{n-1})`: the body commitment, header field 6.
#[must_use]
pub fn commitment(leaves: &[Hash]) -> Hash {
    let mut h = Sha256::new();
    h.update([TAG_COMMITMENT]);
    for l in leaves {
        h.update(l);
    }
    h.finalize().into()
}

/// `H(0x03 ‖ bytes)`: the content hash of a part, without its salt. Used for
/// deduplication and version cursors, and inside the signature preimages.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> Hash {
    tagged(TAG_CONTENT, &[bytes])
}

/// `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`: what a client signs (0015 §1).
#[must_use]
pub fn signature_preimage(content: &[u8], comment: &[u8]) -> Hash {
    tagged(
        TAG_SIGNATURE,
        &[&content_hash(content), &content_hash(comment)],
    )
}

/// `H(0x06 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment) ‖ authority)`: what the instance key
/// signs on an instance attestation (0040 §3), `authority` being the canonical CBOR of
/// the authority map.
#[must_use]
pub fn instance_preimage(content: &[u8], comment: &[u8], authority: &[u8]) -> Hash {
    tagged(
        TAG_INSTANCE,
        &[&content_hash(content), &content_hash(comment), authority],
    )
}

/// Lower-case hex, for messages and test vectors.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_separate_domains() {
        let a = merkle_leaf(b"x");
        let b = content_hash(b"x");
        let c = part_leaf(0, &[0; 16], b"x");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_eq!(
            hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // RFC 6962 §2.1: the empty tree's root is the hash of the empty string; a one-leaf
        // tree's root is H(0x00 || leaf).
        assert_eq!(
            hex(&merkle_leaf(b"")),
            "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
        );
    }
}
