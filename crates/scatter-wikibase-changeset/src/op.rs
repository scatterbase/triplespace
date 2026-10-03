//! The operations (0002 §8.2 as amended; payloads.md §3): one per record in a `local`,
//! `mirror/{provider}` or `pages` partition. Serialized, an operation is the content
//! part of a `scatter:v0/changeset` record; on the wire it is one NDJSON line after the
//! job line.
//!
//! Two shapes exist only on the wire and never as records: `create-or-add`, which the
//! server resolves into a `create` or an `add` carrying `match` and `via`, and a `create`
//! with a temporary `ref` and no `id`, which the server mints an ID for. `retain` with
//! `ids` fans out to one record per entity. [`Operation::is_record_shape`] says whether a
//! value may be appended as it stands.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use scatter_wikibase_model::entity::{Entity, EntityType};
use scatter_wikibase_model::id::{EntityId, StatementId, Subject};
use scatter_wikibase_model::statement::{Rank, Reference, SnakGroups, Statement};
use serde::{Deserialize, Serialize};

use crate::changes::Changes;
use crate::rank::ProviderOrder;

/// The upstream version a mirror operation refers to (payloads.md §3.1): `revid` and
/// `time` for a provider with revisions, `version` for one without.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    /// The upstream revision ID; also header field 7's `n`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revid: Option<u64>,
    /// The upstream timestamp, as the provider's own string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// The provider's own version string, for a provider without revisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Upstream {
    /// A revision.
    #[must_use]
    pub fn revision(revid: u64, time: Option<&str>) -> Self {
        Self {
            revid: Some(revid),
            time: time.map(str::to_owned),
            version: None,
        }
    }

    /// A version string.
    #[must_use]
    pub fn version(v: &str) -> Self {
        Self {
            revid: None,
            time: None,
            version: Some(v.to_string()),
        }
    }

    /// Whether anything names the version.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.revid.is_none() && self.version.is_none()
    }
}

/// How a `create-or-add` was resolved (0002 A19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Via {
    /// The record came from a `create-or-add`.
    CreateOrAdd,
}

/// What a `create-or-add` resolved to ([`Operation::resolve_create_or_add`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The match key found this entity.
    Matched(EntityId),
    /// Nothing matched; this ID was minted.
    Minted(EntityId),
}

/// A match key (0002 §8.5): an identifier property and its value, or a foreign entity ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MatchKey {
    /// `{"P356": "10.1234/x"}`: one property and one value.
    Identifier(BTreeMap<EntityId, String>),
    /// A foreign entity ID.
    Entity(EntityId),
}

/// A retention policy (0002 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Retention {
    /// Gone when upstream deletes it.
    Cascade,
    /// Kept while anything local refers to it.
    Orphan,
    /// Kept.
    Retain,
}

/// A sitelink as `add` carries it: `{site: {title, badges}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SitelinkPatch {
    /// The page title.
    pub title: String,
    /// Badge item IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<EntityId>,
}

/// A link between entities (0004 §9), as `same-as`, `different-from` and
/// `equivalent-property` carry it, and as `remove` repeats it in `link`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Link {
    /// Two items of different namespaces are the same thing.
    SameAs {
        /// Both members.
        ids: Vec<EntityId>,
    },
    /// Two IDs may never share a cluster.
    DifferentFrom {
        /// Both members.
        ids: Vec<EntityId>,
    },
    /// Two properties of different namespaces are equivalent.
    EquivalentProperty {
        /// Both members.
        ids: Vec<EntityId>,
    },
}

impl Link {
    /// The members.
    #[must_use]
    pub fn ids(&self) -> &[EntityId] {
        match self {
            Self::SameAs { ids }
            | Self::DifferentFrom { ids }
            | Self::EquivalentProperty { ids } => ids,
        }
    }

    /// The operation name.
    #[must_use]
    pub fn op(&self) -> &'static str {
        match self {
            Self::SameAs { .. } => "same-as",
            Self::DifferentFrom { .. } => "different-from",
            Self::EquivalentProperty { .. } => "equivalent-property",
        }
    }
}

/// An operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Operation {
    /// Replaces an entity's mirrored state (mirror).
    Put {
        /// The entity, in provider form.
        id: EntityId,
        /// The state, in the storage form.
        entity: Entity,
        /// The upstream version.
        upstream: Upstream,
        /// The version this replaces (0012 §2.2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prev_upstream: Option<Upstream>,
        /// When the instance first mirrored the entity, µs since the epoch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_seen: Option<u64>,
        /// The size of the new state (0012 §2.4).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
        /// The size of the previous state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prev_size: Option<u64>,
        /// A summary of what changed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        changes: Option<Changes>,
        /// The statement-level difference, under `sync_deltas: full`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delta: Option<serde_json::Value>,
    },
    /// Records an upstream deletion (mirror).
    Tombstone {
        /// The entity.
        id: EntityId,
        /// The upstream version that deleted it.
        upstream: Upstream,
    },
    /// Records a merge: upstream's in a mirror, a local one in `local`.
    Redirect {
        /// The retired ID, the record's key.
        from: EntityId,
        /// The target.
        to: EntityId,
        /// The upstream version, in a mirror.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upstream: Option<Upstream>,
    },
    /// Creates a local entity under a freshly minted ID (local).
    Create {
        /// The minted ID; absent on the wire, where `ref` stands in.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// A temporary handle, `$w1`; wire only.
        #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
        temporary_ref: Option<String>,
        /// The state.
        entity: Entity,
        /// The match key, when resolved from a `create-or-add`.
        #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
        match_key: Option<MatchKey>,
        /// Set when resolved from a `create-or-add`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<Via>,
    },
    /// Creates, or adds to the entity the match key finds (wire only; 0002 §8.5).
    CreateOrAdd {
        /// A temporary handle, `$w1`.
        #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
        temporary_ref: Option<String>,
        /// The match key.
        #[serde(rename = "match")]
        match_key: MatchKey,
        /// The state.
        entity: Entity,
        /// Replace the matched entity's state rather than merging; needs a base revision.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        overwrite: bool,
    },
    /// Writes an entity's whole state as its first local record under the source's ID
    /// (0035 §3).
    Adopt {
        /// The ID, in local form.
        id: EntityId,
        /// The state.
        entity: Entity,
        /// The source's revision ID of this state.
        source_revid: u64,
        /// The source's timestamp, as its own string.
        source_time: String,
        /// The page ID of the entity's page on the source.
        source_pageid: u64,
    },
    /// Merges terms, statements, sitelinks, references or qualifiers onto a subject.
    Add {
        /// The entity, when the subject is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// The page ID, when the subject is a page (0038 §1).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<u64>,
        /// Labels by language.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        labels: BTreeMap<String, String>,
        /// Descriptions by language.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        descriptions: BTreeMap<String, String>,
        /// Aliases by language.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        aliases: BTreeMap<String, Vec<String>>,
        /// Statements by property.
        #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
        claims: IndexMap<EntityId, Vec<Statement>>,
        /// Sitelinks by site.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        sitelinks: BTreeMap<String, SitelinkPatch>,
        /// References onto existing statements, by GUID.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        references: BTreeMap<StatementId, Vec<Reference>>,
        /// Qualifiers onto existing statements, by GUID.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        qualifiers: BTreeMap<StatementId, SnakGroups>,
        /// The whole state, with `overwrite`, from a `create-or-add`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<Entity>,
        /// The match key, when resolved from a `create-or-add`.
        #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
        match_key: Option<MatchKey>,
        /// Set when resolved from a `create-or-add`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<Via>,
        /// The state replaces the entity's (needs a base revision).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        overwrite: bool,
    },
    /// Retracts local assertions.
    Remove {
        /// The entity, when the subject is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// The page ID, when the subject is a page.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<u64>,
        /// Statements by GUID.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        statements: Vec<StatementId>,
        /// Labels by language.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        labels: Vec<String>,
        /// Descriptions by language.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        descriptions: Vec<String>,
        /// Aliases by language and value.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        aliases: BTreeMap<String, Vec<String>>,
        /// Sitelinks by site.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sitelinks: Vec<String>,
        /// References by GUID and reference hash.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        references: BTreeMap<StatementId, Vec<String>>,
        /// Qualifiers by GUID and snak hash.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        qualifiers: BTreeMap<StatementId, Vec<String>>,
        /// A link to retract, repeated.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<Link>,
    },
    /// Overrides the rank of a statement from another graph, or suppresses it.
    Override {
        /// The statement's entity, the record's key; derivable from `statement`, so
        /// optional on the wire ([`Operation::complete`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// The statement.
        statement: StatementId,
        /// The rank to show it with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rank: Option<Rank>,
        /// Hide it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        suppress: Option<bool>,
    },
    /// Sets the retention policy of an entity; on the wire, of several.
    Retain {
        /// The entity, in a record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// The entities, on the wire.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ids: Vec<EntityId>,
        /// The policy.
        policy: Retention,
    },
    /// Mints a local entity and makes the foreign ID an alias of it (0002 §6).
    Convert {
        /// The foreign ID, the record's key.
        id: EntityId,
        /// The minted local ID.
        local: EntityId,
    },
    /// Links two items (0004 §9).
    SameAs {
        /// The highest-ranked member, the record's key; derivable from `ids` under the
        /// provider order, so optional on the wire ([`Operation::complete`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// Both members.
        ids: Vec<EntityId>,
    },
    /// Blocks two IDs from ever sharing a cluster.
    DifferentFrom {
        /// The highest-ranked member, the record's key; derivable from `ids` under the
        /// provider order, so optional on the wire ([`Operation::complete`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// Both members.
        ids: Vec<EntityId>,
    },
    /// Links two properties.
    EquivalentProperty {
        /// The highest-ranked member, the record's key; derivable from `ids` under the
        /// provider order, so optional on the wire ([`Operation::complete`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntityId>,
        /// Both members.
        ids: Vec<EntityId>,
    },
}

impl Operation {
    /// The `op` name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Put { .. } => "put",
            Self::Tombstone { .. } => "tombstone",
            Self::Redirect { .. } => "redirect",
            Self::Create { .. } => "create",
            Self::CreateOrAdd { .. } => "create-or-add",
            Self::Adopt { .. } => "adopt",
            Self::Add { .. } => "add",
            Self::Remove { .. } => "remove",
            Self::Override { .. } => "override",
            Self::Retain { .. } => "retain",
            Self::Convert { .. } => "convert",
            Self::SameAs { .. } => "same-as",
            Self::DifferentFrom { .. } => "different-from",
            Self::EquivalentProperty { .. } => "equivalent-property",
        }
    }

    /// The record's header key (payloads.md §3): the subject, as text. `None` for a
    /// shape that is not yet a record.
    #[must_use]
    pub fn key(&self) -> Option<String> {
        match self {
            Self::Put { id, .. }
            | Self::Tombstone { id, .. }
            | Self::Adopt { id, .. }
            | Self::Convert { id, .. } => Some(id.as_str().to_string()),
            Self::Redirect { from, .. } => Some(from.as_str().to_string()),
            Self::Create { id, .. }
            | Self::Retain { id, .. }
            | Self::Override { id, .. }
            | Self::SameAs { id, .. }
            | Self::DifferentFrom { id, .. }
            | Self::EquivalentProperty { id, .. } => id.as_ref().map(|i| i.as_str().to_string()),
            Self::Add { id, page, .. } | Self::Remove { id, page, .. } => match (id, page) {
                (Some(i), None) => Some(i.as_str().to_string()),
                (None, Some(p)) => Some(p.to_string()),
                _ => None,
            },
            Self::CreateOrAdd { .. } => None,
        }
    }

    /// The subject of an `add` or `remove`, when exactly one is named.
    #[must_use]
    pub fn subject(&self) -> Option<Subject> {
        match self {
            Self::Add { id, page, .. } | Self::Remove { id, page, .. } => match (id, page) {
                (Some(i), None) => Some(Subject::Entity(i.clone())),
                (None, Some(p)) => Some(Subject::Page(*p)),
                _ => None,
            },
            _ => Subject::parse(&self.key()?).ok(),
        }
    }

    /// Whether this may be appended as a record as it stands: not a `create-or-add`,
    /// not a `create` with a `ref`, not a `retain` with `ids`, and with a key (an
    /// `override` or a link without `id` is completed first, [`Operation::complete`]).
    #[must_use]
    pub fn is_record_shape(&self) -> bool {
        match self {
            Self::CreateOrAdd { .. } => false,
            Self::Create {
                id, temporary_ref, ..
            } => id.is_some() && temporary_ref.is_none(),
            Self::Retain { id, ids, .. } => id.is_some() && ids.is_empty(),
            other => other.key().is_some(),
        }
    }

    /// Whether this is a mirror operation, written to `mirror/{provider}`.
    #[must_use]
    pub fn is_mirror(&self) -> bool {
        matches!(self, Self::Put { .. } | Self::Tombstone { .. })
            || matches!(
                self,
                Self::Redirect {
                    upstream: Some(_),
                    ..
                }
            )
    }

    /// The entity state an operation carries whole, if any.
    #[must_use]
    pub fn entity(&self) -> Option<&Entity> {
        match self {
            Self::Put { entity, .. }
            | Self::Create { entity, .. }
            | Self::CreateOrAdd { entity, .. }
            | Self::Adopt { entity, .. } => Some(entity),
            Self::Add { entity, .. } => entity.as_ref(),
            _ => None,
        }
    }

    /// Every statement the operation carries, whole states included.
    pub fn statements(&self) -> Box<dyn Iterator<Item = &Statement> + '_> {
        match self {
            Self::Add { claims, entity, .. } => Box::new(
                claims
                    .values()
                    .flatten()
                    .chain(entity.iter().flat_map(Entity::all_statements)),
            ),
            other => match other.entity() {
                Some(e) => Box::new(e.all_statements()),
                None => Box::new(std::iter::empty()),
            },
        }
    }

    /// The `retain` fan-out: one record-shaped operation per entity (payloads.md §3.2).
    #[must_use]
    pub fn fan_out(self) -> Vec<Self> {
        match self {
            Self::Retain { id, ids, policy } => id
                .into_iter()
                .chain(ids)
                .map(|id| Self::Retain {
                    id: Some(id),
                    ids: Vec::new(),
                    policy,
                })
                .collect(),
            other => vec![other],
        }
    }

    /// The members of a link operation, or of the `link` a `remove` retracts.
    #[must_use]
    pub fn link_ids(&self) -> Option<&[EntityId]> {
        match self {
            Self::SameAs { ids, .. }
            | Self::DifferentFrom { ids, .. }
            | Self::EquivalentProperty { ids, .. } => Some(ids),
            Self::Remove { link: Some(l), .. } => Some(l.ids()),
            _ => None,
        }
    }

    /// Fills in a key the wire may leave out: an `override`'s `id` from its statement's
    /// subject, a link's `id` from its highest-ranked member under `order`. Anything
    /// already set is kept, for the validator to check.
    pub fn complete(&mut self, order: &ProviderOrder) {
        match self {
            Self::Override { id, statement, .. } if id.is_none() => {
                *id = statement.entity_id();
            }
            Self::SameAs { id, ids }
            | Self::DifferentFrom { id, ids }
            | Self::EquivalentProperty { id, ids }
                if id.is_none() =>
            {
                *id = order.highest(ids.iter()).cloned();
            }
            _ => {}
        }
    }

    /// Resolves a `create-or-add` into what happened (0002 §8.5, A19): an `add` keyed by
    /// the matched entity when the match key found one, else a `create` under the minted
    /// ID. Both carry `match` and `via`. With `overwrite`, the `add` carries the whole
    /// `entity` and `overwrite: true`; without it, the entity's parts are merged. Any
    /// other operation is returned as it is.
    #[must_use]
    pub fn resolve_create_or_add(self, resolution: Resolution) -> Self {
        let Self::CreateOrAdd {
            match_key,
            entity,
            overwrite,
            ..
        } = self
        else {
            return self;
        };
        let (matched, minted) = match resolution {
            Resolution::Matched(id) => (Some(id), None),
            Resolution::Minted(id) => (None, Some(id)),
        };
        if let Some(id) = matched {
            if overwrite {
                let mut entity = entity;
                entity.id = id.clone();
                return Self::Add {
                    id: Some(id),
                    page: None,
                    labels: BTreeMap::new(),
                    descriptions: BTreeMap::new(),
                    aliases: BTreeMap::new(),
                    claims: IndexMap::new(),
                    sitelinks: BTreeMap::new(),
                    references: BTreeMap::new(),
                    qualifiers: BTreeMap::new(),
                    entity: Some(entity),
                    match_key: Some(match_key),
                    via: Some(Via::CreateOrAdd),
                    overwrite: true,
                };
            }
            return Self::Add {
                id: Some(id),
                page: None,
                labels: entity.labels,
                descriptions: entity.descriptions,
                aliases: entity.aliases,
                claims: entity.statements,
                sitelinks: entity
                    .sitelinks
                    .into_iter()
                    .map(|(site, l)| {
                        (
                            site,
                            SitelinkPatch {
                                title: l.title,
                                badges: l.badges,
                            },
                        )
                    })
                    .collect(),
                references: BTreeMap::new(),
                qualifiers: BTreeMap::new(),
                entity: None,
                match_key: Some(match_key),
                via: Some(Via::CreateOrAdd),
                overwrite: false,
            };
        }
        let id = minted.expect("one of the two");
        let mut entity = entity;
        entity.id = id.clone();
        Self::Create {
            id: Some(id),
            temporary_ref: None,
            entity,
            match_key: Some(match_key),
            via: Some(Via::CreateOrAdd),
        }
    }

    /// The entity type a `create` or `adopt` writes.
    #[must_use]
    pub fn created_type(&self) -> Option<&EntityType> {
        match self {
            Self::Create { entity, .. }
            | Self::CreateOrAdd { entity, .. }
            | Self::Adopt { entity, .. } => Some(&entity.entity_type),
            _ => None,
        }
    }
}
