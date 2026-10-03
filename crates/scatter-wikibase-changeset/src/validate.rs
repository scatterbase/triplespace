//! Eager validation, the first tier of 0002 §8.6: the shape of each operation, where it
//! may be written, and property data types against the property type map. References
//! between entities are the second tier and are checked later, because dumps contain
//! forward references; nothing here looks anything up but the type map.
//!
//! [`validate`] returns every problem it finds rather than the first, so a rejects file
//! can say all that was wrong with a line.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use scatter_providers::Registry;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::{EntityId, IdForm, StatementId, Subject};
use scatter_wikibase_model::statement::{Snak, SnakKind, Statement};
use scatter_wikibase_model::value::{DataType, DataValue};

use crate::job::{Graph, JobHeader, Mode};
use crate::op::{Link, Operation, Upstream};
use crate::rank::ProviderOrder;

/// The property type map (0002 §8.6): what a property's data type is.
pub trait PropertyTypes {
    /// The data type of a property, if known.
    fn datatype(&self, property: &EntityId) -> Option<&DataType>;
}

impl PropertyTypes for BTreeMap<EntityId, DataType> {
    fn datatype(&self, property: &EntityId) -> Option<&DataType> {
        self.get(property)
    }
}

impl<S: std::hash::BuildHasher> PropertyTypes for HashMap<EntityId, DataType, S> {
    fn datatype(&self, property: &EntityId) -> Option<&DataType> {
        self.get(property)
    }
}

/// Knows no property; every snak is checked against its own `datatype` only.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProperties;

impl PropertyTypes for NoProperties {
    fn datatype(&self, _: &EntityId) -> Option<&DataType> {
        None
    }
}

/// What the validator knows about the batch.
pub struct Context<'a> {
    /// The partition written.
    pub graph: Graph,
    /// The job's mode.
    pub mode: Option<Mode>,
    /// The provider order, for link keys.
    pub order: &'a ProviderOrder,
    /// The provider registry, for mirror graphs.
    pub providers: &'a Registry,
    /// The property type map.
    pub types: &'a dyn PropertyTypes,
    /// Reject a statement whose property the map does not know. Off for a dump, whose
    /// properties may arrive after the statements that use them.
    pub strict_properties: bool,
}

impl<'a> Context<'a> {
    /// A context for a job line, with the default order and registry. `None` when the
    /// job's graph name is not a graph.
    #[must_use]
    pub fn for_job(
        job: &JobHeader,
        order: &'a ProviderOrder,
        types: &'a dyn PropertyTypes,
    ) -> Option<Self> {
        Some(Self {
            graph: job.graph()?,
            mode: job.mode,
            order,
            providers: Registry::default_registry(),
            types,
            strict_properties: false,
        })
    }

    /// The provider code of a mirror graph.
    fn mirror_code(&self) -> Option<&str> {
        self.providers
            .by_slug(self.graph.provider_slug()?)
            .map(|p| p.code.as_str())
    }
}

/// One thing wrong with an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// The operation is not written to this graph.
    WrongGraph {
        /// The `op`.
        op: &'static str,
        /// The graph.
        graph: String,
    },
    /// `adopt` outside an adoption job, or anything else inside one.
    WrongMode {
        /// The `op`.
        op: &'static str,
    },
    /// The graph names a provider the registry does not have.
    UnknownProvider(String),
    /// An `add` or `remove` without exactly one of `id` and `page`.
    Subject,
    /// A page's change set carries something other than statements (0038 §1).
    PageTerms,
    /// A `create` with a `ref`, an `add` carrying `entity` without `overwrite`, a
    /// `retain` naming no entity or both ways, or an `override` or link without its
    /// key: a shape that has to be resolved before it is a record.
    WireShape,
    /// An `add` or `remove` that does nothing.
    Empty,
    /// `create` or `create-or-add` of a keyed entity, which exists by its key (0009 §4).
    KeyedCreate(EntityId),
    /// `tombstone`, `redirect` or `convert` of a keyed entity (0009 §10).
    KeyedOperation(EntityId),
    /// The `id` and the `entity`'s `id` differ.
    IdMismatch {
        /// The operation's.
        id: EntityId,
        /// The entity's.
        entity: EntityId,
    },
    /// An `entity` whose type its ID does not imply.
    TypeMismatch {
        /// The ID.
        id: EntityId,
        /// The entity's `type`.
        entity_type: String,
    },
    /// An ID that has to be in local form and is not (0035 §3; `convert.local`).
    NotLocal(EntityId),
    /// An ID that has to be foreign and is not.
    NotForeign(EntityId),
    /// A mirror operation naming an entity of another provider.
    WrongProvider {
        /// The ID.
        id: EntityId,
        /// The graph's provider code.
        code: String,
    },
    /// A `put` or `tombstone` with no upstream version.
    NoUpstream,
    /// An `override` without exactly one of `rank` and `suppress: true`.
    OverrideShape,
    /// A statement under a subject it does not belong to.
    StatementSubject {
        /// The statement.
        statement: StatementId,
        /// The operation's subject.
        subject: String,
    },
    /// A statement under a property other than its main snak's.
    PropertyGroup {
        /// The group's property.
        group: EntityId,
        /// The main snak's.
        property: EntityId,
    },
    /// A link without exactly two distinct members, or whose `id` is not a member.
    LinkShape,
    /// A link's `id` is not its highest-ranked member.
    LinkKey {
        /// The `id` given.
        id: EntityId,
        /// The highest-ranked member.
        highest: EntityId,
    },
    /// Two members of one namespace, or two keyed members (0004 §9; 0009 §10).
    LinkNamespace,
    /// Members of the wrong or differing entity types for the link.
    LinkTypes,
    /// A redirect to itself, across namespaces for items, or between providers.
    RedirectShape,
    /// A snak's `datatype` differs from the property's.
    Datatype {
        /// The property.
        property: EntityId,
        /// The map's.
        expected: DataType,
        /// The snak's.
        found: DataType,
    },
    /// A value of the wrong value type for the property's data type.
    ValueType {
        /// The property.
        property: EntityId,
        /// The data type.
        datatype: DataType,
        /// The value's `type`.
        value_type: String,
    },
    /// An entity value whose `entity-type` the data type does not take.
    EntityType {
        /// The property.
        property: EntityId,
        /// The data type.
        datatype: DataType,
        /// The value's `entity-type`.
        entity_type: String,
    },
    /// A property the map does not know, under `strict_properties`.
    UnknownProperty(EntityId),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongGraph { op, graph } => write!(f, "`{op}` is not written to `{graph}`"),
            Self::WrongMode { op } => write!(f, "`{op}` does not belong in a job of this mode"),
            Self::UnknownProvider(slug) => write!(f, "no provider `{slug}`"),
            Self::Subject => f.write_str("exactly one of `id` and `page` names the subject"),
            Self::PageTerms => f.write_str("a page's change set carries statements only"),
            Self::WireShape => f.write_str("a wire-only shape: resolve it before validating"),
            Self::Empty => f.write_str("the operation does nothing"),
            Self::KeyedCreate(id) => write!(f, "`{id}` exists by its key; write it with `add`"),
            Self::KeyedOperation(id) => write!(f, "the operation does not apply to a keyed entity (`{id}`)"),
            Self::IdMismatch { id, entity } => write!(f, "`id` is `{id}` but the entity's is `{entity}`"),
            Self::TypeMismatch { id, entity_type } => write!(f, "`{id}` is not a {entity_type}"),
            Self::NotLocal(id) => write!(f, "`{id}` is not in local form"),
            Self::NotForeign(id) => write!(f, "`{id}` is not a foreign ID"),
            Self::WrongProvider { id, code } => write!(f, "`{id}` is not a `{code}` entity"),
            Self::NoUpstream => f.write_str("`upstream` names no version"),
            Self::OverrideShape => f.write_str("`override` carries exactly one of `rank` and `suppress: true`"),
            Self::StatementSubject { statement, subject } => {
                write!(f, "statement `{statement}` is not about `{subject}`")
            }
            Self::PropertyGroup { group, property } => {
                write!(f, "a statement under `{group}` has main snak property `{property}`")
            }
            Self::LinkShape => f.write_str("a link has exactly two distinct members and `id` is one of them"),
            Self::LinkKey { id, highest } => {
                write!(f, "`id` is `{id}` but the highest-ranked member is `{highest}`")
            }
            Self::LinkNamespace => f.write_str("a link joins two namespaces, never two keyed entities"),
            Self::LinkTypes => f.write_str("the members' entity types do not fit the link"),
            Self::RedirectShape => f.write_str("a redirect stays within one namespace (properties may cross) and never points at itself"),
            Self::Datatype { property, expected, found } => {
                write!(f, "`{property}` is `{expected}`, the snak says `{found}`")
            }
            Self::ValueType { property, datatype, value_type } => {
                write!(f, "`{property}` is `{datatype}`, which does not take a `{value_type}` value")
            }
            Self::EntityType { property, datatype, entity_type } => {
                write!(f, "`{property}` is `{datatype}`, which does not take a {entity_type}")
            }
            Self::UnknownProperty(p) => write!(f, "unknown property `{p}`"),
        }
    }
}

impl std::error::Error for Problem {}

/// Validates one operation. Empty means it passed.
#[must_use]
pub fn validate(op: &Operation, cx: &Context<'_>) -> Vec<Problem> {
    let mut out = Vec::new();
    graph_and_mode(op, cx, &mut out);
    shape(op, cx, &mut out);
    for s in op.statements() {
        statement(s, cx, &mut out);
    }
    if let Operation::Add {
        qualifiers,
        references,
        ..
    } = op
    {
        for snaks in qualifiers.values() {
            for (group, ss) in snaks {
                for s in ss {
                    snak(s, Some(group), cx, &mut out);
                }
            }
        }
        for refs in references.values() {
            for r in refs {
                for (group, ss) in &r.snaks {
                    for s in ss {
                        snak(s, Some(group), cx, &mut out);
                    }
                }
            }
        }
    }
    out
}

/// Validates a whole batch: the job line, then each operation, which is completed
/// first ([`Operation::complete`]). Problems are returned with the operation's index.
#[must_use]
pub fn validate_all(ops: &mut [Operation], cx: &Context<'_>) -> Vec<(usize, Problem)> {
    let mut out = Vec::new();
    if let Some(slug) = cx.graph.provider_slug()
        && cx.providers.by_slug(slug).is_none()
    {
        out.push((0, Problem::UnknownProvider(slug.to_string())));
    }
    for (i, op) in ops.iter_mut().enumerate() {
        op.complete(cx.order);
        out.extend(validate(op, cx).into_iter().map(|p| (i, p)));
    }
    out
}

fn graph_and_mode(op: &Operation, cx: &Context<'_>, out: &mut Vec<Problem>) {
    let allowed = match (&cx.graph, op) {
        (Graph::Mirror(_), Operation::Put { .. } | Operation::Tombstone { .. }) => true,
        (Graph::Mirror(_), Operation::Redirect { upstream, .. }) => upstream.is_some(),
        (Graph::Pages, Operation::Add { page, .. } | Operation::Remove { page, .. }) => {
            page.is_some()
        }
        (Graph::Local, Operation::Redirect { upstream, .. }) => upstream.is_none(),
        (Graph::Local, Operation::Add { page, .. } | Operation::Remove { page, .. }) => {
            page.is_none()
        }
        (Graph::Local, Operation::Put { .. } | Operation::Tombstone { .. })
        | (Graph::Mirror(_) | Graph::Pages, _) => false,
        (Graph::Local, _) => true,
    };
    if !allowed {
        out.push(Problem::WrongGraph {
            op: op.name(),
            graph: cx.graph.name(),
        });
    }
    let adopt = matches!(op, Operation::Adopt { .. });
    if adopt != (cx.mode == Some(Mode::Adopt)) {
        out.push(Problem::WrongMode { op: op.name() });
    }
}

fn check_statement_subject(id: &StatementId, op: &Operation, out: &mut Vec<Problem>) {
    if let Some(subject) = op.subject()
        && id.subject() != subject
    {
        out.push(Problem::StatementSubject {
            statement: id.clone(),
            subject: subject.to_string(),
        });
    }
}

fn check_entity(id: &EntityId, entity: &Entity, out: &mut Vec<Problem>) {
    if entity.id != *id {
        out.push(Problem::IdMismatch {
            id: id.clone(),
            entity: entity.id.clone(),
        });
    }
    if let Some(implied) = id.implied_entity_type()
        && implied != entity.entity_type.name()
    {
        out.push(Problem::TypeMismatch {
            id: id.clone(),
            entity_type: entity.entity_type.name().to_string(),
        });
    }
    for s in entity.all_statements() {
        if let Some(sid) = &s.id
            && sid.subject() != Subject::Entity(id.clone())
        {
            out.push(Problem::StatementSubject {
                statement: sid.clone(),
                subject: id.to_string(),
            });
        }
    }
    for (group, ss) in &entity.statements {
        for s in ss {
            if s.mainsnak.property != *group {
                out.push(Problem::PropertyGroup {
                    group: group.clone(),
                    property: s.mainsnak.property.clone(),
                });
            }
        }
    }
}

fn check_mirror_id(id: &EntityId, cx: &Context<'_>, keyed_ok: bool, out: &mut Vec<Problem>) {
    match id.form() {
        IdForm::Keyed if keyed_ok => {}
        IdForm::Keyed => out.push(Problem::KeyedOperation(id.clone())),
        IdForm::Local => out.push(Problem::NotForeign(id.clone())),
        IdForm::Foreign => {
            if let Some(code) = cx.mirror_code()
                && !id.as_str().starts_with(code)
            {
                out.push(Problem::WrongProvider {
                    id: id.clone(),
                    code: code.to_string(),
                });
            }
        }
    }
}

fn check_upstream(upstream: &Upstream, out: &mut Vec<Problem>) {
    if upstream.is_empty() {
        out.push(Problem::NoUpstream);
    }
}

/// The namespace an ID is in, for 0004 §9: local, keyed, or a provider.
fn namespace(id: &EntityId) -> &str {
    match id.form() {
        IdForm::Local => "",
        IdForm::Keyed => id.keyed_parts().map_or("", |(t, _)| t),
        IdForm::Foreign => &id.as_str()[..2],
    }
}

fn check_link(
    id: Option<&EntityId>,
    ids: &[EntityId],
    property: bool,
    cx: &Context<'_>,
    out: &mut Vec<Problem>,
) {
    if ids.len() != 2 || ids[0] == ids[1] || id.is_some_and(|i| !ids.contains(i)) {
        out.push(Problem::LinkShape);
        return;
    }
    if let (Some(id), Some(highest)) = (id, cx.order.highest(ids))
        && id != highest
    {
        out.push(Problem::LinkKey {
            id: id.clone(),
            highest: highest.clone(),
        });
    }
    let (a, b) = (&ids[0], &ids[1]);
    if (a.form() == IdForm::Keyed && b.form() == IdForm::Keyed) || namespace(a) == namespace(b) {
        out.push(Problem::LinkNamespace);
    }
    let types_ok = if property {
        a.type_code() == Some('P') && b.type_code() == Some('P')
    } else {
        // Items link to items, or to a keyed entity (0009 §8); a property never links here.
        let item_or_keyed =
            |x: &EntityId| x.form() == IdForm::Keyed || x.implied_entity_type() != Some("property");
        item_or_keyed(a) && item_or_keyed(b)
    };
    if !types_ok {
        out.push(Problem::LinkTypes);
    }
}

fn shape(op: &Operation, cx: &Context<'_>, out: &mut Vec<Problem>) {
    match op {
        Operation::Put {
            id,
            entity,
            upstream,
            ..
        } => {
            check_mirror_id(id, cx, true, out);
            check_upstream(upstream, out);
            check_entity(id, entity, out);
        }
        Operation::Tombstone { id, upstream } => {
            check_mirror_id(id, cx, false, out);
            check_upstream(upstream, out);
        }
        Operation::Redirect { from, to, upstream } => {
            shape_redirect(from, to, upstream.as_ref(), cx, out);
        }
        Operation::Create { .. } | Operation::CreateOrAdd { .. } | Operation::Adopt { .. } => {
            shape_create(op, out);
        }
        Operation::Add { .. } => shape_add(op, out),
        Operation::Remove { .. } => shape_remove(op, cx, out),
        Operation::Override {
            id,
            statement,
            rank,
            suppress,
        } => {
            if rank.is_some() == suppress.is_some() || *suppress == Some(false) {
                out.push(Problem::OverrideShape);
            }
            match id {
                Some(id) if statement.subject() != Subject::Entity(id.clone()) => {
                    out.push(Problem::StatementSubject {
                        statement: statement.clone(),
                        subject: id.to_string(),
                    });
                }
                Some(_) => {}
                None => out.push(Problem::WireShape),
            }
        }
        Operation::Retain { id, ids, .. } => {
            // One entity in a record, several on the wire ([`Operation::fan_out`]).
            if id.is_none() == ids.is_empty() {
                out.push(Problem::WireShape);
            }
        }
        Operation::Convert { id, local } => {
            match id.form() {
                IdForm::Foreign => {}
                IdForm::Keyed => out.push(Problem::KeyedOperation(id.clone())),
                IdForm::Local => out.push(Problem::NotForeign(id.clone())),
            }
            if local.form() != IdForm::Local {
                out.push(Problem::NotLocal(local.clone()));
            }
            if id.implied_entity_type().is_some()
                && id.implied_entity_type() != local.implied_entity_type()
            {
                out.push(Problem::TypeMismatch {
                    id: local.clone(),
                    entity_type: id.implied_entity_type().unwrap_or_default().to_string(),
                });
            }
        }
        Operation::SameAs { id, ids } | Operation::DifferentFrom { id, ids } => {
            check_link(id.as_ref(), ids, false, cx, out);
            if id.is_none() {
                out.push(Problem::WireShape);
            }
        }
        Operation::EquivalentProperty { id, ids } => {
            check_link(id.as_ref(), ids, true, cx, out);
            if id.is_none() {
                out.push(Problem::WireShape);
            }
        }
    }
}

fn shape_redirect(
    from: &EntityId,
    to: &EntityId,
    upstream: Option<&Upstream>,
    cx: &Context<'_>,
    out: &mut Vec<Problem>,
) {
    if from == to {
        out.push(Problem::RedirectShape);
    }
    if from.form() == IdForm::Keyed || to.form() == IdForm::Keyed {
        out.push(Problem::KeyedOperation(if from.form() == IdForm::Keyed {
            from.clone()
        } else {
            to.clone()
        }));
    } else if upstream.is_some() {
        check_mirror_id(from, cx, false, out);
        check_mirror_id(to, cx, false, out);
    } else {
        let same_type = from.type_code() == to.type_code();
        let property = from.type_code() == Some('P');
        let same_namespace = namespace(from) == namespace(to);
        // Within one namespace, same type; properties only may cross (0004 §9), retiring
        // a local property for a mirrored one.
        let ok = same_type
            && (same_namespace
                || (property && from.form() == IdForm::Local && to.form() == IdForm::Foreign));
        if !ok {
            out.push(Problem::RedirectShape);
        }
    }
}

fn shape_create(op: &Operation, out: &mut Vec<Problem>) {
    match op {
        Operation::Create {
            id,
            temporary_ref,
            entity,
            ..
        } => {
            if entity.entity_type.is_keyed() || entity.id.form() == IdForm::Keyed {
                out.push(Problem::KeyedCreate(entity.id.clone()));
            }
            match id {
                Some(id) if temporary_ref.is_none() => {
                    if id.form() != IdForm::Local {
                        out.push(Problem::NotLocal(id.clone()));
                    }
                    check_entity(id, entity, out);
                }
                _ => out.push(Problem::WireShape),
            }
        }
        Operation::CreateOrAdd { entity, .. } => {
            if entity.entity_type.is_keyed() || entity.id.form() == IdForm::Keyed {
                out.push(Problem::KeyedCreate(entity.id.clone()));
            }
            check_entity(&entity.id, entity, out);
        }
        Operation::Adopt { id, entity, .. } => {
            if id.form() != IdForm::Local {
                out.push(Problem::NotLocal(id.clone()));
            }
            check_entity(id, entity, out);
        }
        _ => {}
    }
}

fn shape_add(op: &Operation, out: &mut Vec<Problem>) {
    let Operation::Add {
        id,
        page,
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
    if id.is_some() == page.is_some() {
        out.push(Problem::Subject);
        return;
    }
    let terms = !(labels.is_empty()
        && descriptions.is_empty()
        && aliases.is_empty()
        && sitelinks.is_empty());
    if page.is_some() && terms {
        out.push(Problem::PageTerms);
    }
    if !terms
        && claims.is_empty()
        && references.is_empty()
        && qualifiers.is_empty()
        && entity.is_none()
    {
        out.push(Problem::Empty);
    }
    if let Some(e) = entity {
        if !*overwrite {
            out.push(Problem::WireShape);
        }
        if let Some(id) = id {
            check_entity(id, e, out);
        }
    }
    for (group, ss) in claims {
        for s in ss {
            if s.mainsnak.property != *group {
                out.push(Problem::PropertyGroup {
                    group: group.clone(),
                    property: s.mainsnak.property.clone(),
                });
            }
            if let Some(sid) = &s.id {
                check_statement_subject(sid, op, out);
            }
        }
    }
    for sid in references.keys().chain(qualifiers.keys()) {
        check_statement_subject(sid, op, out);
    }
}

fn shape_remove(op: &Operation, cx: &Context<'_>, out: &mut Vec<Problem>) {
    let Operation::Remove {
        id,
        page,
        statements,
        labels,
        descriptions,
        aliases,
        sitelinks,
        references,
        qualifiers,
        link,
    } = op
    else {
        return;
    };
    if id.is_some() == page.is_some() {
        out.push(Problem::Subject);
        return;
    }
    let terms = !(labels.is_empty()
        && descriptions.is_empty()
        && aliases.is_empty()
        && sitelinks.is_empty()
        && link.is_none());
    if page.is_some() && terms {
        out.push(Problem::PageTerms);
    }
    if !terms && statements.is_empty() && references.is_empty() && qualifiers.is_empty() {
        out.push(Problem::Empty);
    }
    for sid in statements
        .iter()
        .chain(references.keys())
        .chain(qualifiers.keys())
    {
        check_statement_subject(sid, op, out);
    }
    if let Some(l) = link {
        let property = matches!(l, Link::EquivalentProperty { .. });
        check_link(id.as_ref(), l.ids(), property, cx, out);
    }
}

fn statement(s: &Statement, cx: &Context<'_>, out: &mut Vec<Problem>) {
    for sn in s.snaks() {
        snak(sn, None, cx, out);
    }
    for (group, ss) in &s.qualifiers {
        for q in ss {
            if q.property != *group {
                out.push(Problem::PropertyGroup {
                    group: group.clone(),
                    property: q.property.clone(),
                });
            }
        }
    }
    for r in &s.references {
        for (group, ss) in &r.snaks {
            for q in ss {
                if q.property != *group {
                    out.push(Problem::PropertyGroup {
                        group: group.clone(),
                        property: q.property.clone(),
                    });
                }
            }
        }
    }
}

/// Checks one snak against the type map, and against its own `datatype` where the map
/// does not know the property. `group` is the property it is filed under, if any.
fn snak(s: &Snak, group: Option<&EntityId>, cx: &Context<'_>, out: &mut Vec<Problem>) {
    if let Some(g) = group
        && *g != s.property
    {
        out.push(Problem::PropertyGroup {
            group: g.clone(),
            property: s.property.clone(),
        });
    }
    let known = cx.types.datatype(&s.property);
    if let (Some(expected), Some(found)) = (known, &s.datatype)
        && expected != found
    {
        out.push(Problem::Datatype {
            property: s.property.clone(),
            expected: expected.clone(),
            found: found.clone(),
        });
    }
    if known.is_none() && cx.strict_properties {
        out.push(Problem::UnknownProperty(s.property.clone()));
    }
    let Some(datatype) = known.or(s.datatype.as_ref()) else {
        return;
    };
    let SnakKind::Value(v) = &s.kind else { return };
    if let Some(vt) = datatype.value_type()
        && vt.name() != v.value_type_name()
    {
        out.push(Problem::ValueType {
            property: s.property.clone(),
            datatype: datatype.clone(),
            value_type: v.value_type_name().to_string(),
        });
        return;
    }
    if let (Some(expected), DataValue::EntityId(ev)) = (datatype.entity_type(), v)
        && expected != ev.entity_type
    {
        out.push(Problem::EntityType {
            property: s.property.clone(),
            datatype: datatype.clone(),
            entity_type: ev.entity_type.clone(),
        });
    }
}
