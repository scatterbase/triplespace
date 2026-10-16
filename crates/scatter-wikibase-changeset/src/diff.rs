//! The structured diff of two states of one source (0010 §6; 18 §5.1): what
//! `GET /entity/{id}/changes/{partition}/{offset}`, `GET /entity/{id}/compare` and the
//! diff page draw. Terms and sitelinks are diffed as keyed lists; statements are matched
//! by GUID where both sides have one, by equality otherwise (the rule of
//! [`super::changes`]), and each property the change touched is returned as a group
//! with its unchanged statements beside the changed ones, for context.
//!
//! The diff is pure: two entities in, the differences out. The JSON form is the one the
//! API returns and the site reads; statements are serialized in the storage form the
//! change-set format uses.

use std::collections::BTreeMap;

use scatter_wikibase_model::entity::{Entity, Sitelink};
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::Statement;
use serde::Serialize;

/// A keyed value on either side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TermChange {
    /// The language.
    pub language: String,
    /// The value before; absent when added.
    pub before: Option<String>,
    /// The value after; absent when removed.
    pub after: Option<String>,
}

/// A language's aliases on either side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AliasChange {
    /// The language.
    pub language: String,
    /// Present after only.
    pub added: Vec<String>,
    /// Present before only.
    pub removed: Vec<String>,
    /// Present on both sides.
    pub kept: Vec<String>,
}

/// A sitelink on either side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SitelinkChange {
    /// The site.
    pub site: String,
    /// Before; absent when added.
    pub before: Option<Sitelink>,
    /// After; absent when removed.
    pub after: Option<Sitelink>,
}

/// One statement's fate in a group.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "change", rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum StatementChange {
    /// Present after only.
    Added {
        /// The statement.
        after: Statement,
    },
    /// Present before only.
    Removed {
        /// The statement.
        before: Statement,
    },
    /// Present on both sides, differently.
    Changed {
        /// Before.
        before: Statement,
        /// After.
        after: Statement,
    },
    /// Present on both sides, the same: context.
    Unchanged {
        /// The statement.
        statement: Statement,
    },
}

impl StatementChange {
    /// Whether the statement changed at all.
    #[must_use]
    pub fn is_change(&self) -> bool {
        !matches!(self, Self::Unchanged { .. })
    }
}

/// One property's statements, in the order of the state after (then the removed ones).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupDiff {
    /// The property.
    pub property: EntityId,
    /// Each statement, changed or kept for context.
    pub statements: Vec<StatementChange>,
}

impl GroupDiff {
    /// How many statements changed.
    #[must_use]
    pub fn changes(&self) -> usize {
        self.statements.iter().filter(|s| s.is_change()).count()
    }
}

/// The differences between two states of one entity.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Diff {
    /// Labels that differ.
    pub labels: Vec<TermChange>,
    /// Descriptions that differ.
    pub descriptions: Vec<TermChange>,
    /// Languages whose aliases differ.
    pub aliases: Vec<AliasChange>,
    /// Sitelinks that differ.
    pub sitelinks: Vec<SitelinkChange>,
    /// The properties with a changed statement, each with its whole group.
    pub claims: Vec<GroupDiff>,
}

impl Diff {
    /// Nothing differs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
            && self.descriptions.is_empty()
            && self.aliases.is_empty()
            && self.sitelinks.is_empty()
            && self.claims.is_empty()
    }

    /// How many things changed: terms, alias lists, sitelinks and statements.
    #[must_use]
    pub fn count(&self) -> usize {
        self.labels.len()
            + self.descriptions.len()
            + self.aliases.len()
            + self.sitelinks.len()
            + self.claims.iter().map(GroupDiff::changes).sum::<usize>()
    }

    /// The diff from `old` to `new`; either side absent is an empty state.
    #[must_use]
    pub fn between(old: Option<&Entity>, new: Option<&Entity>) -> Self {
        let Some(any) = old.or(new) else {
            return Self::default();
        };
        let empty = Entity::new(any.id.clone(), any.entity_type.clone());
        let old = old.unwrap_or(&empty);
        let new = new.unwrap_or(&empty);
        let mut out = Self {
            labels: terms(&old.labels, &new.labels),
            descriptions: terms(&old.descriptions, &new.descriptions),
            ..Self::default()
        };
        let mut langs: Vec<&String> = old.aliases.keys().chain(new.aliases.keys()).collect();
        langs.sort();
        langs.dedup();
        for lang in langs {
            let o = old.aliases.get(lang).map_or(&[][..], Vec::as_slice);
            let n = new.aliases.get(lang).map_or(&[][..], Vec::as_slice);
            let c = AliasChange {
                language: lang.clone(),
                added: n.iter().filter(|a| !o.contains(a)).cloned().collect(),
                removed: o.iter().filter(|a| !n.contains(a)).cloned().collect(),
                kept: n.iter().filter(|a| o.contains(a)).cloned().collect(),
            };
            if !c.added.is_empty() || !c.removed.is_empty() {
                out.aliases.push(c);
            }
        }
        let mut sites: Vec<&String> = old.sitelinks.keys().chain(new.sitelinks.keys()).collect();
        sites.sort();
        sites.dedup();
        for site in sites {
            let (b, a) = (old.sitelinks.get(site), new.sitelinks.get(site));
            if b != a {
                out.sitelinks.push(SitelinkChange {
                    site: site.clone(),
                    before: b.cloned(),
                    after: a.cloned(),
                });
            }
        }
        let mut properties: Vec<&EntityId> =
            old.statements.keys().chain(new.statements.keys()).collect();
        properties.dedup();
        let mut seen = std::collections::BTreeSet::new();
        for property in properties {
            if !seen.insert(property.to_string()) {
                continue;
            }
            let o = old.statements.get(property).map_or(&[][..], Vec::as_slice);
            let n = new.statements.get(property).map_or(&[][..], Vec::as_slice);
            let g = group(property, o, n);
            if g.changes() > 0 {
                out.claims.push(g);
            }
        }
        out
    }
}

fn terms(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<TermChange> {
    let mut langs: Vec<&String> = old.keys().chain(new.keys()).collect();
    langs.sort();
    langs.dedup();
    langs
        .into_iter()
        .filter_map(|lang| {
            let (b, a) = (old.get(lang), new.get(lang));
            (b != a).then(|| TermChange {
                language: lang.clone(),
                before: b.cloned(),
                after: a.cloned(),
            })
        })
        .collect()
}

/// Matches as [`super::changes`] counts: by GUID where both sides have one, by equality
/// otherwise. The group is ordered as the state after, the removed statements last.
fn group(property: &EntityId, old: &[Statement], new: &[Statement]) -> GroupDiff {
    let mut matched_old = vec![false; old.len()];
    let mut statements = Vec::with_capacity(new.len());
    for n in new {
        let found = old.iter().enumerate().find(|(i, o)| {
            !matched_old[*i]
                && match (&o.id, &n.id) {
                    (Some(a), Some(b)) => a == b,
                    _ => *o == n,
                }
        });
        match found {
            Some((i, o)) => {
                matched_old[i] = true;
                statements.push(if o == n {
                    StatementChange::Unchanged {
                        statement: n.clone(),
                    }
                } else {
                    StatementChange::Changed {
                        before: o.clone(),
                        after: n.clone(),
                    }
                });
            }
            None => statements.push(StatementChange::Added { after: n.clone() }),
        }
    }
    for (i, o) in old.iter().enumerate() {
        if !matched_old[i] {
            statements.push(StatementChange::Removed { before: o.clone() });
        }
    }
    GroupDiff {
        property: property.clone(),
        statements,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_wikibase_model::entity::EntityType;
    use scatter_wikibase_model::statement::{Rank, Snak};
    use scatter_wikibase_model::value::DataValue;

    fn statement(property: &str, value: &str, id: Option<&str>) -> Statement {
        let mut s = Statement::new(Snak::value(
            EntityId::parse(property).unwrap(),
            DataValue::String(value.into()),
            None,
        ));
        s.id = id.map(|i| i.parse().unwrap());
        s
    }

    fn entity() -> Entity {
        let mut e = Entity::new(EntityId::parse("Q6").unwrap(), EntityType::Item);
        e.labels.insert("en".into(), "Six".into());
        e.labels.insert("fr".into(), "Six".into());
        e.aliases.insert("en".into(), vec!["VI".into(), "6".into()]);
        e.statements.insert(
            EntityId::parse("P1").unwrap(),
            vec![
                statement("P1", "a", Some("Q6$00000000-0000-0000-0000-000000000001")),
                statement("P1", "b", Some("Q6$00000000-0000-0000-0000-000000000002")),
            ],
        );
        e.statements.insert(
            EntityId::parse("P2").unwrap(),
            vec![statement("P2", "c", None)],
        );
        e
    }

    #[test]
    fn nothing_differs_from_itself() {
        let e = entity();
        let d = Diff::between(Some(&e), Some(&e));
        assert!(d.is_empty());
        assert_eq!(d.count(), 0);
    }

    #[test]
    fn terms_aliases_and_statements_are_matched() {
        let old = entity();
        let mut new = entity();
        new.labels.insert("en".into(), "Six & more".into());
        new.labels.remove("fr");
        new.descriptions.insert("en".into(), "a number".into());
        new.aliases
            .insert("en".into(), vec!["6".into(), "half dozen".into()]);
        let p1 = EntityId::parse("P1").unwrap();
        let group = new.statements.get_mut(&p1).unwrap();
        group[1].rank = Rank::Preferred;
        group.push(statement("P1", "d", None));
        let d = Diff::between(Some(&old), Some(&new));
        assert_eq!(
            d.labels,
            vec![
                TermChange {
                    language: "en".into(),
                    before: Some("Six".into()),
                    after: Some("Six & more".into())
                },
                TermChange {
                    language: "fr".into(),
                    before: Some("Six".into()),
                    after: None
                }
            ]
        );
        assert_eq!(d.descriptions.len(), 1);
        assert_eq!(d.aliases[0].added, vec!["half dozen"]);
        assert_eq!(d.aliases[0].removed, vec!["VI"]);
        assert_eq!(d.aliases[0].kept, vec!["6"]);
        assert_eq!(d.claims.len(), 1, "P2 is untouched");
        let g = &d.claims[0];
        assert_eq!(g.property.as_str(), "P1");
        assert_eq!(g.changes(), 2);
        assert!(matches!(g.statements[0], StatementChange::Unchanged { .. }));
        assert!(matches!(g.statements[1], StatementChange::Changed { .. }));
        assert!(matches!(g.statements[2], StatementChange::Added { .. }));
        assert_eq!(d.count(), 2 + 1 + 1 + 2);
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["claims"][0]["statements"][1]["change"], "changed");
    }

    #[test]
    fn an_absent_side_is_empty() {
        let e = entity();
        let d = Diff::between(None, Some(&e));
        assert_eq!(d.labels.len(), 2);
        assert_eq!(d.claims.len(), 2);
        assert!(
            d.claims
                .iter()
                .flat_map(|g| &g.statements)
                .all(|s| matches!(s, StatementChange::Added { .. }))
        );
        let d = Diff::between(Some(&e), None);
        assert!(
            d.claims
                .iter()
                .flat_map(|g| &g.statements)
                .all(|s| matches!(s, StatementChange::Removed { .. }))
        );
        assert!(Diff::between(None, None).is_empty());
    }
}
