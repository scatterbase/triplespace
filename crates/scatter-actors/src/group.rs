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

/// A group.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The group registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRegistry {
    version: u32,
    permissions: BTreeSet<String>,
    groups: Vec<Group>,
    graph_acls: Vec<GraphAclDefault>,
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
            groups,
            graph_acls,
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
            |g| {
                g.all_permissions
                    && g.minimum_members >= 1
                    && g.membership_changed_by.as_deref() == Some(OWNER)
            },
            "must hold every permission, keep a member, and be changed only by owner",
        )?;
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

    /// Adds or replaces a group, as a `config` record of kind `group` does on an instance.
    /// A group with `all_permissions` takes every permission the registry knows.
    pub fn set_group(&mut self, mut group: Group) -> Result<(), GroupRegistryError> {
        for p in &group.permissions {
            if !self.permissions.contains(p) {
                return Err(GroupRegistryError::UnknownPermission {
                    group: group.name,
                    permission: p.clone(),
                });
            }
        }
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
    Ok(Group {
        mediawiki_name: g.mediawiki_name.unwrap_or_else(|| g.name.clone()),
        name: g.name,
        implicit,
        permissions: perms,
        all_permissions: g.all_permissions,
        minimum_members: g.minimum_members,
        membership_changed_by: g.membership_changed_by,
        scope,
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
}
