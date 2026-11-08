//! The difference between two states of an entity as change-set operations (0002 §8.1;
//! 0006 §8): what an interactive edit submits as a batch of one, and what a revert writes
//! to take an entity back (0104 §3). A changed statement is a `remove` of its GUID and an
//! `add` of the new statement under the same GUID; a mirrored statement the editor drops
//! or changes is not the tenant's to retract, so it is suppressed with an `override`
//! (0004 §8), and a changed one re-added under a fresh local GUID. Terms, aliases and
//! sitelinks are added and removed by language and site.

use std::collections::{BTreeMap, BTreeSet};

use scatter_wikibase_changeset::{Operation, SitelinkPatch};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::StatementId;
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
                        None => add_claims
                            .entry(property.clone())
                            .or_default()
                            .push(s.clone()),
                    }
                }
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
