//! Property tests of the Merkle proofs (22 §4.9; 0033 §15; 0006 §5, §9): over random
//! logs, every inclusion proof verifies at its index and nowhere else, and every
//! consistency proof between two sizes verifies against the two roots.

use proptest::prelude::*;
use scatter_integrity::proof::{
    consistency_proof, inclusion_proof, verify_consistency, verify_inclusion,
};
use scatter_log::hash::Hash;
use scatter_log::tree::root_of;

fn leaves() -> impl Strategy<Value = Vec<Hash>> {
    prop::collection::vec(any::<[u8; 32]>(), 1..=48)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// RFC 9162 §2.1.3 (0006 §5): the audit path of every leaf verifies against the root,
    /// and a changed leaf or a wrong index does not.
    #[test]
    fn inclusion_proofs_verify_at_their_index(leaves in leaves(), flip in 0usize..32) {
        let n = leaves.len() as u64;
        let root = root_of(&leaves);
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = inclusion_proof(&leaves, i as u64);
            prop_assert!(verify_inclusion(leaf, i as u64, n, &proof, &root).is_ok(), "leaf {}", i);
            let mut other = *leaf;
            other[flip] ^= 1;
            prop_assert!(verify_inclusion(&other, i as u64, n, &proof, &root).is_err(), "changed leaf {}", i);
            let wrong = (i as u64 + 1) % n;
            if wrong != i as u64 {
                prop_assert!(verify_inclusion(leaf, wrong, n, &proof, &root).is_err(), "wrong index {}", i);
            }
        }
    }

    /// RFC 9162 §2.1.4 (0006 §5, §9): the tree of the first `second` leaves extends the
    /// tree of the first `first` leaves, and does not extend a tree with another root.
    #[test]
    fn consistency_proofs_verify_between_sizes(leaves in leaves(), a in any::<u8>(), b in any::<u8>()) {
        let n = leaves.len();
        let (mut first, mut second) = (1 + usize::from(a) % n, 1 + usize::from(b) % n);
        if first > second {
            std::mem::swap(&mut first, &mut second);
        }
        let first_root = root_of(&leaves[..first]);
        let second_root = root_of(&leaves[..second]);
        let (first, second) = (first as u64, second as u64);
        let proof = consistency_proof(&leaves, first, second);
        prop_assert!(
            verify_consistency(first, second, &first_root, &second_root, &proof).is_ok(),
            "{} -> {} of {}", first, second, n
        );
        if first < second {
            let mut other = first_root;
            other[0] ^= 1;
            prop_assert!(
                verify_consistency(first, second, &other, &second_root, &proof).is_err(),
                "another first root"
            );
            prop_assert!(
                verify_consistency(first, second, &first_root, &other, &proof).is_err(),
                "another second root"
            );
        }
    }
}
