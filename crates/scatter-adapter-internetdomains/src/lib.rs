//! The key-mapped adapter for internetdomains.wiki (0009 §9; `providers.toml`, `XD`).
//!
//! An upstream item with exactly one valid value for the identity property (`P1`, a
//! domain name) is written under its Domain ID, `domain:{key}`; its upstream item ID
//! becomes an alias through the key map (`upstream_id` on the `put`). Values that point
//! at a mapped item become `wikibase-domain` values with the same key. Items without an
//! identity value stay ordinary foreign items, `XDQ{n}`; an invalid identity value or more
//! than one is a reject, and the item stays foreign with the value kept as data. Two items
//! carrying the same key are a conflict: neither is mapped until upstream resolves it.
//!
//! Mapping needs the whole dump in view before any item is rewritten, because a value
//! pointing at `Q9` can only become `domain:example.org` once `Q9` is known to map there.
//! So the adapter works in two passes: [`Index::observe`] over every entity, then
//! [`InternetDomainsAdapter::sync_item`] over every entity again.

use std::collections::{BTreeMap, BTreeSet};

use scatter_normalize::domain;
use scatter_providers::{Provider, Registry};
use scatter_wikibase_changeset::{Adapter, AdapterError, KeyMap, Upstream, WikibaseAdapter};
use scatter_wikibase_model::entity::{Entity, EntityType, ParsedEntity};
use scatter_wikibase_model::id::{EntityId, IdForm};
use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::{DataType, DataValue};

/// The provider slug.
pub const SLUG: &str = "internetdomains";
/// The keyed type mapped onto.
pub const KEYED_TYPE: &str = "domain";

/// Why the adapter could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The provider is not registered.
    #[error("provider `{0}` is not registered")]
    UnknownProvider(String),
    /// The registry entry names no identity property.
    #[error("provider `{0}` has no `identity_property` in its adapter configuration")]
    NoIdentityProperty(String),
    /// The identity property is not an entity ID.
    #[error("identity property `{0}` is not an entity ID")]
    BadIdentityProperty(String),
}

/// The Domain key map: the identity property's string values, normalized as domain names.
#[derive(Debug, Clone)]
pub struct DomainKeyMap {
    /// The identity property, in the provider's own form (`P1`).
    property: EntityId,
}

impl DomainKeyMap {
    /// From the provider's adapter configuration.
    pub fn from_provider(provider: &Provider) -> Result<Self, ConfigError> {
        let cfg = provider
            .adapter_config
            .as_ref()
            .and_then(|c| c.get("identity_property"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConfigError::NoIdentityProperty(provider.slug.clone()))?;
        Ok(Self {
            property: EntityId::parse(cfg)
                .map_err(|_| ConfigError::BadIdentityProperty(cfg.to_string()))?,
        })
    }
}

impl KeyMap for DomainKeyMap {
    fn keyed_type(&self) -> &str {
        KEYED_TYPE
    }

    fn identity_property(&self) -> &EntityId {
        &self.property
    }

    fn key_of(&self, value: &str) -> Option<String> {
        domain::normalize(value).ok()
    }
}

/// The first pass: which upstream items map to which keys, and which keys are claimed
/// by more than one item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    by_upstream: BTreeMap<String, String>,
    by_key: BTreeMap<String, Vec<String>>,
    conflicts: BTreeSet<String>,
    finished: bool,
}

impl Index {
    /// Observes an upstream entity as the dump wrote it (IDs in the provider's own form).
    /// Returns the key it maps to, if any; a reject is returned as the error, and the
    /// item stays foreign.
    pub fn observe(
        &mut self,
        entity: &Entity,
        key_map: &dyn KeyMap,
    ) -> Result<Option<String>, AdapterError> {
        let Some(id) = key_map.map(entity)? else {
            return Ok(None);
        };
        let Some((_, key)) = id.keyed_parts() else {
            return Ok(None);
        };
        let key = key.to_string();
        self.by_upstream
            .insert(entity.id.as_str().to_string(), key.clone());
        self.by_key
            .entry(key.clone())
            .or_default()
            .push(entity.id.as_str().to_string());
        Ok(Some(key))
    }

    /// Settles the index: a key claimed by two items maps neither (0009 §9).
    pub fn finish(&mut self) {
        for (key, items) in &self.by_key {
            if items.len() > 1 {
                self.conflicts.insert(key.clone());
                for item in items {
                    self.by_upstream.remove(item);
                }
            }
        }
        self.finished = true;
    }

    /// The key an upstream item ID (`Q9`) maps to.
    #[must_use]
    pub fn key_of(&self, upstream_id: &str) -> Option<&str> {
        self.by_upstream.get(upstream_id).map(String::as_str)
    }

    /// The keys more than one item claims, with the claimants.
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
        /// The entity: a Domain or an `XD` item/property.
        entity: Box<Entity>,
        /// Its upstream version.
        upstream: Upstream,
        /// The upstream item ID, for a mapped Domain.
        upstream_id: Option<String>,
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

/// The adapter.
#[derive(Debug, Clone)]
pub struct InternetDomainsAdapter {
    inner: WikibaseAdapter,
    key_map: DomainKeyMap,
    index: Index,
    drop_identity: bool,
}

impl InternetDomainsAdapter {
    /// Over the default registry's `internetdomains` entry and a settled index.
    pub fn new(index: Index) -> Result<Self, ConfigError> {
        Self::with_registry(Registry::default_registry(), index)
    }

    /// Over a registry.
    pub fn with_registry(registry: &'static Registry, index: Index) -> Result<Self, ConfigError> {
        let inner = WikibaseAdapter::with_registry(SLUG, registry)
            .ok_or_else(|| ConfigError::UnknownProvider(SLUG.to_string()))?;
        let key_map = DomainKeyMap::from_provider(inner.provider())?;
        let drop_identity = inner
            .provider()
            .adapter_config
            .as_ref()
            .and_then(|c| c.get("identity_property_on_mapped"))
            .and_then(|v| v.as_str())
            .is_none_or(|v| v == "drop");
        Ok(Self {
            inner,
            key_map,
            index,
            drop_identity,
        })
    }

    /// The key map.
    #[must_use]
    pub fn domain_key_map(&self) -> &DomainKeyMap {
        &self.key_map
    }

    /// The settled index.
    #[must_use]
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// The identity property in stored form (`XDP1`).
    fn stored_identity_property(&self) -> Result<EntityId, AdapterError> {
        self.inner.rewrite_id(self.key_map.property.as_str())
    }

    /// A dump entity as the mirror stores it: a Domain with its upstream ID, a foreign
    /// item, or a reported reject written as a foreign item.
    pub fn sync_item(&self, parsed: ParsedEntity) -> Result<Outcome, AdapterError> {
        let upstream = match parsed.page.as_ref().and_then(|p| p.lastrevid) {
            Some(revid) => Upstream::revision(
                revid,
                parsed.page.as_ref().and_then(|p| p.modified.as_deref()),
            ),
            None => Upstream::default(),
        };
        let mut entity = parsed.entity;
        let upstream_id = entity.id.as_str().to_string();
        // Mapped?
        let mapped = self.index.key_of(&upstream_id).is_some();
        let reject = if mapped {
            None
        } else {
            match self.key_map.map(&entity) {
                Ok(None) => None,
                Ok(Some(id)) => {
                    // Mapped by its own value, but the index did not keep it: a conflict.
                    let key = id.keyed_parts().map_or("", |(_, k)| k);
                    Some(format!(
                        "key `{key}` is claimed by {}; neither item is mapped until upstream resolves the duplicate",
                        self.index
                            .by_key
                            .get(key)
                            .map_or_else(|| "another item".to_string(), |items| items.join(", "))
                    ))
                }
                Err(e) => Some(e.to_string()),
            }
        };
        self.rewrite_entity(&mut entity)?;
        if mapped {
            // A Domain is a Domain (0009 §3), whatever type the item had upstream.
            entity.entity_type = EntityType::Domain;
            entity.sitelinks.clear();
            if self.drop_identity {
                let p = self.stored_identity_property()?;
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
                upstream_id: mapped.then_some(upstream_id),
            },
        })
    }
}

impl Adapter for InternetDomainsAdapter {
    fn provider(&self) -> &Provider {
        self.inner.provider()
    }

    fn rewrite_id(&self, upstream: &str) -> Result<EntityId, AdapterError> {
        if let Some(key) = self.index.key_of(upstream) {
            return Ok(EntityId::parse(&format!("{KEYED_TYPE}:{key}"))?);
        }
        self.inner.rewrite_id(upstream)
    }

    fn canonical_iri(&self, id: &EntityId) -> Option<String> {
        self.inner.canonical_iri(id)
    }

    fn key_map(&self) -> Option<&dyn KeyMap> {
        Some(&self.key_map)
    }

    /// As the default, then a value that became a Domain is typed `wikibase-domain`.
    fn rewrite_snak(&self, snak: &mut Snak) -> Result<(), AdapterError> {
        snak.property = self.rewrite_id(snak.property.as_str())?;
        if let SnakKind::Value(DataValue::EntityId(v)) = &mut snak.kind {
            v.id = self.rewrite_id(v.id.as_str())?;
            if v.id.form() == IdForm::Keyed
                && v.id.keyed_parts().is_some_and(|(t, _)| t == KEYED_TYPE)
            {
                v.entity_type = KEYED_TYPE.to_string();
                if snak.datatype == Some(DataType::WikibaseItem) {
                    snak.datatype = Some(DataType::WikibaseDomain);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn maps_rewrites_and_reports() {
        let provider = Registry::default_registry().by_slug(SLUG).unwrap();
        let key_map = DomainKeyMap::from_provider(provider).unwrap();
        let mut index = Index::default();
        let mut rejects = 0;
        for v in dump() {
            if index.observe(&parsed(&v).entity, &key_map).is_err() {
                rejects += 1;
            }
        }
        assert_eq!(rejects, 1, "Q21's value is not a domain");
        index.finish();
        assert_eq!(index.mapped(), 2);
        assert_eq!(index.key_of("Q9"), Some("example.org"));
        assert_eq!(index.key_of("Q30"), None, "a duplicate key maps neither");
        assert_eq!(index.conflicts().count(), 1);

        let adapter = InternetDomainsAdapter::new(index).unwrap();
        let out: Vec<Outcome> = dump()
            .into_iter()
            .map(|v| adapter.sync_item(parsed(&v)).unwrap())
            .collect();

        let Outcome::State {
            entity,
            upstream,
            upstream_id,
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
}
