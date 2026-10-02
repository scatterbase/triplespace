//! ACLs, enclosure and read evaluation (0016 §4; 0023 §1–3; 0039 §10; 0056 §2–6).
//!
//! An ACL attaches a restriction to a **target**, named by identifier: for each
//! permission it restricts, the group whose members may still perform it, until an
//! expiry. Evaluation is conjunctive: to perform action *A* on target *T*, a principal
//! holds the permission for *A* after blocks ([`mod@crate::evaluate`]) **and** satisfies every
//! ACL that restricts *A* on *T* or on a target enclosing *T*. Nothing loosens.
//!
//! What encloses what is data the caller already has (the tenant, the graph, the
//! namespace, the sets a target is in, the page, the entity, the property of a snak, the
//! hash of a file version), so this module takes the ACLs **along the enclosure chain**
//! as a slice and evaluates them; it does not look anything up.
//!
//! A `read` restriction is of one of two kinds (0056 §2): **moderation**, the target was
//! removed and outsiders see a notice; **confidential**, the target exists for a group
//! and is absent for everyone else. [`read_decision`] tells the three outcomes apart, and
//! [`visibility`] computes the set of groups a reader must be in, which caches, search
//! documents and derived outputs key on (0056 §6–8).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::evaluate::Effective;
use crate::key::ActorKey;
use crate::time::Timestamp;

/// The permission every read evaluation is about.
pub const READ: &str = "read";

/// What an ACL is attached to (0016 §4; 0023 §2; 0039 §10; 0056 §3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// Writing records to a graph: `acl:graph:{name}`.
    Graph(String),
    /// Every page and entity in a namespace: `acl:namespace:{number}`.
    Namespace(i64),
    /// One document page (threads included) and its subpages: `acl:page:{page id}`.
    Page(u64),
    /// One entity: `acl:entity:{id}`.
    Entity(String),
    /// One statement, by GUID: `acl:statement:{guid}`.
    Statement(String),
    /// Every snak using a property: `acl:property:{id}`.
    Property(String),
    /// The parts of one record: `acl:record:{partition}:{offset}`.
    Record {
        /// The partition ID.
        partition: u64,
        /// The record's offset.
        offset: u64,
    },
    /// An actor's name: `acl:actor:{key}`.
    Actor(ActorKey),
    /// Every file version with a hash, at instance scope: `acl:blob:{takedown id}`.
    Blob(u64),
    /// Everything in a tenant: `acl:tenant:{slug}`.
    Tenant(String),
    /// The members of a set: `acl:set:{id}`.
    Set(u64),
}

impl Target {
    /// The record key, `acl:{kind}:{id}`.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Graph(g) => format!("acl:graph:{g}"),
            Self::Namespace(n) => format!("acl:namespace:{n}"),
            Self::Page(p) => format!("acl:page:{p}"),
            Self::Entity(e) => format!("acl:entity:{e}"),
            Self::Statement(s) => format!("acl:statement:{s}"),
            Self::Property(p) => format!("acl:property:{p}"),
            Self::Record { partition, offset } => format!("acl:record:{partition}:{offset}"),
            Self::Actor(a) => format!("acl:actor:{a}"),
            Self::Blob(t) => format!("acl:blob:{t}"),
            Self::Tenant(t) => format!("acl:tenant:{t}"),
            Self::Set(s) => format!("acl:set:{s}"),
        }
    }

    /// Parses a record key.
    pub fn parse(key: &str) -> Result<Self, TargetError> {
        let bad = || TargetError(key.to_string());
        let rest = key.strip_prefix("acl:").ok_or_else(bad)?;
        let (kind, id) = rest.split_once(':').ok_or_else(bad)?;
        if id.is_empty() {
            return Err(bad());
        }
        let num = |s: &str| s.parse::<u64>().map_err(|_| bad());
        Ok(match kind {
            "graph" => Self::Graph(id.to_string()),
            "namespace" => Self::Namespace(id.parse().map_err(|_| bad())?),
            "page" => Self::Page(num(id)?),
            "entity" => Self::Entity(id.to_string()),
            "statement" => Self::Statement(id.to_string()),
            "property" => Self::Property(id.to_string()),
            "record" => {
                let (p, o) = id.split_once(':').ok_or_else(bad)?;
                Self::Record {
                    partition: num(p)?,
                    offset: num(o)?,
                }
            }
            "actor" => Self::Actor(ActorKey::parse(id).map_err(|_| bad())?),
            "blob" => Self::Blob(num(id)?),
            "tenant" => Self::Tenant(id.to_string()),
            "set" => Self::Set(num(id)?),
            _ => return Err(bad()),
        })
    }

    /// The kind, as `view.acl.target_kind` spells it.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Graph(_) => "graph",
            Self::Namespace(_) => "namespace",
            Self::Page(_) => "page",
            Self::Entity(_) => "entity",
            Self::Statement(_) => "statement",
            Self::Property(_) => "property",
            Self::Record { .. } => "record",
            Self::Actor(_) => "actor",
            Self::Blob(_) => "blob",
            Self::Tenant(_) => "tenant",
            Self::Set(_) => "set",
        }
    }

    /// Which partition the ACL record goes to (0023 §3): a graph or tenant target's to
    /// the tenant `config`, a blob's to the instance `log`, everything else to the tenant
    /// `log`.
    #[must_use]
    pub const fn partition(&self) -> AclPartition {
        match self {
            Self::Graph(_) | Self::Tenant(_) => AclPartition::TenantConfig,
            Self::Blob(_) => AclPartition::InstanceLog,
            _ => AclPartition::TenantLog,
        }
    }

    /// The right that sets or changes a non-`read` restriction on this target (0016 §4).
    #[must_use]
    pub const fn protecting_right(&self) -> &'static str {
        match self {
            Self::Graph(_) | Self::Tenant(_) => "ts-config",
            Self::Blob(_) => "ts-takedown",
            _ => "protect",
        }
    }
}

/// Why a string is not a target key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an ACL target key")]
pub struct TargetError(pub String);

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.key())
    }
}

impl Serialize for Target {
    /// As its key, `acl:{kind}:{id}`.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = std::borrow::Cow::<str>::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Where an ACL record is appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclPartition {
    /// The tenant's `config`: configuration, exported publicly.
    TenantConfig,
    /// The tenant's `log`: moderation, internal.
    TenantLog,
    /// The instance's `log`: takedowns.
    InstanceLog,
}

/// The two kinds of `read` restriction (0056 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadKind {
    /// The target was removed: deleted, hidden, suppressed; outsiders see a notice.
    Moderation,
    /// The target exists for its group and is absent for everyone else.
    Confidential,
}

/// Who may still perform a restricted permission, until when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restriction {
    /// The group; `None` restricts to nobody (a takedown, 0039 §10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// When the restriction lapses; absent means until retired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<Timestamp>,
}

impl Restriction {
    /// Restricted to a group.
    #[must_use]
    pub fn to_group(group: &str, expires: Option<Timestamp>) -> Self {
        Self {
            group: Some(group.to_string()),
            expires,
        }
    }

    /// Restricted to nobody.
    #[must_use]
    pub const fn to_nobody() -> Self {
        Self {
            group: None,
            expires: None,
        }
    }

    /// Whether the restriction is in force at `at`.
    #[must_use]
    pub fn active_at(&self, at: Timestamp) -> bool {
        self.expires.is_none_or(|e| at < e)
    }
}

/// A member of a set (0056 §3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum SetMember {
    /// A page, with its subpages (threads are pages).
    Page(u64),
    /// An entity.
    Entity(String),
}

/// The content part of a `scatter:v0/acl` record (0023 §3). A record whose content is
/// `null` retires the ACL; that is [`Option<Acl>`], not a field here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acl {
    /// The target.
    pub target: Target,
    /// For each restricted permission, who may still perform it.
    pub restrictions: BTreeMap<String, Restriction>,
    /// For a `read` restriction, which kind it is. Set by the write path from the right
    /// that authorized the record (`delete`, `deleterevision`, `suppressrevision`,
    /// `hideuser` → moderation; `protect` → confidential), and required whenever `read`
    /// is restricted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_kind: Option<ReadKind>,
    /// For a `record` target: the parts hidden; empty means the whole record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<String>,
    /// For create-protection: the namespace and title reserved under a page ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved: Option<ReservedTitle>,
    /// For a whole-page or whole-entity deletion: the paired talk page went with it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub talk: bool,
    /// For a `set` target: its name, shown where the restriction is shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For a `set` target: its members.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<SetMember>,
}

/// A title reserved by create-protection (0023 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservedTitle {
    /// The namespace number.
    pub namespace: i64,
    /// The title.
    pub title: String,
}

/// Why an ACL record is not well formed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AclError {
    /// `read` is restricted without a kind, or a kind is given without `read`.
    #[error("a `read` restriction needs its kind, and only a `read` restriction has one")]
    ReadKind,
    /// A restriction to nobody on anything but a `blob` target.
    #[error("only a takedown restricts to no group")]
    Nobody,
    /// A blob target restricts something other than `read` and `upload`, or names a group.
    #[error("a takedown restricts `read` and `upload` to nobody and nothing else")]
    Blob,
    /// A confidential restriction on a target that cannot carry one.
    #[error("a {0} target cannot carry a confidential restriction")]
    ConfidentialTarget(&'static str),
    /// `parts`, `reserved`, `talk`, `name` or `members` on the wrong target kind.
    #[error("`{field}` is not a field of a {kind} ACL")]
    Field {
        /// The field.
        field: &'static str,
        /// The target kind.
        kind: &'static str,
    },
    /// An empty record.
    #[error("an ACL restricts at least one permission")]
    Empty,
    /// A set member repeated, or a set containing nothing it may.
    #[error("set members are pages and entities, each once")]
    Members,
    /// The actor is not in the group a confidential restriction names (`ts-locked-out`).
    #[error("ts-locked-out: the actor is not a member of `{0}`")]
    LockedOut(String),
}

impl Acl {
    /// A protection: `edit` and/or `move` restricted to a group.
    #[must_use]
    pub fn protect(
        target: Target,
        permissions: &[&str],
        group: &str,
        expires: Option<Timestamp>,
    ) -> Self {
        Self {
            restrictions: permissions
                .iter()
                .map(|p| ((*p).to_string(), Restriction::to_group(group, expires)))
                .collect(),
            ..Self::empty(target)
        }
    }

    /// A `read` restriction of the given kind.
    #[must_use]
    pub fn read(target: Target, kind: ReadKind, group: &str, expires: Option<Timestamp>) -> Self {
        Self {
            restrictions: [(READ.to_string(), Restriction::to_group(group, expires))].into(),
            read_kind: Some(kind),
            ..Self::empty(target)
        }
    }

    /// A takedown (0039 §10): `read` and `upload` to nobody.
    #[must_use]
    pub fn takedown(takedown_id: u64) -> Self {
        Self {
            restrictions: [
                (READ.to_string(), Restriction::to_nobody()),
                ("upload".to_string(), Restriction::to_nobody()),
            ]
            .into(),
            read_kind: Some(ReadKind::Moderation),
            ..Self::empty(Target::Blob(takedown_id))
        }
    }

    fn empty(target: Target) -> Self {
        Self {
            target,
            restrictions: BTreeMap::new(),
            read_kind: None,
            parts: Vec::new(),
            reserved: None,
            talk: false,
            name: None,
            members: Vec::new(),
        }
    }

    /// Checks the record's shape (0023 §2–3; 0039 §10; 0056 §3).
    pub fn validate(&self) -> Result<(), AclError> {
        if self.restrictions.is_empty() {
            return Err(AclError::Empty);
        }
        let restricts_read = self.restrictions.contains_key(READ);
        if restricts_read != self.read_kind.is_some() {
            return Err(AclError::ReadKind);
        }
        let kind = self.target.kind();
        if let Target::Blob(_) = self.target {
            let ok = self.restrictions.len() == 2
                && self.restrictions.contains_key(READ)
                && self.restrictions.contains_key("upload")
                && self.restrictions.values().all(|r| r.group.is_none());
            if !ok {
                return Err(AclError::Blob);
            }
        } else if self.restrictions.values().any(|r| r.group.is_none()) {
            return Err(AclError::Nobody);
        }
        if self.read_kind == Some(ReadKind::Confidential)
            && matches!(
                self.target,
                Target::Graph(_) | Target::Record { .. } | Target::Actor(_) | Target::Blob(_)
            )
        {
            return Err(AclError::ConfidentialTarget(kind));
        }
        let field = |field: &'static str| AclError::Field { field, kind };
        if !self.parts.is_empty() && !matches!(self.target, Target::Record { .. }) {
            return Err(field("parts"));
        }
        if self.reserved.is_some() && !matches!(self.target, Target::Page(_)) {
            return Err(field("reserved"));
        }
        if self.talk && !matches!(self.target, Target::Page(_) | Target::Entity(_)) {
            return Err(field("talk"));
        }
        if let Target::Set(_) = self.target {
            let distinct: BTreeSet<&SetMember> = self.members.iter().collect();
            if distinct.len() != self.members.len() {
                return Err(AclError::Members);
            }
        } else {
            if self.name.is_some() {
                return Err(field("name"));
            }
            if !self.members.is_empty() {
                return Err(field("members"));
            }
        }
        Ok(())
    }

    /// The restriction on `permission`, if it is in force at `at`.
    #[must_use]
    pub fn restriction_at(&self, permission: &str, at: Timestamp) -> Option<&Restriction> {
        self.restrictions
            .get(permission)
            .filter(|r| r.active_at(at))
    }

    /// The right that sets this ACL (0016 §4; 0023 §1; 0056 §4): `protect` for a
    /// protection or a confidential restriction, the moderation rights for a moderation
    /// `read` restriction by target, `ts-config` for graphs and the tenant, `ts-takedown`
    /// for a blob. `suppress_group` is the site's suppression group.
    #[must_use]
    pub fn required_right(&self, suppress_group: &str) -> &'static str {
        match (self.read_kind, &self.target) {
            (Some(ReadKind::Moderation), Target::Blob(_)) => "ts-takedown",
            (Some(ReadKind::Moderation), Target::Actor(_)) => "hideuser",
            (Some(ReadKind::Moderation), t) => {
                let suppressed = self
                    .restrictions
                    .get(READ)
                    .is_some_and(|r| r.group.as_deref() == Some(suppress_group));
                match t {
                    Target::Record { .. } | Target::Statement(_) | Target::Property(_) => {
                        if suppressed {
                            "suppressrevision"
                        } else {
                            "deleterevision"
                        }
                    }
                    _ => {
                        if suppressed {
                            "suppressrevision"
                        } else {
                            "delete"
                        }
                    }
                }
            }
            _ => self.target.protecting_right(),
        }
    }

    /// The lock-out check of 0056 §4: a confidential `read` restriction may name only a
    /// group its setter belongs to.
    pub fn check_lock_out(&self, setter: &Effective) -> Result<(), AclError> {
        if self.read_kind == Some(ReadKind::Confidential)
            && let Some(r) = self.restrictions.get(READ)
            && let Some(g) = &r.group
            && !setter.in_group(g)
        {
            return Err(AclError::LockedOut(g.clone()));
        }
        Ok(())
    }
}

/// The current ACL of a target from its records in log order: the latest wins, and a
/// `null` content retires it.
#[must_use]
pub fn current_acl(records: &[Option<Acl>]) -> Option<&Acl> {
    records.last().and_then(Option::as_ref)
}

/// Why an action is refused (`permissiondenied` names the permission; an ACL refusal
/// names the target and group).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Denied {
    /// The principal does not hold the permission.
    #[error("permissiondenied: `{0}`")]
    Permission(String),
    /// An ACL restricts the action to a group the principal is not in.
    #[error("`{permission}` on `{target}` is restricted to `{group}`")]
    Restricted {
        /// The permission.
        permission: String,
        /// The target whose ACL refused.
        target: String,
        /// The group it is restricted to.
        group: String,
    },
    /// An ACL restricts the action to nobody.
    #[error("`{permission}` on `{target}` is restricted to nobody")]
    Nobody {
        /// The permission.
        permission: String,
        /// The target.
        target: String,
    },
    /// The request's credential lacks `editprotected` and an ACL restricts the action.
    #[error("`{0}` is restricted and the credential has no `editprotected` grant")]
    EditProtected(String),
}

/// Evaluates action `permission` for a principal against the ACLs along the enclosure
/// chain of the target, at `at` (0016 §4). `chain` is every current ACL on the target and
/// on each target enclosing it, in any order.
pub fn check<'a>(
    permission: &str,
    effective: &Effective,
    chain: impl IntoIterator<Item = &'a Acl>,
    at: Timestamp,
) -> Result<(), Denied> {
    if !effective.holds(permission) {
        return Err(Denied::Permission(permission.to_string()));
    }
    let mut restricted = false;
    for acl in chain {
        let Some(r) = acl.restriction_at(permission, at) else {
            continue;
        };
        restricted = true;
        match &r.group {
            None => {
                return Err(Denied::Nobody {
                    permission: permission.to_string(),
                    target: acl.target.key(),
                });
            }
            Some(g) if !effective.in_group(g) => {
                return Err(Denied::Restricted {
                    permission: permission.to_string(),
                    target: acl.target.key(),
                    group: g.clone(),
                });
            }
            Some(_) => {}
        }
    }
    if restricted && permission != READ && !effective.edit_protected {
        return Err(Denied::EditProtected(permission.to_string()));
    }
    Ok(())
}

/// The visibility of a target (0056 §2): the groups a reader must all be in, from every
/// `read` restriction along the enclosure chain; `nobody` when one restricts to no group.
/// Public is the empty set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Visibility {
    /// The groups, sorted; empty means public.
    pub groups: BTreeSet<String>,
    /// Restricted to nobody: a taken-down file version.
    pub nobody: bool,
}

impl Visibility {
    /// Whether every principal may read.
    #[must_use]
    pub fn is_public(&self) -> bool {
        self.groups.is_empty() && !self.nobody
    }

    /// Whether a principal that holds `read` satisfies this visibility.
    #[must_use]
    pub fn satisfied_by(&self, effective: &Effective) -> bool {
        !self.nobody && self.groups.iter().all(|g| effective.in_group(g))
    }

    /// The `{vis}` cache-key segment of 0056 §7 before hashing: the sorted group names
    /// joined with `,`, `-` for public, `!` for nobody.
    #[must_use]
    pub fn key(&self) -> String {
        if self.nobody {
            "!".to_string()
        } else if self.groups.is_empty() {
            "-".to_string()
        } else {
            self.groups.iter().cloned().collect::<Vec<_>>().join(",")
        }
    }

    /// The union of two visibilities: what an output derived from both carries (0056 §6,
    /// Rule 1).
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self {
            groups: self.groups.union(&other.groups).cloned().collect(),
            nobody: self.nobody || other.nobody,
        }
    }
}

/// The visibility of a target from the ACLs along its enclosure chain.
#[must_use]
pub fn visibility<'a>(chain: impl IntoIterator<Item = &'a Acl>, at: Timestamp) -> Visibility {
    let mut v = Visibility::default();
    for acl in chain {
        if let Some(r) = acl.restriction_at(READ, at) {
            match &r.group {
                None => v.nobody = true,
                Some(g) => {
                    v.groups.insert(g.clone());
                }
            }
        }
    }
    v
}

/// The outcome of a read (0056 §5; 0023 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadDecision {
    /// Served.
    Readable,
    /// Withheld by a moderation restriction: the target is shown as deleted, hidden or
    /// suppressed, with MediaWiki's notice; the first refusing target is named.
    Removed {
        /// The target whose restriction refused.
        target: String,
    },
    /// Withheld by a confidential restriction: answered as a target that does not exist.
    Absent,
}

/// Decides a read against the ACLs along the enclosure chain: a confidential refusal
/// anywhere makes the target absent, which takes precedence over a moderation notice so
/// that a notice never reveals a confidential target; otherwise a moderation refusal
/// shows the notice; otherwise the target is readable. A principal without `read` is
/// treated as refused by whatever restricts first, or `Absent` for a public target with
/// nothing to show.
#[must_use]
pub fn read_decision<'a>(
    effective: &Effective,
    chain: impl IntoIterator<Item = &'a Acl>,
    at: Timestamp,
) -> ReadDecision {
    let mut removed: Option<String> = None;
    for acl in chain {
        let Some(r) = acl.restriction_at(READ, at) else {
            continue;
        };
        let satisfied = r.group.as_ref().is_some_and(|g| effective.in_group(g));
        if satisfied {
            continue;
        }
        match acl.read_kind {
            Some(ReadKind::Confidential) => return ReadDecision::Absent,
            _ => removed.get_or_insert_with(|| acl.target.key()),
        };
    }
    match removed {
        Some(target) => ReadDecision::Removed { target },
        None if effective.holds(READ) => ReadDecision::Readable,
        None => ReadDecision::Absent,
    }
}

/// The include rule of 0056 §6 (Rule 2): a page *A* may include a target *B* only if
/// every reader of *A* could read *B* anyway, `groups(B) ⊆ groups(A)`, and *B* is not
/// restricted to nobody.
#[must_use]
pub fn may_include(container: &Visibility, included: &Visibility) -> bool {
    !included.nobody && included.groups.is_subset(&container.groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluate::{Principal, evaluate};
    use crate::grant::GrantRegistry;
    use crate::group::GroupRegistry;

    fn eff(groups: &[&str]) -> Effective {
        evaluate(
            &Principal::registered(groups.iter().map(|s| (*s).to_string())),
            GroupRegistry::default_registry(),
            GrantRegistry::default_registry(),
        )
    }

    const NOW: Timestamp = Timestamp(1000);

    #[test]
    fn target_keys_round_trip() {
        let targets = [
            Target::Graph("mirror/wikidata".into()),
            Target::Namespace(120),
            Target::Page(7),
            Target::Entity("Q5".into()),
            Target::Statement("Q8$C13E7A23-11E7-4C91-A799-3D1806B65444".into()),
            Target::Property("P31".into()),
            Target::Record {
                partition: 3,
                offset: 99,
            },
            Target::Actor(ActorKey::local(42)),
            Target::Blob(123),
            Target::Tenant("librarybase".into()),
            Target::Set(8),
        ];
        for t in targets {
            assert_eq!(Target::parse(&t.key()).unwrap(), t, "{}", t.key());
        }
        assert_eq!(Target::Page(7).key(), "acl:page:7");
        assert_eq!(
            Target::Record {
                partition: 3,
                offset: 99
            }
            .key(),
            "acl:record:3:99"
        );
        for bad in [
            "acl:page:",
            "page:7",
            "acl:page:x",
            "acl:record:3",
            "acl:what:1",
            "acl:actor:nocolon",
        ] {
            assert!(Target::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Target::Blob(1).partition(), AclPartition::InstanceLog);
        assert_eq!(
            Target::Graph("x".into()).partition(),
            AclPartition::TenantConfig
        );
        assert_eq!(Target::Page(1).partition(), AclPartition::TenantLog);
    }

    #[test]
    fn conjunctive_evaluation_over_the_chain() {
        let ns = Acl::protect(Target::Namespace(0), &["edit"], "autoconfirmed", None);
        let page = Acl::protect(
            Target::Page(1),
            &["edit", "move"],
            "sysop",
            Some(Timestamp(2000)),
        );
        let chain = [&ns, &page];
        // Conjunction: sysop alone is not autoconfirmed by membership, so the namespace
        // ACL refuses; both groups are needed.
        assert!(matches!(
            check("edit", &eff(&["sysop"]), chain, NOW),
            Err(Denied::Restricted { ref group, .. }) if group == "autoconfirmed"
        ));
        let both = {
            let mut p = Principal::registered(["sysop".to_string()]);
            p.autoconfirmed = true;
            evaluate(
                &p,
                GroupRegistry::default_registry(),
                GrantRegistry::default_registry(),
            )
        };
        assert!(check("edit", &both, chain, NOW).is_ok());
        let confirmed = {
            let mut p = Principal::registered([]);
            p.autoconfirmed = true;
            evaluate(
                &p,
                GroupRegistry::default_registry(),
                GrantRegistry::default_registry(),
            )
        };
        assert!(matches!(
            check("edit", &confirmed, chain, NOW),
            Err(Denied::Restricted { ref group, .. }) if group == "sysop"
        ));
        // After the page protection expires, autoconfirmed suffices.
        assert!(check("edit", &confirmed, chain, Timestamp(2000)).is_ok());
        // Without the permission at all, the ACLs are not even consulted.
        assert!(matches!(
            check("delete", &confirmed, chain, NOW),
            Err(Denied::Permission(p)) if p == "delete"
        ));
        // A takedown restricts to nobody, whoever asks.
        let td = Acl::takedown(5);
        assert!(matches!(
            check("read", &eff(&["owner"]), [&td], NOW),
            Err(Denied::Nobody { .. })
        ));
        // A credential needs editprotected to make an edit an ACL admits (0024 §4).
        let bot = evaluate(
            &Principal {
                kind: Some(crate::actor::ActorKind::Bot),
                groups: ["sysop".to_string()].into(),
                autoconfirmed: true,
                credential: Some(crate::evaluate::Credential {
                    grants: ["editentity".to_string()].into(),
                }),
                ..Principal::default()
            },
            GroupRegistry::default_registry(),
            GrantRegistry::default_registry(),
        );
        assert!(matches!(
            check("edit", &bot, chain, NOW),
            Err(Denied::EditProtected(_))
        ));
        assert!(check("edit", &bot, [], NOW).is_ok(), "unrestricted: fine");
    }

    #[test]
    fn visibility_and_read_decisions() {
        let public = visibility([], NOW);
        assert!(public.is_public());
        assert_eq!(public.key(), "-");
        let tenant = Acl::read(
            Target::Tenant("t".into()),
            ReadKind::Confidential,
            "user",
            None,
        );
        let set = Acl::read(Target::Set(1), ReadKind::Confidential, "board", None);
        let deleted = Acl::read(Target::Page(9), ReadKind::Moderation, "sysop", None);
        let v = visibility([&tenant, &set], NOW);
        assert_eq!(v.key(), "board,user");
        assert!(v.satisfied_by(&eff(&["board"])), "user is implicit");
        assert!(!v.satisfied_by(&eff(&[])));
        assert!(
            !v.satisfied_by(&eff(&["owner"])),
            "owner is in no group it has not joined"
        );
        // Decisions.
        assert_eq!(read_decision(&eff(&[]), [], NOW), ReadDecision::Readable);
        assert_eq!(
            read_decision(&eff(&[]), [&deleted], NOW),
            ReadDecision::Removed {
                target: "acl:page:9".into()
            }
        );
        assert_eq!(
            read_decision(&eff(&["sysop"]), [&deleted], NOW),
            ReadDecision::Readable
        );
        assert_eq!(
            read_decision(&eff(&[]), [&deleted, &set], NOW),
            ReadDecision::Absent,
            "absence wins over the notice"
        );
        assert_eq!(
            read_decision(&eff(&["board", "sysop"]), [&deleted, &set], NOW),
            ReadDecision::Readable
        );
        assert_eq!(
            read_decision(&eff(&["board"]), [&deleted, &set], NOW),
            ReadDecision::Removed {
                target: "acl:page:9".into()
            }
        );
        let td = Acl::takedown(1);
        assert_eq!(
            read_decision(&eff(&["owner"]), [&td], NOW),
            ReadDecision::Removed {
                target: "acl:blob:1".into()
            }
        );
        assert!(visibility([&td], NOW).nobody);
        assert_eq!(visibility([&td], NOW).key(), "!");
        // Expired restrictions do not count.
        let temp = Acl::read(
            Target::Page(2),
            ReadKind::Confidential,
            "board",
            Some(Timestamp(500)),
        );
        assert!(visibility([&temp], NOW).is_public());
        // Union, for derived outputs.
        let u = visibility([&tenant], NOW).union(&visibility([&set], NOW));
        assert_eq!(u, v);
    }

    #[test]
    fn include_rule() {
        let a = Visibility {
            groups: ["board".to_string(), "user".to_string()].into(),
            nobody: false,
        };
        let b = Visibility {
            groups: ["user".to_string()].into(),
            nobody: false,
        };
        assert!(
            may_include(&a, &b),
            "a more restricted page includes a less restricted one"
        );
        assert!(
            !may_include(&b, &a),
            "a public-ish page may not include a secret"
        );
        assert!(may_include(&b, &Visibility::default()));
        assert!(!may_include(
            &a,
            &Visibility {
                groups: BTreeSet::new(),
                nobody: true
            }
        ));
    }

    #[test]
    fn required_rights() {
        let r = |a: &Acl| a.required_right("suppress");
        assert_eq!(
            r(&Acl::protect(Target::Page(1), &["edit"], "sysop", None)),
            "protect"
        );
        assert_eq!(
            r(&Acl::read(
                Target::Page(1),
                ReadKind::Confidential,
                "staff",
                None
            )),
            "protect"
        );
        assert_eq!(
            r(&Acl::read(
                Target::Page(1),
                ReadKind::Moderation,
                "sysop",
                None
            )),
            "delete"
        );
        assert_eq!(
            r(&Acl::read(
                Target::Page(1),
                ReadKind::Moderation,
                "suppress",
                None
            )),
            "suppressrevision"
        );
        assert_eq!(
            r(&Acl::read(
                Target::Record {
                    partition: 1,
                    offset: 2
                },
                ReadKind::Moderation,
                "sysop",
                None
            )),
            "deleterevision"
        );
        assert_eq!(
            r(&Acl::read(
                Target::Actor(ActorKey::local(1)),
                ReadKind::Moderation,
                "suppress",
                None
            )),
            "hideuser"
        );
        assert_eq!(
            r(&Acl::read(
                Target::Tenant("t".into()),
                ReadKind::Confidential,
                "user",
                None
            )),
            "ts-config"
        );
        assert_eq!(r(&Acl::takedown(1)), "ts-takedown");
        assert_eq!(
            r(&Acl::protect(
                Target::Graph("local".into()),
                &["edit"],
                "bot",
                None
            )),
            "ts-config"
        );
    }

    #[test]
    fn shapes_are_checked() {
        assert!(Acl::takedown(1).validate().is_ok());
        let mut no_kind = Acl::read(Target::Page(1), ReadKind::Moderation, "sysop", None);
        no_kind.read_kind = None;
        assert!(matches!(no_kind.validate(), Err(AclError::ReadKind)));
        let mut nobody = Acl::protect(Target::Page(1), &["edit"], "sysop", None);
        nobody.restrictions.get_mut("edit").unwrap().group = None;
        assert!(matches!(nobody.validate(), Err(AclError::Nobody)));
        assert!(matches!(
            Acl::read(
                Target::Actor(ActorKey::local(1)),
                ReadKind::Confidential,
                "x",
                None
            )
            .validate(),
            Err(AclError::ConfidentialTarget("actor"))
        ));
        let mut parts_on_page = Acl::read(Target::Page(1), ReadKind::Moderation, "sysop", None);
        parts_on_page.parts = vec!["comment".into()];
        assert!(matches!(
            parts_on_page.validate(),
            Err(AclError::Field { field: "parts", .. })
        ));
        let mut set = Acl::read(Target::Set(1), ReadKind::Confidential, "board", None);
        set.name = Some("Board minutes".into());
        set.members = vec![
            SetMember::Page(1),
            SetMember::Entity("Q5".into()),
            SetMember::Page(1),
        ];
        assert!(matches!(set.validate(), Err(AclError::Members)));
        set.members.pop();
        assert!(set.validate().is_ok());
    }

    #[test]
    fn set_serde_lock_out_and_retirement() {
        let mut set = Acl::read(Target::Set(1), ReadKind::Confidential, "board", None);
        set.name = Some("Board minutes".into());
        set.members = vec![SetMember::Page(1), SetMember::Entity("Q5".into())];
        assert_eq!(
            serde_json::to_string(&set).unwrap(),
            r#"{"target":"acl:set:1","restrictions":{"read":{"group":"board"}},"read_kind":"confidential","name":"Board minutes","members":[{"kind":"page","id":1},{"kind":"entity","id":"Q5"}]}"#
        );
        // Lock-out.
        assert!(set.check_lock_out(&eff(&["board"])).is_ok());
        assert!(
            matches!(set.check_lock_out(&eff(&["sysop", "owner"])), Err(AclError::LockedOut(g)) if g == "board")
        );
        assert!(
            Acl::read(Target::Page(1), ReadKind::Moderation, "sysop", None)
                .check_lock_out(&eff(&[]))
                .is_ok(),
            "moderation has no lock-out"
        );
        // Retirement.
        let records = vec![Some(set.clone()), None];
        assert!(current_acl(&records).is_none());
        let records = vec![None, Some(set.clone())];
        assert_eq!(current_acl(&records), Some(&set));
    }
}
