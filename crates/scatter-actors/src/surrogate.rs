//! Surrogates for actors with no stable ID (0007 §5; 0022 §8).
//!
//! An upstream IP edit, an imported edit under an unregistered prefix, or a fediverse
//! actor gets a **surrogate**: an opaque number the instance mints in sequence, whose IRI is
//! `{base}/actor/{n}` and whose actor record alone holds the raw value it stands for. The
//! same raw value maps to the same surrogate through an index projected from actor
//! records; after that record is erased only the number remains, and the raw value seen
//! again gets a new surrogate.
//!
//! This module holds the allocator's contract and an in-memory implementation for tests
//! and single-process tools. The durable one is a projection over actor records
//! (`view.actor.raw`) and lives in the surfaces.

use std::collections::BTreeMap;

use crate::actor::ActorKind;

/// Resolves raw values to surrogates, minting one the first time a value is seen.
pub trait ActorSurrogates {
    /// The implementation's error, such as a database error.
    type Error;

    /// The surrogate for `raw` of the given surrogate kind, minted if none exists.
    /// Kinds are separate spaces: an IP and an imported name never share a number.
    fn surrogate(&mut self, kind: ActorKind, raw: &str) -> Result<u64, Self::Error>;

    /// The surrogate for `raw` if one exists, without minting.
    fn lookup(&self, kind: ActorKind, raw: &str) -> Result<Option<u64>, Self::Error>;

    /// The raw value a surrogate stands for, or `None` after its record was erased.
    fn raw(&self, surrogate: u64) -> Result<Option<(ActorKind, String)>, Self::Error>;
}

/// An allocator that keeps its mappings in memory. Surrogates start at 1.
#[derive(Debug, Default, Clone)]
pub struct InMemoryActorSurrogates {
    by_raw: BTreeMap<(ActorKind, String), u64>,
    by_surrogate: BTreeMap<u64, (ActorKind, String)>,
    next: u64,
}

impl InMemoryActorSurrogates {
    /// An empty allocator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the raw value behind a surrogate, as an erasure does; the number stays
    /// taken, and the value seen again gets a new one.
    pub fn erase(&mut self, surrogate: u64) {
        if let Some(key) = self.by_surrogate.remove(&surrogate) {
            self.by_raw.remove(&key);
        }
    }
}

/// The in-memory allocator cannot fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Never {}

/// Why a kind cannot have a surrogate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0} actors are not surrogates")]
pub struct NotASurrogateKind(pub &'static str);

impl ActorSurrogates for InMemoryActorSurrogates {
    type Error = NotASurrogateKind;

    fn surrogate(&mut self, kind: ActorKind, raw: &str) -> Result<u64, Self::Error> {
        if !kind.is_surrogate() {
            return Err(NotASurrogateKind(kind.name()));
        }
        if let Some(n) = self.by_raw.get(&(kind, raw.to_string())) {
            return Ok(*n);
        }
        self.next += 1;
        let n = self.next;
        self.by_raw.insert((kind, raw.to_string()), n);
        self.by_surrogate.insert(n, (kind, raw.to_string()));
        Ok(n)
    }

    fn lookup(&self, kind: ActorKind, raw: &str) -> Result<Option<u64>, Self::Error> {
        Ok(self.by_raw.get(&(kind, raw.to_string())).copied())
    }

    fn raw(&self, surrogate: u64) -> Result<Option<(ActorKind, String)>, Self::Error> {
        Ok(self.by_surrogate.get(&surrogate).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_in_sequence_and_forgets_on_erasure() {
        let mut s = InMemoryActorSurrogates::new();
        let a = s.surrogate(ActorKind::Anonymous, "192.0.2.1").unwrap();
        let b = s.surrogate(ActorKind::Imported, "enwiki>Example").unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(s.surrogate(ActorKind::Anonymous, "192.0.2.1").unwrap(), 1);
        assert_eq!(
            s.lookup(ActorKind::Anonymous, "192.0.2.1").unwrap(),
            Some(1)
        );
        assert_eq!(
            s.raw(2).unwrap(),
            Some((ActorKind::Imported, "enwiki>Example".into()))
        );
        s.erase(1);
        assert_eq!(s.raw(1).unwrap(), None);
        assert_eq!(
            s.surrogate(ActorKind::Anonymous, "192.0.2.1").unwrap(),
            3,
            "a new number after erasure"
        );
        assert!(s.surrogate(ActorKind::Registered, "x").is_err());
    }
}
