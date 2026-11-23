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

/// The first number a source's namespace may take: a tenant's own range (05 §6.5).
pub const FIRST_SOURCE_NAMESPACE: i32 = 3000;

/// A source's namespace on its tenant (05 §6.5): of kind `resolver`, its talk number the
/// next, reserved. The server gives it the lowest free even number from 3000 on the tenant
/// when the source is first declared with one, or first written again (0078 A10); the
/// number is then fixed, and the names may change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SourceNamespace {
    /// Its number, the server's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,
    /// Its canonical name, the tenant's choice: `MHC` or `Miraheze Communities`.
    pub canonical: String,
    /// Its other names. The source's name is always one of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

impl SourceNamespace {
    /// A namespace of these names, its number left to the server.
    #[must_use]
    pub fn new(canonical: impl Into<String>, aliases: Vec<String>) -> Self {
        Self {
            id: None,
            canonical: canonical.into(),
            aliases,
        }
    }

    /// The namespace a source of this name gets when its declaration names none: the name
    /// in capitals, `MHC` for `mhc` (0078 A10).
    #[must_use]
    pub fn default_for(name: &str) -> Self {
        Self {
            id: None,
            canonical: name.to_ascii_uppercase(),
            aliases: Vec::new(),
        }
    }

    /// Its names, canonical first.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.canonical.as_str()).chain(self.aliases.iter().map(String::as_str))
    }

    /// Whether `prefix` is one of its names, in any case, `_` as a space.
    #[must_use]
    pub fn is_named(&self, prefix: &str) -> bool {
        let want = prefix.replace('_', " ");
        let want = want.trim();
        self.names().any(|n| n.eq_ignore_ascii_case(want))
    }
}

/// The `entity-source` config record's content (0078 §1).
///
/// Built outside this crate with [`EntitySource::new`] and its public fields, so that a new
/// field is not a breaking change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
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
    /// The source's namespace (05 §6.5); the server fills it in on a write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<SourceNamespace>,
    /// Empty, or the registry provider code that replaced the source (0078 §9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted_to: Option<String>,
    /// Retired: the record is kept, so that the name, the number and the templates stay
    /// reserved and the source's IDs keep resolving, but it mints nothing new (0078 A8).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retired: bool,
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
    /// A later record changes what the first fixed: the number, or a type's prefix,
    /// entity type, ID grammar or IRI template (05 §6.4).
    #[error("source `{name}`: {what} is fixed by its first record (ts-source-binding)")]
    Binding {
        /// The source.
        name: String,
        /// What changed.
        what: String,
    },
    /// A type's IRI template is a registry provider's: the tenant adds the provider to
    /// its `providers` list instead (05 §6.8).
    #[error(
        "source `{name}`: its IRI templates are registry provider {code}'s; add the provider instead (ts-source-is-provider)"
    )]
    IsProvider {
        /// The source.
        name: String,
        /// The provider's code.
        code: String,
    },
    /// The namespace's names are malformed: empty, or with a colon.
    #[error("source `{name}`: `{what}` is not a namespace name")]
    NamespaceName {
        /// The source.
        name: String,
        /// The name refused.
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
    /// A source of this name, label and types, with every other field empty: no API, no
    /// number or namespace yet, not promoted or retired.
    #[must_use]
    pub fn new(name: impl Into<String>, label: impl Into<String>, types: Vec<SourceType>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            types,
            api: None,
            entity_data: None,
            revision_ids: false,
            licence: None,
            number: None,
            namespace: None,
            promoted_to: None,
            retired: false,
        }
    }

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
        if let Some(p) = self.duplicates(registry) {
            return Err(SourceError::IsProvider {
                name: self.name.clone(),
                code: p.code.clone(),
            });
        }
        self.validate_shape()
    }

    /// The shape rules alone, without the registry's: what a later record of a source
    /// already declared is held to, since a source keeps a name a registry update added
    /// later (05 §6.3 rule 2), and a provider added later that duplicates it is a
    /// promotion, not a refusal (05 §6.8).
    ///
    /// # Errors
    ///
    /// [`SourceError`].
    pub fn validate_shape(&self) -> Result<(), SourceError> {
        if !is_source_name(&self.name) {
            return Err(SourceError::Name(self.name.clone()));
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
        if let Some(ns) = &self.namespace
            && let Some(bad) = ns
                .names()
                .find(|n| n.trim().is_empty() || n.contains(':') || n.trim() != *n)
        {
            return Err(SourceError::NamespaceName {
                name: self.name.clone(),
                what: bad.to_string(),
            });
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

    /// The registry provider one of whose type IRI templates this source declares too, if
    /// any: a declaration of it is refused (05 §6.8), and a source that predates the
    /// provider can be promoted to it.
    #[must_use]
    pub fn duplicates<'r>(&self, registry: &'r Registry) -> Option<&'r Provider> {
        registry.providers().iter().find(|p| {
            !p.source
                && p.types
                    .iter()
                    .any(|pt| self.types.iter().any(|t| t.iri == pt.iri))
        })
    }

    /// The registry provider this unpromoted source can be promoted to (05 §6.8): one
    /// that declares every one of the source's IRI templates.
    #[must_use]
    pub fn promotable_to<'r>(&self, registry: &'r Registry) -> Option<&'r Provider> {
        if self.promoted_to.as_deref().is_some_and(|c| !c.is_empty()) || self.types.is_empty() {
            return None;
        }
        registry.providers().iter().find(|p| {
            !p.source
                && self
                    .types
                    .iter()
                    .all(|t| p.types.iter().any(|pt| pt.iri == t.iri))
        })
    }

    /// Checks a later record of this source against `earlier`, the record it replaces
    /// (05 §6.4): the number and every type `earlier` declares, with its entity type, ID
    /// grammar and IRI template, are kept. A type may be added; none may be dropped.
    ///
    /// # Errors
    ///
    /// [`SourceError::Binding`].
    pub fn check_binding(&self, earlier: &EntitySource) -> Result<(), SourceError> {
        let binding = |what: String| SourceError::Binding {
            name: self.name.clone(),
            what,
        };
        if earlier.number.is_some() && self.number != earlier.number {
            return Err(binding("the number".into()));
        }
        let ns_id = |s: &EntitySource| s.namespace.as_ref().and_then(|n| n.id);
        if ns_id(earlier).is_some() && ns_id(self) != ns_id(earlier) {
            return Err(binding("the namespace's number".into()));
        }
        for was in &earlier.types {
            let Some(now) = self
                .types
                .iter()
                .find(|t| t.upstream_prefix.eq_ignore_ascii_case(&was.upstream_prefix))
            else {
                return Err(binding(format!("type `{}`", was.upstream_prefix)));
            };
            if now.entity_type != was.entity_type {
                return Err(binding(format!(
                    "the entity type of `{}`",
                    was.upstream_prefix
                )));
            }
            if now.id_grammar != was.id_grammar {
                return Err(binding(format!(
                    "the ID grammar of `{}`",
                    was.upstream_prefix
                )));
            }
            if now.iri != was.iri {
                return Err(binding(format!(
                    "the IRI template of `{}`",
                    was.upstream_prefix
                )));
            }
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
                page_id_slot: None,
            })
            .collect();
        Some(Provider {
            code: String::new(),
            slug: self.name.clone(),
            name: self.label.clone(),
            number,
            issuer: String::new(),
            revision_ids: self.revision_ids,
            // A source is a Wikibase: its entities have page IDs (0078 §1).
            page_ids: true,
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
            retired: self.retired || self.promoted_to.is_some(),
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
            namespace: None,
            promoted_to: None,
            retired: false,
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

    // 05 §6.4: a later record keeps the number and every type the first declared; it may
    // change the label and the endpoints, and add a type.
    #[test]
    fn a_later_record_keeps_what_the_first_fixed() {
        let mut first = source();
        first.number = Some(4096);
        let mut later = first.clone();
        later.label = "Renamed".into();
        later.api = Some("https://elsewhere.example/w/api.php".into());
        later.types.push(SourceType {
            upstream_prefix: "L".into(),
            entity_type: "lexeme".into(),
            id_grammar: "digits".into(),
            iri: "https://creditunions.miraheze.org/entity/{upstream_id}".into(),
        });
        assert_eq!(later.check_binding(&first), Ok(()));
        let refused = |change: &dyn Fn(&mut EntitySource)| {
            let mut later = first.clone();
            change(&mut later);
            later.check_binding(&first).unwrap_err().to_string()
        };
        assert!(refused(&|s| s.number = Some(4097)).contains("the number is fixed"));
        assert!(
            refused(&|s| s.types[0].iri = "https://x.example/{upstream_id}".into()).contains(
                "the IRI template of `Q` is fixed by its first record (ts-source-binding)"
            )
        );
        assert!(refused(&|s| s.types[0].entity_type = "lexeme".into()).contains("entity type"));
        assert!(refused(&|s| s.types[0].id_grammar = "token".into()).contains("ID grammar"));
        assert!(
            refused(&|s| {
                s.types.remove(1);
            })
            .contains("type `P`")
        );
        // The prefix's case is not a change.
        assert_eq!(
            {
                let mut later = first.clone();
                later.types[0].upstream_prefix = "q".into();
                later.check_binding(&first)
            },
            Ok(())
        );
    }

    // A retired source keeps its place in the tenant's table of names, retired.
    #[test]
    fn a_retired_source_is_a_retired_provider() {
        let mut s = source();
        s.number = Some(4096);
        assert!(!s.to_provider().unwrap().retired);
        s.retired = true;
        assert!(s.to_provider().unwrap().retired);
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
        // A source with a name a registry update gave a provider shadows the provider's
        // slug on its tenant; the provider is still reached by its code (05 §6.3 rule 2).
        let mut shadowing = source().to_provider().unwrap();
        shadowing.slug = "librarybase".into();
        let r = Registry::default_registry()
            .with_sources(vec![shadowing.clone()])
            .unwrap();
        assert!(r.by_slug("librarybase").unwrap().source);
        assert_eq!(r.by_code("LB").unwrap().slug, "librarybase");
        assert!(!r.by_code("LB").unwrap().source);
        // Two sources of one name are refused.
        assert!(
            Registry::default_registry()
                .with_sources(vec![shadowing.clone(), shadowing])
                .is_err()
        );
    }

    /// A retired source's IDs keep parsing and resolving read-only: "its IDs keep parsing and resolving read-only" (0078 §9).
    #[test]
    fn a_retired_sources_ids_keep_parsing_and_resolving() {
        let mut s = source();
        s.number = Some(4096);
        s.retired = true;
        let p = s.to_provider().unwrap();
        assert!(p.retired);
        let r = Registry::default_registry().with_sources(vec![p]).unwrap();
        let id = r.parse_foreign_id("opencredit:Q8").unwrap();
        assert_eq!(id.upstream_id(), "Q8");
        assert_eq!(
            id.concept_iri().as_str(),
            "https://creditunions.miraheze.org/entity/Q8"
        );
        let p = r.by_slug("opencredit").unwrap();
        assert_eq!(
            r.from_upstream(p, "P3").unwrap().to_string(),
            "opencredit:P3"
        );
    }

    /// A later record may change `entity_data`: "A later record may change its label, `api`, `entity_data`," (0078 §9).
    #[test]
    fn a_later_record_may_change_entity_data() {
        let first = source();
        let mut later = first.clone();
        assert!(first.entity_data.is_none());
        later.entity_data =
            Some("https://creditunions.miraheze.org/wiki/Special:EntityData/".into());
        assert_eq!(later.check_binding(&first), Ok(()));
        // The change reaches the provider the record makes.
        assert_eq!(
            later.to_provider().unwrap().entity_data.as_deref(),
            Some("https://creditunions.miraheze.org/wiki/Special:EntityData/")
        );
    }

    // 05 §6.5, 0078 A10: a later record may rename the namespace but keeps its number.
    #[test]
    fn a_namespace_keeps_its_number_and_may_be_renamed() {
        let mut first = source();
        first.namespace = Some(SourceNamespace {
            id: Some(3000),
            canonical: "OC".into(),
            aliases: vec!["opencredit".into()],
        });
        let mut later = first.clone();
        later.namespace.as_mut().unwrap().canonical = "Open Credit".into();
        assert_eq!(later.check_binding(&first), Ok(()));
        later.namespace.as_mut().unwrap().id = Some(3002);
        assert!(
            later
                .check_binding(&first)
                .unwrap_err()
                .to_string()
                .contains("the namespace's number")
        );
        let ns = first.namespace.as_ref().unwrap();
        assert!(ns.is_named("oc") && ns.is_named("OpenCredit") && !ns.is_named("Item"));
        assert_eq!(SourceNamespace::default_for("mhc").canonical, "MHC");
        let mut colon = first.clone();
        colon.namespace.as_mut().unwrap().canonical = "A:B".into();
        assert!(matches!(
            colon.validate_shape(),
            Err(SourceError::NamespaceName { .. })
        ));
    }
}
