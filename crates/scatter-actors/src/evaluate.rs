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

use std::collections::BTreeSet;

use crate::actor::{ActorKind, ActorStatus};
use crate::grant::{EDIT_PROTECTED, GrantRegistry};
use crate::group::{GroupRegistry, Implicit};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// The groups, implicit and explicit, as `meta=userinfo&uiprop=groups` lists them.
    pub groups: BTreeSet<String>,
    /// The permissions after blocks and grants.
    pub permissions: BTreeSet<String>,
    /// Whether an edit a protection ACL admits may be made: always for a session, and for
    /// a credential only with the `editprotected` grant (0024 §4).
    pub edit_protected: bool,
}

impl Effective {
    /// Whether the permission is held.
    #[must_use]
    pub fn holds(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    /// Whether the actor is in the group.
    #[must_use]
    pub fn in_group(&self, group: &str) -> bool {
        self.groups.contains(group)
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
    let mut permissions: BTreeSet<String> = my_groups
        .iter()
        .filter_map(|g| groups.by_name(g))
        .flat_map(|g| g.permissions.iter().cloned())
        .collect();
    for removed in [&p.blocked, &p.operator_blocked, &p.farm_blocked] {
        for perm in removed {
            permissions.remove(perm);
        }
    }
    let edit_protected = match &p.credential {
        None => true,
        Some(c) => {
            let covered = grants.coverage(c.grants.iter().map(String::as_str));
            permissions.retain(|perm| covered.contains(perm));
            c.grants.contains(EDIT_PROTECTED)
        }
    };
    Effective {
        groups: my_groups,
        permissions,
        edit_protected,
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
}
