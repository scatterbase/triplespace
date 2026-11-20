//! The references of an entity and what changed among them between two states (0005 §2,
//! A128): the question a citation history asks of an entity's revisions, "when did this
//! reference arrive on this statement, and when did it leave".
//!
//! A reference is identified by where it sits and what it says:
//!
//! - **the statement**, by its GUID with the subject canonical, so that a legacy
//!   lowercase GUID and its uppercase form are one statement; a statement with no GUID is
//!   named by its property and main snak;
//! - **the content**, by Wikibase's reference hash recomputed from the snaks
//!   ([`Hasher::reference`]), never the hash the source wrote: the same snaks hash the
//!   same in every revision, whatever the software that wrote the revision put there.
//!
//! Moving a reference within a statement, or reordering its snaks, is therefore no
//! change; editing one snak of it is a removal and an addition; a statement removed takes
//! its references with it.
//!
//! [`ReferenceUses::of`] reads one state. A consumer folding over a page's revisions
//! keeps only the previous state's [`ReferenceUses::keys`] and asks each new state what
//! was [`ReferenceUses::added_since`] and [`removed_since`](ReferenceUses::removed_since)
//! them, so no earlier entity is held. [`ReferenceDiff::between`] is the same for two
//! entities in hand.
//!
//! Pure: entities in, references out.

use std::collections::{BTreeMap, BTreeSet};

use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Reference, Statement};

/// Where a reference sits and what it says.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReferenceKey {
    /// The statement: its GUID with the subject canonical, or `{property}:{main snak
    /// hash}` for a statement that has none.
    pub statement: String,
    /// Wikibase's reference hash, recomputed from the snaks.
    pub hash: String,
}

/// One reference on one statement of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceUse<'a> {
    /// The property of the statement's main snak.
    pub property: &'a EntityId,
    /// The reference.
    pub reference: &'a Reference,
}

/// Every reference of one state of an entity, by key.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReferenceUses<'a> {
    uses: BTreeMap<ReferenceKey, ReferenceUse<'a>>,
}

fn statement_key(statement: &Statement, hasher: &Hasher) -> String {
    match &statement.id {
        Some(id) => id.canonical_str().to_string(),
        None => format!(
            "{}:{}",
            statement.property().as_str(),
            hasher.snak(&statement.mainsnak)
        ),
    }
}

impl<'a> ReferenceUses<'a> {
    /// The references of an entity. `hasher` is the one that suits the entity's IDs:
    /// [`Hasher::local`] for an entity as its source wrote it.
    #[must_use]
    pub fn of(entity: &'a Entity, hasher: &Hasher) -> Self {
        let mut uses = BTreeMap::new();
        for statement in entity.all_statements() {
            if statement.references.is_empty() {
                continue;
            }
            let key = statement_key(statement, hasher);
            for reference in &statement.references {
                uses.insert(
                    ReferenceKey {
                        statement: key.clone(),
                        hash: hasher.reference(reference),
                    },
                    ReferenceUse {
                        property: statement.property(),
                        reference,
                    },
                );
            }
        }
        Self { uses }
    }

    /// How many references the state has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.uses.len()
    }

    /// Whether the state has no reference.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.uses.is_empty()
    }

    /// Every reference with its key, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&ReferenceKey, &ReferenceUse<'a>)> {
        self.uses.iter()
    }

    /// The keys alone: all a fold needs to keep of a state.
    #[must_use]
    pub fn keys(&self) -> BTreeSet<ReferenceKey> {
        self.uses.keys().cloned().collect()
    }

    /// The references this state has that `before` did not.
    pub fn added_since<'s>(
        &'s self,
        before: &'s BTreeSet<ReferenceKey>,
    ) -> impl Iterator<Item = (&'s ReferenceKey, &'s ReferenceUse<'a>)> {
        self.uses.iter().filter(|(k, _)| !before.contains(*k))
    }

    /// The keys `before` had that this state does not.
    pub fn removed_since<'s>(
        &'s self,
        before: &'s BTreeSet<ReferenceKey>,
    ) -> impl Iterator<Item = &'s ReferenceKey> {
        before.iter().filter(|k| !self.uses.contains_key(*k))
    }
}

/// What changed among an entity's references between two states.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReferenceDiff<'a> {
    /// Present after only, in key order.
    pub added: Vec<(ReferenceKey, ReferenceUse<'a>)>,
    /// Present before only, in key order.
    pub removed: Vec<(ReferenceKey, ReferenceUse<'a>)>,
}

impl<'a> ReferenceDiff<'a> {
    /// The difference between two states; an absent side is an entity with no reference
    /// (not yet created, deleted, or turned into a redirect).
    #[must_use]
    pub fn between(old: Option<&'a Entity>, new: Option<&'a Entity>, hasher: &Hasher) -> Self {
        let old = old
            .map(|e| ReferenceUses::of(e, hasher))
            .unwrap_or_default();
        let new = new
            .map(|e| ReferenceUses::of(e, hasher))
            .unwrap_or_default();
        let added = new
            .uses
            .iter()
            .filter(|(k, _)| !old.uses.contains_key(*k))
            .map(|(k, u)| (k.clone(), *u))
            .collect();
        let removed = old
            .uses
            .iter()
            .filter(|(k, _)| !new.uses.contains_key(*k))
            .map(|(k, u)| (k.clone(), *u))
            .collect();
        Self { added, removed }
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(claims: &str) -> Entity {
        let json = format!(
            r#"{{"type":"item","id":"Q6","labels":{{}},"descriptions":{{}},"aliases":{{}},"claims":{claims},"sitelinks":{{}}}}"#
        );
        Entity::from_json(&json).expect("entity").entity
    }

    fn url(u: &str) -> String {
        format!(
            r#"{{"snaks":{{"P854":[{{"snaktype":"value","property":"P854","datavalue":{{"value":"{u}","type":"string"}}}}]}},"snaks-order":["P854"]}}"#
        )
    }

    fn stated_in_then_url(q: u64, u: &str, url_first: bool) -> String {
        let p248 = format!(
            r#""P248":[{{"snaktype":"value","property":"P248","datavalue":{{"value":{{"entity-type":"item","numeric-id":{q},"id":"Q{q}"}},"type":"wikibase-entityid"}}}}]"#
        );
        let p854 = format!(
            r#""P854":[{{"snaktype":"value","property":"P854","datavalue":{{"value":"{u}","type":"string"}}}}]"#
        );
        if url_first {
            format!(r#"{{"snaks":{{{p854},{p248}}},"snaks-order":["P854","P248"]}}"#)
        } else {
            format!(r#"{{"snaks":{{{p248},{p854}}},"snaks-order":["P248","P854"]}}"#)
        }
    }

    fn statement(guid: &str, property: &str, refs: &[String]) -> String {
        format!(
            r#"{{"mainsnak":{{"snaktype":"value","property":"{property}","datavalue":{{"value":"x","type":"string"}}}},"type":"statement","id":"{guid}","rank":"normal","references":[{}]}}"#,
            refs.join(",")
        )
    }

    fn claims(property: &str, statements: &[String]) -> String {
        format!(r#"{{"{property}":[{}]}}"#, statements.join(","))
    }

    const A: &str = "Q6$11111111-1111-1111-1111-111111111111";
    const B: &str = "Q6$22222222-2222-2222-2222-222222222222";

    // 0005 A128: a reference is added, kept, and removed with its statement.
    #[test]
    fn additions_and_removals_between_states() {
        let h = Hasher::local();
        let first = entity(&claims(
            "P1",
            &[statement(A, "P1", &[url("https://a.example/")])],
        ));
        let second = entity(&claims(
            "P1",
            &[
                statement(
                    A,
                    "P1",
                    &[url("https://a.example/"), url("https://b.example/")],
                ),
                statement(B, "P1", &[url("https://a.example/")]),
            ],
        ));
        let d = ReferenceDiff::between(Some(&first), Some(&second), &h);
        assert!(d.removed.is_empty());
        // The second URL on A, and the first URL again on B: the same content elsewhere
        // is another use.
        assert_eq!(d.added.len(), 2);
        assert_eq!(
            d.added
                .iter()
                .filter(|(k, _)| k.statement.ends_with("1111"))
                .count(),
            1
        );
        assert!(d.added.iter().all(|(_, u)| u.property.as_str() == "P1"));

        let third = entity(&claims(
            "P1",
            &[statement(B, "P1", &[url("https://a.example/")])],
        ));
        let d = ReferenceDiff::between(Some(&second), Some(&third), &h);
        assert!(d.added.is_empty());
        assert_eq!(d.removed.len(), 2, "A's references leave with A");

        // A side that is absent has no reference.
        assert_eq!(
            ReferenceDiff::between(Some(&third), None, &h).removed.len(),
            1
        );
        assert_eq!(
            ReferenceDiff::between(None, Some(&third), &h).added.len(),
            1
        );
        assert!(ReferenceDiff::between(Some(&third), Some(&third), &h).is_empty());
    }

    // 0005 A128: identity is the statement (case-blind) and the content, not the order,
    // and not the hash the source wrote.
    #[test]
    fn order_guid_case_and_written_hashes_do_not_matter() {
        let h = Hasher::local();
        let one = entity(&claims(
            "P1",
            &[statement(
                A,
                "P1",
                &[
                    stated_in_then_url(5, "https://a.example/", false),
                    url("https://b.example/"),
                ],
            )],
        ));
        // Snaks reordered within a reference, the references swapped, the GUID's subject
        // in lowercase, and a hash the source wrote that is not the content's.
        let swapped = stated_in_then_url(5, "https://a.example/", true).replacen(
            r#"{"snaks""#,
            r#"{"hash":"0000000000000000000000000000000000000000","snaks""#,
            1,
        );
        let two = entity(&claims(
            "P1",
            &[statement(
                &A.to_lowercase(),
                "P1",
                &[url("https://b.example/"), swapped],
            )],
        ));
        assert!(ReferenceDiff::between(Some(&one), Some(&two), &h).is_empty());

        // One snak edited: the old reference leaves and a new one arrives.
        let three = entity(&claims(
            "P1",
            &[statement(
                A,
                "P1",
                &[
                    stated_in_then_url(5, "https://a.example/changed", false),
                    url("https://b.example/"),
                ],
            )],
        ));
        let d = ReferenceDiff::between(Some(&one), Some(&three), &h);
        assert_eq!((d.added.len(), d.removed.len()), (1, 1));
        assert_eq!(d.added[0].0.statement, d.removed[0].0.statement);
        assert_ne!(d.added[0].0.hash, d.removed[0].0.hash);
    }

    // 0005 A128: the fold keeps keys only.
    #[test]
    fn fold_over_keys_matches_the_pairwise_diff() {
        let h = Hasher::local();
        let states = [
            entity(&claims("P1", &[statement(A, "P1", &[])])),
            entity(&claims(
                "P1",
                &[statement(A, "P1", &[url("https://a.example/")])],
            )),
            entity(&claims(
                "P1",
                &[statement(A, "P1", &[url("https://b.example/")])],
            )),
        ];
        let mut before = BTreeSet::new();
        let mut log = Vec::new();
        for (i, state) in states.iter().enumerate() {
            let uses = ReferenceUses::of(state, &h);
            log.push((
                i,
                uses.added_since(&before).count(),
                uses.removed_since(&before).count(),
            ));
            before = uses.keys();
        }
        assert_eq!(log, [(0, 0, 0), (1, 1, 0), (2, 1, 1)]);
        assert_eq!(before.len(), 1);
    }
}
