//! Entity sources (0078 §1–§4): providers a tenant declares in its own
//! configuration instead of the registry. An [`EntitySource`] is the content of the
//! tenant's `entity-source:{name}` config record, cut to what a source of entities needs,
//! and [`EntitySource::to_provider`] makes it a [`Provider`] so that everything written for
//! registry providers (the adapter, the sync, the reads) serves a source unchanged.
//!
//! A source's entities have IDs of the form `{name}:{upstream ID}` (`mhc:Q1`, 0078 §2):
//! the colon shape, told from a keyed ID by the kind of its prefix in the tenant's table
//! of names (0078 §3), which [`Registry::with_sources`] builds: the registry's names and
//! the tenant's sources in one lookup. A source has no two-letter code, and its records
//! live in the tenant partition `source/{name}` (0078 §4).

use serde::{Deserialize, Serialize};

use crate::grammar::IdGrammar;
use crate::registry::{ActorModel, Deletion, EntityType, Provider, Registry, SyncDeltas};

/// The first provider number an entity source may take: 2^12 (0078 §4).
pub const FIRST_SOURCE_NUMBER: u32 = 1 << 12;
/// One past the last: 2^13.
pub const END_SOURCE_NUMBER: u32 = 1 << 13;

/// The most characters in a source's name.
const NAME_MAX: usize = 32;

/// One entity type a source mints, the fields of a `[[provider.type]]` (0078 §1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceType {
    /// What the source's own IDs of this type start with: `Q`.
    pub upstream_prefix: String,
    /// The Wikibase entity type: `item`, `property`, `lexeme`.
    pub entity_type: String,
    /// The ID grammar after the prefix; `digits` by default.
    #[serde(default = "default_grammar")]
    pub id_grammar: String,
    /// The concept IRI template over `{upstream_id}`.
    pub iri: String,
}

fn default_grammar() -> String {
    IdGrammar::Digits.name().to_string()
}

/// The `entity-source` config record's content (0078 §1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySource {
    /// The prefix of its IDs: `[a-z][a-z0-9-]*`, at most 32 characters.
    pub name: String,
    /// The name shown on chips and origin lines.
    pub label: String,
    /// The entity types it mints.
    #[serde(default)]
    pub types: Vec<SourceType>,
    /// The source's Action API endpoint, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// Its `Special:EntityData/` base, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_data: Option<String>,
    /// Whether it publishes revision IDs.
    #[serde(default)]
    pub revision_ids: bool,
    /// The licence of its data, an SPDX identifier; required once anything is mirrored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// The number in 2^12..2^13, assigned by the server on the first record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// Empty, or the registry provider code that replaced the source (0078 §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted_to: Option<String>,
}

/// Why a declaration is not a source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SourceError {
    /// The name is not `[a-z][a-z0-9-]*` of at most 32 characters.
    #[error(
        "`{0}` is not a source name: lowercase letters, digits and hyphens, starting with a letter, at most 32 characters"
    )]
    Name(String),
    /// The name is one the registry has (0078 §3 rule 1).
    #[error("`{0}` is a name the registry has (ts-source-name-taken)")]
    NameTaken(String),
    /// No types, or a type is malformed.
    #[error("source `{name}`: {what}")]
    Types {
        /// The source.
        name: String,
        /// What is wrong.
        what: String,
    },
    /// The number is outside the sources' range.
    #[error("source `{name}`: number {number} is outside 4096..8191")]
    Number {
        /// The source.
        name: String,
        /// The number.
        number: u32,
    },
}

/// Whether `name` has the shape of a source name.
#[must_use]
pub fn is_source_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= NAME_MAX
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

impl EntitySource {
    /// Checks the declaration against the shape rules and the registry's names (0078 §1,
    /// §6.3 rule 1). The number, when set, must be in range; the server sets it.
    ///
    /// # Errors
    ///
    /// [`SourceError`].
    pub fn validate(&self, registry: &Registry) -> Result<(), SourceError> {
        if !is_source_name(&self.name) {
            return Err(SourceError::Name(self.name.clone()));
        }
        if registry.by_slug(&self.name).is_some() {
            return Err(SourceError::NameTaken(self.name.clone()));
        }
        let types = |what: String| SourceError::Types {
            name: self.name.clone(),
            what,
        };
        if self.types.is_empty() {
            return Err(types("no entity types".into()));
        }
        for t in &self.types {
            if t.upstream_prefix.is_empty()
                || !t.upstream_prefix.bytes().all(|b| b.is_ascii_alphabetic())
            {
                return Err(types(format!(
                    "upstream prefix `{}` is not letters",
                    t.upstream_prefix
                )));
            }
            if t.entity_type.is_empty() {
                return Err(types(format!(
                    "type `{}` names no entity type",
                    t.upstream_prefix
                )));
            }
            if t.id_grammar.parse::<IdGrammar>().is_err() {
                return Err(types(format!("`{}` is not an ID grammar", t.id_grammar)));
            }
            if !t.iri.contains("{upstream_id}") {
                return Err(types(format!(
                    "the IRI template of `{}` lacks `{{upstream_id}}`",
                    t.upstream_prefix
                )));
            }
            if self
                .types
                .iter()
                .filter(|o| o.upstream_prefix.eq_ignore_ascii_case(&t.upstream_prefix))
                .count()
                > 1
            {
                return Err(types(format!(
                    "upstream prefix `{}` is declared twice",
                    t.upstream_prefix
                )));
            }
        }
        if let Some(n) = self.number
            && !(FIRST_SOURCE_NUMBER..END_SOURCE_NUMBER).contains(&n)
        {
            return Err(SourceError::Number {
                name: self.name.clone(),
                number: n,
            });
        }
        Ok(())
    }

    /// The source as a provider (0078 §4): no code, the name as its slug, its number, no
    /// issuer (shallow mirroring keeps no upstream actors), and the Wikibase adapter.
    /// `None` until the server has assigned a number.
    #[must_use]
    pub fn to_provider(&self) -> Option<Provider> {
        let number = self.number?;
        let types = self
            .types
            .iter()
            .map(|t| EntityType {
                code: t
                    .upstream_prefix
                    .chars()
                    .next()
                    .unwrap_or('Q')
                    .to_ascii_uppercase(),
                entity_type: t.entity_type.clone(),
                upstream_prefix: t.upstream_prefix.to_ascii_uppercase(),
                iri: t.iri.clone(),
                namespace: None,
                label: None,
                id_grammar: t.id_grammar.parse().unwrap_or_default(),
                key_mapped: false,
            })
            .collect();
        Some(Provider {
            code: String::new(),
            slug: self.name.clone(),
            name: self.label.clone(),
            number,
            issuer: String::new(),
            revision_ids: self.revision_ids,
            article_path: None,
            api: self.api.clone(),
            entity_data: self.entity_data.clone(),
            adapter: Some("scatter-adapter-wikibase".into()),
            actor_model: ActorModel::ProviderOnly,
            sync_deltas: SyncDeltas::Summary,
            deletion: Deletion::Tombstone,
            agent_iri: None,
            id_grammar: IdGrammar::Digits,
            trust: None,
            keys: None,
            retired: self.promoted_to.is_some(),
            adapter_config: None,
            types,
            chip: None,
            source: true,
        })
    }

    /// The lowest number in the sources' range not among `taken` (0078 §4).
    #[must_use]
    pub fn next_number(taken: impl IntoIterator<Item = u32>) -> Option<u32> {
        let taken: std::collections::BTreeSet<u32> = taken.into_iter().collect();
        (FIRST_SOURCE_NUMBER..END_SOURCE_NUMBER).find(|n| !taken.contains(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> EntitySource {
        EntitySource {
            name: "opencredit".into(),
            label: "OpenCredit".into(),
            types: vec![
                SourceType {
                    upstream_prefix: "Q".into(),
                    entity_type: "item".into(),
                    id_grammar: "digits".into(),
                    iri: "https://creditunions.miraheze.org/entity/{upstream_id}".into(),
                },
                SourceType {
                    upstream_prefix: "P".into(),
                    entity_type: "property".into(),
                    id_grammar: "digits".into(),
                    iri: "https://creditunions.miraheze.org/entity/{upstream_id}".into(),
                },
            ],
            api: Some("https://creditunions.miraheze.org/w/api.php".into()),
            entity_data: None,
            revision_ids: true,
            licence: Some("CC-BY-SA-4.0".into()),
            number: Some(FIRST_SOURCE_NUMBER),
            promoted_to: None,
        }
    }

    #[test]
    fn a_source_validates_and_becomes_a_provider() {
        let r = Registry::default_registry();
        let s = source();
        s.validate(r).unwrap();
        let p = s.to_provider().unwrap();
        assert!(p.source);
        assert_eq!(p.slug, "opencredit");
        assert_eq!(p.mirror_graph(), "source/opencredit");
        assert_eq!(p.entity_type('Q').unwrap().entity_type, "item");

        let mut taken = source();
        taken.name = "wikidata".into();
        assert_eq!(
            taken.validate(r),
            Err(SourceError::NameTaken("wikidata".into()))
        );
        let mut bad = source();
        bad.name = "Open Credit".into();
        assert!(matches!(bad.validate(r), Err(SourceError::Name(_))));
        let mut no_types = source();
        no_types.types.clear();
        assert!(matches!(
            no_types.validate(r),
            Err(SourceError::Types { .. })
        ));
        let mut out = source();
        out.number = Some(7);
        assert!(matches!(out.validate(r), Err(SourceError::Number { .. })));
        assert_eq!(
            EntitySource::next_number([FIRST_SOURCE_NUMBER, FIRST_SOURCE_NUMBER + 1]),
            Some(FIRST_SOURCE_NUMBER + 2)
        );
    }

    #[test]
    fn a_tenant_registry_parses_source_ids() {
        let r = Registry::default_registry()
            .with_sources(vec![source().to_provider().unwrap()])
            .unwrap();
        let id = r.parse_foreign_id("OpenCredit:q8").unwrap();
        assert_eq!(id.to_string(), "opencredit:Q8");
        assert_eq!(id.upstream_id(), "Q8");
        assert_eq!(id.provider().slug, "opencredit");
        assert_eq!(
            id.concept_iri().as_str(),
            "https://creditunions.miraheze.org/entity/Q8"
        );
        let p = r.by_slug("opencredit").unwrap();
        assert_eq!(
            r.from_upstream(p, "P3").unwrap().to_string(),
            "opencredit:P3"
        );
        assert!(r.parse_foreign_id("domain:example.org").is_err());
        assert!(r.parse_foreign_id("opencredit:X8").is_err());
        assert_eq!(r.parse_foreign_id("WDQ42").unwrap().to_string(), "WDQ42");
        assert!(r.by_number(FIRST_SOURCE_NUMBER).is_some());
        // A source may not take a registry name.
        let mut taken = source().to_provider().unwrap();
        taken.slug = "librarybase".into();
        assert!(
            Registry::default_registry()
                .with_sources(vec![taken])
                .is_err()
        );
    }
}
