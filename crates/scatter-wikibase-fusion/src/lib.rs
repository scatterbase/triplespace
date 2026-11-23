//! How Triplespace combines and indexes Wikibase statements (0115 §13): an extension crate
//! over `scatter-wikibase-model`, under 22 §1.2 rule 12.
//!
//! - **Statement keys** ([`key`]): what the resolved view fuses statements by (0004 §7–8),
//!   with the canonical IDs and resolvers passed in through [`key::Canon`].
//! - **The search document** ([`search`]): one document of the `entities` index (0014 §7),
//!   which carries its target's visibility set as `read_groups` (0056 §8).
//! - **Badges** ([`badges`]): marks drawn beside an entity, a sitelink or a statement (0114),
//!   with the registry embedded from `registry/badges.toml`.
//!
//! Pure (0005 §3, rule 2).

#![forbid(unsafe_code)]

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod badges;
pub mod key;
pub mod search;

pub use badges::{Badge, BadgeError, BadgeRegistry, BadgeSource};
pub use key::{Canon, Identity, snak_key, statement_key, value_key};
pub use search::{DocumentContext, EntityDocument, entity_document};

#[cfg(test)]
mod tests {
    /// The embedded badge registry is the repository's: this fails a checkout that did not
    /// make the symlink, or a copy that has drifted (0115 §2).
    #[test]
    fn embedded_registry_is_the_repository_s() {
        let docs = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/registry/badges.toml"
        );
        if let Ok(text) = std::fs::read_to_string(docs) {
            assert_eq!(super::badges::BADGES_TOML, text);
        }
    }
}
