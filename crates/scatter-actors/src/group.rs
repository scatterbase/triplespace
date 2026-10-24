//! Groups and permissions (0016 §1–3): the registry in `docs/registry/groups.toml`.
//!
//! A **permission** is a named capability; a **group** is a named set of permissions.
//! The registry lists every permission, so that a typo in a group is an error, the
//! default groups with their permissions and implicit memberships, and the default graph
//! ACLs an instance is created with. Groups are `config` records on a running instance
//! and may diverge from the defaults; this crate evaluates whatever [`GroupRegistry`] it is
//! handed.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::acl::Target;

/// How a group's membership is implied rather than recorded (0016 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Implicit {
    /// Everyone, including anonymous readers: `universe`.
    Everyone,
    /// Temporary accounts: `temp`.
    TemporaryAccounts,
    /// Every registered account except a pending subsidiary: `user`.
    RegisteredAccounts,
    /// An age and edit-count threshold from `site` configuration: `autoconfirmed`.
    Threshold,
    /// Fediverse surrogates: `federated`.
    FederatedActors,
}

/// A group's scope (0028 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    /// A tenant's own group.
    #[default]
    Tenant,
    /// A global group of the farm, whose memberships are keyed by farm accounts.
    Global,
}

/// The groups a group includes for reading (09 §3.2; 0016 A35, A37): its members pass a
/// `read` restriction naming any of them, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Includes {
    /// `"*"`: every group but `owner` and the tenant's suppression group.
    pub every: bool,
    /// Groups named; naming the suppression group beside `"*"` includes it too.
    pub names: BTreeSet<String>,
}

impl Includes {
    /// Whether nothing is included.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.every && self.names.is_empty()
    }
}

/// A group.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Group {
    /// The name.
    pub name: String,
    /// What MediaWiki clients see (`*` for `universe`).
    pub mediawiki_name: String,
    /// How membership is implied, if it is.
    pub implicit: Option<Implicit>,
    /// The permissions. For `owner`, every registered permission.
    pub permissions: BTreeSet<String>,
    /// Holds every permission, including any added later.
    pub all_permissions: bool,
    /// The group never has fewer members than this.
    pub minimum_members: u32,
    /// The group whose members alone change this group's membership (`owner` for `owner`);
    /// `userrights` otherwise.
    pub membership_changed_by: Option<String>,
    /// Tenant or global.
    pub scope: Scope,
    /// The groups its members read as (09 §3.2).
    pub includes: Includes,
    /// Groups its members may add others to (09 §3.2; 0016 A36).
    pub adds: BTreeSet<String>,
    /// Groups its members may remove others from.
    pub removes: BTreeSet<String>,
    /// Groups its members may join themselves.
    pub adds_self: BTreeSet<String>,
    /// Groups its members may leave themselves.
    pub removes_self: BTreeSet<String>,
    /// Where its permissions hold: namespaces and sets (`Target::Namespace`,
    /// `Target::Set`); empty for everywhere (09 §3.2).
    pub within: BTreeSet<Target>,
    /// Whether each new member is announced to the group's members (09 §3.3; 0016 A38).
    pub notify_joins: bool,
}

impl Group {
    /// Whether the group's permissions hold only within `within`.
    #[must_use]
    pub fn is_scoped(&self) -> bool {
        !self.within.is_empty()
    }

    /// The groups this group's members may change membership of, for others and for
    /// themselves, every list together.
    pub fn delegated(&self) -> impl Iterator<Item = &String> {
        self.adds
            .iter()
            .chain(&self.removes)
            .chain(&self.adds_self)
            .chain(&self.removes_self)
    }

    /// A group from its `config` record (`{"kind": "group", "name", "permissions", …}`;
    /// 09 §3.2). `"*"` among the permissions is every permission. `owner` keeps what
    /// makes it `owner` whatever its record says: every permission, membership changed by
    /// `owner` alone, and `includes = ["*"]` (09 §3.6).
    pub fn from_config(
        name: &str,
        config: &serde_json::Value,
        registry: &GroupRegistry,
    ) -> Result<Self, GroupRegistryError> {
        let bad = |m: String| GroupRegistryError::Toml(format!("group `{name}`: {m}"));
        let list = |key: &str| -> Result<Vec<String>, GroupRegistryError> {
            match config.get(key) {
                None | Some(serde_json::Value::Null) => Ok(Vec::new()),
                Some(serde_json::Value::Array(a)) => a
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| bad(format!("`{key}` must list strings")))
                    })
                    .collect(),
                Some(_) => Err(bad(format!("`{key}` must be a list"))),
            }
        };
        let mut raw_permissions = list("permissions")?;
        let all = raw_permissions.iter().any(|p| p == "*");
        raw_permissions.retain(|p| p != "*");
        let base = registry.by_name(name);
        let mut g = Self {
            name: name.to_string(),
            mediawiki_name: config
                .get("mediawiki_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| base.map(|b| b.mediawiki_name.clone()))
                .unwrap_or_else(|| name.to_string()),
            implicit: base.and_then(|b| b.implicit),
            permissions: raw_permissions.into_iter().collect(),
            all_permissions: all,
            minimum_members: base.map_or(0, |b| b.minimum_members),
            membership_changed_by: base.and_then(|b| b.membership_changed_by.clone()),
            scope: match config.get("scope").and_then(serde_json::Value::as_str) {
                Some("global") => Scope::Global,
                None | Some("tenant") => Scope::Tenant,
                Some(other) => return Err(bad(format!("unknown scope `{other}`"))),
            },
            includes: includes_of(list("includes")?),
            adds: list("adds")?.into_iter().collect(),
            removes: list("removes")?.into_iter().collect(),
            adds_self: list("adds_self")?.into_iter().collect(),
            removes_self: list("removes_self")?.into_iter().collect(),
            within: within_of(name, list("within")?)?,
            notify_joins: config
                .get("notify_joins")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        };
        if name == OWNER {
            g.all_permissions = true;
            g.membership_changed_by = Some(OWNER.to_string());
            g.includes.every = true;
        }
        Ok(g)
    }

    /// The group as its `config` record carries it (the inverse of [`Self::from_config`]).
    #[must_use]
    pub fn to_config(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        m.insert("kind".into(), "group".into());
        m.insert("name".into(), self.name.clone().into());
        let permissions: Vec<String> = if self.all_permissions {
            vec!["*".into()]
        } else {
            self.permissions.iter().cloned().collect()
        };
        m.insert("permissions".into(), permissions.into());
        if self.mediawiki_name != self.name {
            m.insert("mediawiki_name".into(), self.mediawiki_name.clone().into());
        }
        if self.scope == Scope::Global {
            m.insert("scope".into(), "global".into());
        }
        let mut includes: Vec<String> = self
            .includes
            .every
            .then(|| "*".to_string())
            .into_iter()
            .collect();
        includes.extend(self.includes.names.iter().cloned());
        for (k, v) in [
            ("includes", includes),
            ("adds", self.adds.iter().cloned().collect()),
            ("removes", self.removes.iter().cloned().collect()),
            ("adds_self", self.adds_self.iter().cloned().collect()),
            ("removes_self", self.removes_self.iter().cloned().collect()),
            ("within", self.within.iter().map(within_key).collect()),
        ] {
            if !v.is_empty() {
                m.insert(k.into(), v.into());
            }
        }
        if self.notify_joins {
            m.insert("notify_joins".into(), true.into());
        }
        serde_json::Value::Object(m)
    }
}

fn includes_of(names: Vec<String>) -> Includes {
    let mut i = Includes::default();
    for n in names {
        if n == "*" {
            i.every = true;
        } else {
            i.names.insert(n);
        }
    }
    i
}

/// `within` entries are written as `namespace:{number}` and `set:{id}`, the ACL target
/// keys without `acl:`.
fn within_of(group: &str, entries: Vec<String>) -> Result<BTreeSet<Target>, GroupRegistryError> {
    entries
        .into_iter()
        .map(|e| match Target::parse(&format!("acl:{e}")) {
            Ok(t @ (Target::Namespace(_) | Target::Set(_))) => Ok(t),
            _ => Err(GroupRegistryError::Toml(format!(
                "group `{group}`: `within` takes `namespace:N` and `set:N`, not `{e}`"
            ))),
        })
        .collect()
}

fn within_key(t: &Target) -> String {
    t.key().trim_start_matches("acl:").to_string()
}

/// Who may write a graph by default (0016 §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphWriters {
    /// Members of the group, who must also hold `edit`.
    Group(String),
    /// Account management or the login system only: no group at all.
    System,
}

/// A default graph ACL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphAclDefault {
    /// The graph name, possibly with a `{provider}` placeholder.
    pub graph: String,
    /// Who may write it.
    pub edit: GraphWriters,
}

/// Why `groups.toml` could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GroupRegistryError {
    /// The TOML did not parse or did not have the expected shape.
    #[error("groups.toml: {0}")]
    Toml(String),
    /// A registry version this crate does not know.
    #[error("groups.toml: version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// A group names a permission the registry does not list.
    #[error("group `{group}` names unknown permission `{permission}`")]
    UnknownPermission {
        /// The group.
        group: String,
        /// The permission.
        permission: String,
    },
    /// Two groups share a name, or a permission is listed twice.
    #[error("`{0}` is declared twice")]
    Duplicate(String),
    /// A built-in group is missing or has the wrong shape.
    #[error("groups.toml: group `{0}` {1}")]
    Builtin(&'static str, &'static str),
    /// An `implicit` value this crate does not know.
    #[error("group `{0}`: unknown implicit membership `{1}`")]
    Implicit(String, String),
    /// An ACL target that is not `graph:…`, or names an unknown group.
    #[error("acl `{0}`: {1}")]
    Acl(String, String),
    /// A delegation list names `owner` or a group that holds `userrights` or `ts-config`
    /// (09 §3.2).
    #[error("group `{group}` may not delegate membership of `{named}`, which {why}")]
    Delegation {
        /// The delegating group.
        group: String,
        /// The group named.
        named: String,
        /// Why it may not be named.
        why: &'static str,
    },
    /// A scoped group holds a permission that acts on no page, entity or thread.
    #[error("group `{group}` is scoped and so may not hold `{permission}`, which has no target")]
    Untargeted {
        /// The group.
        group: String,
        /// The permission.
        permission: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    permissions: RawPermissions,
    #[serde(default, rename = "group")]
    groups: Vec<RawGroup>,
    #[serde(default, rename = "acl")]
    acls: Vec<RawAcl>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPermissions {
    #[serde(default)]
    mediawiki: Vec<String>,
    #[serde(default)]
    wikibase: Vec<String>,
    #[serde(default)]
    triplespace: Vec<String>,
    /// The permissions that act on a page, entity or thread, which alone a scoped group
    /// may hold (09 §3.2).
    #[serde(default)]
    targeted: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGroup {
    name: String,
    #[serde(default)]
    mediawiki_name: Option<String>,
    #[serde(default)]
    implicit: Option<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    all_permissions: bool,
    #[serde(default)]
    minimum_members: u32,
    #[serde(default)]
    membership_changed_by: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    adds: Vec<String>,
    #[serde(default)]
    removes: Vec<String>,
    #[serde(default)]
    adds_self: Vec<String>,
    #[serde(default)]
    removes_self: Vec<String>,
    #[serde(default)]
    within: Vec<String>,
    #[serde(default)]
    notify_joins: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAcl {
    target: String,
    edit: String,
}

/// The group everyone is in.
pub const UNIVERSE: &str = "universe";
/// Every registered account.
pub const USER: &str = "user";
/// Temporary accounts.
pub const TEMP: &str = "temp";
/// The threshold group.
pub const AUTOCONFIRMED: &str = "autoconfirmed";
/// Fediverse surrogates.
pub const FEDERATED: &str = "federated";
/// Subsidiaries with the bot flag.
pub const BOT: &str = "bot";
/// The group that holds everything.
pub const OWNER: &str = "owner";
/// The default suppression group, `moderation.suppress_group` unless a tenant sets another.
pub const SUPPRESS: &str = "suppress";

/// The group registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRegistry {
    version: u32,
    permissions: BTreeSet<String>,
    targeted: BTreeSet<String>,
    groups: Vec<Group>,
    graph_acls: Vec<GraphAclDefault>,
    suppress_group: String,
}

impl GroupRegistry {
    /// Parses a `groups.toml`.
    pub fn parse(text: &str) -> Result<Self, GroupRegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| GroupRegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(GroupRegistryError::Version(raw.version));
        }
        let mut permissions = BTreeSet::new();
        for p in raw
            .permissions
            .mediawiki
            .iter()
            .chain(&raw.permissions.wikibase)
            .chain(&raw.permissions.triplespace)
        {
            if !permissions.insert(p.clone()) {
                return Err(GroupRegistryError::Duplicate(p.clone()));
            }
        }
        let mut targeted = BTreeSet::new();
        for p in raw.permissions.targeted {
            if !permissions.contains(&p) {
                return Err(GroupRegistryError::UnknownPermission {
                    group: "(targeted)".into(),
                    permission: p,
                });
            }
            if !targeted.insert(p.clone()) {
                return Err(GroupRegistryError::Duplicate(p));
            }
        }
        let mut groups: Vec<Group> = Vec::with_capacity(raw.groups.len());
        for g in raw.groups {
            let group = parse_group(g, &permissions)?;
            if groups.iter().any(|x| x.name == group.name) {
                return Err(GroupRegistryError::Duplicate(group.name));
            }
            groups.push(group);
        }
        let mut graph_acls = Vec::with_capacity(raw.acls.len());
        for a in raw.acls {
            let Some(graph) = a.target.strip_prefix("graph:") else {
                return Err(GroupRegistryError::Acl(
                    a.target,
                    "only graph targets are registry defaults".into(),
                ));
            };
            let edit = if a.edit == "system" {
                GraphWriters::System
            } else if groups.iter().any(|g| g.name == a.edit) {
                GraphWriters::Group(a.edit)
            } else {
                return Err(GroupRegistryError::Acl(
                    a.target.clone(),
                    format!("unknown group `{}`", a.edit),
                ));
            };
            graph_acls.push(GraphAclDefault {
                graph: graph.to_string(),
                edit,
            });
        }
        let reg = Self {
            version: raw.version,
            permissions,
            targeted,
            groups,
            graph_acls,
            suppress_group: SUPPRESS.to_string(),
        };
        let check = |name: &'static str, ok: fn(&Group) -> bool, why: &'static str| match reg
            .by_name(name)
        {
            None => Err(GroupRegistryError::Builtin(name, "is missing")),
            Some(g) if !ok(g) => Err(GroupRegistryError::Builtin(name, why)),
            Some(_) => Ok(()),
        };
        check(
            UNIVERSE,
            |g| g.implicit == Some(Implicit::Everyone) && g.mediawiki_name == "*",
            "must be implicit for everyone and `*` to MediaWiki",
        )?;
        check(
            USER,
            |g| g.implicit == Some(Implicit::RegisteredAccounts),
            "must be implicit for registered accounts",
        )?;
        check(
            OWNER,
            // `owner` may lose its last member once nothing needs it (0016 A37): the
            // registry no longer insists on one.
            |g| g.all_permissions && g.membership_changed_by.as_deref() == Some(OWNER),
            "must hold every permission and be changed only by owner",
        )?;
        for g in &reg.groups {
            reg.validate(g)?;
        }
        Ok(reg)
    }

    /// The embedded `docs/registry/groups.toml`.
    ///
    /// # Panics
    ///
    /// If the embedded registry is invalid, which the crate's tests rule out.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: std::sync::OnceLock<GroupRegistry> = std::sync::OnceLock::new();
        DEFAULT.get_or_init(|| Self::parse(crate::GROUPS_TOML).expect("embedded groups.toml"))
    }

    /// The registry version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every permission the registry knows.
    #[must_use]
    pub fn permissions(&self) -> &BTreeSet<String> {
        &self.permissions
    }

    /// Whether a permission is registered.
    #[must_use]
    pub fn is_permission(&self, name: &str) -> bool {
        self.permissions.contains(name)
    }

    /// Every group, in registry order.
    #[must_use]
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// A group by name.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// The group with the given implicit membership rule.
    #[must_use]
    pub fn implicit(&self, rule: Implicit) -> Option<&Group> {
        self.groups.iter().find(|g| g.implicit == Some(rule))
    }

    /// The default graph ACLs.
    #[must_use]
    pub fn graph_acls(&self) -> &[GraphAclDefault] {
        &self.graph_acls
    }

    /// The permissions that act on a page, entity or thread (09 §3.2).
    #[must_use]
    pub fn targeted(&self) -> &BTreeSet<String> {
        &self.targeted
    }

    /// The tenant's suppression group (`moderation.suppress_group`), which `includes = ["*"]`
    /// leaves out.
    #[must_use]
    pub fn suppress_group(&self) -> &str {
        &self.suppress_group
    }

    /// Sets the suppression group.
    pub fn set_suppress_group(&mut self, name: &str) {
        self.suppress_group = name.to_string();
    }

    /// Why `group` may not be named in another group's `adds`, `removes`, `adds_self` or
    /// `removes_self` (09 §3.2), if it may not: `owner`, and any group that holds
    /// `userrights` or `ts-config`, so delegation never reaches the rights that reach
    /// everything.
    #[must_use]
    pub fn undelegable(&self, group: &str) -> Option<&'static str> {
        if group == OWNER {
            return Some("is owner");
        }
        let g = self.by_name(group)?;
        if g.all_permissions || g.permissions.contains("userrights") {
            Some("holds userrights")
        } else if g.permissions.contains("ts-config") {
            Some("holds ts-config")
        } else {
            None
        }
    }

    /// Checks a group against the rules of 09 §3.2: every permission registered; the
    /// delegation lists name no group [`Self::undelegable`] refuses; a scoped group holds
    /// only targeted permissions.
    pub fn validate(&self, group: &Group) -> Result<(), GroupRegistryError> {
        for p in &group.permissions {
            if !self.permissions.contains(p) {
                return Err(GroupRegistryError::UnknownPermission {
                    group: group.name.clone(),
                    permission: p.clone(),
                });
            }
        }
        for named in group.delegated() {
            let why = if *named == group.name && group_is_privileged(group) {
                Some("holds userrights or ts-config")
            } else {
                self.undelegable(named)
            };
            if let Some(why) = why {
                return Err(GroupRegistryError::Delegation {
                    group: group.name.clone(),
                    named: named.clone(),
                    why,
                });
            }
        }
        if group.is_scoped() {
            if group.all_permissions {
                return Err(GroupRegistryError::Untargeted {
                    group: group.name.clone(),
                    permission: "*".into(),
                });
            }
            if let Some(p) = group
                .permissions
                .iter()
                .find(|p| !self.targeted.contains(*p))
            {
                return Err(GroupRegistryError::Untargeted {
                    group: group.name.clone(),
                    permission: p.clone(),
                });
            }
        }
        Ok(())
    }

    /// Adds or replaces a group, as a `config` record of kind `group` does on an instance.
    /// A group with `all_permissions` takes every permission the registry knows.
    pub fn set_group(&mut self, mut group: Group) -> Result<(), GroupRegistryError> {
        self.validate(&group)?;
        if group.all_permissions {
            group.permissions.clone_from(&self.permissions);
        }
        match self.groups.iter_mut().find(|g| g.name == group.name) {
            Some(existing) => *existing = group,
            None => self.groups.push(group),
        }
        Ok(())
    }

    /// Adds a permission, as an ADR or an extension may; `owner` and every other
    /// `all_permissions` group gain it.
    pub fn add_permission(&mut self, name: &str) {
        if self.permissions.insert(name.to_string()) {
            for g in &mut self.groups {
                if g.all_permissions {
                    g.permissions.insert(name.to_string());
                }
            }
        }
    }

    /// The groups and their MediaWiki names, as `meta=siteinfo&siprop=usergroups` lists
    /// them.
    #[must_use]
    pub fn mediawiki_names(&self) -> BTreeMap<&str, &str> {
        self.groups
            .iter()
            .map(|g| (g.name.as_str(), g.mediawiki_name.as_str()))
            .collect()
    }
}

fn group_is_privileged(g: &Group) -> bool {
    g.all_permissions || g.permissions.contains("userrights") || g.permissions.contains("ts-config")
}

fn parse_group(g: RawGroup, permissions: &BTreeSet<String>) -> Result<Group, GroupRegistryError> {
    let implicit = match g.implicit.as_deref() {
        None => None,
        Some("everyone") => Some(Implicit::Everyone),
        Some("temporary-accounts") => Some(Implicit::TemporaryAccounts),
        Some("registered-accounts") => Some(Implicit::RegisteredAccounts),
        Some("threshold") => Some(Implicit::Threshold),
        Some("federated-actors") => Some(Implicit::FederatedActors),
        Some(other) => {
            return Err(GroupRegistryError::Implicit(g.name, other.to_string()));
        }
    };
    let scope = match g.scope.as_deref() {
        None | Some("tenant") => Scope::Tenant,
        Some("global") => Scope::Global,
        Some(other) => {
            return Err(GroupRegistryError::Toml(format!(
                "group `{}`: scope must be \"tenant\" or \"global\", not \"{other}\"",
                g.name
            )));
        }
    };
    let mut perms = BTreeSet::new();
    for p in g.permissions {
        if !permissions.contains(&p) {
            return Err(GroupRegistryError::UnknownPermission {
                group: g.name,
                permission: p,
            });
        }
        perms.insert(p);
    }
    if g.all_permissions {
        perms.clone_from(permissions);
    }
    let within = within_of(&g.name, g.within)?;
    Ok(Group {
        mediawiki_name: g.mediawiki_name.unwrap_or_else(|| g.name.clone()),
        name: g.name,
        implicit,
        permissions: perms,
        all_permissions: g.all_permissions,
        minimum_members: g.minimum_members,
        membership_changed_by: g.membership_changed_by,
        scope,
        includes: includes_of(g.includes),
        adds: g.adds.into_iter().collect(),
        removes: g.removes.into_iter().collect(),
        adds_self: g.adds_self.into_iter().collect(),
        removes_self: g.removes_self.into_iter().collect(),
        within,
        notify_joins: g.notify_joins,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_parses() {
        let r = GroupRegistry::default_registry();
        assert!(
            r.is_permission("edit") && r.is_permission("ts-keys") && r.is_permission("item-term")
        );
        assert!(!r.is_permission("fly"));
        let owner = r.by_name("owner").unwrap();
        assert_eq!(&owner.permissions, r.permissions());
        assert_eq!(r.by_name("universe").unwrap().mediawiki_name, "*");
        assert_eq!(
            r.implicit(Implicit::RegisteredAccounts).unwrap().name,
            "user"
        );
        assert_eq!(
            r.implicit(Implicit::Threshold).unwrap().name,
            "autoconfirmed"
        );
        assert!(r.by_name("federated").unwrap().permissions.is_empty());
        assert!(r.by_name("sysop").unwrap().permissions.contains("protect"));
        // Wikibase's default: any registered account creates properties (0016 A32);
        // `propertycreator` remains for a tenant that takes it from `user`.
        assert!(
            r.by_name("user")
                .unwrap()
                .permissions
                .contains("property-create")
        );
        assert!(
            !r.by_name("temp")
                .unwrap()
                .permissions
                .contains("property-create")
        );
        assert!(
            r.by_name("propertycreator")
                .unwrap()
                .permissions
                .contains("property-create")
        );
        // Linking properties changes predicates for every consumer: not `user`'s.
        let holds = |g: &str| {
            r.by_name(g)
                .unwrap()
                .permissions
                .contains("ts-linkproperty")
        };
        assert!(!holds("user"));
        assert!(holds("propertycreator") && holds("sysop"));
        assert_eq!(r.mediawiki_names()["universe"], "*");
        let local = r.graph_acls().iter().find(|a| a.graph == "local").unwrap();
        assert_eq!(local.edit, GraphWriters::Group("universe".into()));
        let accounts = r
            .graph_acls()
            .iter()
            .find(|a| a.graph == "accounts")
            .unwrap();
        assert_eq!(accounts.edit, GraphWriters::System);
        assert!(
            r.graph_acls()
                .iter()
                .any(|a| a.graph == "mirror/{provider}")
        );
    }

    #[test]
    fn changes_on_an_instance() {
        let mut r = GroupRegistry::default_registry().clone();
        r.add_permission("ts-fly");
        assert!(r.by_name("owner").unwrap().permissions.contains("ts-fly"));
        assert!(!r.by_name("sysop").unwrap().permissions.contains("ts-fly"));
        let mut reviewers = Group {
            name: "reviewer".into(),
            mediawiki_name: "reviewer".into(),
            implicit: None,
            permissions: ["patrol".to_string()].into(),
            all_permissions: false,
            minimum_members: 0,
            membership_changed_by: None,
            scope: Scope::Tenant,
            ..Group::default()
        };
        r.set_group(reviewers.clone()).unwrap();
        assert_eq!(r.by_name("reviewer").unwrap().permissions.len(), 1);
        reviewers.permissions.insert("nope".into());
        assert!(matches!(
            r.set_group(reviewers),
            Err(GroupRegistryError::UnknownPermission { .. })
        ));
    }

    #[test]
    fn rejects_bad_registries() {
        let with = |extra: &str| format!("{}\n{extra}", crate::GROUPS_TOML);
        assert!(matches!(
            GroupRegistry::parse(&with("[[group]]\nname = \"x\"\npermissions = [\"fly\"]")),
            Err(GroupRegistryError::UnknownPermission { .. })
        ));
        assert!(matches!(
            GroupRegistry::parse(&with("[[group]]\nname = \"sysop\"")),
            Err(GroupRegistryError::Duplicate(_))
        ));
        assert!(matches!(
            GroupRegistry::parse(&with("[[group]]\nname = \"x\"\nimplicit = \"sometimes\"")),
            Err(GroupRegistryError::Implicit(..))
        ));
        assert!(matches!(
            GroupRegistry::parse(&with("[[acl]]\ntarget = \"page:1\"\nedit = \"sysop\"")),
            Err(GroupRegistryError::Acl(..))
        ));
        assert!(matches!(
            GroupRegistry::parse(&with("[[acl]]\ntarget = \"graph:x\"\nedit = \"nobody\"")),
            Err(GroupRegistryError::Acl(..))
        ));
        assert!(matches!(
            GroupRegistry::parse(
                &crate::GROUPS_TOML.replace("name = \"owner\"", "name = \"ownr\"")
            ),
            Err(GroupRegistryError::Builtin("owner", _))
        ));
        assert!(matches!(
            GroupRegistry::parse("version = 1\n[permissions]\n"),
            Err(GroupRegistryError::Builtin("universe", _))
        ));
    }

    #[test]
    fn delegation_and_scope_rules() {
        let r = GroupRegistry::default_registry();
        assert!(r.by_name("owner").unwrap().includes.every);
        assert!(r.by_name("read-all").unwrap().includes.every);
        assert!(r.by_name("read-all").unwrap().permissions.is_empty());
        let g = |v: serde_json::Value| Group::from_config("x", &v, r).unwrap();
        for named in ["owner", "bureaucrat"] {
            assert!(matches!(
                r.validate(&g(serde_json::json!({"permissions": [], "adds": [named]}))),
                Err(GroupRegistryError::Delegation { .. })
            ));
        }
        assert!(
            r.validate(&g(
                serde_json::json!({"permissions": [], "adds": ["sysop"], "removes_self": ["hr"]})
            ))
            .is_ok()
        );
        assert!(matches!(
            r.validate(&g(
                serde_json::json!({"permissions": ["block"], "within": ["namespace:4"]})
            )),
            Err(GroupRegistryError::Untargeted { .. })
        ));
        assert!(
            r.validate(&g(
                serde_json::json!({"permissions": ["delete"], "within": ["namespace:4", "set:2"]})
            ))
            .is_ok()
        );
        assert!(Group::from_config("x", &serde_json::json!({"within": ["page:4"]}), r).is_err());
        // The record round-trips, and owner keeps what makes it owner.
        let x = g(
            serde_json::json!({"permissions": ["delete"], "within": ["set:2"], "includes": ["*", "suppress"], "notify_joins": true}),
        );
        assert_eq!(Group::from_config("x", &x.to_config(), r).unwrap(), x);
        let owner =
            Group::from_config("owner", &serde_json::json!({"permissions": ["*"]}), r).unwrap();
        assert!(owner.all_permissions && owner.includes.every);
        assert_eq!(owner.membership_changed_by.as_deref(), Some("owner"));
    }
}
