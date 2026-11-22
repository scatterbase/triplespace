//! Badges (0114; 06 §6): marks drawn beside an entity, a sitelink or a statement, each of
//! which is an item. The registry, `docs/registry/badges.toml`, says what each badge marks,
//! where it comes from and how it is drawn; the item gives it its name. A tenant adds its
//! own badges in the `badges.local` setting, read by [`BadgeRegistry::with_local`].
//!
//! A `stored` badge is one a sitelink's `badges` may name, as Wikibase's `badgeItems` are;
//! `class` and `rank` badges are computed when an entity is read and never stored.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use scatter_wikibase_model::id::{EntityId, IdForm};
use scatter_wikibase_model::statement::Rank;

/// The default badge registry, `docs/registry/badges.toml`, embedded at build time.
pub const BADGES_TOML: &str = include_str!("../registry/badges.toml");

/// Where a badge comes from, which fixes what it marks (06 §6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum BadgeSource {
    /// A sitelink's `badges` names it.
    Stored,
    /// An entity is an instance of `class`, directly or through subclasses.
    Class {
        /// The class.
        class: EntityId,
    },
    /// A statement has this rank.
    Rank {
        /// The rank.
        #[serde(serialize_with = "rank_name")]
        rank: Rank,
    },
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's `serialize_with` passes a reference
fn rank_name<S: serde::Serializer>(rank: &Rank, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(match rank {
        Rank::Preferred => "preferred",
        Rank::Normal => "normal",
        Rank::Deprecated => "deprecated",
    })
}

/// One badge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Badge {
    /// The item the badge is, stored form (`WDQ17437796`).
    pub item: EntityId,
    /// Where it comes from.
    #[serde(flatten)]
    pub source: BadgeSource,
    /// A Codex icon name.
    pub icon: String,
    /// Where it stands among badges on one thing, lowest first.
    pub order: i64,
    /// An interface message whose text the badge shows in place of its item's label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A badge registry: the instance's, with a tenant's own added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BadgeRegistry {
    badges: Vec<Badge>,
}

/// What is wrong with a badge registry or a tenant's `badges.local`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BadgeError {
    /// The text does not parse.
    #[error("the badge list does not parse: {0}")]
    Parse(String),
    /// An item or class is not a local-form or prefixed item ID.
    #[error("`{0}` is not an item ID")]
    NotAnItem(String),
    /// A field the source needs is missing, or one it does not take is given.
    #[error("badge `{item}`: {reason}")]
    Shape {
        /// The badge's item.
        item: String,
        /// What is wrong.
        reason: String,
    },
    /// Two badges name one item.
    #[error("badge `{0}` is listed twice")]
    Duplicate(String),
    /// A tenant badge that is a `rank` badge (0114 §3).
    #[error("badge `{0}`: a wiki cannot add a rank badge")]
    LocalRank(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    #[serde(default, rename = "badge")]
    badges: Vec<RawBadge>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBadge {
    item: String,
    source: String,
    class: Option<String>,
    rank: Option<String>,
    icon: Option<String>,
    order: Option<i64>,
    message: Option<String>,
}

fn item(text: &str) -> Result<EntityId, BadgeError> {
    let id = EntityId::parse(text).map_err(|_| BadgeError::NotAnItem(text.to_string()))?;
    if id.implied_entity_type() != Some("item") || id.form() == IdForm::Keyed {
        return Err(BadgeError::NotAnItem(text.to_string()));
    }
    Ok(id)
}

impl RawBadge {
    fn read(self) -> Result<Badge, BadgeError> {
        let shape = |reason: &str| BadgeError::Shape {
            item: self.item.clone(),
            reason: reason.to_string(),
        };
        let source = match (self.source.as_str(), &self.class, &self.rank) {
            ("stored", None, None) => BadgeSource::Stored,
            ("class", Some(class), None) => BadgeSource::Class {
                class: item(class)?,
            },
            ("rank", None, Some(rank)) => BadgeSource::Rank {
                rank: match rank.as_str() {
                    "preferred" => Rank::Preferred,
                    "normal" => Rank::Normal,
                    "deprecated" => Rank::Deprecated,
                    _ => return Err(shape("`rank` is preferred, normal or deprecated")),
                },
            },
            ("stored" | "class" | "rank", _, _) => {
                return Err(shape(
                    "a class badge takes `class`, a rank badge `rank`, a stored badge neither",
                ));
            }
            _ => return Err(shape("`source` is stored, class or rank")),
        };
        Ok(Badge {
            item: item(&self.item)?,
            source,
            icon: self.icon.unwrap_or_else(|| "tag".to_string()),
            order: self.order.unwrap_or(100),
            message: self.message,
        })
    }
}

impl BadgeRegistry {
    /// Parses a registry in the format of `docs/registry/badges.toml`.
    pub fn parse(text: &str) -> Result<Self, BadgeError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| BadgeError::Parse(e.to_string()))?;
        let mut registry = Self::default();
        for b in raw.badges {
            registry.add(b.read()?)?;
        }
        Ok(registry)
    }

    /// The embedded default registry.
    ///
    /// # Panics
    /// If the embedded file does not parse, which its test rules out.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static REGISTRY: OnceLock<BadgeRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| Self::parse(BADGES_TOML).expect("badges.toml parses"))
    }

    fn add(&mut self, badge: Badge) -> Result<(), BadgeError> {
        if self.badges.iter().any(|b| b.item == badge.item) {
            return Err(BadgeError::Duplicate(badge.item.as_str().to_string()));
        }
        let at = self
            .badges
            .partition_point(|b| (b.order, b.item.as_str()) <= (badge.order, badge.item.as_str()));
        self.badges.insert(at, badge);
        Ok(())
    }

    /// This registry with a tenant's `badges.local` added: a JSON list of entries with the
    /// fields of a registry entry, `stored` or `class` only; none may take a registry
    /// badge's item (0114 §3).
    pub fn with_local(&self, local: &serde_json::Value) -> Result<Self, BadgeError> {
        let mut registry = self.clone();
        if local.is_null() {
            return Ok(registry);
        }
        let raw: Vec<RawBadge> =
            serde_json::from_value(local.clone()).map_err(|e| BadgeError::Parse(e.to_string()))?;
        for b in raw {
            let badge = b.read()?;
            if matches!(badge.source, BadgeSource::Rank { .. }) {
                return Err(BadgeError::LocalRank(badge.item.as_str().to_string()));
            }
            registry.add(badge)?;
        }
        Ok(registry)
    }

    /// Every badge, in drawing order.
    #[must_use]
    pub fn badges(&self) -> &[Badge] {
        &self.badges
    }

    /// The badge an item is, if any.
    #[must_use]
    pub fn get(&self, item: &EntityId) -> Option<&Badge> {
        self.badges.iter().find(|b| b.item == *item)
    }

    /// The items a sitelink's `badges` may name (06 §4.1).
    #[must_use]
    pub fn stored(&self) -> BTreeSet<&EntityId> {
        self.badges
            .iter()
            .filter(|b| b.source == BadgeSource::Stored)
            .map(|b| &b.item)
            .collect()
    }

    /// The `class` badges, each with its class.
    pub fn classes(&self) -> impl Iterator<Item = (&Badge, &EntityId)> {
        self.badges.iter().filter_map(|b| match &b.source {
            BadgeSource::Class { class } => Some((b, class)),
            _ => None,
        })
    }

    /// The badge a statement of `rank` carries, if any.
    #[must_use]
    pub fn for_rank(&self, rank: Rank) -> Option<&Badge> {
        self.badges
            .iter()
            .find(|b| matches!(b.source, BadgeSource::Rank { rank: r } if r == rank))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> EntityId {
        EntityId::parse(s).unwrap()
    }

    // 0114 §3: the default registry holds "Best value", "(deprecated)" and Wikidata's
    // sitelink badges, in drawing order.
    #[test]
    fn the_default_registry_has_the_three_kinds() {
        let r = BadgeRegistry::default_registry();
        let best = r.for_rank(Rank::Preferred).unwrap();
        assert_eq!(best.item, id("WDQ71533031"));
        assert_eq!(best.message.as_deref(), Some("ts-best-value"));
        assert!(r.for_rank(Rank::Normal).is_none());
        let classes: Vec<_> = r.classes().map(|(_, c)| c.as_str()).collect();
        assert_eq!(classes, ["WDQ142241643"]);
        let stored = r.stored();
        assert_eq!(stored.len(), 12);
        assert!(stored.contains(&id("WDQ17437796")));
        assert!(!stored.contains(&id("WDQ71533031")));
        let orders: Vec<_> = r.badges().iter().map(|b| b.order).collect();
        assert!(orders.is_sorted());
    }

    // 0114 §3: a tenant adds stored and class badges, never a rank badge or a registry
    // badge's item.
    #[test]
    fn a_tenant_adds_its_own_badges() {
        let r = BadgeRegistry::default_registry();
        let local = serde_json::json!([
            {"item": "Q5", "source": "class", "class": "Q6", "icon": "flag"},
            {"item": "Q7", "source": "stored"}
        ]);
        let with = r.with_local(&local).unwrap();
        assert_eq!(with.badges().len(), r.badges().len() + 2);
        assert!(with.stored().contains(&id("Q7")));
        assert!(
            with.classes()
                .any(|(b, c)| b.item == id("Q5") && *c == id("Q6"))
        );
        assert_eq!(with.get(&id("Q7")).unwrap().icon, "tag");

        let err = |v: serde_json::Value| r.with_local(&v).unwrap_err();
        assert!(matches!(
            err(serde_json::json!([{"item": "Q5", "source": "rank", "rank": "normal"}])),
            BadgeError::LocalRank(_)
        ));
        assert!(matches!(
            err(serde_json::json!([{"item": "WDQ17437796", "source": "stored"}])),
            BadgeError::Duplicate(_)
        ));
        assert!(matches!(
            err(serde_json::json!([{"item": "P5", "source": "stored"}])),
            BadgeError::NotAnItem(_)
        ));
        assert!(matches!(
            err(serde_json::json!([{"item": "Q5", "source": "class"}])),
            BadgeError::Shape { .. }
        ));
        assert_eq!(r.with_local(&serde_json::Value::Null).unwrap(), *r);
    }
}
