//! Property tests of the canonical encodings (22 §4.9; 0033 §15): every hashed type has
//! exactly one encoding (22 §1.2 rule 4), so whatever is encoded must decode to itself,
//! and the strict decoder must accept exactly the encoder's bytes.
//!
//! Case counts are small on purpose: these run in the ordinary `cargo nextest run`. The
//! fuzz target `fuzz/fuzz_targets/cbor_record.rs` does the open-ended search.

use proptest::prelude::*;
use scatter_log::body::{Body, Part};
use scatter_log::cbor::{self, Value};
use scatter_log::hash::Hash;
use scatter_log::header::{FORMAT_VERSION, Header};
use scatter_log::record::Record;
use scatter_log::tree::{Frontier, root_of};

/// A CBOR value the core deterministic encoding can carry (0006 §2): integers in
/// `[-2^64, 2^64 - 1]`, finite or infinite floats (NaN is not equal to itself, so it is
/// left to the vectors), and maps with distinct keys.
fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        (-(1i128 << 64)..(1i128 << 64)).prop_map(Value::Int),
        any::<i64>().prop_map(|i| Value::Int(i128::from(i))),
        (prop::num::f64::POSITIVE
            | prop::num::f64::NEGATIVE
            | prop::num::f64::NORMAL
            | prop::num::f64::SUBNORMAL
            | prop::num::f64::ZERO
            | prop::num::f64::INFINITE)
            .prop_map(Value::Float),
        prop::collection::vec(any::<u8>(), 0..24).prop_map(Value::Bytes),
        ".{0,24}".prop_map(Value::Text),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::btree_map(".{0,8}", inner.clone(), 0..5).prop_map(|m| {
                Value::map(m.into_iter().map(|(k, v)| (Value::Text(k), v)).collect())
            }),
            prop::collection::btree_map(any::<i64>(), inner, 0..4).prop_map(|m| {
                Value::map(
                    m.into_iter()
                        .map(|(k, v)| (Value::Int(i128::from(k)), v))
                        .collect(),
                )
            }),
        ]
    })
}

fn hash() -> impl Strategy<Value = Hash> {
    any::<[u8; 32]>()
}

/// A header of the current format (0006 §3; 0015 §2), with any commitment: sealing
/// replaces it.
fn header() -> impl Strategy<Value = Header> {
    (
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        ".{0,32}",
        prop::option::of(".{0,32}"),
        hash(),
        prop::option::of(any::<u64>()),
        prop::option::of(any::<u64>()),
        prop::option::of(any::<u64>()),
    )
        .prop_map(
            |(
                partition,
                offset,
                appended_at,
                payload_type,
                key,
                commitment,
                revid,
                logid,
                page_id,
            )| {
                Header {
                    version: FORMAT_VERSION,
                    partition,
                    offset,
                    appended_at,
                    payload_type,
                    key,
                    commitment,
                    revid,
                    logid,
                    page_id,
                }
            },
        )
}

/// A part: present with any salt and bytes, or erased down to its leaf (0006 §5).
fn part() -> impl Strategy<Value = Part> {
    prop_oneof![
        4 => (any::<[u8; 16]>(), prop::collection::vec(any::<u8>(), 0..48))
            .prop_map(|(salt, bytes)| Part::Present { salt, bytes }),
        1 => hash().prop_map(|leaf| Part::Erased { leaf }),
    ]
}

/// A body of three to six parts (0015 §2: the three core parts, then a type's own).
fn body() -> impl Strategy<Value = Body> {
    prop::collection::vec(part(), 3..=6).prop_map(|parts| Body::new(parts).expect("3..=6 parts"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// 0006 §2: encoding is a function of the value, decoding inverts it, and the strict
    /// decoder accepts the encoder's bytes.
    #[test]
    fn cbor_round_trips(v in value()) {
        let bytes = cbor::encode(&v)?;
        prop_assert_eq!(cbor::decode(&bytes)?, v.clone());
        prop_assert_eq!(cbor::encode(&v)?, bytes.clone(), "encoding is deterministic");
        let (prefix, used) = cbor::decode_prefix(&bytes)?;
        prop_assert_eq!(prefix, v);
        prop_assert_eq!(used, bytes.len());
    }

    /// 0006 §2: bytes the decoder accepts are the encoder's bytes, so a trailing byte or a
    /// truncation is never read as a value.
    #[test]
    fn cbor_rejects_what_is_not_canonical(v in value(), extra in any::<u8>()) {
        let bytes = cbor::encode(&v)?;
        let mut longer = bytes.clone();
        longer.push(extra);
        prop_assert!(cbor::decode(&longer).is_err(), "a trailing byte");
        if bytes.len() > 1 {
            prop_assert!(cbor::decode(&bytes[..bytes.len() - 1]).is_err(), "a truncation");
        }
    }

    /// 0006 §3; 0015 §2: the ten-field header round-trips and its leaf is a function of
    /// its bytes.
    #[test]
    fn header_round_trips(h in header()) {
        let bytes = h.encode();
        let back = Header::decode(&bytes)?;
        prop_assert_eq!(&back, &h);
        prop_assert_eq!(back.leaf(), h.leaf());
        prop_assert_eq!(Header::from_value(&h.to_value())?, h);
    }

    /// 0006 §5: a body round-trips; its commitment is over the part leaves, so erasing a
    /// part changes neither the leaves nor the commitment, and the erased body still
    /// verifies against the header that sealed the full one.
    #[test]
    fn body_round_trips_and_erasure_keeps_the_commitment(b in body(), which in 0usize..6) {
        let bytes = b.encode();
        prop_assert_eq!(Body::decode(&bytes)?, b.clone());
        let commitment = b.commitment();
        prop_assert!(b.verify(&commitment).is_ok());
        let leaves = b.leaves();
        let mut erased = b.clone();
        if which < erased.parts().len() {
            erased.erase(which)?;
            prop_assert!(erased.part(which).expect("in range").is_erased());
        }
        prop_assert_eq!(erased.leaves(), leaves);
        prop_assert_eq!(erased.commitment(), commitment);
        prop_assert!(erased.verify(&commitment).is_ok());
        prop_assert_eq!(Body::decode(&erased.encode())?, erased);
    }

    /// 0006 §3: `[header, body]` round-trips, and a decoded record's header commits to
    /// its body.
    #[test]
    fn record_round_trips(h in header(), b in body()) {
        let record = Record::seal(h, b);
        let bytes = record.encode();
        let back = Record::decode(&bytes)?;
        prop_assert_eq!(&back, &record);
        prop_assert_eq!(back.leaf(), record.leaf());
        prop_assert_eq!(Record::from_value(&record.to_value())?, record);
    }

    /// 0006 §3: a record whose commitment does not match its body is refused.
    #[test]
    fn record_with_a_wrong_commitment_is_refused(h in header(), b in body(), flip in 0usize..32) {
        let mut record = Record::seal(h, b);
        let (mut header, body) = record.clone().into_parts();
        header.commitment[flip] ^= 1;
        record = Record::seal(header.clone(), body.clone());
        prop_assert_eq!(record.header().commitment, body.commitment(), "sealing recomputes");
        prop_assert!(Record::new(header, body).is_err());
    }

    /// 0006 §5: the frontier folds to the same root as the whole tree, at every prefix,
    /// and sealed segments pushed as subtrees give the same root as their leaves.
    #[test]
    fn frontier_agrees_with_root_of(leaves in prop::collection::vec(hash(), 0..40), exponent in 0u32..4) {
        let mut f = Frontier::new();
        for (n, leaf) in leaves.iter().enumerate() {
            prop_assert_eq!(f.root(), root_of(&leaves[..n]), "prefix of {}", n);
            prop_assert_eq!(f.size(), n as u64);
            f.push_leaf(*leaf);
        }
        prop_assert_eq!(f.root(), root_of(&leaves));
        prop_assert_eq!(Frontier::from_leaves(leaves.iter().copied()).root(), root_of(&leaves));

        let segment = 1usize << exponent;
        let mut by_segments = Frontier::new();
        let mut chunks = leaves.chunks(segment);
        for chunk in &mut chunks {
            if chunk.len() == segment {
                by_segments.push_subtree(exponent, root_of(chunk)).expect("aligned");
            } else {
                for leaf in chunk {
                    by_segments.push_leaf(*leaf);
                }
            }
        }
        prop_assert_eq!(by_segments.root(), root_of(&leaves));
        prop_assert_eq!(by_segments.size(), leaves.len() as u64);
    }
}
