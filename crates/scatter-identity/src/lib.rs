//! Identity clusters (0004 §1–3, §10; chapter 04 §4.1–§4.3, §4.11): which IDs denote one
//! thing, worked out from the links between them.
//!
//! The crate is pure and generic: an ID is any ordered type, and the caller says which
//! namespace an ID belongs to (whoever minted it: the local instance, a provider, a keyed
//! type, an entity source) and how two IDs rank for the canonical member. The rules:
//!
//! - **One member per namespace** (§4.2). A cluster never holds two IDs of one namespace.
//! - **Tiers** (§4.3). Links come in tiers, `1` the highest (asserted), then `2`
//!   (mirror-derived) and `3` (inferred). A tier's links are applied on top of what the
//!   higher tiers formed, so a higher tier wins where they conflict.
//! - **Conflicts are held, all of them** (§4.2, §4.11). The links of one tier that join
//!   into one would-be cluster are applied together or held together: held when that
//!   cluster would have two members of one namespace, or would join two IDs a
//!   `different-from` keeps apart. Holding every link involved, not only the last to
//!   arrive, makes the result independent of the order the links came in.
//! - **The canonical member** is the highest-ranked (§4.4), by the caller's order.
//!
//! Aliases (IDs retired by a redirect) are not members (§4.1): the caller resolves a
//! link's ends to their targets before passing it in.

#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// A link between two IDs, from one tier of source (0004 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link<I> {
    /// One end.
    pub a: I,
    /// The other end.
    pub b: I,
    /// `1` asserted, `2` mirror-derived, `3` inferred; a lower number is a higher tier.
    pub tier: u8,
}

/// Why a conflict's links are held (0004 §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// They would put two members of one namespace into one cluster.
    OneToOne,
    /// They would join two IDs a `different-from` keeps apart.
    Blocked,
}

/// A set of links held instead of applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict<I> {
    /// The held links, as indices into the links passed in, ascending.
    pub links: Vec<usize>,
    /// The smallest ID the held links name, which identifies the conflict.
    pub key: I,
    /// Why they are held.
    pub reason: Reason,
}

/// Clusters, and the links held from them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clusters<I> {
    /// Every cluster of two or more members, canonical member first and the rest by rank;
    /// the clusters in the order of their canonical members.
    pub clusters: Vec<Vec<I>>,
    /// The conflicts, in the order of their keys.
    pub conflicts: Vec<Conflict<I>>,
}

impl<I: Ord> Clusters<I> {
    /// The conflict a link is held in, by its index; `None` when it is applied.
    #[must_use]
    pub fn held(&self, link: usize) -> Option<&Conflict<I>> {
        self.conflicts
            .iter()
            .find(|c| c.links.binary_search(&link).is_ok())
    }

    /// The cluster `id` is a member of.
    #[must_use]
    pub fn cluster_of(&self, id: &I) -> Option<&[I]> {
        self.clusters
            .iter()
            .find(|c| c.contains(id))
            .map(Vec::as_slice)
    }
}

/// Union–find over indices.
#[derive(Clone)]
struct Sets(Vec<usize>);

impl Sets {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// Works out the clusters `links` form, under the `different-from` `blocks`.
///
/// The one-to-one rule: implements 0004 §2 (A24).
///
/// `namespace` names the namespace of an ID, and `order` ranks two IDs, the
/// higher-ranked first; it should be total, so that the canonical member is defined.
#[must_use]
pub fn compute<I, N>(
    links: &[Link<I>],
    blocks: &[(I, I)],
    namespace: impl Fn(&I) -> N,
    order: impl Fn(&I, &I) -> Ordering,
) -> Clusters<I>
where
    I: Ord + Clone,
    N: Ord,
{
    // Every ID a link names, indexed in text order so that the result does not depend on
    // the order of the input.
    let ids: Vec<&I> = links
        .iter()
        .flat_map(|l| [&l.a, &l.b])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let index: BTreeMap<&I, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let blocked: BTreeSet<(usize, usize)> = blocks
        .iter()
        .filter_map(|(x, y)| {
            let (x, y) = (*index.get(x)?, *index.get(y)?);
            Some((x.min(y), x.max(y)))
        })
        .collect();

    let mut applied = Sets((0..ids.len()).collect());
    let mut conflicts = Vec::new();
    let tiers: BTreeSet<u8> = links.iter().map(|l| l.tier).collect();
    for tier in tiers {
        let in_tier: Vec<usize> = (0..links.len())
            .filter(|&i| links[i].tier == tier && links[i].a != links[i].b)
            .collect();
        // The would-be clusters: what the higher tiers formed, joined by this tier's links.
        let mut joined = applied.clone();
        for &i in &in_tier {
            joined.union(index[&links[i].a], index[&links[i].b]);
        }
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &i in &in_tier {
            groups
                .entry(joined.find(index[&links[i].a]))
                .or_default()
                .push(i);
        }
        let mut accepted = Vec::new();
        for (root, group) in groups {
            let members: Vec<usize> = (0..ids.len()).filter(|&x| joined.find(x) == root).collect();
            let mut seen = BTreeSet::new();
            let one_to_one = !members.iter().all(|&x| seen.insert(namespace(ids[x])));
            let split = members
                .iter()
                .any(|&x| members.iter().any(|&y| blocked.contains(&(x, y))));
            if one_to_one || split {
                let key = group
                    .iter()
                    .flat_map(|&i| [&links[i].a, &links[i].b])
                    .min()
                    .cloned()
                    .unwrap_or_else(|| ids[root].clone());
                conflicts.push(Conflict {
                    links: group,
                    key,
                    reason: if one_to_one {
                        Reason::OneToOne
                    } else {
                        Reason::Blocked
                    },
                });
            } else {
                accepted.extend(group);
            }
        }
        for i in accepted {
            applied.union(index[&links[i].a], index[&links[i].b]);
        }
    }

    let mut by_root: BTreeMap<usize, Vec<I>> = BTreeMap::new();
    for (x, id) in ids.iter().enumerate() {
        by_root
            .entry(applied.find(x))
            .or_default()
            .push((*id).clone());
    }
    let mut clusters: Vec<Vec<I>> = by_root
        .into_values()
        .filter(|c| c.len() > 1)
        .map(|mut c| {
            c.sort_by(|a, b| order(a, b).then_with(|| a.cmp(b)));
            c
        })
        .collect();
    clusters.sort_by(|a, b| a[0].cmp(&b[0]));
    conflicts.sort_by(|a, b| a.key.cmp(&b.key));
    Clusters {
        clusters,
        conflicts,
    }
}

/// A stable 63-bit number for a text, FNV-1a, for a cluster's or a conflict's ID: the same
/// on every instance and every rebuild, and never negative, so that it fits a `bigint`.
#[must_use]
pub fn stable_id(text: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    i64::try_from(h >> 1).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The namespace of a test ID: its letters before the first digit (`WD` of `WDQ1`
    /// is spelt `WDQ`, which is enough here), `local` for a bare `Q`.
    fn ns(id: &&str) -> String {
        let p: String = id.chars().take_while(|c| !c.is_ascii_digit()).collect();
        if p == "Q" { "local".into() } else { p }
    }

    /// Local first, then by text.
    fn order(a: &&str, b: &&str) -> Ordering {
        (ns(a) != "local").cmp(&(ns(b) != "local")).then(a.cmp(b))
    }

    fn link(a: &'static str, b: &'static str, tier: u8) -> Link<&'static str> {
        Link { a, b, tier }
    }

    fn run(
        links: &[Link<&'static str>],
        blocks: &[(&'static str, &'static str)],
    ) -> Clusters<&'static str> {
        compute(links, blocks, ns, order)
    }

    // 0004 §2 (04 §4.2): a chain that would join two Wikidata items holds both links.
    #[test]
    fn two_members_of_one_namespace_hold_every_link() {
        let links = [
            link("WDQ1", "OAW1", 1),
            link("OAW1", "WDQ2", 1),
            link("Q5", "WDQ9", 1),
        ];
        let c = run(&links, &[]);
        assert_eq!(c.clusters, vec![vec!["Q5", "WDQ9"]]);
        assert_eq!(c.conflicts.len(), 1);
        assert_eq!(c.conflicts[0].links, vec![0, 1]);
        assert_eq!(c.conflicts[0].reason, Reason::OneToOne);
        assert_eq!(c.conflicts[0].key, "OAW1");
        assert!(c.held(0).is_some() && c.held(2).is_none());
    }

    // 04 §4.3: different-from beats a link, even one that joins the two through a third.
    #[test]
    fn different_from_keeps_two_apart() {
        let links = [link("Q1", "WDQ1", 1), link("WDQ1", "OAW1", 1)];
        let c = run(&links, &[("Q1", "OAW1")]);
        assert!(c.clusters.is_empty());
        assert_eq!(c.conflicts[0].reason, Reason::Blocked);
        assert_eq!(c.conflicts[0].links, vec![0, 1]);
        // A block between IDs no link names changes nothing.
        let c = run(&links, &[("Q7", "OAW7")]);
        assert_eq!(c.clusters, vec![vec!["Q1", "OAW1", "WDQ1"]]);
    }

    // 04 §4.3: a higher tier beats a lower one; the lower tier's link is held.
    #[test]
    fn a_higher_tier_wins() {
        let links = [
            link("Q1", "WDQ1", 1),
            link("Q1", "WDQ2", 2),
            link("WDQ3", "OAW3", 2),
        ];
        let c = run(&links, &[]);
        assert_eq!(c.clusters, vec![vec!["OAW3", "WDQ3"], vec!["Q1", "WDQ1"]]);
        assert_eq!(c.conflicts.len(), 1);
        assert_eq!(c.conflicts[0].links, vec![1]);
    }

    // 04 §4.2: the result does not depend on the order the links arrive in.
    #[test]
    fn the_order_of_links_does_not_matter() {
        let links = [
            link("WDQ1", "OAW1", 1),
            link("Q1", "WDQ1", 1),
            link("OAW1", "WDQ2", 1),
            link("Q2", "OAA2", 2),
            link("OAA2", "WDQ5", 2),
        ];
        let a = run(&links, &[]);
        let mut reversed = links.to_vec();
        reversed.reverse();
        let b = run(&reversed, &[]);
        assert_eq!(a.clusters, b.clusters);
        let keys = |c: &Clusters<&'static str>| -> Vec<&'static str> {
            c.conflicts.iter().map(|x| x.key).collect()
        };
        assert_eq!(keys(&a), keys(&b));
        assert_eq!(a.clusters, vec![vec!["Q2", "OAA2", "WDQ5"]]);
    }

    // 04 §4.4: the canonical member is the highest-ranked; a self-link joins nothing.
    #[test]
    fn the_canonical_member_comes_first() {
        let c = run(&[link("WDQ1", "Q9", 1), link("Q9", "Q9", 1)], &[]);
        assert_eq!(c.clusters, vec![vec!["Q9", "WDQ1"]]);
        assert!(c.conflicts.is_empty());
        assert_eq!(c.cluster_of(&"WDQ1"), Some(&["Q9", "WDQ1"][..]));
        assert_eq!(c.cluster_of(&"Q2"), None);
    }

    #[test]
    fn stable_ids_are_stable_and_positive() {
        assert_eq!(stable_id("Q9"), stable_id("Q9"));
        assert_ne!(stable_id("Q9"), stable_id("Q8"));
        assert!(stable_id("") >= 0 && stable_id("WDQ123456789") >= 0);
    }
}
