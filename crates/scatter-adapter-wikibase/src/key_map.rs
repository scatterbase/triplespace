//! Key-mapped Wikibase providers (0009 §9; 0091 §2–§3): upstream items mapped onto a
//! keyed type's keys by rules in the provider's registry entry, instead of rewritten to
//! prefixed IDs.
//!
//! A provider's `adapter_config` holds `[[key_map]]` rules, one per class of mapped item
//! ([`KeyRules::from_provider`]). Each rule names the upstream **identity property**
//! whose value is the key, the **keyed type** mapped onto, a notation's **scheme**, and
//! optionally a **class**: the rule then applies only to items that have it as a direct
//! value of the configuration's `class_property`. Everything that differs between keyed
//! types (the normalizer, the entity type, the data type of a value pointing at a mapped
//! item) comes from the keyed-type registry, never from this code (0091 §3).
//!
//! Mapping needs the whole dump in view before any item is rewritten, because a value
//! pointing at `Q9` can only become `domain:example.org` once `Q9` is known to map there.
//! So the adapter works in two passes: [`Index::observe`] over every entity, then
//! [`KeyMappedAdapter::sync_item`] over every entity again. Two items that claim the same
//! key are a conflict, and neither is mapped until upstream resolves it. An upstream merge
//! into a mapped item makes the merged item an alias of the key (0009 A13); one between
//! unmapped items is an ordinary mirror redirect ([`KeyMappedAdapter::sync_redirect`]).
//!
//! A provider without rules is read through the same adapter, which then does nothing but
//! [`WikibaseAdapter`]'s rewriting: one adapter serves every Wikibase provider (0091 §1).

use std::collections::{BTreeMap, BTreeSet};

use scatter_keyed::{KeyedRegistry, KeyedType, SchemeRegistry};
use scatter_providers::Provider;
use scatter_wikibase_changeset::{
    Adapter, AdapterError, KeyMap, Upstream, WikibaseAdapter, deterministic_statement_id,
};
use scatter_wikibase_model::entity::{Entity, EntityType, ParsedEntity};
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, IdForm};
use scatter_wikibase_model::statement::{Snak, SnakKind, Statement};
use scatter_wikibase_model::value::{DataType, DataValue};

/// Why a provider's key map could not be read (0091 §2). A configuration error is
/// reported when the adapter is built, never per item.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The provider still uses the flat `identity_property`/`key_type` fields, which 0091
    /// §2 retired.
    #[error(
        "provider `{0}`: `identity_property` and `key_type` on `adapter_config` are retired; write a `[[provider.adapter_config.key_map]]` rule (0091 §2)"
    )]
    FlatForm(String),
    /// `key_map` is not a list of tables.
    #[error("provider `{0}`: `adapter_config.key_map` must be a list of rules")]
    NotAList(String),
    /// A rule lacks a required field.
    #[error("provider `{provider}`: key-map rule {rule} has no `{field}`")]
    Missing {
        /// The provider slug.
        provider: String,
        /// The rule's position, from 1.
        rule: usize,
        /// The field.
        field: &'static str,
    },
    /// A field that must be an upstream entity ID is not one.
    #[error("provider `{provider}`: `{field}` = `{value}` is not an entity ID")]
    NotAnId {
        /// The provider slug.
        provider: String,
        /// The field.
        field: &'static str,
        /// The value.
        value: String,
    },
    /// The keyed type is not in `keyed-types.toml`, or has no normalizer in this build.
    #[error("provider `{provider}`: keyed type `{keyed_type}` is not known to this build")]
    UnknownKeyedType {
        /// The provider slug.
        provider: String,
        /// The keyed type.
        keyed_type: String,
    },
    /// A notation rule names no scheme, or one the build does not know.
    #[error("provider `{provider}`: notation scheme `{scheme}` is not known to this build")]
    UnknownScheme {
        /// The provider slug.
        provider: String,
        /// The scheme, empty when none was named.
        scheme: String,
    },
    /// A rule names a class but the configuration names no `class_property`.
    #[error("provider `{0}`: a key-map rule names a `class`, so `class_property` is required")]
    NoClassProperty(String),
    /// `identity_property_on_mapped` is neither `drop` nor `keep`.
    #[error(
        "provider `{provider}`: `identity_property_on_mapped` must be `drop` or `keep`, not `{value}`"
    )]
    BadDrop {
        /// The provider slug.
        provider: String,
        /// The value.
        value: String,
    },
}

/// Why an item is not mapped though a rule applies to it: a reject, reported, with the
/// item kept as an ordinary foreign item (0009 §9; 0091 §2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MapError {
    /// An invalid identity value, or more than one.
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    /// An item of a mapped class without its identity value.
    #[error("an item of class {class} has no {property} value, so it is not mapped")]
    NoIdentity {
        /// The class, upstream form.
        class: String,
        /// The identity property, upstream form.
        property: String,
    },
    /// Two or more rules give the item identity values.
    #[error("matched by {count} key-map rules ({properties}); an item two rules map is not mapped")]
    SeveralRules {
        /// How many.
        count: usize,
        /// Their identity properties.
        properties: String,
    },
}

/// One rule: items of a class (or every item), mapped onto a keyed type by an identity
/// property.
#[derive(Debug, Clone)]
pub struct KeyRule {
    keyed: KeyedType,
    scheme: Option<String>,
    property: EntityId,
    class: Option<EntityId>,
}

impl KeyRule {
    /// The class the rule is limited to, upstream form.
    #[must_use]
    pub fn class(&self) -> Option<&EntityId> {
        self.class.as_ref()
    }

    /// The keyed type's registry entry.
    #[must_use]
    pub fn keyed(&self) -> &KeyedType {
        &self.keyed
    }

    fn has_values(&self, entity: &Entity) -> bool {
        entity
            .statements
            .get(&self.property)
            .into_iter()
            .flatten()
            .any(|s| matches!(s.mainsnak.data_value(), Some(DataValue::String(_))))
    }
}

impl KeyMap for KeyRule {
    fn keyed_type(&self) -> &str {
        &self.keyed.name
    }

    fn identity_property(&self) -> &EntityId {
        &self.property
    }

    fn key_of(&self, value: &str) -> Option<String> {
        match &self.scheme {
            Some(scheme) => self.keyed.normalize(&format!("{scheme}:{value}")).ok(),
            None => self.keyed.normalize(value).ok(),
        }
    }
}

/// A provider's key map: its rules, with the class property and the identity drop, and
/// the vocabulary settings beside them (0091 §2): the properties whose string values are
/// Wikidata IDs, and whether properties the wiki marks deprecated are skipped.
#[derive(Debug, Clone, Default)]
pub struct KeyRules {
    rules: Vec<KeyRule>,
    class_property: Option<EntityId>,
    drop_identity: bool,
    vocabulary: Vocabulary,
}

/// The vocabulary settings beside a key map (0091 §2 (A3)).
#[derive(Debug, Clone, Default)]
struct Vocabulary {
    /// `concept_link_properties`, upstream form: a string value `Q42` of one becomes an
    /// item value naming the Wikidata item, `WDQ42` (0091 A3).
    concept_links: BTreeSet<EntityId>,
    /// The stored prefix of a Wikidata ID, the registry's `wikidata` code (`WD`); `None`
    /// leaves concept links as strings.
    concept_prefix: Option<String>,
    /// `mark_deprecated_properties`, stored form: the property and class of the statement
    /// written onto a property the wiki marks deprecated (`WDP31` → `WDQ142241643`,
    /// *obsolete predicate*; 0091 A4, 0114 §2).
    deprecated_mark: Option<Box<(EntityId, EntityId)>>,
}

/// The vocabulary settings beside a key map (0091 §2 (A3)): `concept_link_properties`, each
/// an upstream property ID, and `mark_deprecated_properties`, a Wikidata property and class.
fn vocabulary(provider: &Provider) -> Result<Vocabulary, ConfigError> {
    let Some(cfg) = provider.adapter_config.as_ref() else {
        return Ok(Vocabulary::default());
    };
    let bad = |value: String| ConfigError::NotAnId {
        provider: provider.slug.clone(),
        field: "concept_link_properties",
        value,
    };
    let mut concept_links = BTreeSet::new();
    if let Some(list) = cfg.get("concept_link_properties") {
        let list = list.as_array().ok_or_else(|| bad(list.to_string()))?;
        for v in list {
            let text = v.as_str().unwrap_or_default();
            match EntityId::parse(text) {
                Ok(p) if p.implied_entity_type() == Some("property") => {
                    concept_links.insert(p);
                }
                _ => return Err(bad(text.to_string())),
            }
        }
    }
    let deprecated_mark = match cfg.get("mark_deprecated_properties") {
        None => None,
        Some(mark) => {
            let bad = |value: String| ConfigError::NotAnId {
                provider: provider.slug.clone(),
                field: "mark_deprecated_properties",
                value,
            };
            let part = |key: &str, kind: &str| {
                let text = mark.get(key).and_then(|v| v.as_str()).unwrap_or_default();
                let id = EntityId::parse(text).map_err(|_| bad(mark.to_string()))?;
                if id.form() != IdForm::Local || id.implied_entity_type() != Some(kind) {
                    return Err(bad(text.to_string()));
                }
                let prefix = wikidata_prefix().ok_or_else(|| bad(text.to_string()))?;
                EntityId::parse(&format!("{prefix}{text}")).map_err(|_| bad(text.to_string()))
            };
            Some(Box::new((
                part("property", "property")?,
                part("class", "item")?,
            )))
        }
    };
    Ok(Vocabulary {
        concept_prefix: (!concept_links.is_empty()).then(wikidata_prefix).flatten(),
        concept_links,
        deprecated_mark,
    })
}

/// The stored prefix Wikidata IDs are written under, the registry's `wikidata` code.
fn wikidata_prefix() -> Option<String> {
    scatter_providers::Registry::default_registry()
        .by_slug("wikidata")
        .map(|p| p.code.clone())
}

/// How the wiki marks a deprecated property: its English label opens with this, as the
/// OpenStreetMap Wiki's do (`(DEPRECATED) image`; 0091 A3).
const DEPRECATED_MARK: &str = "(DEPRECATED)";

impl KeyRules {
    /// The rules of a provider's registry entry, over the default keyed-type and scheme
    /// registries, with `class_property`, `identity_property_on_mapped`,
    /// `concept_link_properties` and `mark_deprecated_properties`; a provider without
    /// `key_map` has none (implements 0091 §2 (A4)).
    pub fn from_provider(provider: &Provider) -> Result<Self, ConfigError> {
        Self::with_registries(
            provider,
            KeyedRegistry::default_registry(),
            SchemeRegistry::default_registry(),
        )
    }

    /// As [`from_provider`](Self::from_provider), over given registries.
    pub fn with_registries(
        provider: &Provider,
        keyed: &KeyedRegistry,
        schemes: &SchemeRegistry,
    ) -> Result<Self, ConfigError> {
        let slug = provider.slug.clone();
        let Some(cfg) = provider.adapter_config.as_ref() else {
            return Ok(Self {
                drop_identity: true,
                ..Self::default()
            });
        };
        if cfg.contains_key("identity_property") || cfg.contains_key("key_type") {
            return Err(ConfigError::FlatForm(slug));
        }
        let id = |field: &'static str, value: &str| {
            EntityId::parse(value).map_err(|_| ConfigError::NotAnId {
                provider: slug.clone(),
                field,
                value: value.to_string(),
            })
        };
        let drop_identity = match cfg
            .get("identity_property_on_mapped")
            .and_then(|v| v.as_str())
        {
            None | Some("drop") => true,
            Some("keep") => false,
            Some(other) => {
                return Err(ConfigError::BadDrop {
                    provider: slug,
                    value: other.to_string(),
                });
            }
        };
        let class_property = match cfg.get("class_property").and_then(|v| v.as_str()) {
            Some(p) => Some(id("class_property", p)?),
            None => None,
        };
        let mut rules = Vec::new();
        if let Some(list) = cfg.get("key_map") {
            let list = list
                .as_array()
                .ok_or_else(|| ConfigError::NotAList(slug.clone()))?;
            for (n, rule) in list.iter().enumerate() {
                let rule = rule
                    .as_table()
                    .ok_or_else(|| ConfigError::NotAList(slug.clone()))?;
                let field = |field: &'static str| {
                    rule.get(field)
                        .and_then(|v| v.as_str())
                        .ok_or(ConfigError::Missing {
                            provider: slug.clone(),
                            rule: n + 1,
                            field,
                        })
                };
                let property = id("identity_property", field("identity_property")?)?;
                let type_name = field("key_type")?;
                let keyed_type = keyed
                    .by_name(type_name)
                    .filter(|t| {
                        !matches!(
                            t.normalize(""),
                            Err(scatter_keyed::KeyError::NoNormalizer(_))
                        )
                    })
                    .ok_or_else(|| ConfigError::UnknownKeyedType {
                        provider: slug.clone(),
                        keyed_type: type_name.to_string(),
                    })?
                    .clone();
                let scheme = rule.get("key_scheme").and_then(|v| v.as_str());
                if keyed_type.name == "notation" {
                    let known = scheme.and_then(|s| schemes.by_name(s)).is_some();
                    if !known {
                        return Err(ConfigError::UnknownScheme {
                            provider: slug,
                            scheme: scheme.unwrap_or_default().to_string(),
                        });
                    }
                }
                let class = match rule.get("class").and_then(|v| v.as_str()) {
                    Some(c) => Some(id("class", c)?),
                    None => None,
                };
                if class.is_some() && class_property.is_none() {
                    return Err(ConfigError::NoClassProperty(slug));
                }
                rules.push(KeyRule {
                    keyed: keyed_type,
                    scheme: scheme.map(str::to_string),
                    property,
                    class,
                });
            }
        }
        Ok(Self {
            rules,
            class_property,
            drop_identity,
            vocabulary: vocabulary(provider)?,
        })
    }

    /// The Wikidata ID a concept link's value names, stored form (`WDQ42`): for a snak of
    /// one of `concept_link_properties` (upstream form) whose value is an item, property
    /// or lexeme ID.
    fn concept_target(&self, property: &EntityId, value: &str) -> Option<EntityId> {
        let prefix = self.vocabulary.concept_prefix.as_ref()?;
        if !self.vocabulary.concept_links.contains(property) {
            return None;
        }
        let id = EntityId::parse(value).ok()?;
        if id.form() != IdForm::Local {
            return None;
        }
        EntityId::parse(&format!("{prefix}{value}")).ok()
    }

    /// Whether `entity` is a property the wiki marks deprecated, and this provider marks
    /// such properties.
    fn deprecated(&self, entity: &Entity) -> bool {
        self.vocabulary.deprecated_mark.is_some()
            && entity.entity_type == EntityType::Property
            && entity
                .labels
                .get("en")
                .is_some_and(|l| l.trim_start().starts_with(DEPRECATED_MARK))
    }

    /// The rules, in registry order.
    #[must_use]
    pub fn rules(&self) -> &[KeyRule] {
        &self.rules
    }

    /// The provider maps nothing by key.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The statement written onto a property the wiki marks deprecated, stored form: its
    /// property and class (0091 A4).
    #[must_use]
    pub fn deprecated_mark(&self) -> Option<(&EntityId, &EntityId)> {
        self.vocabulary
            .deprecated_mark
            .as_deref()
            .map(|(p, c)| (p, c))
    }

    /// The index of the rule onto a keyed type, by the type's name.
    #[must_use]
    pub fn rule_for_type(&self, keyed_type: &str) -> Option<usize> {
        self.rules.iter().position(|r| r.keyed.name == keyed_type)
    }

    fn has_class(&self, entity: &Entity, class: &EntityId) -> bool {
        let Some(p) = &self.class_property else {
            return false;
        };
        entity.statements.get(p).into_iter().flatten().any(
            |s| matches!(s.mainsnak.data_value(), Some(DataValue::EntityId(v)) if v.id == *class),
        )
    }

    /// The rule that maps an upstream entity (IDs in the provider's own form) and the
    /// keyed ID it maps to (0091 §2): `Ok(None)` for an item that stays an ordinary
    /// foreign item, an error for a reject.
    pub fn map(&self, entity: &Entity) -> Result<Option<(usize, EntityId)>, MapError> {
        if self.rules.is_empty() || entity.entity_type != EntityType::Item {
            return Ok(None);
        }
        let in_scope: Vec<usize> = (0..self.rules.len())
            .filter(|&i| {
                self.rules[i]
                    .class
                    .as_ref()
                    .is_none_or(|c| self.has_class(entity, c))
            })
            .collect();
        let with_values: Vec<usize> = in_scope
            .iter()
            .copied()
            .filter(|&i| self.rules[i].has_values(entity))
            .collect();
        match with_values.as_slice() {
            [] => match in_scope
                .iter()
                .find_map(|&i| self.rules[i].class.as_ref().map(|c| (i, c)))
            {
                Some((i, class)) => Err(MapError::NoIdentity {
                    class: class.to_string(),
                    property: self.rules[i].property.to_string(),
                }),
                None => Ok(None),
            },
            [i] => Ok(self.rules[*i].map(entity)?.map(|id| (*i, id))),
            many => Err(MapError::SeveralRules {
                count: many.len(),
                properties: many
                    .iter()
                    .map(|&i| self.rules[i].property.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }
}

/// The first pass: which upstream items map to which keyed IDs, and which keyed IDs are
/// claimed by more than one item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    /// Upstream item ID → keyed ID (`domain:example.org`).
    by_upstream: BTreeMap<String, String>,
    /// Upstream item ID → the rule that maps it.
    rule_of: BTreeMap<String, usize>,
    by_key: BTreeMap<String, Vec<String>>,
    conflicts: BTreeSet<String>,
    redirects: BTreeMap<String, String>,
    aliases: BTreeMap<String, Vec<String>>,
}

impl Index {
    /// Observes an upstream entity as the dump wrote it (IDs in the provider's own form).
    /// Returns the keyed ID it maps to, if any; a reject is returned as the error, and the
    /// item stays foreign.
    pub fn observe(
        &mut self,
        entity: &Entity,
        rules: &KeyRules,
    ) -> Result<Option<String>, MapError> {
        let Some((rule, id)) = rules.map(entity)? else {
            return Ok(None);
        };
        let upstream = entity.id.as_str().to_string();
        let keyed = id.as_str().to_string();
        // Observed again (a follower re-reading an item): a changed key releases the old
        // one, and the same key is not a claim against itself.
        if let Some(old) = self.by_upstream.insert(upstream.clone(), keyed.clone())
            && old != keyed
            && let Some(items) = self.by_key.get_mut(&old)
        {
            items.retain(|i| *i != upstream);
        }
        self.rule_of.insert(upstream.clone(), rule);
        let items = self.by_key.entry(keyed.clone()).or_default();
        if !items.contains(&upstream) {
            items.push(upstream);
        }
        Ok(Some(keyed))
    }

    /// Restores a settled index from what a mirror projected of an earlier load
    /// (`view.keyed_map`), for a follower that continues it (0092 §2): the mapped items
    /// with the rule and keyed ID each maps to, and the aliases as (`from`, `to`) pairs.
    /// [`Index::finish`] is still called after new entities are observed.
    pub fn restore(
        mapped: impl IntoIterator<Item = (String, String, usize)>,
        aliases: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let mut index = Index::default();
        for (upstream, keyed, rule) in mapped {
            index.rule_of.insert(upstream.clone(), rule);
            index
                .by_key
                .entry(keyed.clone())
                .or_default()
                .push(upstream.clone());
            index.by_upstream.insert(upstream, keyed);
        }
        for (from, to) in aliases {
            index.redirects.insert(from, to);
        }
        index.finish();
        index
    }

    /// Observes an upstream redirect (`Q18817` → `Q738`), IDs in the provider's form.
    pub fn observe_redirect(&mut self, from: &str, to: &str) {
        self.redirects.insert(from.to_string(), to.to_string());
    }

    /// Settles the index: a key claimed by two items maps neither (0009 §9), and an item
    /// merged into a mapped item becomes an alias of its key (0009 A13).
    pub fn finish(&mut self) {
        self.conflicts.clear();
        for (key, items) in &self.by_key {
            if items.len() > 1 {
                self.conflicts.insert(key.clone());
                for item in items {
                    self.by_upstream.remove(item);
                    self.rule_of.remove(item);
                }
            }
        }
        self.aliases.clear();
        for (from, to) in &self.redirects {
            if self.by_upstream.contains_key(to) {
                self.aliases
                    .entry(to.clone())
                    .or_default()
                    .push(from.clone());
            }
        }
    }

    /// The items upstream merged into a mapped item, in ID order.
    #[must_use]
    pub fn aliases_of(&self, upstream_id: &str) -> &[String] {
        self.aliases.get(upstream_id).map_or(&[], Vec::as_slice)
    }

    /// How many redirects were observed.
    #[must_use]
    pub fn redirects(&self) -> usize {
        self.redirects.len()
    }

    /// The keyed ID an upstream item ID (`Q9`) maps to (`domain:example.org`).
    #[must_use]
    pub fn keyed_id_of(&self, upstream_id: &str) -> Option<&str> {
        self.by_upstream.get(upstream_id).map(String::as_str)
    }

    /// The keyed IDs more than one item claims, with the claimants.
    pub fn conflicts(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.conflicts
            .iter()
            .map(|k| (k.as_str(), self.by_key[k].as_slice()))
    }

    /// How many items map.
    #[must_use]
    pub fn mapped(&self) -> usize {
        self.by_upstream.len()
    }
}

/// What the adapter makes of one upstream entity.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A state for the mirror, IDs in stored form.
    State {
        /// The entity: a keyed entity, or a foreign item or property.
        entity: Box<Entity>,
        /// Its upstream version.
        upstream: Upstream,
        /// The upstream item ID, for a mapped item.
        upstream_id: Option<String>,
        /// The items upstream merged into it, for a mapped item.
        upstream_aliases: Vec<String>,
    },
    /// The item is written as an ordinary foreign item, and this is reported.
    Rejected {
        /// The state, as a foreign item.
        entity: Box<Entity>,
        /// Its upstream version.
        upstream: Upstream,
        /// The upstream ID.
        upstream_id: String,
        /// Why it was not mapped.
        reason: String,
    },
}

/// The adapter for any Wikibase provider in mirror mode, with its key map (0091 §1–§3).
#[derive(Debug, Clone)]
pub struct KeyMappedAdapter {
    inner: WikibaseAdapter,
    rules: KeyRules,
    index: Index,
    /// Keyed type name → its data type, for values that point at mapped items.
    data_types: BTreeMap<String, DataType>,
}

impl KeyMappedAdapter {
    /// Over a provider's adapter, its rules and a settled index: what a mapped item becomes
    /// comes from its rule's keyed type, its normalizer, entity type and data type
    /// (implements 0091 §3).
    #[must_use]
    pub fn new(inner: WikibaseAdapter, rules: KeyRules, index: Index) -> Self {
        let data_types = rules
            .rules
            .iter()
            .map(|r| (r.keyed.name.clone(), DataType::parse(&r.keyed.data_type)))
            .collect();
        Self {
            inner,
            rules,
            index,
            data_types,
        }
    }

    /// The rules.
    #[must_use]
    pub fn rules(&self) -> &KeyRules {
        &self.rules
    }

    /// The settled index.
    #[must_use]
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// The index, to observe entities a follower reads; call [`Index::finish`] after.
    pub fn index_mut(&mut self) -> &mut Index {
        &mut self.index
    }

    /// A dump entity as the mirror stores it: a keyed entity with its upstream ID, a
    /// foreign item or property, or a reported reject written as a foreign item.
    pub fn sync_item(&self, parsed: ParsedEntity) -> Result<Outcome, AdapterError> {
        let upstream = match parsed.page.as_ref().and_then(|p| p.lastrevid) {
            Some(revid) => Upstream::revision(
                revid,
                parsed.page.as_ref().and_then(|p| p.modified.as_deref()),
            ),
            None => Upstream::default(),
        }
        .with_pageid(parsed.page.as_ref().and_then(|p| p.pageid));
        let mut entity = parsed.entity;
        let upstream_id = entity.id.as_str().to_string();
        let deprecated = self.rules.deprecated(&entity);
        if entity.entity_type == EntityType::Property
            && self.rules.vocabulary.concept_prefix.is_some()
            && self.rules.vocabulary.concept_links.contains(&entity.id)
        {
            // Its values are items now, so the property is one of items (0091 A3).
            entity.datatype = Some(DataType::WikibaseItem);
        }
        let rule = self.index.rule_of.get(&upstream_id).copied();
        let reject = if rule.is_some() {
            None
        } else {
            match self.rules.map(&entity) {
                Ok(None) => None,
                Ok(Some((_, id))) => {
                    // Mapped by its own value, but the index did not keep it: a conflict.
                    let key = id.keyed_parts().map_or("", |(_, k)| k);
                    Some(format!(
                        "key `{key}` is claimed by {}; neither item is mapped until upstream resolves the duplicate",
                        self.index
                            .by_key
                            .get(id.as_str())
                            .map_or_else(|| "another item".to_string(), |items| items.join(", "))
                    ))
                }
                Err(e) => Some(e.to_string()),
            }
        };
        self.rewrite_entity(&mut entity)?;
        if deprecated {
            self.mark_deprecated(&mut entity);
        }
        if let Some(rule) = rule.map(|i| &self.rules.rules[i]) {
            // A mapped item is its keyed type (0009 §3), whatever type it had upstream.
            entity.entity_type = EntityType::parse(&rule.keyed.name);
            entity.sitelinks.clear();
            if self.rules.drop_identity {
                let p = self.inner.rewrite_id(rule.property.as_str())?;
                entity.statements.shift_remove(&p);
            }
        }
        Ok(match reject {
            Some(reason) => Outcome::Rejected {
                entity: Box::new(entity),
                upstream,
                upstream_id,
                reason,
            },
            None => Outcome::State {
                entity: Box::new(entity),
                upstream,
                upstream_aliases: if rule.is_some() {
                    self.index.aliases_of(&upstream_id).to_vec()
                } else {
                    Vec::new()
                },
                upstream_id: rule.is_some().then_some(upstream_id),
            },
        })
    }

    /// Writes the deprecation mark onto a property the wiki marks deprecated, stored form:
    /// `WDP31` → `WDQ142241643`, *obsolete predicate*, once, with an ID derived from the
    /// property and the value. Its uses are kept: the mark draws a "(deprecated)" badge
    /// beside them (0091 A4; 0114 §2).
    fn mark_deprecated(&self, entity: &mut Entity) {
        let Some((property, class)) = self.rules.deprecated_mark() else {
            return;
        };
        let value = DataValue::EntityId(scatter_wikibase_model::value::EntityIdValue {
            entity_type: "item".into(),
            id: class.clone(),
        });
        let group = entity.statements.entry(property.clone()).or_default();
        if group
            .iter()
            .any(|s| s.mainsnak.data_value() == Some(&value))
        {
            return;
        }
        let mut statement = Statement::new(Snak::value(
            property.clone(),
            value,
            Some(DataType::WikibaseItem),
        ));
        statement.id = Some(deterministic_statement_id(
            &entity.id,
            &statement,
            &Hasher::local(),
        ));
        group.push(statement);
    }

    /// An upstream redirect (`from` → `to`, in the provider's form) as the mirror records
    /// it: `None` when the target is mapped, since the merged item is then an alias on
    /// the target's state; otherwise a redirect between the two foreign items.
    pub fn sync_redirect(
        &self,
        from: &str,
        to: &str,
        upstream: Upstream,
    ) -> Result<Option<(EntityId, EntityId, Upstream)>, AdapterError> {
        if self.index.keyed_id_of(to).is_some() {
            return Ok(None);
        }
        Ok(Some((
            self.inner.rewrite_id(from)?,
            self.rewrite_id(to)?,
            upstream,
        )))
    }
}

impl Adapter for KeyMappedAdapter {
    fn provider(&self) -> &Provider {
        self.inner.provider()
    }

    fn rewrite_id(&self, upstream: &str) -> Result<EntityId, AdapterError> {
        if let Some(id) = self.index.keyed_id_of(upstream) {
            return Ok(EntityId::parse(id)?);
        }
        self.inner.rewrite_id(upstream)
    }

    fn canonical_iri(&self, id: &EntityId) -> Option<String> {
        self.inner.canonical_iri(id)
    }

    fn value_iris(&self) -> Option<&scatter_wikibase_model::value_iri::ValueIris> {
        self.inner.value_iris()
    }

    /// The provider's one rule, when it has exactly one.
    fn key_map(&self) -> Option<&dyn KeyMap> {
        match self.rules.rules.as_slice() {
            [rule] => Some(rule),
            _ => None,
        }
    }

    /// As the default, then a value that became a keyed ID is typed with its keyed
    /// type's data type (`wikibase-domain`, `wikibase-notation`, …; 0091 §3).
    fn rewrite_snak(&self, snak: &mut Snak) -> Result<(), AdapterError> {
        // A concept link names a Wikidata item by its ID as a string: it becomes an item
        // value naming the item in Wikidata's stored form, never `same-as` (0091 A3).
        if let SnakKind::Value(DataValue::String(text)) = &snak.kind
            && let Some(target) = self.rules.concept_target(&snak.property, text)
        {
            let entity_type = target.implied_entity_type().unwrap_or("item").to_string();
            snak.datatype = Some(DataType::parse(&format!("wikibase-{entity_type}")));
            snak.kind = SnakKind::Value(DataValue::EntityId(
                scatter_wikibase_model::value::EntityIdValue {
                    entity_type,
                    id: target,
                },
            ));
            snak.hash = None;
            snak.property = self.rewrite_id(snak.property.as_str())?;
            return Ok(());
        }
        snak.property = self.rewrite_id(snak.property.as_str())?;
        if let SnakKind::Value(DataValue::EntityId(v)) = &mut snak.kind {
            v.id = self.rewrite_id(v.id.as_str())?;
            if v.id.form() == IdForm::Keyed
                && let Some((t, _)) = v.id.keyed_parts()
                && let Some(data_type) = self.data_types.get(t)
            {
                v.entity_type = t.to_string();
                if snak.datatype == Some(DataType::WikibaseItem) {
                    snak.datatype = Some(data_type.clone());
                }
            }
        }
        self.store_value_iris(snak);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_providers::Registry;
    use scatter_wikibase_model::id::StatementId;
    use scatter_wikibase_model::value::DataType;
    use serde_json::json;

    const XD: &str = "internetdomains";

    fn parsed(v: &serde_json::Value) -> ParsedEntity {
        Entity::from_json(&v.to_string()).unwrap()
    }

    fn item(id: &str, label: &str, claims: &serde_json::Value, revid: u64) -> serde_json::Value {
        json!({"type": "item", "id": id, "labels": {"en": {"language": "en", "value": label}},
            "claims": claims, "lastrevid": revid, "modified": "2026-10-01T00:00:00Z"})
    }

    fn string_claim(p: &str, v: &str, guid: &str) -> serde_json::Value {
        json!([{"mainsnak": {"snaktype": "value", "property": p, "datatype": "string",
            "datavalue": {"value": v, "type": "string"}}, "type": "statement", "rank": "normal", "id": guid}])
    }

    fn item_claim(p: &str, target: &str, guid: &str) -> serde_json::Value {
        json!([{"mainsnak": {"snaktype": "value", "property": p, "datatype": "wikibase-item",
            "datavalue": {"value": {"entity-type": "item", "id": target}, "type": "wikibase-entityid"}},
            "type": "statement", "rank": "normal", "id": guid}])
    }

    /// internetdomains.wiki in miniature: two mapped domains, a registrar, an invalid
    /// value and a duplicate.
    fn dump() -> Vec<serde_json::Value> {
        let mut c9 = serde_json::Map::new();
        c9.insert(
            "P1".into(),
            string_claim(
                "P1",
                "Example.ORG",
                "Q9$00000000-0000-0000-0000-000000000001",
            ),
        );
        c9.insert(
            "P7".into(),
            item_claim("P7", "Q20", "Q9$00000000-0000-0000-0000-000000000002"),
        );
        let mut c11 = serde_json::Map::new();
        c11.insert(
            "P1".into(),
            string_claim(
                "P1",
                "wikipedia.org",
                "Q11$00000000-0000-0000-0000-000000000003",
            ),
        );
        c11.insert(
            "P8".into(),
            item_claim("P8", "Q9", "Q11$00000000-0000-0000-0000-000000000004"),
        );
        vec![
            item("Q9", "example.org", &c9.into(), 55),
            item("Q11", "Wikipedia", &c11.into(), 56),
            item("Q20", "Example Registrar", &json!({}), 57),
            item(
                "Q21",
                "bad",
                &json!({"P1": string_claim("P1", "not a domain!", "Q21$00000000-0000-0000-0000-000000000005")}),
                58,
            ),
            item(
                "Q30",
                "dup a",
                &json!({"P1": string_claim("P1", "dup.example", "Q30$00000000-0000-0000-0000-000000000006")}),
                59,
            ),
            item(
                "Q31",
                "dup b",
                &json!({"P1": string_claim("P1", "dup.example", "Q31$00000000-0000-0000-0000-000000000007")}),
                60,
            ),
        ]
    }

    fn xd_rules() -> KeyRules {
        KeyRules::from_provider(Registry::default_registry().by_slug(XD).unwrap()).unwrap()
    }

    fn adapter(index: Index) -> KeyMappedAdapter {
        KeyMappedAdapter::new(WikibaseAdapter::new(XD).unwrap(), xd_rules(), index)
    }

    /// A mapped item's state keeps the item's ID beside it (05 §4.2), and its statements
    /// keep their upstream UUIDs under the keyed subject, so each statement's node upstream,
    /// `…/statement/Q9-{UUID}`, follows from the two (0002 A44).
    #[test]
    fn a_mapped_state_names_the_item_its_statements_came_from() {
        let rules = xd_rules();
        let mut index = Index::default();
        for v in dump() {
            let _ = index.observe(&parsed(&v).entity, &rules);
        }
        index.finish();
        let adapter = adapter(index);
        let q9 = dump().into_iter().find(|v| v["id"] == "Q9").unwrap();
        let Outcome::State {
            entity,
            upstream_id,
            ..
        } = adapter.sync_item(parsed(&q9)).unwrap()
        else {
            panic!("Q9 maps")
        };
        assert_eq!(entity.id.as_str(), "domain:example.org");
        assert_eq!(upstream_id.as_deref(), Some("Q9"));
        let ids: Vec<StatementId> = entity
            .all_statements()
            .map(|s| s.id.clone().unwrap())
            .collect();
        assert_eq!(
            ids.iter().map(StatementId::as_str).collect::<Vec<_>>(),
            ["domain:example.org$00000000-0000-0000-0000-000000000002"]
        );
        assert_eq!(
            format!("{}-{}", upstream_id.unwrap(), ids[0].uuid()),
            "Q9-00000000-0000-0000-0000-000000000002"
        );
    }

    #[test]
    fn a_restored_index_continues_and_re_observing_is_not_a_claim() {
        let rules = xd_rules();
        let rule = rules.rule_for_type("domain").unwrap();
        // As a follower restores it from `view.keyed_map`: one mapped item with an alias.
        let mut index = Index::restore(
            [("Q9".to_string(), "domain:example.org".to_string(), rule)],
            [("Q8".to_string(), "Q9".to_string())],
        );
        assert_eq!(index.keyed_id_of("Q9"), Some("domain:example.org"));
        assert_eq!(index.aliases_of("Q9"), ["Q8".to_string()]);
        assert_eq!(index.conflicts().count(), 0);

        // The same item read again with the same key: still mapped, no conflict.
        let again = parsed(&dump()[0]);
        index.observe(&again.entity, &rules).unwrap();
        index.finish();
        assert_eq!(index.keyed_id_of("Q9"), Some("domain:example.org"));
        assert_eq!(index.conflicts().count(), 0);

        // The item's key changed upstream: the old key is released, the new one held.
        let moved = parsed(&item(
            "Q9",
            "moved",
            &json!({"P1": string_claim("P1", "moved.example", "Q9$00000000-0000-0000-0000-000000000009")}),
            70,
        ));
        index.observe(&moved.entity, &rules).unwrap();
        index.finish();
        assert_eq!(index.keyed_id_of("Q9"), Some("domain:moved.example"));
        // Another item may now take the released key without a conflict.
        let taker = parsed(&item(
            "Q40",
            "taker",
            &json!({"P1": string_claim("P1", "example.org", "Q40$00000000-0000-0000-0000-000000000010")}),
            71,
        ));
        index.observe(&taker.entity, &rules).unwrap();
        index.finish();
        assert_eq!(index.keyed_id_of("Q40"), Some("domain:example.org"));
        assert_eq!(index.conflicts().count(), 0);
        assert!(rules.rule_for_type("no-such-type").is_none());
    }

    #[test]
    fn the_registered_key_map_is_one_rule_without_a_class() {
        let rules = xd_rules();
        assert_eq!(rules.rules().len(), 1);
        let rule = &rules.rules()[0];
        assert_eq!(rule.keyed_type(), "domain");
        assert_eq!(rule.identity_property().as_str(), "P1");
        assert!(rule.class().is_none());
        assert_eq!(rule.key_of("Example.ORG.").as_deref(), Some("example.org"));
    }

    #[test]
    fn maps_rewrites_and_reports() {
        let rules = xd_rules();
        let mut index = Index::default();
        let mut rejects = 0;
        for v in dump() {
            if index.observe(&parsed(&v).entity, &rules).is_err() {
                rejects += 1;
            }
        }
        assert_eq!(rejects, 1, "Q21's value is not a domain");
        index.finish();
        assert_eq!(index.mapped(), 2);
        assert_eq!(index.keyed_id_of("Q9"), Some("domain:example.org"));
        assert_eq!(
            index.keyed_id_of("Q30"),
            None,
            "a duplicate key maps neither"
        );
        assert_eq!(index.conflicts().count(), 1);

        let adapter = adapter(index);
        let out: Vec<Outcome> = dump()
            .into_iter()
            .map(|v| adapter.sync_item(parsed(&v)).unwrap())
            .collect();

        let Outcome::State {
            entity,
            upstream,
            upstream_id,
            ..
        } = &out[0]
        else {
            panic!("Q9 maps")
        };
        assert_eq!(entity.id.as_str(), "domain:example.org");
        assert_eq!(entity.entity_type, EntityType::Domain);
        assert_eq!(upstream_id.as_deref(), Some("Q9"));
        assert_eq!(upstream.revid, Some(55));
        assert!(
            !entity.statements.keys().any(|p| p.as_str() == "XDP1"),
            "the identity property is dropped on a mapped item"
        );
        let s = entity.all_statements().next().unwrap();
        assert_eq!(s.mainsnak.property.as_str(), "XDP7");
        assert_eq!(
            s.id.as_ref().unwrap().as_str(),
            "domain:example.org$00000000-0000-0000-0000-000000000002"
        );
        assert_eq!(
            s.mainsnak
                .data_value()
                .unwrap()
                .entity_id()
                .unwrap()
                .as_str(),
            "XDQ20",
            "an unmapped target stays foreign"
        );

        let Outcome::State { entity, .. } = &out[1] else {
            panic!("Q11 maps")
        };
        assert_eq!(entity.id.as_str(), "domain:wikipedia.org");
        let s = entity.all_statements().next().unwrap();
        assert_eq!(
            s.mainsnak
                .data_value()
                .unwrap()
                .entity_id()
                .unwrap()
                .as_str(),
            "domain:example.org",
            "a value pointing at a mapped item becomes a domain value"
        );
        assert_eq!(s.mainsnak.datatype, Some(DataType::WikibaseDomain));
        let DataValue::EntityId(v) = s.mainsnak.data_value().unwrap() else {
            panic!()
        };
        assert_eq!(v.entity_type, "domain");

        let Outcome::State {
            entity,
            upstream_id,
            ..
        } = &out[2]
        else {
            panic!("Q20 is foreign")
        };
        assert_eq!(entity.id.as_str(), "XDQ20");
        assert!(upstream_id.is_none());

        let Outcome::Rejected { entity, reason, .. } = &out[3] else {
            panic!("Q21 is rejected")
        };
        assert_eq!(entity.id.as_str(), "XDQ21");
        assert!(reason.contains("not a valid domain key"), "{reason}");
        assert!(
            entity.statements.keys().any(|p| p.as_str() == "XDP1"),
            "the value stays as data on a rejected item"
        );

        let Outcome::Rejected { entity, reason, .. } = &out[4] else {
            panic!("Q30 is a conflict")
        };
        assert_eq!(entity.id.as_str(), "XDQ30");
        assert!(reason.contains("Q30, Q31"), "{reason}");
    }

    #[test]
    fn a_merge_into_a_mapped_item_is_an_alias_and_others_are_redirects() {
        let rules = xd_rules();
        let mut index = Index::default();
        for v in dump() {
            let _ = index.observe(&parsed(&v).entity, &rules);
        }
        index.observe_redirect("Q40", "Q9"); // into example.org
        index.observe_redirect("Q41", "Q20"); // into the registrar, which is not mapped
        index.observe_redirect("Q42", "Q30"); // into one side of a duplicate
        index.finish();
        assert_eq!(index.redirects(), 3);
        assert_eq!(index.aliases_of("Q9"), ["Q40".to_string()]);
        assert!(
            index.aliases_of("Q30").is_empty(),
            "a duplicate is not mapped"
        );

        let adapter = adapter(index);
        let q9 = dump().into_iter().next().unwrap();
        let Outcome::State {
            upstream_id,
            upstream_aliases,
            ..
        } = adapter.sync_item(parsed(&q9)).unwrap()
        else {
            panic!("Q9 maps")
        };
        assert_eq!(upstream_id.as_deref(), Some("Q9"));
        assert_eq!(upstream_aliases, ["Q40"]);

        let up = Upstream::revision(70, Some("2026-10-01T00:00:00Z"));
        assert_eq!(
            adapter.sync_redirect("Q40", "Q9", up.clone()).unwrap(),
            None
        );
        let (from, to, _) = adapter
            .sync_redirect("Q41", "Q20", up.clone())
            .unwrap()
            .unwrap();
        assert_eq!((from.as_str(), to.as_str()), ("XDQ41", "XDQ20"));
        let (from, to, _) = adapter.sync_redirect("Q42", "Q30", up).unwrap().unwrap();
        assert_eq!((from.as_str(), to.as_str()), ("XDQ42", "XDQ30"));
    }

    /// A provider whose key map has class rules, as the OpenStreetMap wiki's does, plus an
    /// unconditional domain rule.
    fn class_registry(extra: &str) -> &'static Registry {
        let text = format!(
            r#"version = 1
[[provider]]
code = "KM"
slug = "keymapped"
name = "Key-mapped test wiki"
number = 99
issuer = "keymapped"
revision_ids = true
adapter = "scatter-adapter-wikibase"
{extra}
  [[provider.type]]
  code = "Q"
  entity_type = "item"
  upstream_prefix = "Q"
  iri = "https://km.example/entity/{{upstream_id}}"
  namespace = "Item"

  [[provider.type]]
  code = "P"
  entity_type = "property"
  upstream_prefix = "P"
  iri = "https://km.example/entity/{{upstream_id}}"
  namespace = "Property"
"#
        );
        Box::leak(Box::new(Registry::parse(&text).unwrap()))
    }

    const CLASS_RULES: &str = r#"
  [[provider.adapter_config.key_map]]
  class = "Q7"
  identity_property = "P16"
  key_type = "notation"
  key_scheme = "osm"

  [[provider.adapter_config.key_map]]
  class = "Q2"
  identity_property = "P19"
  key_type = "notation"
  key_scheme = "osm"

  [provider.adapter_config]
  class_property = "P2"
"#;

    fn of_class(id: &str, class: &str, claims: &[(&str, serde_json::Value)]) -> serde_json::Value {
        let mut c = serde_json::Map::new();
        c.insert(
            "P2".into(),
            item_claim(
                "P2",
                class,
                &format!("{id}$00000000-0000-0000-0000-0000000000C0"),
            ),
        );
        for (p, v) in claims {
            c.insert((*p).into(), v.clone());
        }
        item(id, id, &c.into(), 1)
    }

    /// An OpenStreetMap key item and a tag item that links to it.
    fn osm_key_and_tag() -> (serde_json::Value, serde_json::Value) {
        let key = of_class(
            "Q100",
            "Q7",
            &[(
                "P16",
                string_claim(
                    "P16",
                    "amenity",
                    "Q100$00000000-0000-0000-0000-000000000001",
                ),
            )],
        );
        let tag = of_class(
            "Q101",
            "Q2",
            &[
                (
                    "P19",
                    string_claim(
                        "P19",
                        "amenity=cafe",
                        "Q101$00000000-0000-0000-0000-000000000001",
                    ),
                ),
                (
                    "P5",
                    item_claim("P5", "Q100", "Q101$00000000-0000-0000-0000-000000000002"),
                ),
            ],
        );
        (key, tag)
    }

    #[test]
    fn class_rules_map_only_their_class_and_report_what_they_cannot_map() {
        let registry = class_registry(CLASS_RULES);
        let rules = KeyRules::from_provider(registry.by_slug("keymapped").unwrap()).unwrap();
        assert_eq!(rules.rules().len(), 2);
        let (key, tag) = osm_key_and_tag();
        let keyless = of_class("Q102", "Q7", &[]);
        let other = of_class(
            "Q103",
            "Q50",
            &[(
                "P16",
                string_claim("P16", "x", "Q103$00000000-0000-0000-0000-000000000001"),
            )],
        );
        let both = of_class(
            "Q104",
            "Q7",
            &[
                (
                    "P16",
                    string_claim("P16", "k", "Q104$00000000-0000-0000-0000-000000000001"),
                ),
                (
                    "P2",
                    json!([
                    {"mainsnak": {"snaktype": "value", "property": "P2", "datatype": "wikibase-item",
                      "datavalue": {"value": {"entity-type": "item", "id": "Q7"}, "type": "wikibase-entityid"}},
                      "type": "statement", "rank": "normal", "id": "Q104$00000000-0000-0000-0000-0000000000C1"},
                    {"mainsnak": {"snaktype": "value", "property": "P2", "datatype": "wikibase-item",
                      "datavalue": {"value": {"entity-type": "item", "id": "Q2"}, "type": "wikibase-entityid"}},
                      "type": "statement", "rank": "normal", "id": "Q104$00000000-0000-0000-0000-0000000000C2"}]),
                ),
                (
                    "P19",
                    string_claim("P19", "k=v", "Q104$00000000-0000-0000-0000-000000000002"),
                ),
            ],
        );

        let mapped = |v: &serde_json::Value| rules.map(&parsed(v).entity);
        assert_eq!(
            mapped(&key)
                .unwrap()
                .map(|(_, id)| id.to_string())
                .as_deref(),
            Some("notation:osm:amenity")
        );
        assert_eq!(
            mapped(&tag)
                .unwrap()
                .map(|(_, id)| id.to_string())
                .as_deref(),
            Some("notation:osm:amenity=cafe")
        );
        assert!(
            matches!(mapped(&keyless), Err(MapError::NoIdentity { .. })),
            "an item of a mapped class without its identity value is reported"
        );
        assert_eq!(
            mapped(&other).unwrap(),
            None,
            "a rule with a class leaves other classes alone"
        );
        assert!(
            matches!(mapped(&both), Err(MapError::SeveralRules { count: 2, .. })),
            "two rules giving identity values is a reject"
        );
    }

    // 0091 §3: the keyed type supplies the entity type and the data type.
    #[test]
    fn a_mapped_item_takes_its_keyed_types_entity_type_and_data_type() {
        let registry = class_registry(CLASS_RULES);
        let rules = KeyRules::from_provider(registry.by_slug("keymapped").unwrap()).unwrap();
        let (key, tag) = osm_key_and_tag();
        let mut index = Index::default();
        for v in [&key, &tag] {
            index.observe(&parsed(v).entity, &rules).unwrap();
        }
        index.finish();
        let adapter = KeyMappedAdapter::new(
            WikibaseAdapter::with_registry("keymapped", registry).unwrap(),
            rules,
            index,
        );
        let Outcome::State { entity, .. } = adapter.sync_item(parsed(&tag)).unwrap() else {
            panic!("the tag maps")
        };
        assert_eq!(entity.entity_type, EntityType::Notation);
        assert!(
            !entity.statements.keys().any(|p| p.as_str() == "KMP19"),
            "the rule's own identity property is dropped"
        );
        let p5 = EntityId::parse("KMP5").unwrap();
        let snak = &entity.statements[&p5][0].mainsnak;
        assert_eq!(snak.datatype, Some(DataType::WikibaseNotation));
        assert_eq!(
            snak.data_value().unwrap().entity_id().unwrap().as_str(),
            "notation:osm:amenity"
        );
    }

    /// The OpenStreetMap wiki's registered key map: two class rules onto `osm` notations,
    /// matched through its "instance of", `P2` (0036 §5; 0091 §2).
    #[test]
    fn the_osm_wiki_key_map_is_two_class_rules() {
        let provider = Registry::default_registry().by_slug("osmwiki").unwrap();
        let rules = KeyRules::from_provider(provider).unwrap();
        let summary: Vec<(String, String, &str)> = rules
            .rules()
            .iter()
            .map(|r| {
                (
                    r.class().unwrap().to_string(),
                    r.identity_property().to_string(),
                    r.keyed_type(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Q7".into(), "P16".into(), "notation"),
                ("Q2".into(), "P19".into(), "notation")
            ]
        );
        let key = of_class(
            "Q500",
            "Q7",
            &[(
                "P16",
                string_claim(
                    "P16",
                    "highway",
                    "Q500$00000000-0000-0000-0000-000000000001",
                ),
            )],
        );
        let (_, id) = rules.map(&parsed(&key).entity).unwrap().unwrap();
        assert_eq!(id.as_str(), "notation:osm:highway");
    }

    fn property(id: &str, label: &str, datatype: &str) -> serde_json::Value {
        json!({"type": "property", "id": id, "datatype": datatype,
            "labels": {"en": {"language": "en", "value": label}},
            "claims": {}, "lastrevid": 1, "modified": "2026-10-01T00:00:00Z"})
    }

    fn state(outcome: Outcome) -> Entity {
        match outcome {
            Outcome::State { entity, .. } => *entity,
            other @ Outcome::Rejected { .. } => panic!("not a state: {other:?}"),
        }
    }

    // 0091 §2 (A3, A4): the OSM wiki's vocabulary settings. A property whose English label
    // opens with "(DEPRECATED)" is written as an instance of obsolete predicate
    // (`WDP31` → `WDQ142241643`, 0114 §2) and its uses are kept; a concept link's string
    // `Q787417` becomes an item value naming `WDQ787417`, and its property a property of
    // items.
    #[test]
    fn the_osm_wiki_marks_deprecated_properties_and_links_concepts_to_wikidata() {
        let provider = Registry::default_registry().by_slug("osmwiki").unwrap();
        let rules = KeyRules::from_provider(provider).unwrap();
        let (mark_property, mark_class) = rules.deprecated_mark().unwrap();
        assert_eq!(
            (mark_property.as_str(), mark_class.as_str()),
            ("WDP31", "WDQ142241643")
        );
        let image = json!([{"mainsnak": {"snaktype": "value", "property": "P28", "datatype": "string",
                "datavalue": {"value": "Highway.png", "type": "string"}},
            "qualifiers": {"P4": [{"snaktype": "value", "property": "P4", "datatype": "string",
                "datavalue": {"value": "Old.png", "type": "string"}}]},
            "references": [
                {"snaks": {"P4": [{"snaktype": "value", "property": "P4", "datatype": "string",
                    "datavalue": {"value": "Old.png", "type": "string"}}]}},
                {"snaks": {"P31": [{"snaktype": "value", "property": "P31", "datatype": "string",
                    "datavalue": {"value": "Key:highway", "type": "string"}}]}}],
            "type": "statement", "rank": "normal", "id": "Q500$00000000-0000-0000-0000-000000000003"}]);
        let entities = [
            property("P4", "(DEPRECATED) image", "string"),
            property("P28", "image", "string"),
            property("P12", "Wikidata concept", "external-id"),
            of_class(
                "Q500",
                "Q7",
                &[
                    (
                        "P16",
                        string_claim(
                            "P16",
                            "highway",
                            "Q500$00000000-0000-0000-0000-000000000001",
                        ),
                    ),
                    (
                        "P4",
                        string_claim("P4", "Old.png", "Q500$00000000-0000-0000-0000-000000000002"),
                    ),
                    ("P28", image),
                    (
                        "P12",
                        string_claim(
                            "P12",
                            "Q787417",
                            "Q500$00000000-0000-0000-0000-000000000004",
                        ),
                    ),
                ],
            ),
        ];
        let mut index = Index::default();
        for e in &entities {
            let _ = index.observe(&parsed(e).entity, &rules);
        }
        index.finish();
        let adapter = KeyMappedAdapter::new(WikibaseAdapter::new("osmwiki").unwrap(), rules, index);
        let out: Vec<Entity> = entities
            .iter()
            .map(|e| state(adapter.sync_item(parsed(e)).unwrap()))
            .collect();

        // The deprecated property is mirrored, marked as an instance of obsolete predicate,
        // once, with a stable ID.
        let p = |id: &str| EntityId::parse(id).unwrap();
        assert_eq!(out[0].id.as_str(), "OWP4");
        let mark = &out[0].statements[&p("WDP31")];
        assert_eq!(mark.len(), 1);
        assert!(
            matches!(mark[0].mainsnak.data_value(), Some(DataValue::EntityId(v)) if v.id.as_str() == "WDQ142241643"),
            "{mark:?}"
        );
        assert!(mark[0].id.as_ref().unwrap().as_str().starts_with("OWP4$"));
        let again = state(adapter.sync_item(parsed(&entities[0])).unwrap());
        assert_eq!(again.statements[&p("WDP31")], *mark);
        // A property not so marked gets nothing.
        assert!(!out[1].statements.contains_key(&p("WDP31")));
        // The concept-link property is one of items now.
        assert_eq!(out[2].datatype, Some(DataType::WikibaseItem));
        let key = &out[3];
        assert_eq!(key.id.as_str(), "notation:osm:highway");
        // The deprecated property's uses are kept: statement, qualifier and reference.
        assert_eq!(key.statements[&p("OWP4")].len(), 1);
        let image = &key.statements[&p("OWP28")][0];
        assert!(image.qualifiers.contains_key(&p("OWP4")));
        assert_eq!(image.references.len(), 2);
        assert!(image.references[0].snaks.contains_key(&p("OWP4")));
        let concept = &key.statements[&p("OWP12")][0].mainsnak;
        assert_eq!(concept.datatype, Some(DataType::WikibaseItem));
        assert!(
            matches!(concept.data_value(), Some(DataValue::EntityId(v)) if v.id.as_str() == "WDQ787417" && v.entity_type == "item"),
            "{concept:?}"
        );
    }

    // 0091 §2: a concept-link entry that is not a property is a configuration error.
    #[test]
    fn a_concept_link_must_name_a_property() {
        let err = KeyRules::from_provider(
            class_registry("  [provider.adapter_config]\n  concept_link_properties = [\"Q7\"]\n")
                .by_slug("keymapped")
                .unwrap(),
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::NotAnId {
                    field: "concept_link_properties",
                    ..
                }
            ),
            "{err:?}"
        );
    }

    // 0091 §3: a type or scheme the build does not know is a configuration error.
    #[test]
    fn a_bad_key_map_is_a_configuration_error() {
        let err = |extra: &str| {
            KeyRules::from_provider(class_registry(extra).by_slug("keymapped").unwrap())
                .unwrap_err()
        };
        assert!(matches!(
            err(
                "  [provider.adapter_config]\n  identity_property = \"P1\"\n  key_type = \"domain\"\n"
            ),
            ConfigError::FlatForm(_)
        ));
        assert!(matches!(
            err(
                "  [[provider.adapter_config.key_map]]\n  identity_property = \"P1\"\n  key_type = \"isbn\"\n"
            ),
            ConfigError::UnknownKeyedType { .. }
        ));
        assert!(matches!(
            err(
                "  [[provider.adapter_config.key_map]]\n  identity_property = \"P1\"\n  key_type = \"notation\"\n"
            ),
            ConfigError::UnknownScheme { .. }
        ));
        assert!(matches!(
            err(
                "  [[provider.adapter_config.key_map]]\n  class = \"Q7\"\n  identity_property = \"P1\"\n  key_type = \"domain\"\n"
            ),
            ConfigError::NoClassProperty(_)
        ));
        assert!(matches!(
            err("  [[provider.adapter_config.key_map]]\n  key_type = \"domain\"\n"),
            ConfigError::Missing {
                field: "identity_property",
                ..
            }
        ));
    }

    #[test]
    fn a_provider_without_a_key_map_only_rewrites() {
        let rules =
            KeyRules::from_provider(Registry::default_registry().by_slug("librarybase").unwrap())
                .unwrap();
        assert!(rules.is_empty());
        let adapter = KeyMappedAdapter::new(
            WikibaseAdapter::new("librarybase").unwrap(),
            rules,
            Index::default(),
        );
        let Outcome::State {
            entity,
            upstream_id,
            ..
        } = adapter.sync_item(parsed(&dump()[0])).unwrap()
        else {
            panic!("a state")
        };
        assert_eq!(entity.id.as_str(), "LBQ9");
        assert!(upstream_id.is_none());
        assert!(entity.statements.keys().any(|p| p.as_str() == "LBP1"));
    }
}
