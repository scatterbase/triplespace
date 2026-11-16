//! Surrogates for keyed entities (ADR 0009 §7).
//!
//! A key can be personal data and a log header survives erasure, so a header key is never
//! the key itself and never a hash of it (domain names can be enumerated from zone files).
//! Each present keyed entity gets a surrogate: an opaque number the instance mints in
//! sequence, one sequence per keyed type, the first time a key is written to. The mapping
//! from surrogate to key is a log record whose body can be erased; an index projected
//! from those records resolves keys to surrogates when a change set is appended.
//!
//! This module holds the allocator's contract and an in-memory implementation for tests
//! and single-process tools. The durable implementation is a projection over the mapping
//! records (0013 §5.4, `view.keyed_surrogate`) and lives in the surfaces.

use std::collections::BTreeMap;

/// Resolves keys to surrogates, minting one the first time a key is seen.
pub trait SurrogateAllocator {
    /// The implementation's error, such as a database error.
    type Error;

    /// The surrogate for `key` of `keyed_type`, minted if none exists. `key` must be in
    /// canonical form; the allocator does not normalize.
    fn surrogate(&mut self, keyed_type: &str, key: &str) -> Result<u64, Self::Error>;

    /// The surrogate for `key` if one exists, without minting.
    fn lookup(&self, keyed_type: &str, key: &str) -> Result<Option<u64>, Self::Error>;

    /// The key a surrogate stands for, or `None` if none was minted or the mapping was
    /// erased (0009 §7: after erasure only an opaque number remains).
    fn key(&self, keyed_type: &str, surrogate: u64) -> Result<Option<String>, Self::Error>;
}

/// An allocator that keeps its mappings in memory. Surrogates start at 1 per type.
#[derive(Debug, Default, Clone)]
pub struct InMemorySurrogates {
    by_key: BTreeMap<(String, String), u64>,
    by_surrogate: BTreeMap<(String, u64), String>,
    next: BTreeMap<String, u64>,
}

impl InMemorySurrogates {
    /// An empty allocator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the key behind a surrogate, as an erasure does; the surrogate stays taken.
    pub fn erase(&mut self, keyed_type: &str, surrogate: u64) {
        if let Some(key) = self
            .by_surrogate
            .remove(&(keyed_type.to_string(), surrogate))
        {
            self.by_key.remove(&(keyed_type.to_string(), key));
        }
    }
}

/// The in-memory allocator cannot fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Never {}

impl SurrogateAllocator for InMemorySurrogates {
    type Error = Never;

    fn surrogate(&mut self, keyed_type: &str, key: &str) -> Result<u64, Never> {
        let k = (keyed_type.to_string(), key.to_string());
        if let Some(&s) = self.by_key.get(&k) {
            return Ok(s);
        }
        let next = self.next.entry(keyed_type.to_string()).or_insert(1);
        let s = *next;
        *next += 1;
        self.by_key.insert(k, s);
        self.by_surrogate
            .insert((keyed_type.to_string(), s), key.to_string());
        Ok(s)
    }

    fn lookup(&self, keyed_type: &str, key: &str) -> Result<Option<u64>, Never> {
        Ok(self
            .by_key
            .get(&(keyed_type.to_string(), key.to_string()))
            .copied())
    }

    fn key(&self, keyed_type: &str, surrogate: u64) -> Result<Option<String>, Never> {
        Ok(self
            .by_surrogate
            .get(&(keyed_type.to_string(), surrogate))
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_in_sequence_per_type_and_is_stable() {
        let mut a = InMemorySurrogates::new();
        assert_eq!(a.surrogate("domain", "en.wikipedia.org"), Ok(1));
        assert_eq!(a.surrogate("domain", "example.org"), Ok(2));
        assert_eq!(a.surrogate("domain", "en.wikipedia.org"), Ok(1));
        assert_eq!(
            a.surrogate("keyword", "dna"),
            Ok(1),
            "one sequence per type"
        );
        assert_eq!(a.lookup("domain", "example.org"), Ok(Some(2)));
        assert_eq!(a.lookup("domain", "nope.example"), Ok(None));
        assert_eq!(a.key("domain", 2), Ok(Some("example.org".to_string())));
    }

    #[test]
    fn erasure_leaves_an_opaque_number() {
        let mut a = InMemorySurrogates::new();
        a.surrogate("domain", "personal-name.example").unwrap();
        a.erase("domain", 1);
        assert_eq!(a.key("domain", 1), Ok(None));
        assert_eq!(a.lookup("domain", "personal-name.example"), Ok(None));
        // A key seen after erasure gets a new surrogate (0007 §5's rule for IPs, applied here).
        assert_eq!(a.surrogate("domain", "personal-name.example"), Ok(2));
    }
}
