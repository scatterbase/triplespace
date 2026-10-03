//! The `changes` summary a `put` carries (0012 §2.2; payloads.md §3.1): per property,
//! language or site, how many statements, terms or sitelinks were added, removed or
//! changed between the previous mirrored state and the new one. It is what history shows
//! for a mirrored revision once compaction has removed the states themselves.

use std::collections::BTreeMap;

use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::Statement;
use serde::{Deserialize, Serialize};

/// Counts for one property, language or site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    /// Present in the new state only.
    pub added: u32,
    /// Present in the old state only.
    pub removed: u32,
    /// Present in both, differently.
    pub changed: u32,
}

impl Counts {
    /// Nothing happened.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What changed between two states of an entity.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    /// Statements, by property.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub claims: BTreeMap<EntityId, Counts>,
    /// Labels, by language.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, Counts>,
    /// Descriptions, by language.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub descriptions: BTreeMap<String, Counts>,
    /// Aliases, by language; each alias value counts one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, Counts>,
    /// Sitelinks, by site.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sitelinks: BTreeMap<String, Counts>,
}

impl Changes {
    /// Nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
            && self.labels.is_empty()
            && self.descriptions.is_empty()
            && self.aliases.is_empty()
            && self.sitelinks.is_empty()
    }

    /// The total of every count.
    #[must_use]
    pub fn total(&self) -> u32 {
        fn sum<'a>(it: impl Iterator<Item = &'a Counts>) -> u32 {
            it.map(|c| c.added + c.removed + c.changed).sum()
        }
        sum(self.claims.values())
            + sum(self.labels.values())
            + sum(self.descriptions.values())
            + sum(self.aliases.values())
            + sum(self.sitelinks.values())
    }

    /// The summary of going from `old` to `new`. `old` is `None` for a first state, where
    /// everything is added.
    #[must_use]
    pub fn between(old: Option<&Entity>, new: &Entity) -> Self {
        let empty = Entity::new(new.id.clone(), new.entity_type.clone());
        let old = old.unwrap_or(&empty);
        let mut out = Self {
            labels: terms(&old.labels, &new.labels),
            descriptions: terms(&old.descriptions, &new.descriptions),
            ..Self::default()
        };
        for lang in old.aliases.keys().chain(new.aliases.keys()) {
            if out.aliases.contains_key(lang) {
                continue;
            }
            let o = old.aliases.get(lang).map_or(&[][..], Vec::as_slice);
            let n = new.aliases.get(lang).map_or(&[][..], Vec::as_slice);
            let c = Counts {
                added: count(n.iter().filter(|a| !o.contains(a))),
                removed: count(o.iter().filter(|a| !n.contains(a))),
                changed: 0,
            };
            if !c.is_empty() {
                out.aliases.insert(lang.clone(), c);
            }
        }
        for site in old.sitelinks.keys().chain(new.sitelinks.keys()) {
            if out.sitelinks.contains_key(site) {
                continue;
            }
            let c = match (old.sitelinks.get(site), new.sitelinks.get(site)) {
                (None, Some(_)) => Counts {
                    added: 1,
                    ..Counts::default()
                },
                (Some(_), None) => Counts {
                    removed: 1,
                    ..Counts::default()
                },
                (Some(a), Some(b)) if a != b => Counts {
                    changed: 1,
                    ..Counts::default()
                },
                _ => Counts::default(),
            };
            if !c.is_empty() {
                out.sitelinks.insert(site.clone(), c);
            }
        }
        for property in old.statements.keys().chain(new.statements.keys()) {
            if out.claims.contains_key(property) {
                continue;
            }
            let o = old.statements.get(property).map_or(&[][..], Vec::as_slice);
            let n = new.statements.get(property).map_or(&[][..], Vec::as_slice);
            let c = statements(o, n);
            if !c.is_empty() {
                out.claims.insert(property.clone(), c);
            }
        }
        out
    }
}

fn count<I: Iterator>(it: I) -> u32 {
    u32::try_from(it.count()).unwrap_or(u32::MAX)
}

fn terms(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> BTreeMap<String, Counts> {
    let mut out = BTreeMap::new();
    for lang in old.keys().chain(new.keys()) {
        if out.contains_key(lang) {
            continue;
        }
        let c = match (old.get(lang), new.get(lang)) {
            (None, Some(_)) => Counts {
                added: 1,
                ..Counts::default()
            },
            (Some(_), None) => Counts {
                removed: 1,
                ..Counts::default()
            },
            (Some(a), Some(b)) if a != b => Counts {
                changed: 1,
                ..Counts::default()
            },
            _ => Counts::default(),
        };
        if !c.is_empty() {
            out.insert(lang.clone(), c);
        }
    }
    out
}

/// Statements are matched by GUID where both sides have one; a statement without a GUID
/// matches an equal statement. The rest are added or removed.
fn statements(old: &[Statement], new: &[Statement]) -> Counts {
    let mut c = Counts::default();
    let mut matched_new = vec![false; new.len()];
    for o in old {
        let found = new.iter().enumerate().find(|(i, n)| {
            !matched_new[*i]
                && match (&o.id, &n.id) {
                    (Some(a), Some(b)) => a == b,
                    _ => o == *n,
                }
        });
        match found {
            Some((i, n)) => {
                matched_new[i] = true;
                if o != n {
                    c.changed += 1;
                }
            }
            None => c.removed += 1,
        }
    }
    c.added = count(matched_new.iter().filter(|m| !**m));
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_wikibase_model::entity::EntityType;
    use serde_json::json;

    fn entity(v: serde_json::Value) -> Entity {
        Entity::from_value(v).unwrap().entity
    }

    fn statement(p: &str, v: &str, id: Option<&str>) -> serde_json::Value {
        let mut s = json!({"mainsnak": {"snaktype": "value", "property": p,
            "datavalue": {"value": v, "type": "string"}, "datatype": "string"},
            "type": "statement", "rank": "normal"});
        if let Some(id) = id {
            s["id"] = json!(id);
        }
        s
    }

    #[test]
    fn summarizes_terms_sitelinks_and_claims() {
        let old = entity(json!({"type": "item", "id": "Q1",
            "labels": {"en": {"language": "en", "value": "A"}, "de": {"language": "de", "value": "A"}},
            "aliases": {"en": [{"language": "en", "value": "x"}, {"language": "en", "value": "y"}]},
            "claims": {"P1": [statement("P1", "a", Some("Q1$00000000-0000-0000-0000-000000000001")),
                              statement("P1", "b", Some("Q1$00000000-0000-0000-0000-000000000002"))],
                       "P2": [statement("P2", "c", None)]},
            "sitelinks": {"enwiki": {"site": "enwiki", "title": "A"}}}));
        let new = entity(json!({"type": "item", "id": "Q1",
            "labels": {"en": {"language": "en", "value": "A"}, "fr": {"language": "fr", "value": "A"}},
            "descriptions": {"en": {"language": "en", "value": "d"}},
            "aliases": {"en": [{"language": "en", "value": "y"}, {"language": "en", "value": "z"}]},
            "claims": {"P1": [statement("P1", "a", Some("Q1$00000000-0000-0000-0000-000000000001")),
                              statement("P1", "B", Some("Q1$00000000-0000-0000-0000-000000000002")),
                              statement("P1", "c", None)],
                       "P2": [statement("P2", "c", None)]},
            "sitelinks": {"enwiki": {"site": "enwiki", "title": "B"}, "dewiki": {"site": "dewiki", "title": "A"}}}));
        let c = Changes::between(Some(&old), &new);
        assert_eq!(
            c.labels["de"],
            Counts {
                removed: 1,
                ..Counts::default()
            }
        );
        assert_eq!(
            c.labels["fr"],
            Counts {
                added: 1,
                ..Counts::default()
            }
        );
        assert!(!c.labels.contains_key("en"));
        assert_eq!(c.descriptions["en"].added, 1);
        assert_eq!(
            c.aliases["en"],
            Counts {
                added: 1,
                removed: 1,
                changed: 0
            }
        );
        assert_eq!(c.sitelinks["enwiki"].changed, 1);
        assert_eq!(c.sitelinks["dewiki"].added, 1);
        let p1 = EntityId::parse("P1").unwrap();
        assert_eq!(
            c.claims[&p1],
            Counts {
                added: 1,
                removed: 0,
                changed: 1
            }
        );
        assert!(!c.claims.contains_key(&EntityId::parse("P2").unwrap()));
        assert_eq!(c.total(), 9);

        let text = serde_json::to_string(&c).unwrap();
        assert!(text.contains(r#""claims":{"P1":{"added":1,"removed":0,"changed":1}}"#));
        assert_eq!(serde_json::from_str::<Changes>(&text).unwrap(), c);
    }

    #[test]
    fn a_first_state_is_all_added() {
        let new = entity(json!({"type": "item", "id": "Q1",
            "labels": {"en": {"language": "en", "value": "A"}},
            "claims": {"P1": [statement("P1", "a", None)]}}));
        let c = Changes::between(None, &new);
        assert_eq!(c.labels["en"].added, 1);
        assert_eq!(c.claims[&EntityId::parse("P1").unwrap()].added, 1);
        assert!(Changes::between(Some(&new), &new).is_empty());
        assert!(
            Changes::between(
                None,
                &Entity::new(EntityId::parse("Q2").unwrap(), EntityType::Item)
            )
            .is_empty()
        );
    }
}
