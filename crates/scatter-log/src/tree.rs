//! The Merkle tree over record headers (0006 §5): RFC 6962 (RFC 9162 §2.1) with SHA-256,
//! leaves `H(0x00 ‖ header)` and nodes `H(0x01 ‖ left ‖ right)`.
//!
//! A [`Frontier`] is the tree's right edge: the roots of the perfect subtrees that make
//! up a tree of `n` leaves, largest first, one for each set bit of `n`. Appending a leaf
//! costs O(log n) node hashes; the root folds the frontier from the right. A sealed
//! segment of `2^k` records is one perfect subtree, so a partition root can be folded
//! from segment roots computed in parallel ([`Frontier::push_subtree`]).
//!
//! Inclusion and consistency proofs are `scatter-integrity`'s (0006 §10); this module
//! gives it the hashes.

use sha2::{Digest as _, Sha256};

use crate::hash::{self, Hash};

/// The root of the empty tree: SHA-256 of the empty string (RFC 6962 §2.1).
#[must_use]
pub fn empty_root() -> Hash {
    Sha256::digest([]).into()
}

/// The RFC 6962 tree head over leaf hashes, computed recursively. Linear in the number
/// of leaves and allocation-free apart from the recursion; the reference for tests and
/// the way to hash one sealed segment.
#[must_use]
pub fn root_of(leaves: &[Hash]) -> Hash {
    match leaves {
        [] => empty_root(),
        [one] => *one,
        _ => {
            let k = split_point(leaves.len());
            hash::merkle_node(&root_of(&leaves[..k]), &root_of(&leaves[k..]))
        }
    }
}

/// The largest power of two less than `n` (RFC 6962 §2.1), for `n >= 2`.
#[must_use]
pub fn split_point(n: usize) -> usize {
    debug_assert!(n >= 2);
    1 << (usize::BITS - 1 - (n - 1).leading_zeros())
}

/// The right edge of a tree: the roots of its perfect subtrees, largest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frontier {
    /// `(height, root)`, heights strictly decreasing.
    nodes: Vec<(u32, Hash)>,
    size: u64,
}

/// A subtree pushed where it does not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a subtree of 2^{height} leaves cannot follow {size} leaves")]
pub struct Misaligned {
    /// The subtree's height.
    pub height: u32,
    /// The tree's size when it was pushed.
    pub size: u64,
}

impl Frontier {
    /// The empty tree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The frontier of a tree over these leaves.
    pub fn from_leaves<I: IntoIterator<Item = Hash>>(leaves: I) -> Self {
        let mut f = Self::new();
        for leaf in leaves {
            f.push_leaf(leaf);
        }
        f
    }

    /// The number of leaves.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// The subtree roots, largest first, with their heights.
    #[must_use]
    pub fn nodes(&self) -> &[(u32, Hash)] {
        &self.nodes
    }

    /// Appends one leaf hash.
    pub fn push_leaf(&mut self, leaf: Hash) {
        self.push(0, leaf);
        self.size += 1;
    }

    /// Appends one leaf hash and returns every node it completes, as `(level, index,
    /// hash)`: the leaf itself at level 0, then one node per level the new leaf closes.
    /// These are exactly the rows a stored tree gains on the append (0013 §2), so a
    /// store that holds the frontier writes them without reading any sibling back.
    pub fn push_leaf_completing(&mut self, leaf: Hash) -> Vec<(u32, u64, Hash)> {
        let index = self.size;
        let mut out = vec![(0, index, leaf)];
        let mut h = 0u32;
        let mut r = leaf;
        while let Some(&(top_h, top)) = self.nodes.last() {
            if top_h != h {
                break;
            }
            self.nodes.pop();
            r = hash::merkle_node(&top, &r);
            h += 1;
            out.push((h, index >> h, r));
        }
        self.nodes.push((h, r));
        self.size += 1;
        out
    }

    /// Appends a perfect subtree of `2^height` leaves by its root, as when folding sealed
    /// segments. The tree's size must be a multiple of `2^height`.
    pub fn push_subtree(&mut self, height: u32, root: Hash) -> Result<(), Misaligned> {
        if height >= u64::BITS || !self.size.is_multiple_of(1u64 << height) {
            return Err(Misaligned {
                height,
                size: self.size,
            });
        }
        self.push(height, root);
        self.size += 1u64 << height;
        Ok(())
    }

    fn push(&mut self, height: u32, root: Hash) {
        let mut h = height;
        let mut r = root;
        while let Some(&(top_h, top)) = self.nodes.last() {
            if top_h != h {
                break;
            }
            self.nodes.pop();
            r = hash::merkle_node(&top, &r);
            h += 1;
        }
        self.nodes.push((h, r));
    }

    /// The tree head: the frontier folded from the right (RFC 6962 §2.1).
    #[must_use]
    pub fn root(&self) -> Hash {
        let mut it = self.nodes.iter().rev();
        let Some(&(_, last)) = it.next() else {
            return empty_root();
        };
        it.fold(last, |acc, &(_, left)| hash::merkle_node(&left, &acc))
    }
}

/// The segment layout of a partition: `2^exponent` records per segment, the last one
/// partial (0006 §5; 0013 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Segments {
    exponent: u8,
}

impl Segments {
    /// The layout with `2^exponent` records per segment; the exponent is below 64.
    #[must_use]
    pub fn new(exponent: u8) -> Option<Self> {
        (exponent < 64).then_some(Self { exponent })
    }

    /// The exponent `k`.
    #[must_use]
    pub fn exponent(&self) -> u8 {
        self.exponent
    }

    /// Records per full segment.
    #[must_use]
    pub fn len(&self) -> u64 {
        1 << self.exponent
    }

    /// Whether a segment holds nothing, which it never does.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The segment that holds `offset`.
    #[must_use]
    pub fn segment_of(&self, offset: u64) -> u64 {
        offset >> self.exponent
    }

    /// The offsets of segment `n`: `[n·2^k, (n+1)·2^k)`.
    #[must_use]
    pub fn range(&self, n: u64) -> std::ops::Range<u64> {
        (n << self.exponent)..((n + 1) << self.exponent)
    }

    /// Whether segment `n` is full in a partition of `size` records: every offset of
    /// its range has been appended.
    #[must_use]
    pub fn is_full(&self, n: u64, size: u64) -> bool {
        self.range(n).end <= size
    }

    /// The number of full segments in a partition of `size` records.
    #[must_use]
    pub fn full_segments(&self, size: u64) -> u64 {
        size >> self.exponent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{hex, merkle_leaf};

    /// RFC 6962's eight test leaves and the roots of their prefixes, as used by the
    /// Certificate Transparency reference implementations.
    const LEAVES: [&[u8]; 8] = [
        b"",
        b"\x00",
        b"\x10",
        b"\x20\x21",
        b"\x30\x31",
        b"\x40\x41\x42\x43",
        b"\x50\x51\x52\x53\x54\x55\x56\x57",
        b"\x60\x61\x62\x63\x64\x65\x66\x67\x68\x69\x6a\x6b\x6c\x6d\x6e\x6f",
    ];
    const ROOTS: [&str; 9] = [
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
        "fac54203e7cc696cf0dfcb42c92a1d9dbaf70ad9e621f4bd8d98662f00e3c125",
        "aeb6bcfe274b70a14fb067a5e5578264db0fa9b51af5e0ba159158f329e06e77",
        "d37ee418976dd95753c1c73862b9398fa2a2cf9b4ff0fdfe8b30cd95209614b7",
        "4e3bbb1f7b478dcfe71fb631631519a3bca12c9aefca1612bfce4c13a86264d4",
        "76e67dadbcdf1e10e1b74ddc608abd2f98dfb16fbce75277b5232a127f2087ef",
        "ddb89be403809e325750d3d263cd78929c2942b7942a34b77e122c9594a74c8c",
        "5dc9da79a70659a9ad559cb701ded9a2ab9d823aad2f4960cfe370eff4604328",
    ];

    fn leaves() -> Vec<Hash> {
        LEAVES.iter().map(|l| merkle_leaf(l)).collect()
    }

    #[test]
    fn rfc6962_roots() {
        let leaves = leaves();
        for (n, want) in ROOTS.iter().enumerate() {
            assert_eq!(hex(&root_of(&leaves[..n])), *want, "root of {n}");
            let f = Frontier::from_leaves(leaves[..n].iter().copied());
            assert_eq!(hex(&f.root()), *want, "frontier root of {n}");
            assert_eq!(f.size(), n as u64);
        }
    }

    #[test]
    fn frontier_matches_the_recursion_at_every_size() {
        let leaves: Vec<Hash> = (0..200u32).map(|i| merkle_leaf(&i.to_be_bytes())).collect();
        let mut f = Frontier::new();
        for n in 0..=leaves.len() {
            assert_eq!(f.root(), root_of(&leaves[..n]), "size {n}");
            assert_eq!(f.nodes().len(), n.count_ones() as usize);
            if n < leaves.len() {
                f.push_leaf(leaves[n]);
            }
        }
    }

    #[test]
    fn completed_nodes_are_the_complete_subtrees() {
        // Every node push_leaf_completing reports is the root of a complete subtree of
        // the leaves so far, at the (level, index) a stored tree files it under; the
        // frontier itself advances exactly as push_leaf does.
        let leaves: Vec<Hash> = (0..64u32).map(|i| merkle_leaf(&i.to_be_bytes())).collect();
        let mut f = Frontier::new();
        let mut plain = Frontier::new();
        let mut total = 0usize;
        for (i, leaf) in leaves.iter().enumerate() {
            let done = f.push_leaf_completing(*leaf);
            plain.push_leaf(*leaf);
            assert_eq!(f, plain);
            assert_eq!(
                done.len(),
                1 + (i as u64).trailing_ones() as usize,
                "leaf {i}"
            );
            for (level, index, hash) in &done {
                let start = usize::try_from(index << level).unwrap();
                let end = start + (1usize << level);
                assert_eq!(
                    *hash,
                    root_of(&leaves[start..end]),
                    "node ({level}, {index})"
                );
            }
            total += done.len();
        }
        // A tree over n leaves stores 2n - popcount(n) nodes.
        assert_eq!(total, 2 * 64 - 1);
    }

    #[test]
    fn segment_roots_fold_to_the_partition_root() {
        let leaves: Vec<Hash> = (0..37u32).map(|i| merkle_leaf(&i.to_be_bytes())).collect();
        let seg = Segments::new(3).unwrap();
        let mut f = Frontier::new();
        for n in 0..seg.full_segments(37) {
            let r = seg.range(n);
            let chunk = &leaves[usize::try_from(r.start).unwrap()..usize::try_from(r.end).unwrap()];
            f.push_subtree(3, root_of(chunk)).unwrap();
        }
        for leaf in &leaves[32..] {
            f.push_leaf(*leaf);
        }
        assert_eq!(f.root(), root_of(&leaves));
        assert_eq!(f.size(), 37);
        assert!(matches!(f.push_subtree(3, [0; 32]), Err(Misaligned { .. })));
        assert!(f.push_subtree(0, [0; 32]).is_ok());
        assert!(f.push_subtree(64, [0; 32]).is_err());
    }

    #[test]
    fn split_points() {
        assert_eq!(split_point(2), 1);
        assert_eq!(split_point(3), 2);
        assert_eq!(split_point(4), 2);
        assert_eq!(split_point(5), 4);
        assert_eq!(split_point(8), 4);
        assert_eq!(split_point(9), 8);
    }

    #[test]
    fn segment_layout() {
        let s = Segments::new(16).unwrap();
        assert_eq!(s.len(), 65_536);
        assert_eq!(s.segment_of(0), 0);
        assert_eq!(s.segment_of(65_535), 0);
        assert_eq!(s.segment_of(65_536), 1);
        assert_eq!(s.range(2), 131_072..196_608);
        assert!(s.is_full(0, 65_536));
        assert!(!s.is_full(0, 65_535));
        assert_eq!(s.full_segments(200_000), 3);
        assert!(Segments::new(64).is_none());
        assert_eq!(Segments::new(0).unwrap().len(), 1);
    }
}
