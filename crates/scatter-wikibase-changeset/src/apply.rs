//! Applying local operations to a state: what the local graph says about one subject,
//! folded from its records in order (0002 §3, §7; payloads.md §3.2). A projection replays
//! a key's local records through [`LocalState::apply`] and hands the result to the
//! resolver, together with the mirror states, as one contribution among several.
//!
//! The fold is the local graph's own semantics only: merging terms and statements,
//! retracting by GUID, language, value, site or hash, and recording the corrections
//! (`override`) and the retention policy. Nothing here reads another graph.

use std::collections::{BTreeMap, BTreeSet};

use scatter_wikibase_model::StatementId;
use scatter_wikibase_model::entity::{Entity, EntityType, Sitelink};
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, Subject};
use scatter_wikibase_model::key::{Identity, statement_key};
use scatter_wikibase_model::statement::{Rank, Statement};

use crate::op::{Operation, Retention};

/// A local correction of a statement another graph asserts (0002 §7): a rank, or a
/// suppression. A later override on the same statement replaces the earlier one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Correction {
    /// Show it with this rank.
    Rank(Rank),
    /// Hide it.
    Suppress,
}

/// What the local graph says about one subject.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocalState {
    /// The local assertions, when there are any: a whole state from `create` or `adopt`,
    /// or the parts `add` merged onto a subject that lives in another graph.
    pub entity: Option<Entity>,
    /// Corrections of statements by GUID.
    pub corrections: BTreeMap<StatementId, Correction>,
    /// The retention policy, where set.
    pub retention: Option<Retention>,
    /// Where a local `redirect` sent the subject.
    pub redirect_to: Option<EntityId>,
    /// The foreign ID a `convert` made an alias of this local entity, or the local ID a
    /// foreign subject was converted into.
    pub converted: Option<EntityId>,
    /// The records applied, by offset where the caller supplied one.
    pub offsets: Vec<u64>,
}

impl LocalState {
    /// Nothing asserted yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the local graph asserts anything about the subject.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entity.is_none()
            && self.corrections.is_empty()
            && self.retention.is_none()
            && self.redirect_to.is_none()
            && self.converted.is_none()
    }

    /// The state to merge parts onto, made empty for `subject` if there is none yet; the
    /// type is the one the subject's ID implies, or `item`.
    fn entity_for(&mut self, subject: &EntityId) -> &mut Entity {
        self.entity.get_or_insert_with(|| {
            let kind = subject
                .implied_entity_type()
                .map_or(EntityType::Item, EntityType::parse);
            Entity::new(subject.clone(), kind)
        })
    }

    /// Applies one operation. A mirror operation (`put`, `tombstone`, a `redirect` with
    /// `upstream`) is not local and is ignored; a `create-or-add` has to be resolved
    /// first and is ignored too. `offset` is recorded when given.
    pub fn apply(&mut self, op: &Operation, offset: Option<u64>) {
        if let Some(o) = offset {
            self.offsets.push(o);
        }
        match op {
            Operation::Create { entity, .. } | Operation::Adopt { entity, .. } => {
                self.entity = Some(entity.clone());
            }
            Operation::Add { id: Some(id), .. } => self.add(id, op),
            Operation::Remove { id: Some(_), .. } => self.remove(op),
            Operation::Override {
                statement,
                rank,
                suppress,
                ..
            } => {
                let correction = match (rank, suppress) {
                    (Some(r), _) => Correction::Rank(*r),
                    (None, Some(true)) => Correction::Suppress,
                    (None, _) => return,
                };
                self.corrections.insert(statement.clone(), correction);
            }
            Operation::Retain { policy, .. } => self.retention = Some(*policy),
            Operation::Redirect {
                to, upstream: None, ..
            } => self.redirect_to = Some(to.clone()),
            Operation::Convert { local, .. } => self.converted = Some(local.clone()),
            // Links are cluster facts, not entity state; mirror operations and unresolved
            // wire shapes are not the local graph's.
            Operation::SameAs { .. }
            | Operation::DifferentFrom { .. }
            | Operation::EquivalentProperty { .. }
            | Operation::Put { .. }
            | Operation::Tombstone { .. }
            | Operation::Redirect { .. }
            | Operation::CreateOrAdd { .. }
            | Operation::Add { .. }
            | Operation::Remove { .. } => {}
        }
    }

    fn add(&mut self, id: &EntityId, op: &Operation) {
        let Operation::Add {
            labels,
            descriptions,
            aliases,
            claims,
            sitelinks,
            references,
            qualifiers,
            entity,
            overwrite,
            ..
        } = op
        else {
            return;
        };
        if let (Some(whole), true) = (entity, *overwrite) {
            self.entity = Some(whole.clone());
            return;
        }
        let e = self.entity_for(id);
        e.labels
            .extend(labels.iter().map(|(l, v)| (l.clone(), v.clone())));
        e.descriptions
            .extend(descriptions.iter().map(|(l, v)| (l.clone(), v.clone())));
        for (lang, values) in aliases {
            let list = e.aliases.entry(lang.clone()).or_default();
            for v in values {
                if !list.contains(v) {
                    list.push(v.clone());
                }
            }
        }
        for (property, statements) in claims {
            for s in statements {
                add_statement(e, property, s);
            }
        }
        for (site, patch) in sitelinks {
            e.sitelinks.insert(
                site.clone(),
                Sitelink {
                    site: site.clone(),
                    title: patch.title.clone(),
                    badges: patch.badges.clone(),
                    url: None,
                },
            );
        }
        for (guid, refs) in references {
            if let Some(s) = statement_mut(e, guid) {
                for r in refs {
                    if !s.references.contains(r) {
                        s.references.push(r.clone());
                    }
                }
            }
        }
        for (guid, groups) in qualifiers {
            if let Some(s) = statement_mut(e, guid) {
                for (property, snaks) in groups {
                    let list = s.qualifiers.entry(property.clone()).or_default();
                    for q in snaks {
                        if !list.contains(q) {
                            list.push(q.clone());
                        }
                    }
                }
            }
        }
    }

    fn remove(&mut self, op: &Operation) {
        let Operation::Remove {
            statements,
            labels,
            descriptions,
            aliases,
            sitelinks,
            references,
            qualifiers,
            ..
        } = op
        else {
            return;
        };
        let Some(e) = self.entity.as_mut() else {
            return;
        };
        let hasher = Hasher::local();
        let gone: BTreeSet<&StatementId> = statements.iter().collect();
        for group in e.statements.values_mut() {
            group.retain(|s| !s.id.as_ref().is_some_and(|i| gone.contains(i)));
        }
        e.statements.retain(|_, g| !g.is_empty());
        for l in labels {
            e.labels.remove(l);
        }
        for l in descriptions {
            e.descriptions.remove(l);
        }
        for (lang, values) in aliases {
            if let Some(list) = e.aliases.get_mut(lang) {
                list.retain(|v| !values.contains(v));
                if list.is_empty() {
                    e.aliases.remove(lang);
                }
            }
        }
        for site in sitelinks {
            e.sitelinks.remove(site);
        }
        for (guid, hashes) in references {
            if let Some(s) = statement_mut(e, guid) {
                s.references.retain(|r| {
                    let h = r.hash.clone().unwrap_or_else(|| hasher.reference(r));
                    !hashes.contains(&h)
                });
            }
        }
        for (guid, hashes) in qualifiers {
            if let Some(s) = statement_mut(e, guid) {
                for group in s.qualifiers.values_mut() {
                    group.retain(|q| {
                        let h = q.hash.clone().unwrap_or_else(|| hasher.snak(q));
                        !hashes.contains(&h)
                    });
                }
                s.qualifiers.retain(|_, g| !g.is_empty());
            }
        }
    }

    /// Folds a sequence of operations.
    pub fn fold<'a>(ops: impl IntoIterator<Item = (&'a Operation, Option<u64>)>) -> Self {
        let mut state = Self::new();
        for (op, offset) in ops {
            state.apply(op, offset);
        }
        state
    }

    /// Whether the subject is a page's own statements rather than an entity's.
    #[must_use]
    pub fn subject_is_page(op: &Operation) -> bool {
        matches!(op.subject(), Some(Subject::Page(_)))
    }
}

/// Adds a statement unless an identical one exists (0002 §8.5: identical means the main
/// snak and qualifiers key the same), in which case the incoming references are merged
/// onto it.
fn add_statement(e: &mut Entity, property: &EntityId, incoming: &Statement) {
    let key = statement_key(incoming, &Identity);
    let group = e.statements.entry(property.clone()).or_default();
    if let Some(existing) = group
        .iter_mut()
        .find(|s| statement_key(s, &Identity) == key)
    {
        for r in &incoming.references {
            if !existing.references.contains(r) {
                existing.references.push(r.clone());
            }
        }
        if existing.id.is_none() {
            existing.id.clone_from(&incoming.id);
        }
        return;
    }
    group.push(incoming.clone());
}

fn statement_mut<'a>(e: &'a mut Entity, guid: &StatementId) -> Option<&'a mut Statement> {
    e.statements
        .values_mut()
        .flatten()
        .find(|s| s.id.as_ref() == Some(guid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn op(v: serde_json::Value) -> Operation {
        serde_json::from_value(v).unwrap()
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

    const G1: &str = "Q5$00000000-0000-0000-0000-000000000001";
    const G2: &str = "Q5$00000000-0000-0000-0000-000000000002";

    #[test]
    fn create_add_remove_fold() {
        let ops = [
            op(
                json!({"op": "create", "id": "Q5", "entity": {"type": "item", "id": "Q5",
                "labels": {"en": {"language": "en", "value": "Five"}},
                "claims": {"P1": [statement("P1", "a", Some(G1))]}}}),
            ),
            op(
                json!({"op": "add", "id": "Q5", "labels": {"fr": "Cinq", "en": "FIVE"},
                "aliases": {"en": ["V", "five"]},
                "claims": {"P1": [statement("P1", "a", None), statement("P1", "b", Some(G2))],
                           "P2": [statement("P2", "c", None)]},
                "sitelinks": {"enwiki": {"title": "Five"}},
                "references": {G1: [{"snaks": {"P3": [{"snaktype": "value", "property": "P3",
                    "datavalue": {"value": "r", "type": "string"}, "datatype": "string"}]}, "snaks-order": ["P3"]}]}}),
            ),
            op(json!({"op": "retain", "id": "Q5", "policy": "retain"})),
            op(
                json!({"op": "remove", "id": "Q5", "statements": [G2], "labels": ["fr"],
                "aliases": {"en": ["V"]}, "sitelinks": ["enwiki"]}),
            ),
            op(
                json!({"op": "override", "id": "WDQ1", "statement": "WDQ1$00000000-0000-0000-0000-000000000009", "rank": "deprecated"}),
            ),
        ];
        let state = LocalState::fold(ops.iter().zip(0..).map(|(o, i)| (o, Some(i))));
        let e = state.entity.as_ref().unwrap();
        assert_eq!(e.labels["en"], "FIVE", "a later add replaces the term");
        assert!(!e.labels.contains_key("fr"));
        assert_eq!(e.aliases["en"], vec!["five"]);
        let p1 = &e.statements[&EntityId::parse("P1").unwrap()];
        assert_eq!(
            p1.len(),
            1,
            "the identical statement merged, G2 was removed"
        );
        assert_eq!(p1[0].references.len(), 1, "the reference was added by GUID");
        assert_eq!(e.statements.len(), 2);
        assert!(e.sitelinks.is_empty());
        assert_eq!(state.retention, Some(Retention::Retain));
        assert_eq!(state.corrections.len(), 1);
        assert_eq!(state.offsets, vec![0, 1, 2, 3, 4]);
        assert!(!state.is_empty());
    }

    #[test]
    fn adds_onto_a_foreign_subject_make_a_partial_state() {
        let mut s = LocalState::new();
        s.apply(
            &op(
                json!({"op": "add", "id": "WDQ42", "claims": {"P7": [statement("P7", "x", None)]}}),
            ),
            None,
        );
        let e = s.entity.unwrap();
        assert_eq!(e.id.as_str(), "WDQ42");
        assert_eq!(e.entity_type, EntityType::Item);
        assert_eq!(e.statement_count(), 1);
        let mut d = LocalState::new();
        d.apply(
            &op(json!({"op": "add", "id": "domain:example.org", "labels": {"en": "x"}})),
            None,
        );
        assert_eq!(d.entity.unwrap().entity_type, EntityType::Domain);
        // Mirror operations are not local.
        let mut m = LocalState::new();
        m.apply(
            &op(json!({"op": "tombstone", "id": "WDQ42", "upstream": {"revid": 1}})),
            None,
        );
        assert!(m.is_empty());
        let mut over = LocalState::new();
        over.apply(&op(json!({"op": "add", "id": "Q5", "entity": {"type": "item", "id": "Q5", "claims": {}}, "overwrite": true})), None);
        assert!(over.entity.unwrap().labels.is_empty());
    }
}
