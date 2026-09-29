//! Normalizers and keys (ADR 0004 §7, 0009 §1–2 and §6–7, 0017 §5, 0026 §1, 0029 §1, 0036 §3).
//!
//! Three things live here, all pure and all built for `wasm32-unknown-unknown` so that the
//! browser normalizes titles and keys with the same code as the server (0005 §3, rule 7):
//!
//! - **Keyed entity types** ([`keyed`]): the registry embedded from
//!   `docs/registry/keyed-types.toml`, the key grammars and normalizers of each type
//!   ([`domain`], [`keyword`], [`osmtag`]), keyed IDs such as `domain:en.wikipedia.org`,
//!   concept IRIs, and the surrogate allocator trait ([`surrogate`]) that keeps keys out
//!   of log headers.
//! - **Normalized value keys** ([`value`]): the canonical form the resolved view compares
//!   literal values by, per data type, including the URL normalizer of 0026 §1 ([`url`]).
//! - **Resolvers** ([`resolver`]): the registry embedded from `docs/registry/resolvers.toml`
//!   and the normalizers of its defaults (`doi`, `url`), whose case rule is the rule for
//!   every property bound to the resolver's role.
//!
//! Policy is data (0005 §3, rule 3): the registries are values a caller passes in, with
//! the embedded files as the defaults an instance starts from.

#![forbid(unsafe_code)]

pub mod domain;
pub mod keyed;
pub mod keyword;
pub mod osmtag;
pub mod resolver;
pub mod surrogate;
pub mod url;
pub mod value;

pub use keyed::{Display, KeyError, KeyedId, KeyedRegistry, KeyedType, KeyedTypeError};
pub use resolver::{Case, Resolver, ResolverError, ResolverRegistry};
pub use surrogate::{InMemorySurrogates, SurrogateAllocator};
pub use url::{UrlError, normalize_url};
pub use value::ValueKey;

/// The default keyed-type registry, `docs/registry/keyed-types.toml`, embedded at build time.
pub const KEYED_TYPES_TOML: &str = include_str!("../../../docs/registry/keyed-types.toml");

/// The default resolver registry, `docs/registry/resolvers.toml`, embedded at build time.
pub const RESOLVERS_TOML: &str = include_str!("../../../docs/registry/resolvers.toml");

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
