//! The difference between two states of an entity as change-set operations (0002 §8.1;
//! 0006 §8): what an interactive edit submits as a batch of one, and what a revert writes
//! to take an entity back (0104 §3). A changed statement is a `remove` of its GUID and an
//! `add` of the new statement under the same GUID; a mirrored statement the editor drops
//! or changes is not the tenant's to retract, so it is suppressed with an `override`
//! (0004 §8), and a changed one re-added under a fresh local GUID. Terms, aliases and
//! sitelinks are added and removed by language and site.
//!
//! A new statement that an `add` would merge onto one the local graph already holds
//! (0002 §8.5: the same main snak and qualifiers), bringing no reference it lacks, is no
//! difference: nothing is written for it, so an edit that only repeats a statement
//! appends no record and makes no revision.

use std::collections::{BTreeMap, BTreeSet};

use scatter_wikibase_changeset::{Operation, SitelinkPatch};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, StatementId};
use scatter_wikibase_model::key::{Identity, statement_key};
use scatter_wikibase_model::statement::Statement;

/// The statement with this GUID in an entity.
fn by_guid<'a>(e: &'a Entity, guid: &StatementId) -> Option<&'a Statement> {
    e.all_statements().find(|s| s.id.as_ref() == Some(guid))
}

/// Whether two statements say the same thing, hashes aside.
fn same_statement(a: &Statement, b: &Statement) -> bool {
    let h = Hasher::local();
    h.statement(a) == h.statement(b)
        && a.rank == b.rank
        && a.references.len() == b.references.len()
        && a.references
            .iter()
            .zip(&b.references)
            .all(|(x, y)| h.reference(x) == h.reference(y))
}

/// Whether adding `new` under `property` would change nothing: the local graph already
/// asserts an identical statement (0002 §8.5), which the edit keeps, and every reference
/// of `new` is on it already, so the `add` would merge it away.
fn merges_away(
    was: &Entity,
    is_local: &dyn Fn(&StatementId) -> bool,
    after: &Entity,
    property: &EntityId,
    new: &Statement,
) -> bool {
    let h = Hasher::local();
    let key = statement_key(new, &Identity);
    was.statements.get(property).is_some_and(|group| {
        group.iter().any(|old| {
            old.id
                .as_ref()
                .is_some_and(|g| is_local(g) && by_guid(after, g).is_some())
                && statement_key(old, &Identity) == key
                && new.references.iter().all(|r| {
                    old.references
                        .iter()
                        .any(|o| h.reference(o) == h.reference(r))
                })
        })
    })
}

/// The operations that take a state whose local statements are `local` to `after`; for a
/// keyed entity no graph holds yet, `was` is empty and everything is an `add`.
#[must_use]
pub fn operations_from(was: &Entity, local: &[StatementId], after: &Entity) -> Vec<Operation> {
    diff(was, &|g| local.contains(g), after)
}

/// The operations that take `was` to `after`, `is_local` saying which of `was`'s
/// statements the local graph asserts.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn diff(
    was: &Entity,
    is_local: &dyn Fn(&StatementId) -> bool,
    after: &Entity,
) -> Vec<Operation> {
    let id = after.id.clone();
    let mut remove_statements: Vec<StatementId> = Vec::new();
    let mut suppress: Vec<StatementId> = Vec::new();
    let mut add_claims = scatter_wikibase_model::entity::StatementGroups::default();

    // Statements the editor kept, changed or added.
    let mut seen: BTreeSet<StatementId> = BTreeSet::new();
    for (property, group) in &after.statements {
        for s in group {
            match &s.id {
                Some(guid) => {
                    seen.insert(guid.clone());
                    match by_guid(was, guid) {
                        Some(old) if same_statement(old, s) => {}
                        Some(_) if is_local(guid) => {
                            remove_statements.push(guid.clone());
                            add_claims
                                .entry(property.clone())
                                .or_default()
                                .push(s.clone());
                        }
                        Some(_) => {
                            // A mirrored statement changed: suppress upstream's, add ours
                            // under a fresh GUID.
                            suppress.push(guid.clone());
                            let mut fresh = s.clone();
                            fresh.id = None;
                            add_claims.entry(property.clone()).or_default().push(fresh);
                        }
                        None if merges_away(was, is_local, after, property, s) => {}
                        None => add_claims
                            .entry(property.clone())
                            .or_default()
                            .push(s.clone()),
                    }
                }
                None if merges_away(was, is_local, after, property, s) => {}
                None => add_claims
                    .entry(property.clone())
                    .or_default()
                    .push(s.clone()),
            }
        }
    }
    // Statements the editor dropped.
    for s in was.all_statements() {
        if let Some(guid) = &s.id
            && !seen.contains(guid)
        {
            if is_local(guid) {
                remove_statements.push(guid.clone());
            } else {
                suppress.push(guid.clone());
            }
        }
    }

    // Terms and sitelinks.
    let mut add_labels = BTreeMap::new();
    let mut remove_labels = Vec::new();
    for (lang, v) in &after.labels {
        if was.labels.get(lang) != Some(v) {
            add_labels.insert(lang.clone(), v.clone());
        }
    }
    for lang in was.labels.keys() {
        if !after.labels.contains_key(lang) {
            remove_labels.push(lang.clone());
        }
    }
    let mut add_descriptions = BTreeMap::new();
    let mut remove_descriptions = Vec::new();
    for (lang, v) in &after.descriptions {
        if was.descriptions.get(lang) != Some(v) {
            add_descriptions.insert(lang.clone(), v.clone());
        }
    }
    for lang in was.descriptions.keys() {
        if !after.descriptions.contains_key(lang) {
            remove_descriptions.push(lang.clone());
        }
    }
    let mut add_aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut remove_aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let langs: BTreeSet<&String> = was.aliases.keys().chain(after.aliases.keys()).collect();
    for lang in langs {
        let old: Vec<String> = was.aliases.get(lang).cloned().unwrap_or_default();
        let new: Vec<String> = after.aliases.get(lang).cloned().unwrap_or_default();
        let added: Vec<String> = new.iter().filter(|a| !old.contains(a)).cloned().collect();
        let gone: Vec<String> = old.iter().filter(|a| !new.contains(a)).cloned().collect();
        if !added.is_empty() {
            add_aliases.insert(lang.clone(), added);
        }
        if !gone.is_empty() {
            remove_aliases.insert(lang.clone(), gone);
        }
    }
    let mut add_sitelinks = BTreeMap::new();
    let mut remove_sitelinks = Vec::new();
    for (site, link) in &after.sitelinks {
        if was.sitelinks.get(site) != Some(link) {
            add_sitelinks.insert(
                site.clone(),
                SitelinkPatch {
                    title: link.title.clone(),
                    badges: link.badges.clone(),
                },
            );
        }
    }
    for site in was.sitelinks.keys() {
        if !after.sitelinks.contains_key(site) {
            remove_sitelinks.push(site.clone());
        }
    }

    let mut ops = Vec::new();
    if !(remove_statements.is_empty()
        && remove_labels.is_empty()
        && remove_descriptions.is_empty()
        && remove_aliases.is_empty()
        && remove_sitelinks.is_empty())
    {
        ops.push(Operation::Remove {
            id: Some(id.clone()),
            page: None,
            statements: remove_statements,
            labels: remove_labels,
            descriptions: remove_descriptions,
            aliases: remove_aliases,
            sitelinks: remove_sitelinks,
            references: BTreeMap::new(),
            qualifiers: BTreeMap::new(),
            link: None,
        });
    }
    for statement in suppress {
        ops.push(Operation::Override {
            id: Some(id.clone()),
            statement,
            rank: None,
            suppress: Some(true),
        });
    }
    if !(add_claims.is_empty()
        && add_labels.is_empty()
        && add_descriptions.is_empty()
        && add_aliases.is_empty()
        && add_sitelinks.is_empty())
    {
        ops.push(Operation::Add {
            id: Some(id),
            page: None,
            labels: add_labels,
            descriptions: add_descriptions,
            aliases: add_aliases,
            claims: add_claims,
            sitelinks: add_sitelinks,
            references: BTreeMap::new(),
            qualifiers: BTreeMap::new(),
            entity: None,
            match_key: None,
            via: None,
            overwrite: false,
        });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_wikibase_model::entity::EntityType;
    use serde_json::json;

    fn statement(id: Option<&str>, value: &str, reference: Option<&str>) -> Statement {
        let mut v = json!({
            "mainsnak": {
                "snaktype": "value", "property": "P2", "datatype": "wikibase-item",
                "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": value}},
            },
            "type": "statement", "rank": "normal",
        });
        if let Some(id) = id {
            v["id"] = json!(id);
        }
        if let Some(url) = reference {
            v["references"] = json!([{"snaks": {"P11": [{
                "snaktype": "value", "property": "P11", "datatype": "url",
                "datavalue": {"type": "string", "value": url},
            }]}, "snaks-order": ["P11"]}]);
        }
        serde_json::from_value(v).unwrap()
    }

    fn entity(statements: Vec<Statement>) -> Entity {
        let mut e = Entity::new(EntityId::parse("Q6").unwrap(), EntityType::Item);
        let p2 = EntityId::parse("P2").unwrap();
        e.statements.insert(p2, statements);
        e
    }

    const OLD: &str = "Q6$00000003-0000-0000-0000-000000000000";
    const NEW: &str = "Q6$3A6B906B-1E1E-5B43-B369-B881E41CA9ED";

    fn claims_added(ops: &[Operation]) -> usize {
        ops.iter()
            .map(|op| match op {
                Operation::Add { claims, .. } => claims.values().map(Vec::len).sum(),
                _ => 0,
            })
            .sum()
    }

    // 0002 §8.5: an identical statement merges onto the one there, so repeating a
    // statement the local graph holds is no difference and writes nothing.
    #[test]
    fn a_repeated_statement_is_no_difference() {
        let was = entity(vec![statement(Some(OLD), "Q1", None)]);
        let local = [StatementId::parse(OLD).unwrap()];
        let again = entity(vec![
            statement(Some(OLD), "Q1", None),
            statement(Some(NEW), "Q1", None),
        ]);
        assert!(operations_from(&was, &local, &again).is_empty());
        // Without a GUID of its own, the same.
        let bare = entity(vec![
            statement(Some(OLD), "Q1", None),
            statement(None, "Q1", None),
        ]);
        assert!(operations_from(&was, &local, &bare).is_empty());
    }

    #[test]
    fn a_statement_that_brings_something_is_added() {
        let was = entity(vec![statement(Some(OLD), "Q1", None)]);
        let local = [StatementId::parse(OLD).unwrap()];
        // Another value.
        let other = entity(vec![
            statement(Some(OLD), "Q1", None),
            statement(Some(NEW), "Q2", None),
        ]);
        assert_eq!(claims_added(&operations_from(&was, &local, &other)), 1);
        // The same value with a reference the one there lacks: the add merges it on.
        let cited = entity(vec![
            statement(Some(OLD), "Q1", None),
            statement(Some(NEW), "Q1", Some("https://example.org/")),
        ]);
        assert_eq!(claims_added(&operations_from(&was, &local, &cited)), 1);
        // The same value where only a mirror asserts it: the local graph gains it.
        assert_eq!(
            claims_added(&operations_from(
                &was,
                &[],
                &entity(vec![
                    statement(Some(OLD), "Q1", None),
                    statement(Some(NEW), "Q1", None),
                ])
            )),
            1
        );
        // The one there is dropped in the same edit: the new one replaces it.
        let replaced = entity(vec![statement(Some(NEW), "Q1", None)]);
        let ops = operations_from(&was, &local, &replaced);
        assert_eq!(claims_added(&ops), 1);
        assert!(matches!(&ops[0], Operation::Remove { statements, .. } if statements.len() == 1));
    }
}
