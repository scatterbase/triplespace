//! Enclosure (0016 §4; 0023 §2; 0039 §10; 0056 §3): which targets' ACLs apply to a target.
//!
//! Three axes:
//!
//! - **containment**: tenant ⊃ graph ⊃ namespace ⊃ page ⊃ subpage; tenant ⊃ set ⊃ member;
//!   a talk page or board ⊃ the threads whose home it is; entity ⊃ statement;
//! - **predicate**: a property ACL applies to every snak using that property;
//! - **content**: a `blob` ACL applies to every file version whose bytes hash to its value.
//!
//! There is no other inheritance and nothing loosens. The *facts* (which namespace, which
//! parent pages, which sets, which property, which takedowns match the hash) come from the
//! projection's tables; [`chain`] turns them into the list of target keys whose current
//! ACLs the evaluator needs, which is the one indexed lookup 0023 §2 describes. The result
//! is what [`crate::acl::check`], [`crate::acl::visibility`] and
//! [`crate::acl::read_decision`] take, once the keys have been resolved to records.

use crate::acl::Target;

/// What encloses a target, as the projection knows it. Every field is optional because
/// not every kind of target has every kind of container; what is given is used.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Enclosure {
    /// The tenant's slug. Always the root.
    pub tenant: Option<String>,
    /// The graph the record is in (`local`, `pages`, `mirror/wikidata`, …).
    pub graph: Option<String>,
    /// The namespace of the page or entity.
    pub namespace: Option<i64>,
    /// The page's ancestors, nearest first, where the namespace has subpages; for a thread,
    /// its home talk page or board (0019 §12, 0049 §7).
    pub parent_pages: Vec<u64>,
    /// The sets the target, or any page enclosing it, is a member of.
    pub sets: Vec<u64>,
    /// For a statement: its entity.
    pub entity: Option<String>,
    /// For a snak: its property (the predicate axis).
    pub property: Option<String>,
    /// For a file version: the takedowns whose hash matches it (the content axis).
    pub takedowns: Vec<u64>,
}

/// The targets whose ACLs apply to `target`: the target itself, then its enclosures,
/// nearest first, each once.
#[must_use]
pub fn chain(target: &Target, enclosure: &Enclosure) -> Vec<Target> {
    let mut out = vec![target.clone()];
    let mut push = |t: Target| {
        if !out.contains(&t) {
            out.push(t);
        }
    };
    if let Some(e) = &enclosure.entity {
        push(Target::Entity(e.clone()));
    }
    if let Some(p) = &enclosure.property {
        push(Target::Property(p.clone()));
    }
    for page in &enclosure.parent_pages {
        push(Target::Page(*page));
    }
    for set in &enclosure.sets {
        push(Target::Set(*set));
    }
    if let Some(n) = enclosure.namespace {
        push(Target::Namespace(n));
    }
    if let Some(g) = &enclosure.graph {
        push(Target::Graph(g.clone()));
    }
    for takedown in &enclosure.takedowns {
        push(Target::Blob(*takedown));
    }
    if let Some(t) = &enclosure.tenant {
        push(Target::Tenant(t.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subpage_in_a_set_in_a_namespace() {
        let e = Enclosure {
            tenant: Some("t".into()),
            graph: Some("pages".into()),
            namespace: Some(2),
            parent_pages: vec![7, 3],
            sets: vec![8],
            ..Enclosure::default()
        };
        let keys: Vec<String> = chain(&Target::Page(9), &e)
            .iter()
            .map(Target::key)
            .collect();
        assert_eq!(
            keys,
            [
                "acl:page:9",
                "acl:page:7",
                "acl:page:3",
                "acl:set:8",
                "acl:namespace:2",
                "acl:graph:pages",
                "acl:tenant:t"
            ]
        );
    }

    #[test]
    fn a_statement_has_its_entity_and_its_property() {
        let e = Enclosure {
            tenant: Some("t".into()),
            graph: Some("local".into()),
            namespace: Some(120),
            entity: Some("Q8".into()),
            property: Some("P31".into()),
            ..Enclosure::default()
        };
        let keys: Vec<String> = chain(&Target::Statement("Q8$x".into()), &e)
            .iter()
            .map(Target::key)
            .collect();
        assert_eq!(
            keys,
            [
                "acl:statement:Q8$x",
                "acl:entity:Q8",
                "acl:property:P31",
                "acl:namespace:120",
                "acl:graph:local",
                "acl:tenant:t"
            ]
        );
    }

    #[test]
    fn a_file_version_under_a_takedown() {
        let e = Enclosure {
            tenant: Some("t".into()),
            namespace: Some(6),
            takedowns: vec![123],
            ..Enclosure::default()
        };
        let keys: Vec<String> = chain(&Target::Page(5), &e)
            .iter()
            .map(Target::key)
            .collect();
        assert_eq!(
            keys,
            [
                "acl:page:5",
                "acl:namespace:6",
                "acl:blob:123",
                "acl:tenant:t"
            ]
        );
        // The target itself is never repeated.
        let e = Enclosure {
            sets: vec![1],
            ..Enclosure::default()
        };
        assert_eq!(chain(&Target::Set(1), &e).len(), 1);
    }
}
