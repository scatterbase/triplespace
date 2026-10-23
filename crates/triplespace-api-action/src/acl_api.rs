//! The ACL routes (0016 §7; 0023 §8; 0056 §13; 18 §3.2): what `GET`, `PUT` and `DELETE`
//! on `/acl/{kind}/{id}` and `/acl/tenant/{slug}` do, shared by the REST handlers and,
//! later, the CLI. Every write goes through the one ACL writer
//! ([`triplespace_api_ingest::acl`]), which checks the rights, the lock-out of 09 §4.5 and
//! `security.restrictions`.
//!
//! **Targets served.** `page` (by page ID, a reservation included), `namespace` (by
//! number, a namespace that holds pages), `set` (by ID, with [`create_set`],
//! [`add_member`] and [`remove_member`]) and the tenant. Entities wait for their read
//! enforcement (tier 6); `record` and `actor` ACLs are written by `action=revisiondelete`
//! and, later, the block form; graph ACLs are deferred (09 §4.6). Any other kind is
//! `ts-not-supported`.
//!
//! **Absence.** A target the caller may not read answers as one that does not exist
//! (`ts-no-target`, 404), on every method (09 §5.3). A tenant's ACL is not secret: that a
//! tenant is private is the first thing a visitor learns (09 §4.7), so anyone may read it.
//!
//! **What a `PUT` sets.** The body's `restrictions` replace the target's: for each
//! permission, `group` and an optional `expiry` (`infinite` or a timestamp). A `read`
//! restriction set here is confidential (09 §5.2); deletion and hiding are the moderation
//! modules', and a deleted target's ACL is changed only by undeleting it (`ts-deleted`,
//! 409). Fields that are not restrictions, such as a reservation, are kept.

use std::collections::BTreeMap;

use scatter_actors::acl::{
    Acl, ReadDecision, ReadKind, Restriction, Target, check, read_decision, visibility,
};
use scatter_actors::{GroupRegistry, Timestamp};
use serde_json::{Map, Value, json};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::modules::moderation::{current, denied, expiry, groups, save};
use crate::response::ApiError;

/// The refusal for a target that does not exist, or that the caller may not read.
fn no_target(key: &str) -> ApiError {
    ApiError::new("ts-no-target", format!("There is no ACL target {key}."))
}

/// A target as the route names it: `{kind}/{id}`.
///
/// # Errors
///
/// `ts-not-supported` for a kind not served here, `badvalue` for a malformed ID.
pub fn target_of(kind: &str, id: &str) -> Result<Target, ApiError> {
    match kind {
        "page" => id
            .parse()
            .map(Target::Page)
            .map_err(|_| ApiError::bad_value("id", id)),
        "namespace" => id
            .parse()
            .map(Target::Namespace)
            .map_err(|_| ApiError::bad_value("id", id)),
        "set" => id
            .parse()
            .map(Target::Set)
            .map_err(|_| ApiError::bad_value("id", id)),
        "entity" | "statement" | "property" | "graph" | "record" | "actor" | "blob" => {
            Err(ApiError::new(
                "ts-not-supported",
                format!("ACLs on `{kind}` targets are not served by this route yet."),
            ))
        }
        other => Err(ApiError::bad_value("kind", other)),
    }
}

/// The ACLs that decide whether the caller may know the target exists and act on it: its
/// chain (09 §4.4). `None` when the target does not exist.
async fn chain_of(ctx: &Ctx, target: &Target) -> Result<Option<Vec<Acl>>, ApiError> {
    let tenant = ctx.tenant.slug.as_str();
    let internal = ApiError::internal;
    Ok(match target {
        Target::Page(id) => {
            let exists = ctx
                .db()
                .query_opt(
                    "SELECT 1 FROM view.page WHERE tenant = $1 AND page_id = $2",
                    &[&tenant, &i64::try_from(*id).unwrap_or(i64::MAX)],
                )
                .await?
                .is_some();
            if !exists {
                return Ok(None);
            }
            Some(
                triplespace_projections::acl_read::page_chain(ctx.db(), tenant, *id)
                    .await
                    .map_err(internal)?,
            )
        }
        Target::Namespace(n) => {
            let holds = i32::try_from(*n).ok().is_some_and(|ns| {
                scatter_titles::Namespaces::embedded()
                    .get(ns)
                    .is_some_and(|x| x.holds_pages(|_| false))
            });
            if !holds {
                return Ok(None);
            }
            Some(
                triplespace_projections::acl_read::current(
                    ctx.db(),
                    tenant,
                    &[
                        Target::Namespace(*n),
                        Target::Graph("pages".into()),
                        Target::Tenant(tenant.to_string()),
                    ],
                )
                .await
                .map_err(internal)?,
            )
        }
        Target::Set(_) => {
            let chain = triplespace_projections::acl_read::current(
                ctx.db(),
                tenant,
                &[target.clone(), Target::Tenant(tenant.to_string())],
            )
            .await
            .map_err(internal)?;
            // A set exists while its record does.
            chain.iter().any(|a| a.target == *target).then_some(chain)
        }
        Target::Tenant(slug) if *slug == ctx.tenant.slug => Some(
            triplespace_projections::acl_read::current(
                ctx.db(),
                tenant,
                std::slice::from_ref(target),
            )
            .await
            .map_err(internal)?,
        ),
        _ => None,
    })
}

/// The chain, when the caller may know the target: a confidential restriction it does not
/// satisfy makes the target absent, except a tenant's own, which is public (09 §4.7).
async fn known_chain(ctx: &Ctx, target: &Target) -> Result<Vec<Acl>, ApiError> {
    let chain = chain_of(ctx, target)
        .await?
        .ok_or_else(|| no_target(&target.key()))?;
    let at = Timestamp(now_micros());
    let absent = !matches!(target, Target::Tenant(_))
        && read_decision(&ctx.caller.effective, &chain, at) == ReadDecision::Absent;
    if absent {
        return Err(no_target(&target.key()));
    }
    Ok(chain)
}

/// The JSON of a target's ACL and the visibility its chain resolves to.
fn body(target: &Target, acl: Option<&Acl>, chain: &[Acl]) -> Value {
    let v = visibility(chain, Timestamp(now_micros()));
    json!({
        "target": target.key(),
        "acl": acl,
        "visibility": {
            "public": v.is_public(),
            "groups": v.groups,
        },
    })
}

/// `GET`: the target's ACL and its visibility.
///
/// # Errors
///
/// `ts-no-target` for a target that does not exist or that the caller may not read.
pub async fn read(ctx: &Ctx, target: &Target) -> Result<Value, ApiError> {
    ctx.caller.require("read")?;
    let chain = known_chain(ctx, target).await?;
    let acl = current(ctx, target).await?;
    Ok(body(target, acl.as_ref(), &chain))
}

/// Whether each group is one the tenant has: a registry default, a configured group, or
/// one with members. Nothing writes `group` records yet (a milestone-1 gap), so a group
/// that memberships name is taken as existing; a misspelt name is still caught.
async fn known_groups(ctx: &Ctx, names: &[String]) -> Result<(), ApiError> {
    let registry = GroupRegistry::default_registry();
    let configured: Vec<String> = ctx
        .db()
        .query(
            "SELECT name FROM view.\"group\" WHERE tenant IN ($1, '') AND name = ANY($2)
             UNION SELECT DISTINCT \"group\" FROM view.membership
             WHERE tenant IN ($1, '') AND \"group\" = ANY($2)",
            &[&ctx.tenant.slug, &names],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for n in names {
        if registry.by_name(n).is_none() && !configured.contains(n) {
            return Err(ApiError::new(
                "ts-unknown-group",
                format!("There is no group `{n}`."),
            ));
        }
    }
    Ok(())
}

/// The restrictions a `PUT` body names.
fn restrictions_of(body: &Map<String, Value>) -> Result<BTreeMap<String, Restriction>, ApiError> {
    let registry = GroupRegistry::default_registry();
    let Some(Value::Object(r)) = body.get("restrictions") else {
        return Err(ApiError::new(
            "badbody",
            "`restrictions` is an object: {permission: {group, expiry}}.",
        ));
    };
    let mut out = BTreeMap::new();
    for (permission, v) in r {
        if !registry.is_permission(permission) {
            return Err(ApiError::bad_value("restrictions", permission));
        }
        let group = v
            .get("group")
            .and_then(Value::as_str)
            .filter(|g| !g.is_empty())
            .ok_or_else(|| ApiError::new("badbody", format!("`{permission}` needs a `group`.")))?;
        let expires = match v.get("expiry").and_then(Value::as_str) {
            Some(text) => expiry(text)?,
            None => None,
        };
        out.insert(permission.clone(), Restriction::to_group(group, expires));
    }
    Ok(out)
}

/// The caller's own permission to change the target's ACL, checked on its chain as the
/// moderation modules check theirs (09 §4.4); the writer checks the right and the lock-out.
fn check_chain(ctx: &Ctx, target: &Target, chain: &[Acl]) -> Result<(), ApiError> {
    let right = target.protecting_right();
    check(right, &ctx.caller.effective, chain, Timestamp(now_micros())).map_err(|e| denied(&e))
}

/// `PUT`: replaces the target's restrictions.
///
/// # Errors
///
/// `ts-no-target`, `ts-deleted` (409), `badbody`, `ts-unknown-group`, and the writer's
/// refusals (`permissiondenied`, `ts-locked-out`, `ts-policy`).
pub async fn write(
    ctx: &Ctx,
    target: Target,
    body: &Map<String, Value>,
) -> Result<Value, ApiError> {
    ctx.caller.require("read")?;
    let chain = known_chain(ctx, &target).await?;
    check_chain(ctx, &target, &chain)?;
    let restrictions = restrictions_of(body)?;
    if restrictions.is_empty() {
        return Err(ApiError::new(
            "badbody",
            "`restrictions` is empty; DELETE retires an ACL.",
        ));
    }
    let names: Vec<String> = restrictions
        .values()
        .filter_map(|r| r.group.clone())
        .collect();
    known_groups(ctx, &names).await?;
    let existing = current(ctx, &target).await?;
    if existing
        .as_ref()
        .is_some_and(|a| a.read_kind == Some(ReadKind::Moderation))
    {
        return Err(ApiError::new(
            "ts-deleted",
            "The target is deleted; undelete it before changing its ACL.",
        ));
    }
    let mut acl = existing.unwrap_or_else(|| Acl::protect(target.clone(), &[], "", None));
    if let Some(name) = body.get("name").and_then(Value::as_str) {
        if !matches!(target, Target::Set(_)) {
            return Err(ApiError::new("badbody", "Only a set has a `name`."));
        }
        acl.name = Some(name.to_string());
    }
    acl.read_kind = restrictions
        .contains_key(scatter_actors::acl::READ)
        .then_some(ReadKind::Confidential);
    acl.restrictions = restrictions;
    acl.validate()
        .map_err(|e| ApiError::new("badbody", e.to_string()))?;
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (_, suppress_group) = groups(ctx).await?;
    let (logid, _) = save(
        ctx,
        target.clone(),
        Some(acl.clone()),
        reason,
        &suppress_group,
        None,
    )
    .await?;
    let chain = chain_of(ctx, &target).await?.unwrap_or_default();
    let mut out = body_of_write(&target, Some(&acl), &chain);
    out["logid"] = json!(logid);
    Ok(out)
}

/// `DELETE`: retires the target's ACL.
///
/// # Errors
///
/// As [`write`]; `ts-no-acl` when there is none.
pub async fn retire(ctx: &Ctx, target: Target, reason: &str) -> Result<Value, ApiError> {
    ctx.caller.require("read")?;
    let chain = known_chain(ctx, &target).await?;
    check_chain(ctx, &target, &chain)?;
    let existing = current(ctx, &target).await?;
    match &existing {
        None => {
            return Err(ApiError::new(
                "ts-no-acl",
                format!("{} has no ACL.", target.key()),
            ));
        }
        Some(a) if a.read_kind == Some(ReadKind::Moderation) => {
            return Err(ApiError::new(
                "ts-deleted",
                "The target is deleted; undelete it instead.",
            ));
        }
        Some(_) => {}
    }
    let (_, suppress_group) = groups(ctx).await?;
    let (logid, _) = save(ctx, target.clone(), None, reason, &suppress_group, None).await?;
    let chain = chain_of(ctx, &target).await?.unwrap_or_default();
    let mut out = body_of_write(&target, None, &chain);
    out["logid"] = json!(logid);
    Ok(out)
}

fn body_of_write(target: &Target, acl: Option<&Acl>, chain: &[Acl]) -> Value {
    body(target, acl, chain)
}

/// A set member as a request names it: `{kind: "page", id}`. Entities wait for their
/// read enforcement (tier 6), so an entity member is `ts-not-supported` for now.
fn member_of(kind: &str, id: &str) -> Result<scatter_actors::acl::SetMember, ApiError> {
    match kind {
        "page" => id
            .parse()
            .map(scatter_actors::acl::SetMember::Page)
            .map_err(|_| ApiError::bad_value("id", id)),
        "entity" => Err(ApiError::new(
            "ts-not-supported",
            "Entities cannot be set members until entity ACLs are enforced.",
        )),
        other => Err(ApiError::bad_value("kind", other)),
    }
}

/// A member the caller may add or remove: a page that exists and that the caller may
/// read; one it may not read answers as missing (09 §5.3).
async fn check_member(ctx: &Ctx, m: &scatter_actors::acl::SetMember) -> Result<(), ApiError> {
    if let scatter_actors::acl::SetMember::Page(p) = m {
        known_chain(ctx, &Target::Page(*p)).await?;
    }
    Ok(())
}

fn members_of(body: &Map<String, Value>) -> Result<Vec<scatter_actors::acl::SetMember>, ApiError> {
    let Some(list) = body.get("members") else {
        return Ok(Vec::new());
    };
    let Value::Array(list) = list else {
        return Err(ApiError::new(
            "badbody",
            "`members` is a list of {kind, id}.",
        ));
    };
    list.iter()
        .map(|m| {
            let kind = m.get("kind").and_then(Value::as_str).unwrap_or_default();
            let id = match m.get("id") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            };
            member_of(kind, &id)
        })
        .collect()
}

/// `POST /acl/set`: creates a set with a `name`, `restrictions` and `members`, under an
/// ID from the page-ID sequence (09 §4.7). The answer carries the ID.
///
/// # Errors
///
/// As [`write`], and `ts-no-target` for a member the caller may not read.
pub async fn create_set(ctx: &Ctx, body: &Map<String, Value>) -> Result<Value, ApiError> {
    ctx.caller.require("read")?;
    let tenant_chain = known_chain(ctx, &Target::Tenant(ctx.tenant.slug.clone())).await?;
    check_chain(ctx, &Target::Set(0), &tenant_chain)?;
    let restrictions = restrictions_of(body)?;
    if restrictions.is_empty() {
        return Err(ApiError::new(
            "badbody",
            "A set restricts something: `restrictions` is empty.",
        ));
    }
    let names: Vec<String> = restrictions
        .values()
        .filter_map(|r| r.group.clone())
        .collect();
    known_groups(ctx, &names).await?;
    let members = members_of(body)?;
    for m in &members {
        check_member(ctx, m).await?;
    }
    let mut acl = Acl::protect(Target::Set(0), &[], "", None);
    acl.read_kind = restrictions
        .contains_key(scatter_actors::acl::READ)
        .then_some(ReadKind::Confidential);
    acl.restrictions = restrictions;
    acl.name = body.get("name").and_then(Value::as_str).map(str::to_owned);
    acl.members = members;
    acl.validate()
        .map_err(|e| ApiError::new("badbody", e.to_string()))?;
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (_, suppress_group) = groups(ctx).await?;
    let (logid, target) = save(
        ctx,
        Target::Set(0),
        Some(acl),
        reason,
        &suppress_group,
        None,
    )
    .await?;
    let acl = current(ctx, &target).await?;
    let chain = chain_of(ctx, &target).await?.unwrap_or_default();
    let mut out = body_of_write(&target, acl.as_ref(), &chain);
    out["logid"] = json!(logid);
    if let Target::Set(id) = target {
        out["id"] = json!(id);
    }
    Ok(out)
}

/// Adds or removes one member of a set: a new version of the set's record, which needs
/// `protect` and membership in a confidential set's group, as any change to it does.
async fn change_member(
    ctx: &Ctx,
    set: u64,
    member: scatter_actors::acl::SetMember,
    add: bool,
    reason: &str,
) -> Result<Value, ApiError> {
    ctx.caller.require("read")?;
    let target = Target::Set(set);
    let chain = known_chain(ctx, &target).await?;
    check_chain(ctx, &target, &chain)?;
    check_member(ctx, &member).await?;
    let mut acl = current(ctx, &target)
        .await?
        .ok_or_else(|| no_target(&target.key()))?;
    let present = acl.members.contains(&member);
    match (add, present) {
        (true, true) => {
            return Err(ApiError::new(
                "ts-already-member",
                "The set already holds it.",
            ));
        }
        (false, false) => {
            return Err(ApiError::new("ts-no-target", "The set does not hold it."));
        }
        (true, false) => acl.members.push(member),
        (false, true) => acl.members.retain(|m| *m != member),
    }
    let (_, suppress_group) = groups(ctx).await?;
    let (logid, _) = save(
        ctx,
        target.clone(),
        Some(acl.clone()),
        reason,
        &suppress_group,
        None,
    )
    .await?;
    let chain = chain_of(ctx, &target).await?.unwrap_or_default();
    let mut out = body_of_write(&target, Some(&acl), &chain);
    out["logid"] = json!(logid);
    Ok(out)
}

/// `POST /acl/set/{id}/members`: adds `{kind, id}`.
///
/// # Errors
///
/// As [`write`]; `ts-already-member`.
pub async fn add_member(ctx: &Ctx, set: u64, body: &Map<String, Value>) -> Result<Value, ApiError> {
    let kind = body.get("kind").and_then(Value::as_str).unwrap_or_default();
    let id = match body.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err(ApiError::new("badbody", "`kind` and `id` name the member.")),
    };
    let member = member_of(kind, &id)?;
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    change_member(ctx, set, member, true, reason).await
}

/// `DELETE /acl/set/{id}/members/{kind}/{member}`.
///
/// # Errors
///
/// As [`write`]; `ts-no-target` for a member the set does not hold.
pub async fn remove_member(
    ctx: &Ctx,
    set: u64,
    kind: &str,
    id: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let member = member_of(kind, id)?;
    change_member(ctx, set, member, false, reason).await
}
