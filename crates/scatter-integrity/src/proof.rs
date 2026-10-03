//! Inclusion and consistency proofs (RFC 9162 §2.1.3–2.1.4; 0006 §5, §9) over the
//! RFC 6962 tree of `scatter_log::tree`.
//!
//! The proof generators here take the leaves as a slice and are the reference; a backend
//! with stored interior nodes (0013 §2, `log.merkle_node`) can produce the same hashes
//! without them. The verifiers take only what a third party holds.

use scatter_log::hash::{Hash, merkle_node};
use scatter_log::tree::{root_of, split_point};

/// Why a proof is not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProofError {
    /// The index is not below the size, or a size is zero where it cannot be.
    #[error("proof: index or size out of range")]
    Range,
    /// The proof has the wrong number of hashes for the sizes.
    #[error("proof: wrong length")]
    Length,
    /// The proof folds to a different root.
    #[error("proof: root mismatch")]
    Mismatch,
}

/// The audit path of leaf `index` in a tree over `leaves` (RFC 9162 §2.1.3.1).
#[must_use]
pub fn inclusion_proof(leaves: &[Hash], index: u64) -> Vec<Hash> {
    let mut path = Vec::new();
    let Ok(mut m) = usize::try_from(index) else {
        return path;
    };
    if m >= leaves.len() {
        return path;
    }
    let mut d = leaves;
    while d.len() > 1 {
        let k = split_point(d.len());
        if m < k {
            path.push(root_of(&d[k..]));
            d = &d[..k];
        } else {
            path.push(root_of(&d[..k]));
            d = &d[k..];
            m -= k;
        }
    }
    path.reverse();
    path
}

/// Checks an audit path (RFC 9162 §2.1.3.2): that `leaf` is leaf `index` of the tree of
/// `size` leaves whose head is `root`.
pub fn verify_inclusion(
    leaf: &Hash,
    index: u64,
    size: u64,
    proof: &[Hash],
    root: &Hash,
) -> Result<(), ProofError> {
    if index >= size {
        return Err(ProofError::Range);
    }
    let mut fnode = index;
    let mut snode = size - 1;
    let mut r = *leaf;
    for p in proof {
        if snode == 0 {
            return Err(ProofError::Length);
        }
        if fnode & 1 == 1 || fnode == snode {
            r = merkle_node(p, &r);
            while fnode & 1 == 0 && fnode != 0 {
                fnode >>= 1;
                snode >>= 1;
            }
        } else {
            r = merkle_node(&r, p);
        }
        fnode >>= 1;
        snode >>= 1;
    }
    if snode != 0 {
        return Err(ProofError::Length);
    }
    if r == *root {
        Ok(())
    } else {
        Err(ProofError::Mismatch)
    }
}

/// The consistency proof between the tree of the first `first` leaves and the tree of
/// the first `second` leaves (RFC 9162 §2.1.4.1), `0 < first < second <= leaves.len()`.
/// Empty when the sizes are out of range or equal.
#[must_use]
pub fn consistency_proof(leaves: &[Hash], first: u64, second: u64) -> Vec<Hash> {
    let (Ok(m), Ok(n)) = (usize::try_from(first), usize::try_from(second)) else {
        return Vec::new();
    };
    if m == 0 || m >= n || n > leaves.len() {
        return Vec::new();
    }
    let mut proof = Vec::new();
    subproof(&mut proof, &leaves[..n], m, true);
    proof.reverse();
    proof
}

fn subproof(out: &mut Vec<Hash>, d: &[Hash], m: usize, complete: bool) {
    if m == d.len() {
        if !complete {
            out.push(root_of(d));
        }
        return;
    }
    let k = split_point(d.len());
    if m <= k {
        out.push(root_of(&d[k..]));
        subproof(out, &d[..k], m, complete);
    } else {
        out.push(root_of(&d[..k]));
        subproof(out, &d[k..], m - k, false);
    }
}

/// Checks a consistency proof (RFC 9162 §2.1.4.2): that the tree of `second` leaves with
/// head `second_root` extends the tree of `first` leaves with head `first_root`.
pub fn verify_consistency(
    first: u64,
    second: u64,
    first_root: &Hash,
    second_root: &Hash,
    proof: &[Hash],
) -> Result<(), ProofError> {
    if first == 0 || first > second {
        return Err(ProofError::Range);
    }
    if first == second {
        return if proof.is_empty() && first_root == second_root {
            Ok(())
        } else if proof.is_empty() {
            Err(ProofError::Mismatch)
        } else {
            Err(ProofError::Length)
        };
    }
    let mut proof = proof.to_vec();
    if first.is_power_of_two() {
        proof.insert(0, *first_root);
    }
    let Some((&start, rest)) = proof.split_first() else {
        return Err(ProofError::Length);
    };
    let mut fnode = first - 1;
    let mut snode = second - 1;
    while fnode & 1 == 1 {
        fnode >>= 1;
        snode >>= 1;
    }
    let (mut fr, mut sr) = (start, start);
    for c in rest {
        if snode == 0 {
            return Err(ProofError::Length);
        }
        if fnode & 1 == 1 || fnode == snode {
            fr = merkle_node(c, &fr);
            sr = merkle_node(c, &sr);
            while fnode & 1 == 0 && fnode != 0 {
                fnode >>= 1;
                snode >>= 1;
            }
        } else {
            sr = merkle_node(&sr, c);
        }
        fnode >>= 1;
        snode >>= 1;
    }
    if snode != 0 {
        return Err(ProofError::Length);
    }
    if fr == *first_root && sr == *second_root {
        Ok(())
    } else {
        Err(ProofError::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_log::hash::merkle_leaf;

    fn leaves(n: u32) -> Vec<Hash> {
        (0..n).map(|i| merkle_leaf(&i.to_be_bytes())).collect()
    }

    #[test]
    fn inclusion_at_every_index_of_every_size() {
        let all = leaves(70);
        for n in 1..=70usize {
            let d = &all[..n];
            let root = root_of(d);
            for i in 0..n {
                let proof = inclusion_proof(d, i as u64);
                verify_inclusion(&d[i], i as u64, n as u64, &proof, &root)
                    .unwrap_or_else(|e| panic!("size {n} index {i}: {e}"));
                // The wrong leaf, root, index or a truncated proof all fail.
                assert!(verify_inclusion(&[0; 32], i as u64, n as u64, &proof, &root).is_err());
                assert!(verify_inclusion(&d[i], i as u64, n as u64, &proof, &[0; 32]).is_err());
                if n > 1 {
                    assert!(
                        verify_inclusion(&d[i], i as u64, n as u64, &proof[1..], &root).is_err()
                    );
                    assert!(
                        verify_inclusion(&d[i], (i as u64 + 1) % n as u64, n as u64, &proof, &root)
                            .is_err()
                    );
                }
            }
            assert_eq!(
                verify_inclusion(&d[0], n as u64, n as u64, &[], &root),
                Err(ProofError::Range)
            );
        }
    }

    #[test]
    fn consistency_between_every_pair_of_sizes() {
        let all = leaves(40);
        for second in 1..=40usize {
            let second_root = root_of(&all[..second]);
            for first in 1..=second {
                let first_root = root_of(&all[..first]);
                let proof = consistency_proof(&all, first as u64, second as u64);
                verify_consistency(
                    first as u64,
                    second as u64,
                    &first_root,
                    &second_root,
                    &proof,
                )
                .unwrap_or_else(|e| panic!("{first} -> {second}: {e}"));
                assert!(
                    verify_consistency(first as u64, second as u64, &[0; 32], &second_root, &proof)
                        .is_err()
                );
                assert!(
                    verify_consistency(first as u64, second as u64, &first_root, &[0; 32], &proof)
                        .is_err()
                );
                if !proof.is_empty() {
                    assert!(
                        verify_consistency(
                            first as u64,
                            second as u64,
                            &first_root,
                            &second_root,
                            &proof[1..]
                        )
                        .is_err()
                    );
                }
            }
        }
        assert_eq!(
            verify_consistency(0, 1, &[0; 32], &[0; 32], &[]),
            Err(ProofError::Range)
        );
        assert_eq!(
            verify_consistency(2, 1, &[0; 32], &[0; 32], &[]),
            Err(ProofError::Range)
        );
    }

    #[test]
    fn rfc9162_shapes() {
        // RFC 9162 §2.1.5, the tree of seven leaves: PATH(0) = [b, h, l], PATH(6) = [i, k],
        // PROOF(3) = [c, d, g, l], PROOF(4) = [l], PROOF(6) = [i, j, k].
        let d = leaves(7);
        assert_eq!(inclusion_proof(&d, 0).len(), 3);
        assert_eq!(inclusion_proof(&d, 6).len(), 2);
        assert_eq!(consistency_proof(&d, 3, 7).len(), 4);
        assert_eq!(consistency_proof(&d, 4, 7).len(), 1);
        assert_eq!(consistency_proof(&d, 6, 7).len(), 3);
        assert_eq!(consistency_proof(&d, 4, 7), vec![root_of(&d[4..7])]);
        assert_eq!(
            inclusion_proof(&d, 0),
            vec![d[1], root_of(&d[2..4]), root_of(&d[4..7])]
        );
        assert!(consistency_proof(&d, 7, 7).is_empty());
    }
}
