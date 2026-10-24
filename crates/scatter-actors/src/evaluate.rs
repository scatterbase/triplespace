//! Effective permissions (0016 §3; 0024 §3–4; 0025 §3; 0028 §3–4; 0040 §5): what a
//! principal holds before any ACL is consulted.
//!
//! An actor's effective permissions are the union of its groups' permissions, minus what
//! its active blocks remove. Groups are the implicit ones its kind and status give it
//! (`universe` always; `temp`, `user`, `autoconfirmed`, `federated` by rule), its
//! explicit memberships, and the global groups of the farm account it is linked to.
//! Blocks subtract: its own, its operator's (so blocking a person blocks their bots), and
//! its farm account's. A request made with a credential then keeps only what the
//! credential's grants cover. There is no other negative rule.
//!
//! Everything here is a function of a [`Principal`], which the caller assembles from
//! records as of a time ([`crate::membership`]), so that the write path, the projection
//! and a rebuild evaluate the same way.

use std::collections::{BTreeMap, BTreeSet};

use crate::acl::Target;
use crate::actor::{ActorKind, ActorStatus};
use crate::grant::{EDIT_PROTECTED, GrantRegistry};
use crate::group::{GroupRegistry, Implicit, OWNER};

/// A credential a request was made with (0024 §4; 0025 §3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Credential {
    /// The grant names of the key or token.
    pub grants: BTreeSet<String>,
}

/// What evaluation knows about the actor behind a request, as of a time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Principal {
    /// The actor's kind; `None` for an anonymous request before a temporary account
    /// exists, which is `universe` and nothing more.
    pub kind: Option<ActorKind>,
    /// The actor's status.
    pub status: ActorStatus,
    /// Explicit tenant memberships ([`crate::membership::memberships_as_of`]).
    pub groups: BTreeSet<String>,
    /// Whether the `autoconfirmed` threshold is met (site configuration; 0016 Q2).
    pub autoconfirmed: bool,
    /// What the actor's own blocks remove, the tenant's and the instance's layers together
    /// ([`crate::membership::blocked_as_of`]; 0040 §5: the instance's is a floor).
    pub blocked: BTreeSet<String>,
    /// What the operator's blocks remove, for a subsidiary (0024 §3).
    pub operator_blocked: BTreeSet<String>,
    /// The global groups of the linked farm account (0028 §3).
    pub global_groups: BTreeSet<String>,
    /// What the farm account's global blocks remove (0028 §4).
    pub farm_blocked: BTreeSet<String>,
    /// The credential, for a request by key or token.
    pub credential: Option<Credential>,
}

impl Principal {
    /// An anonymous request.
    #[must_use]
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// A session of a registered account with the given explicit groups.
    #[must_use]
    pub fn registered(groups: impl IntoIterator<Item = String>) -> Self {
        Self {
            kind: Some(ActorKind::Registered),
            groups: groups.into_iter().collect(),
            ..Self::default()
        }
    }
}

/// The result of evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Effective {
    /// The groups, implicit and explicit, as `meta=userinfo&uiprop=groups` lists them.
    pub groups: BTreeSet<String>,
    /// The permissions after blocks and grants, from unscoped groups: held everywhere.
    pub permissions: BTreeSet<String>,
    /// Whether an edit a protection ACL admits may be made: always for a session, and for
    /// a credential only with the `editprotected` grant (0024 §4).
    pub edit_protected: bool,
    /// The groups whose `read` restrictions are passed beyond [`Self::groups`]: those the
    /// actor's groups include (09 §3.2).
    pub reads_as: BTreeSet<String>,
    /// Whether one of the actor's groups includes `"*"`; then every group but those in
    /// [`Self::reads_excluded`] counts as well.
    pub reads_every: bool,
    /// What `"*"` leaves out: `owner` and the suppression group.
    pub reads_excluded: BTreeSet<String>,
    /// Permissions held only on targets the listed namespaces and sets enclose, from
    /// scoped groups (09 §3.4), after blocks and grants.
    pub scoped: BTreeMap<String, BTreeSet<Target>>,
    /// Groups the actor may add others to, remove others from, join and leave, through
    /// the delegation lists of its groups (09 §3.2).
    pub delegation: Delegation,
}

/// What an actor's groups delegate (09 §3.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Delegation {
    /// `adds`.
    pub adds: BTreeSet<String>,
    /// `removes`.
    pub removes: BTreeSet<String>,
    /// `adds_self`.
    pub adds_self: BTreeSet<String>,
    /// `removes_self`.
    pub removes_self: BTreeSet<String>,
}

impl Effective {
    /// Whether the permission is held everywhere (MediaWiki's `rights`).
    #[must_use]
    pub fn holds(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    /// Whether the permission is held on a target, given the namespaces and sets that
    /// enclose it along the containment axis (the target itself among them when it is a
    /// namespace or set): everywhere, or through a scoped group whose `within` names one
    /// of them (09 §3.4).
    #[must_use]
    pub fn holds_on(&self, permission: &str, enclosing: &[Target]) -> bool {
        self.holds(permission)
            || self
                .scoped
                .get(permission)
                .is_some_and(|within| enclosing.iter().any(|t| within.contains(t)))
    }

    /// Whether the permission is held anywhere at all, scoped or not: whether a check on
    /// a target is worth making.
    #[must_use]
    pub fn holds_somewhere(&self, permission: &str) -> bool {
        self.holds(permission) || self.scoped.contains_key(permission)
    }

    /// Whether the permission is held only through scoped groups, so that a check needs
    /// the target's enclosures.
    #[must_use]
    pub fn needs_scope(&self, permission: &str) -> bool {
        !self.holds(permission) && self.scoped.contains_key(permission)
    }

    /// Whether the actor is in the group: what every ACL test but `read` asks, the
    /// lock-out of 09 §4.5 among them.
    #[must_use]
    pub fn in_group(&self, group: &str) -> bool {
        self.groups.contains(group)
    }

    /// Whether the actor passes a `read` restriction naming the group: a member, or a
    /// member of a group that includes it (09 §3.2, §3.4).
    #[must_use]
    pub fn reads_in(&self, group: &str) -> bool {
        self.in_group(group)
            || self.reads_as.contains(group)
            || (self.reads_every && !self.reads_excluded.contains(group))
    }
}

/// The groups a principal is in: implicit, explicit and global.
#[must_use]
pub fn groups_of(p: &Principal, registry: &GroupRegistry) -> BTreeSet<String> {
    let mut groups = BTreeSet::new();
    let mut implied = |rule: Implicit| {
        if let Some(g) = registry.implicit(rule) {
            groups.insert(g.name.clone());
        }
    };
    implied(Implicit::Everyone);
    let is_account = matches!(p.kind, Some(ActorKind::Registered | ActorKind::Bot));
    match p.kind {
        Some(ActorKind::Temporary) => implied(Implicit::TemporaryAccounts),
        // A pending subsidiary's implicit `user` is withheld (0025 §3).
        Some(ActorKind::Registered | ActorKind::Bot) if p.status != ActorStatus::Pending => {
            implied(Implicit::RegisteredAccounts);
        }
        Some(ActorKind::Federated) => implied(Implicit::FederatedActors),
        _ => {}
    }
    // The threshold is met by accounts, not by surrogates or anonymous requests.
    if p.autoconfirmed && is_account {
        implied(Implicit::Threshold);
    }
    groups.extend(p.groups.iter().cloned());
    groups.extend(p.global_groups.iter().cloned());
    groups
}

/// Evaluates a principal's effective permissions.
#[must_use]
pub fn evaluate(p: &Principal, groups: &GroupRegistry, grants: &GrantRegistry) -> Effective {
    let my_groups = groups_of(p, groups);
    let defined: Vec<_> = my_groups.iter().filter_map(|g| groups.by_name(g)).collect();
    let mut permissions = BTreeSet::new();
    let mut scoped: BTreeMap<String, BTreeSet<Target>> = BTreeMap::new();
    let mut e = Effective::default();
    for g in &defined {
        if g.is_scoped() {
            for perm in &g.permissions {
                scoped
                    .entry(perm.clone())
                    .or_default()
                    .extend(g.within.iter().cloned());
            }
        } else {
            permissions.extend(g.permissions.iter().cloned());
        }
        // Inclusion is one level deep: what an included group includes passes on nothing.
        e.reads_as.extend(g.includes.names.iter().cloned());
        e.reads_every |= g.includes.every;
        e.delegation.adds.extend(g.adds.iter().cloned());
        e.delegation.removes.extend(g.removes.iter().cloned());
        e.delegation.adds_self.extend(g.adds_self.iter().cloned());
        e.delegation
            .removes_self
            .extend(g.removes_self.iter().cloned());
    }
    if e.reads_every {
        e.reads_excluded = [OWNER.to_string(), groups.suppress_group().to_string()].into();
    }
    // A scoped permission also held everywhere needs no scope.
    scoped.retain(|perm, _| !permissions.contains(perm));
    // Delegation is a share of `userrights`: what takes `userrights` away, a block or a
    // credential without it, takes delegation too.
    let mut delegates = true;
    for removed in [&p.blocked, &p.operator_blocked, &p.farm_blocked] {
        for perm in removed {
            permissions.remove(perm);
            scoped.remove(perm);
        }
        delegates &= !removed.contains("userrights");
    }
    e.edit_protected = match &p.credential {
        None => true,
        Some(c) => {
            let covered = grants.coverage(c.grants.iter().map(String::as_str));
            permissions.retain(|perm| covered.contains(perm));
            scoped.retain(|perm, _| covered.contains(perm));
            delegates &= covered.contains("userrights");
            c.grants.contains(EDIT_PROTECTED)
        }
    };
    if !delegates {
        e.delegation = Delegation::default();
    }
    e.groups = my_groups;
    e.permissions = permissions;
    e.scoped = scoped;
    e
}

/// Why a membership change is refused (09 §3.3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MembershipRefusal {
    /// Membership of an implicit group is not recorded (`universe`, `user`, …).
    #[error("membership of `{0}` is implied, not granted")]
    Implicit(String),
    /// Only members of the named group change this one's membership (`owner`).
    #[error("only a member of `{changed_by}` changes membership of `{group}`")]
    ChangedBy {
        /// The group.
        group: String,
        /// Whose members may change it.
        changed_by: String,
    },
    /// Neither `userrights` nor a delegation list covers it.
    #[error("changing membership of `{0}` needs userrights or a group that delegates it")]
    NotDelegated(String),
}

/// Whether `who` may add an account to `group` (`adding`) or remove one from it, `own`
/// when the account is their own (09 §3.3): `owner` only by its members; any other group
/// with `userrights`; otherwise through a delegation list of one of `who`'s groups, which
/// never reaches `owner` or a group holding `userrights` or `ts-config`.
pub fn may_change_membership(
    who: &Effective,
    registry: &GroupRegistry,
    group: &str,
    adding: bool,
    own: bool,
) -> Result<(), MembershipRefusal> {
    let def = registry.by_name(group);
    if def.is_some_and(|g| g.implicit.is_some()) {
        return Err(MembershipRefusal::Implicit(group.to_string()));
    }
    if let Some(by) = def.and_then(|g| g.membership_changed_by.as_deref()) {
        return if who.in_group(by) {
            Ok(())
        } else {
            Err(MembershipRefusal::ChangedBy {
                group: group.to_string(),
                changed_by: by.to_string(),
            })
        };
    }
    if who.holds("userrights") {
        return Ok(());
    }
    let d = &who.delegation;
    let list = match (adding, own) {
        (true, false) => &d.adds,
        (true, true) => &d.adds_self,
        (false, false) => &d.removes,
        (false, true) => &d.removes_self,
    };
    if list.contains(group) && registry.undelegable(group).is_none() {
        Ok(())
    } else {
        Err(MembershipRefusal::NotDelegated(group.to_string()))
    }
}

/// Whether the last member of `owner` may leave (09 §3.6; 0016 A37, A38): every
/// permission a group other than `owner` holds, and `userrights` whether or not one does,
/// must be held by a group other than `owner` that has a member. `populated` lists the
/// groups with a current explicit member; implicit groups count as populated. Returns the
/// first permission nobody would be left holding.
pub fn owner_may_leave(
    registry: &GroupRegistry,
    populated: &BTreeSet<String>,
) -> Result<(), String> {
    let others = || registry.groups().iter().filter(|g| g.name != OWNER);
    let held_by_someone = |perm: &str| {
        others().any(|g| {
            g.permissions.contains(perm) && (g.implicit.is_some() || populated.contains(&g.name))
        })
    };
    let mut needed: BTreeSet<&str> = others()
        .flat_map(|g| g.permissions.iter().map(String::as_str))
        .collect();
    needed.insert("userrights");
    match needed.into_iter().find(|p| !held_by_someone(p)) {
        Some(p) => Err(p.to_string()),
        None => Ok(()),
    }
}

/// The instance rights (0016 §2; 0040 §9): permissions that authorize instance acts.
/// They are evaluated on the tenant that is primary at the time of the act, or through a
/// global group at the farm base; held on any other tenant they grant nothing at instance
/// scope. Some of these names also do ordinary tenant work (`userrights`, `block`,
/// `ts-config`); at instance scope they are the farm-account and farm-base forms.
pub const INSTANCE_RIGHTS: [&str; 11] = [
    "ts-keys",
    "ts-primary",
    "ts-config",
    "abusefilter-modify",
    "userrights",
    "block",
    "renameuser",
    "ts-takedown",
    "ts-expunge",
    "mwoauthmanageconsumer",
    "ts-runjob",
];

/// The instance rights a person holds (0040 §9): the union of what they hold on the
/// primary tenant and through global groups at the farm base, kept to
/// [`INSTANCE_RIGHTS`]. `None` for a scope the person has no account in.
#[must_use]
pub fn instance_rights(
    on_primary: Option<&Effective>,
    via_global_groups: Option<&Effective>,
) -> BTreeSet<String> {
    on_primary
        .into_iter()
        .chain(via_global_groups)
        .flat_map(|e| e.permissions.iter())
        .filter(|p| INSTANCE_RIGHTS.contains(&p.as_str()))
        .cloned()
        .collect()
}

/// The autopatrol rule (0023 §6): a change by an actor who held `autopatrol` when the
/// record was appended is patrolled from the start. `p` is the principal as of that
/// moment; a credential's grants do not matter, since patrol state is about the actor.
#[must_use]
pub fn autopatrolled(p: &Principal, groups: &GroupRegistry, grants: &GrantRegistry) -> bool {
    let without_credential = Principal {
        credential: None,
        ..p.clone()
    };
    evaluate(&without_credential, groups, grants).holds("autopatrol")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regs() -> (&'static GroupRegistry, &'static GrantRegistry) {
        (
            GroupRegistry::default_registry(),
            GrantRegistry::default_registry(),
        )
    }

    #[test]
    fn implicit_groups_by_kind_and_status() {
        let (g, _) = regs();
        assert_eq!(
            groups_of(&Principal::anonymous(), g),
            ["universe".to_string()].into()
        );
        let temp = Principal {
            kind: Some(ActorKind::Temporary),
            ..Principal::default()
        };
        assert_eq!(
            groups_of(&temp, g),
            ["universe".to_string(), "temp".to_string()].into()
        );
        let user = Principal::registered([]);
        assert_eq!(
            groups_of(&user, g),
            ["universe".to_string(), "user".to_string()].into()
        );
        let confirmed = Principal {
            autoconfirmed: true,
            ..Principal::registered(["sysop".to_string()])
        };
        assert_eq!(
            groups_of(&confirmed, g),
            ["autoconfirmed", "sysop", "universe", "user"]
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        );
        let pending = Principal {
            kind: Some(ActorKind::Bot),
            status: ActorStatus::Pending,
            ..Principal::default()
        };
        assert_eq!(
            groups_of(&pending, g),
            ["universe".to_string()].into(),
            "0025 §3: user withheld while pending"
        );
        let federated = Principal {
            kind: Some(ActorKind::Federated),
            ..Principal::default()
        };
        assert!(groups_of(&federated, g).contains("federated"));
        let surrogate = Principal {
            kind: Some(ActorKind::Anonymous),
            autoconfirmed: true,
            ..Principal::default()
        };
        assert_eq!(groups_of(&surrogate, g), ["universe".to_string()].into());
    }

    #[test]
    fn union_minus_blocks() {
        let (g, gr) = regs();
        let sysop = Principal::registered(["sysop".to_string()]);
        let e = evaluate(&sysop, g, gr);
        assert!(e.holds("delete") && e.holds("edit") && e.holds("read"));
        assert!(!e.holds("userrights"));
        assert!(e.edit_protected);
        let blocked = Principal {
            blocked: ["edit".to_string(), "createpage".to_string()].into(),
            ..sysop.clone()
        };
        let e = evaluate(&blocked, g, gr);
        assert!(!e.holds("edit") && e.holds("delete") && e.holds("read"));
        // A bot inherits nothing from its operator but the operator's blocks (0024 §3).
        let bot = Principal {
            kind: Some(ActorKind::Bot),
            groups: ["bot".to_string()].into(),
            operator_blocked: ["edit".to_string()].into(),
            ..Principal::default()
        };
        let e = evaluate(&bot, g, gr);
        assert!(!e.holds("edit") && e.holds("bot") && !e.holds("delete"));
        // Global groups add, farm blocks subtract (0028 §3–4).
        let mut groups = g.clone();
        groups
            .set_group(crate::group::Group {
                name: "steward".into(),
                mediawiki_name: "steward".into(),
                implicit: None,
                permissions: ["userrights".to_string(), "block".to_string()].into(),
                all_permissions: false,
                minimum_members: 0,
                membership_changed_by: None,
                scope: crate::group::Scope::Global,
                ..crate::group::Group::default()
            })
            .unwrap();
        let steward = Principal {
            global_groups: ["steward".to_string()].into(),
            farm_blocked: ["block".to_string()].into(),
            ..Principal::registered([])
        };
        let e = evaluate(&steward, &groups, gr);
        assert!(e.holds("userrights") && !e.holds("block") && e.in_group("steward"));
        // Owner holds everything, including a permission added later.
        let mut groups = g.clone();
        groups.add_permission("ts-fly");
        let owner = Principal::registered(["owner".to_string()]);
        assert!(evaluate(&owner, &groups, gr).holds("ts-fly"));
    }

    #[test]
    fn grants_intersect_and_never_add() {
        let (g, gr) = regs();
        let bot = Principal {
            kind: Some(ActorKind::Bot),
            groups: ["bot".to_string()].into(),
            credential: Some(Credential {
                grants: ["editentity".to_string(), "delete".to_string()].into(),
            }),
            ..Principal::default()
        };
        let e = evaluate(&bot, g, gr);
        assert!(e.holds("edit") && e.holds("item-term") && e.holds("read"));
        assert!(!e.holds("delete"), "a grant never adds a permission");
        assert!(!e.holds("bot"), "not covered by these grants");
        assert!(!e.edit_protected, "no editprotected grant");
        let with_hv = Principal {
            credential: Some(Credential {
                grants: ["highvolume".to_string(), "editprotected".to_string()].into(),
            }),
            ..bot.clone()
        };
        let e = evaluate(&with_hv, g, gr);
        assert!(e.holds("bot") && e.edit_protected && !e.holds("edit"));
        // A pending subsidiary with any grants is universe and nothing more.
        let pending = Principal {
            status: ActorStatus::Pending,
            groups: BTreeSet::new(),
            ..bot
        };
        let e = evaluate(&pending, g, gr);
        assert_eq!(
            e.permissions,
            gr.coverage(["editentity", "delete"])
                .intersection(&g.by_name("universe").unwrap().permissions)
                .cloned()
                .collect()
        );
        assert!(e.holds("read") && !e.holds("edit"));
    }

    #[test]
    fn instance_rights_come_from_the_primary_tenant_or_global_groups() {
        let (g, gr) = regs();
        let owner = evaluate(&Principal::registered(["owner".to_string()]), g, gr);
        let rights = instance_rights(Some(&owner), None);
        assert!(rights.contains("ts-keys") && rights.contains("ts-takedown"));
        assert!(!rights.contains("edit"), "not an instance right");
        assert!(instance_rights(None, None).is_empty());
        let sysop = evaluate(&Principal::registered(["sysop".to_string()]), g, gr);
        assert_eq!(
            instance_rights(None, Some(&sysop)),
            ["abusefilter-modify", "block", "ts-runjob"]
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        );
    }

    #[test]
    fn autopatrol_is_a_function_of_the_actor() {
        let (g, gr) = regs();
        let bot = Principal {
            kind: Some(ActorKind::Bot),
            groups: ["bot".to_string()].into(),
            credential: Some(Credential {
                grants: ["editentity".to_string()].into(),
            }),
            ..Principal::default()
        };
        assert!(autopatrolled(&bot, g, gr), "the credential does not matter");
        assert!(!autopatrolled(&Principal::registered([]), g, gr));
        assert!(autopatrolled(
            &Principal::registered(["autopatrolled".to_string()]),
            g,
            gr
        ));
    }

    #[test]
    fn scoped_groups_hold_only_within() {
        let (g, gr) = regs();
        let mut groups = g.clone();
        let hr = crate::group::Group::from_config(
            "hr-admins",
            &serde_json::json!({
                "kind": "group", "permissions": ["delete", "protect"],
                "within": ["set:5", "namespace:4"], "adds": ["hr"], "removes": ["hr"]
            }),
            &groups,
        )
        .unwrap();
        groups.set_group(hr).unwrap();
        let e = evaluate(
            &Principal::registered(["hr-admins".to_string()]),
            &groups,
            gr,
        );
        assert!(!e.holds("delete"), "not everywhere");
        assert!(e.holds_somewhere("delete") && e.needs_scope("delete"));
        assert!(e.holds_on("delete", &[Target::Set(5)]));
        assert!(e.holds_on("protect", &[Target::Page(1), Target::Namespace(4)]));
        assert!(!e.holds_on("delete", &[Target::Set(6), Target::Namespace(0)]));
        assert!(!e.holds_on("block", &[Target::Set(5)]));
        assert!(e.delegation.adds.contains("hr") && e.delegation.adds_self.is_empty());
        // Also a sysop: held everywhere, no scope needed.
        let both = evaluate(
            &Principal::registered(["hr-admins".to_string(), "sysop".to_string()]),
            &groups,
            gr,
        );
        assert!(both.holds("delete") && !both.needs_scope("delete"));
        // A block takes scoped permissions and delegation away.
        let blocked = evaluate(
            &Principal {
                blocked: ["delete".to_string(), "userrights".to_string()].into(),
                ..Principal::registered(["hr-admins".to_string()])
            },
            &groups,
            gr,
        );
        assert!(!blocked.holds_on("delete", &[Target::Set(5)]));
        assert!(blocked.delegation.adds.is_empty());
    }

    #[test]
    fn inclusion_is_for_reading_only() {
        let (g, gr) = regs();
        let owner = evaluate(&Principal::registered(["owner".to_string()]), g, gr);
        assert!(owner.reads_in("board") && owner.reads_in("sysop"));
        assert!(!owner.reads_in("suppress") && !owner.in_group("board"));
        let mut groups = g.clone();
        groups.set_suppress_group("oversight");
        let ra = evaluate(
            &Principal::registered(["read-all".to_string()]),
            &groups,
            gr,
        );
        assert!(ra.reads_in("suppress") && !ra.reads_in("oversight") && !ra.reads_in("owner"));
        assert_eq!(
            ra.permissions,
            evaluate(&Principal::registered([]), &groups, gr).permissions
        );
        let auditors = crate::group::Group::from_config(
            "auditors",
            &serde_json::json!({"kind": "group", "permissions": [], "includes": ["board"]}),
            &groups,
        )
        .unwrap();
        groups.set_group(auditors).unwrap();
        let a = evaluate(
            &Principal::registered(["auditors".to_string()]),
            &groups,
            gr,
        );
        assert!(a.reads_in("board") && !a.reads_in("sysop") && !a.in_group("board"));
    }

    #[test]
    fn who_changes_which_membership() {
        let (g, gr) = regs();
        let mut groups = g.clone();
        groups
            .set_group(
                crate::group::Group::from_config(
                    "hr-admins",
                    &serde_json::json!({"permissions": [], "adds": ["hr"], "removes_self": ["hr-admins"]}),
                    &groups,
                )
                .unwrap(),
            )
            .unwrap();
        let ev = |gs: &[&str]| {
            evaluate(
                &Principal::registered(gs.iter().map(|s| (*s).to_string())),
                &groups,
                gr,
            )
        };
        let crat = ev(&["bureaucrat"]);
        assert!(may_change_membership(&crat, &groups, "sysop", true, false).is_ok());
        assert!(
            may_change_membership(&crat, &groups, "read-all", true, true).is_ok(),
            "self too"
        );
        assert!(matches!(
            may_change_membership(&crat, &groups, "owner", true, false),
            Err(MembershipRefusal::ChangedBy { .. })
        ));
        assert!(matches!(
            may_change_membership(&crat, &groups, "user", true, false),
            Err(MembershipRefusal::Implicit(_))
        ));
        let owner = ev(&["owner"]);
        assert!(may_change_membership(&owner, &groups, "owner", true, false).is_ok());
        let hr = ev(&["hr-admins"]);
        assert!(may_change_membership(&hr, &groups, "hr", true, false).is_ok());
        assert!(
            may_change_membership(&hr, &groups, "hr", true, true).is_err(),
            "not themselves"
        );
        assert!(
            may_change_membership(&hr, &groups, "hr", false, false).is_err(),
            "adds only"
        );
        assert!(may_change_membership(&hr, &groups, "hr-admins", false, true).is_ok());
        assert!(may_change_membership(&hr, &groups, "sysop", true, false).is_err());
    }

    #[test]
    fn the_last_owner_leaves_only_when_nothing_needs_owner() {
        let (g, _) = regs();
        let every: BTreeSet<String> = g
            .groups()
            .iter()
            .filter(|x| x.name != "owner")
            .map(|x| x.name.clone())
            .collect();
        assert!(owner_may_leave(g, &every).is_ok());
        let mut fewer = every.clone();
        fewer.remove("bureaucrat");
        assert_eq!(
            owner_may_leave(g, &fewer).unwrap_err(),
            "abusefilter-modify-restricted"
        );
        let mut no_suppress = every;
        no_suppress.remove("suppress");
        assert!(owner_may_leave(g, &no_suppress).is_err());
        // ts-keys and ts-primary are held by no group but owner, and do not count.
        assert!(
            !g.groups()
                .iter()
                .any(|x| x.name != "owner" && x.permissions.contains("ts-keys"))
        );
    }
}
