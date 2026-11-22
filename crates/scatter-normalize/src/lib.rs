//! Normalizers and keys for Wikibase values (ADR 0004 §7, 0009 §2, 0026 §1, 0029 §1).
//!
//! The base crate of the pair 0115 §10 makes: what any Wikibase can use, with Triplespace's
//! keyed entity types in `scatter-keyed`, which builds on it. All pure and all built for
//! `wasm32-unknown-unknown`, so that a browser normalizes values with the same code as a
//! server (0005 §3, rule 7):
//!
//! - **Normalized value keys** ([`value`]): the canonical form values are compared by, per
//!   Wikibase data type, including the URL normalizer of 0026 §1 ([`url`]) and the UTS 46 host
//!   normalizer it uses ([`domain`]).
//! - **Resolvers** ([`resolver`]): namespaces whose titles are keys looked up against a
//!   property, the registry embedded from `registry/resolvers.toml` and the normalizers of its
//!   defaults (`doi`, `url`), whose case rule is the rule for every property bound to the
//!   resolver's role.
//! - The text normalizations the others share: NFC and Unicode case folding.
//!
//! Policy is data (0005 §3, rule 3): a registry is a value a caller passes in, with the
//! embedded file as the default.

#![forbid(unsafe_code)]

// The README's example compiles and runs as a doctest (0115 §3).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod domain;
pub mod resolver;
pub mod url;
pub mod value;

pub use resolver::{Case, Resolver, ResolverError, ResolverRegistry};
pub use url::{UrlError, normalize_url};
pub use value::ValueKey;

/// The default resolver registry, embedded at build time from `registry/resolvers.toml`, a
/// symlink to the repository's `docs/registry/resolvers.toml` that `cargo package` stores as
/// a file (0115 §2).
pub const RESOLVERS_TOML: &str = include_str!("../registry/resolvers.toml");

/// Unicode NFC of a string, the key of the `string` data type (0004 §7).
#[must_use]
pub fn nfc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfc().collect()
}

/// Whether a string is already in NFC.
#[must_use]
pub fn is_nfc(s: &str) -> bool {
    unicode_normalization::is_nfc(s)
}

/// Unicode default case folding (full, non-Turkic), as 0017 §5 and 0029 §1 use it.
#[must_use]
pub fn casefold(s: &str) -> String {
    icu_casemap::CaseMapper::new().fold_string(s).into_owned()
}

#[cfg(test)]
mod tests {
    /// The embedded registry is the repository's: this fails a checkout that did not make
    /// the symlink, or a copy that has drifted (0115 §2).
    #[test]
    fn embedded_registry_is_the_repository_s() {
        let docs = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/registry/resolvers.toml"
        );
        if let Ok(text) = std::fs::read_to_string(docs) {
            assert_eq!(super::RESOLVERS_TOML, text);
        }
    }
}
