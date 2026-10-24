//! `triplespace member|group|acl` (09 §3.3, §3.6, §4.3): the operator's writers of
//! memberships, group definitions and ACLs, through the same writers as
//! `action=userrights`, `/group` and `/acl`. Each is an instance act, attested by the
//! farm's operator, and the operator holds every right here, since the command has the
//! database: it may add to or empty `owner`, which is how new owners are appointed and how
//! a tenant without one is given one again (09 §3.6), and no lock-out binds it.
//!
//! - `member add --user NAME --group G [--expiry T]`, `member remove --user NAME --group G`.
//! - `group show NAME`, `group set NAME DEFINITION` (the JSON of a `group` record),
//!   `group unset NAME`.
//! - `acl show TARGET`, `acl set TARGET RESTRICTIONS` (`{"read": {"group": "board"}}`),
//!   `acl unset TARGET`; a target is an ACL key with or without `acl:` (`namespace:4`,
//!   `set:12`, `tenant:lbwiki`, `page:7`).

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_actors::acl::{Acl, ReadKind, Restriction, Target};
use scatter_actors::evaluate::Effective;
use scatter_actors::group::Group;
use scatter_projection::Backend as _;
use triplespace_api_ingest::acl::{AclWrite, write as write_acl};
use triplespace_api_ingest::groups::write as write_group;
use triplespace_api_ingest::membership::{Change, Writer, change};

use crate::common::{connect, farm_of, now, store};

/// Where a command acts.
#[derive(Debug, Args)]
pub struct Tenant {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant.
    #[arg(long)]
    pub tenant: String,
    /// The reason, for the record's comment.
    #[arg(long, default_value = "")]
    pub reason: String,
}

/// The membership commands.
#[derive(Debug, Subcommand)]
pub enum Member {
    /// Add an account to a group (`owner` included).
    Add(MemberArgs),
    /// Remove an account from a group (the last owner included).
    Remove(MemberArgs),
}

/// `member add|remove`.
#[derive(Debug, Args)]
pub struct MemberArgs {
    /// Where.
    #[command(flatten)]
    pub at: Tenant,
    /// The account's name.
    #[arg(long)]
    pub user: String,
    /// The group.
    #[arg(long)]
    pub group: String,
    /// When the membership lapses (an ISO 8601 timestamp); never by default.
    #[arg(long)]
    pub expiry: Option<String>,
}

/// The group commands.
#[derive(Debug, Subcommand)]
pub enum GroupCommand {
    /// Print a group's definition.
    Show {
        /// Where.
        #[command(flatten)]
        at: Tenant,
        /// The group.
        name: String,
    },
    /// Write a group's record.
    Set {
        /// Where.
        #[command(flatten)]
        at: Tenant,
        /// The group.
        name: String,
        /// The definition, as JSON: `{"permissions": [...], "includes": [...], ...}`.
        definition: String,
    },
    /// Retire a group's record.
    Unset {
        /// Where.
        #[command(flatten)]
        at: Tenant,
        /// The group.
        name: String,
    },
}

/// The ACL commands.
#[derive(Debug, Subcommand)]
pub enum AclCommand {
    /// Print a target's ACL.
    Show {
        /// Where.
        #[command(flatten)]
        at: Tenant,
        /// The target.
        target: String,
    },
    /// Replace a target's restrictions.
    Set {
        /// Where.
        #[command(flatten)]
        at: Tenant,
        /// The target.
        target: String,
        /// The restrictions, as JSON: `{"read": {"group": "board", "expiry": "…"}}`.
        restrictions: String,
    },
    /// Retire a target's ACL.
    Unset {
        /// Where.
        #[command(flatten)]
        at: Tenant,
        /// The target.
        target: String,
    },
}

/// The operator: every permission, and in whatever groups `groups` names, so that no
/// lock-out binds an instance act.
fn operator(groups: impl IntoIterator<Item = String>) -> Effective {
    let registry = scatter_actors::group::GroupRegistry::default_registry();
    Effective {
        permissions: registry.permissions().clone(),
        groups: groups.into_iter().collect(),
        edit_protected: true,
        ..Effective::default()
    }
}

fn target_of(text: &str) -> Result<Target> {
    let key = if text.starts_with("acl:") {
        text.to_string()
    } else {
        format!("acl:{text}")
    };
    Target::parse(&key).map_err(|e| anyhow::anyhow!("{e}"))
}

fn timestamp(text: &str) -> Result<scatter_actors::Timestamp> {
    triplespace_projections::feed::parse_timestamp(text)
        .map(scatter_actors::Timestamp)
        .with_context(|| format!("`{text}` is not a timestamp"))
}

/// Runs a `member` command.
pub async fn run_member(cmd: Member) -> Result<()> {
    let (args, adding) = match cmd {
        Member::Add(a) => (a, true),
        Member::Remove(a) => (a, false),
    };
    let client = connect(&args.at.database).await?;
    let farm = farm_of(&client).await?;
    let (account, _) =
        crate::accounts::actor_key(&client, &args.at.tenant, Some(&args.user), None).await?;
    let registry = triplespace_accounts::groups::registry(&client, &args.at.tenant).await?;
    if registry
        .by_name(&args.group)
        .is_some_and(|g| g.implicit.is_some())
    {
        bail!("membership of `{}` is implied, not granted", args.group);
    }
    let expires = args.expiry.as_deref().map(timestamp).transpose()?;
    let c = Change {
        account: account.clone(),
        add: if adding {
            vec![(args.group.clone(), expires)]
        } else {
            Vec::new()
        },
        remove: if adding {
            Vec::new()
        } else {
            vec![args.group.clone()]
        },
        reason: args.at.reason.clone(),
    };
    let (store, pipeline) = store(&args.at.database, &farm)?;
    let mut cx = store.begin().await?;
    match change(
        &store,
        &pipeline,
        &mut cx,
        &args.at.tenant,
        &farm.operator().to_string(),
        Writer::Instance,
        &registry,
        &c,
        now(),
    )
    .await
    {
        Ok(out) => {
            store.commit(cx).await?;
            println!(
                "{account}: added {:?}, removed {:?}",
                out.added, out.removed
            );
            Ok(())
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(e.into())
        }
    }
}

/// Runs a `group` command.
pub async fn run_group(cmd: GroupCommand) -> Result<()> {
    let (at, name, group) = match cmd {
        GroupCommand::Show { at, name } => {
            let client = connect(&at.database).await?;
            let registry = triplespace_accounts::groups::registry(&client, &at.tenant).await?;
            let g = registry
                .by_name(&name)
                .with_context(|| format!("no group `{name}` on `{}`", at.tenant))?;
            println!("{}", serde_json::to_string_pretty(&g.to_config())?);
            return Ok(());
        }
        GroupCommand::Set {
            at,
            name,
            definition,
        } => {
            let client = connect(&at.database).await?;
            let registry = triplespace_accounts::groups::registry(&client, &at.tenant).await?;
            let json: serde_json::Value =
                serde_json::from_str(&definition).context("the definition is JSON")?;
            let g = Group::from_config(&name, &json, &registry)?;
            (at, name, Some(g))
        }
        GroupCommand::Unset { at, name } => (at, name, None),
    };
    let client = connect(&at.database).await?;
    let farm = farm_of(&client).await?;
    let registry = triplespace_accounts::groups::registry(&client, &at.tenant).await?;
    let (store, pipeline) = store(&at.database, &farm)?;
    let mut cx = store.begin().await?;
    let out = write_group(
        &store,
        &pipeline,
        &mut cx,
        &at.tenant,
        &farm.operator().to_string(),
        &operator([]),
        &registry,
        &name,
        group,
        &at.reason,
        now(),
    )
    .await;
    match out {
        Ok(()) => {
            store.commit(cx).await?;
            println!("group `{name}` written");
            Ok(())
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(e.into())
        }
    }
}

/// The ACL `acl set` writes: the existing one with its restrictions replaced.
fn acl_from(existing: Option<&Acl>, t: &Target, text: &str) -> Result<Acl> {
    let parsed: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(text).context("the restrictions are a JSON object")?;
    let mut acl = existing
        .cloned()
        .unwrap_or_else(|| Acl::protect(t.clone(), &[], "", None));
    if acl.read_kind == Some(ReadKind::Moderation) {
        bail!(
            "{} is deleted; undelete it before changing its ACL",
            t.key()
        );
    }
    acl.restrictions.clear();
    for (permission, v) in parsed {
        let group = v
            .get("group")
            .and_then(serde_json::Value::as_str)
            .with_context(|| format!("`{permission}` needs a `group`"))?;
        let expires = v
            .get("expiry")
            .and_then(serde_json::Value::as_str)
            .map(timestamp)
            .transpose()?;
        acl.restrictions
            .insert(permission, Restriction::to_group(group, expires));
    }
    acl.read_kind = acl
        .restrictions
        .contains_key(scatter_actors::acl::READ)
        .then_some(ReadKind::Confidential);
    Ok(acl)
}

/// Runs an `acl` command.
pub async fn run_acl(cmd: AclCommand) -> Result<()> {
    let (at, target, restrictions) = match cmd {
        AclCommand::Show { at, target } => {
            let client = connect(&at.database).await?;
            let t = target_of(&target)?;
            let acl = triplespace_projections::acl_read::current(
                &client,
                &at.tenant,
                std::slice::from_ref(&t),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?
            .pop();
            println!("{}", serde_json::to_string_pretty(&acl)?);
            return Ok(());
        }
        AclCommand::Set {
            at,
            target,
            restrictions,
        } => (at, target, Some(restrictions)),
        AclCommand::Unset { at, target } => (at, target, None),
    };
    let t = target_of(&target)?;
    let client = connect(&at.database).await?;
    let farm = farm_of(&client).await?;
    let existing =
        triplespace_projections::acl_read::current(&client, &at.tenant, std::slice::from_ref(&t))
            .await
            .map_err(|e| anyhow::anyhow!(e))?
            .pop();
    let acl = restrictions
        .map(|text| acl_from(existing.as_ref(), &t, &text))
        .transpose()?;
    let named: BTreeSet<String> = acl
        .iter()
        .chain(existing.iter())
        .flat_map(|a| a.restrictions.values().filter_map(|r| r.group.clone()))
        .collect();
    let suppress = triplespace_accounts::groups::suppress_group(&client, &at.tenant).await?;
    let (store, pipeline) = store(&at.database, &farm)?;
    let mut cx = store.begin().await?;
    let out = write_acl(
        &store,
        &pipeline,
        &mut cx,
        &at.tenant,
        &farm.operator().to_string(),
        &operator(named),
        AclWrite {
            target: t.clone(),
            acl,
            reason: at.reason.clone(),
            suppress_group: suppress,
            reserve: None,
            enclosing: Vec::new(),
        },
        now(),
    )
    .await;
    match out {
        Ok(_) => {
            store.commit(cx).await?;
            println!("{} written", t.key());
            Ok(())
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(e.into())
        }
    }
}
