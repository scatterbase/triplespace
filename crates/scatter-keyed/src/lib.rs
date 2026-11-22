//! Keyed entity types: Triplespace's extension to Wikibase (ADR 0009 §1–2 and §6–7, 0017 §3
//! and §5, 0048).
//!
//! The extension crate of the pair 0115 §10 makes, over `scatter-normalize`. A keyed entity
//! is one whose ID is its normalized natural key with a prefix, not a minted number:
//! `domain:en.wikipedia.org`, `keyword:climate-change`, `notation:osm:amenity=cafe`.
//!
//! - **The registry** ([`keyed`]): each type's namespace, data type, IRI template, ID prefix and
//!   display rule, embedded from `registry/keyed-types.toml`; keyed IDs and concept IRIs.
//! - **Key grammars and normalizers**: the Domain key is `scatter-normalize`'s host
//!   normalizer; [`keyword`]; and [`notation`], with its scheme registry embedded from
//!   `registry/notation-schemes.toml` and the [`notation::osm`] grammar of its `osm` scheme.
//! - **Surrogates** ([`surrogate`]): the allocator trait that keeps keys, which can be personal
//!   data, out of log headers.
//!
//! All pure and built for `wasm32-unknown-unknown` (0005 §3, rule 7). Policy is data (rule 3):
//! a registry is a value a caller passes in, with the embedded files as the defaults, and a
//! keyed registry normalizes notations against the scheme registry it was built with.

#![forbid(unsafe_code)]

// The README's example compiles and runs as a doctest (0115 §3).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod keyed;
pub mod keyword;
pub mod notation;
pub mod surrogate;

pub use keyed::{Display, KeyError, KeyedId, KeyedRegistry, KeyedType, KeyedTypeError};
pub use notation::{NotationError, Scheme, SchemeRegistry, SchemeRegistryError};
pub use scatter_normalize::domain;
pub use surrogate::{InMemorySurrogates, SurrogateAllocator};

/// The default keyed-type registry, embedded at build time from `registry/keyed-types.toml`,
/// a symlink to the repository's `docs/registry/keyed-types.toml` (0115 §2).
pub const KEYED_TYPES_TOML: &str = include_str!("../registry/keyed-types.toml");

/// The default notation scheme registry, embedded at build time from
/// `registry/notation-schemes.toml`, a symlink to the repository's file (0115 §2).
pub const NOTATION_SCHEMES_TOML: &str = include_str!("../registry/notation-schemes.toml");

#[cfg(test)]
mod tests {
    /// The embedded registries are the repository's (0115 §2).
    #[test]
    fn embedded_registries_are_the_repository_s() {
        for (name, embedded) in [
            ("keyed-types.toml", super::KEYED_TYPES_TOML),
            ("notation-schemes.toml", super::NOTATION_SCHEMES_TOML),
        ] {
            let docs = format!("{}/../../docs/registry/{name}", env!("CARGO_MANIFEST_DIR"));
            if let Ok(text) = std::fs::read_to_string(docs) {
                assert_eq!(embedded, text, "{name}");
            }
        }
    }
}
