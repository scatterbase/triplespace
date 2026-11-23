//! The provider registry (ADR 0000 §3, 0002 §4, 0015 §2 and §5, 0017 §2, 0022 §2, 0037 §2).
//!
//! A **provider** is a source whose entity IDs Triplespace can refer to directly: Wikidata,
//! OpenAlex, another Triplespace tenant. Each has a two-letter **code** that prefixes the
//! IDs it minted (`WDQ42`), a **slug** used in graph names (`mirror/wikidata`), and a
//! **number** that partitions the revision-ID space (`revid = number << 40 | n`). Each
//! provider mints one or more **entity types**, each with a one-letter type code, an
//! upstream prefix for rewriting (`Q42` ↔ `WDQ42`), an ID grammar and a concept-IRI
//! template.
//!
//! The registry of record is `docs/registry/providers.toml` in the repository (0015 §5);
//! this crate embeds it as [`DEFAULT_TOML`] and parses it into a [`Registry`]. An instance
//! starts from that default and may diverge through its `config` partition, so nothing
//! here is a constant: the registry is a value passed in by the caller (0005 §3, rule 3).
//!
//! This crate is pure (0005 §3, rule 2): no I/O, no async, no globals beyond the embedded
//! default.

#![forbid(unsafe_code)]

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod grammar;
mod id;
mod registry;
mod revid;
mod source;

pub use grammar::IdGrammar;
pub use id::{ForeignId, IdError, UpstreamRef};
pub use registry::{Chip, Deletion, EntityType, Provider, Registry, RegistryError, Trust};
pub use revid::{RevidError, provider_revid, split_revid};
pub use source::{
    END_SOURCE_NUMBER, EntitySource, FIRST_SOURCE_NUMBER, SourceError, SourceType, is_source_name,
};

/// The default registry, embedded at build time from `registry/providers.toml`, a symlink to
/// the repository's `docs/registry/providers.toml` (0115 §2).
pub const DEFAULT_TOML: &str = include_str!("../registry/providers.toml");

/// The number of bits in the `n` part of a provider-ranged revision ID (0015 §2).
pub const REVID_N_BITS: u32 = 40;

#[cfg(test)]
mod registry_tests {
    /// The embedded registry is the repository's: this fails a checkout that did not make the
    /// symlink, or a copy that has drifted (0115 §2). A packaged crate has no `docs/`, and
    /// skips.
    #[test]
    fn embedded_registry_is_the_repository_s() {
        let docs = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/registry/providers.toml"
        );
        if let Ok(text) = std::fs::read_to_string(docs) {
            assert_eq!(super::DEFAULT_TOML, text);
        }
    }
}
