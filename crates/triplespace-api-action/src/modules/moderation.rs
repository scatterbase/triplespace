//! The moderation modules (0023 §8; 18 §2.3; 09 §6): `action=delete`, `action=undelete`,
//! `action=protect` and `action=revisiondelete`, each an ACL record written through the one
//! writer ([`triplespace_api_ingest::acl`]).
//!
//! A target has one ACL, so each module merges into it: deleting a protected page adds a
//! moderation `read` restriction beside its protection, undeleting takes that restriction
//! away and retires the ACL only when nothing is left, and protecting a deleted page keeps
//! it deleted. Hiding merges the parts already hidden with those named.
//!
//! - **`action=delete`**: `title` or `pageid`, `reason`, `deletetalk`. Document pages and
//!   threads; an entity page is not deletable here yet (`ts-not-supported`).
//! - **`action=undelete`**: `title`, `reason`. The whole page; `timestamps` and `revisions`
//!   (partial undeletion) are not served yet. A title a new page has taken meanwhile is
//!   `ts-title-taken`.
//! - **`action=protect`**: `title` or `pageid`, `protections` (`edit=autoconfirmed|move=sysop`,
//!   `read={group}` for a confidential restriction, `{action}=all` to lift one), `expiry`
//!   (one, or one per protection), `reason`. `cascade` is `ts-no-cascade`. Create-protection
//!   of a missing title is not served yet.
//! - **`action=revisiondelete`**: `type=revision` (`archive` too), `ids`, `hide` and `show`
//!   (`content|comment|user`), `suppress` (`yes|no|nochange`), `reason`. For the revisions of
//!   document pages and thread posts, whose text is the `text` part, and of entities, whose
//!   change set is the `content` part.
//!
//! The deletion and suppression groups are the tenant's `moderation.delete_group` and
//! `moderation.suppress_group`.

use scatter_actors::Timestamp;
use scatter_actors::acl::{Acl, READ, ReadKind, Restriction, Target};
use scatter_projection::Backend as _;
use serde_json::{Value, json};
use triplespace_api_ingest::acl::{AclWrite, AclWriteError, write};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::pages::{self, Page, Site};
use crate::response::{ApiError, ApiResponse};

/// The tenant's deletion and suppression groups.
async fn groups(ctx: &Ctx) -> Result<(String, String), ApiError> {
    let (s, _) = triplespace_accounts::settings::load(ctx.db(), Some(&ctx.tenant.slug)).await?;
    let get = |k: &str, d: &str| {
        s.value(k)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| d.to_string())
    };
    Ok((
        get("moderation.delete_group", "sysop"),
        get("moderation.suppress_group", "suppress"),
    ))
}

fn refused(e: AclWriteError) -> ApiError {
    match e {
        AclWriteError::Denied(right) => ApiError::permission_denied(right),
        AclWriteError::LockedOut(g) => ApiError::new(
            "ts-locked-out",
            format!("A confidential restriction may name only a group you are in: {g}."),
        ),
        AclWriteError::Invalid(m) => ApiError::new("badvalue", m),
        AclWriteError::NothingToRetire(t) => {
            ApiError::new("cantundelete", format!("{t} is not restricted."))
        }
        AclWriteError::Store(m) => ApiError::internal(m),
    }
}

/// The target's current ACL.
async fn current(ctx: &Ctx, target: &Target) -> Result<Option<Acl>, ApiError> {
    Ok(triplespace_projections::acl_read::current(
        ctx.db(),
        &ctx.tenant.slug,
        std::slice::from_ref(target),
    )
    .await
    .map_err(ApiError::internal)?
    .pop())
}

/// Writes one ACL in its own unit of work and returns the record's log ID.
async fn save(
    ctx: &Ctx,
    target: Target,
    acl: Option<Acl>,
    reason: &str,
    suppress_group: &str,
) -> Result<Option<u64>, ApiError> {
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("delete"))?;
    let store = ctx.app.store();
    let mut cx = store.begin().await.map_err(ApiError::internal)?;
    let out = write(
        store,
        ctx.app.pipeline(),
        &mut cx,
        &ctx.tenant.slug,
        &actor,
        &ctx.caller.effective,
        AclWrite {
            target,
            acl,
            reason: reason.to_string(),
            suppress_group: suppress_group.to_string(),
        },
        now_micros(),
    )
    .await;
    match out {
        Ok(record) => {
            store.commit(cx).await.map_err(ApiError::internal)?;
            Ok(record.header().logid)
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(refused(e))
        }
    }
}

/// The page a request names by `title` or `pageid`, not deleted.
async fn page_of(ctx: &Ctx, site: &Site) -> Result<Page, ApiError> {
    let tenant = ctx.tenant.slug.clone();
    if let Some(id) = ctx.params.get("pageid") {
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value("pageid", id))?;
        return pages::by_id(ctx.db(), &tenant, n).await?.ok_or_else(|| {
            ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
        });
    }
    let name = ctx.params.required("title")?;
    if pages::names_entity(site, name) {
        return Err(ApiError::new(
            "ts-not-supported",
            "Deleting and protecting entity pages is not served here yet.",
        ));
    }
    let t = pages::resolve_title(site, name)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
    pages::by_title(ctx.db(), &tenant, t.namespace, &t.text)
        .await?
        .ok_or_else(|| ApiError::new("missingtitle", "The page you specified doesn't exist."))
}

/// `action=delete`.
pub async fn delete(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require("delete")?;
    let site = Site::load(ctx).await?;
    let page = page_of(ctx, &site).await?;
    let (delete_group, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let target = Target::Page(page.page_id);
    let mut acl = current(ctx, &target)
        .await?
        .unwrap_or_else(|| Acl::read(target.clone(), ReadKind::Moderation, &delete_group, None));
    acl.restrictions
        .insert(READ.into(), Restriction::to_group(&delete_group, None));
    acl.read_kind = Some(ReadKind::Moderation);
    acl.talk = ctx.params.flag("deletetalk");
    let logid = save(ctx, target, Some(acl), &reason, &suppress_group).await?;
    Ok(ApiResponse::ok(json!({"delete": {
        "title": site.prefixed(page.ns, &page.title),
        "reason": reason,
        "logid": logid,
    }})))
}

/// `action=undelete`.
pub async fn undelete(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require("undelete")?;
    if ctx.params.get("timestamps").is_some() || ctx.params.get("revisions").is_some() {
        return Err(ApiError::new(
            "ts-not-supported",
            "Undeleting some revisions and not others is not served yet.",
        ));
    }
    let site = Site::load(ctx).await?;
    let name = ctx.params.required("title")?.to_string();
    let t = pages::resolve_title(&site, &name)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
    let tenant = ctx.tenant.slug.clone();
    let row = ctx
        .db()
        .query_opt(
            "SELECT page_id FROM view.page WHERE tenant = $1 AND ns = $2 AND title = $3 AND deleted
             ORDER BY page_id DESC LIMIT 1",
            &[&tenant, &t.namespace, &t.text],
        )
        .await?
        .ok_or_else(|| {
            ApiError::new(
                "cantundelete",
                "There are no deleted revisions of this page.",
            )
        })?;
    let page_id = u64::try_from(row.get::<_, i64>(0)).unwrap_or(0);
    if pages::by_title(ctx.db(), &tenant, t.namespace, &t.text)
        .await?
        .is_some()
    {
        return Err(ApiError::new(
            "ts-title-taken",
            "A new page has taken this title; undeleting into it is refused.",
        ));
    }
    let (_, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let target = Target::Page(page_id);
    let acl = current(ctx, &target).await?.and_then(|mut a| {
        a.restrictions.remove(READ);
        a.read_kind = None;
        a.talk = false;
        (!a.restrictions.is_empty()).then_some(a)
    });
    save(ctx, target, acl, &reason, &suppress_group).await?;
    Ok(ApiResponse::ok(json!({"undelete": {
        "title": site.prefixed(t.namespace, &t.text),
        "revisions": 0,
        "fileversions": 0,
        "reason": reason,
    }})))
}

/// An expiry: `infinite` and its synonyms are none.
fn expiry(text: &str) -> Result<Option<Timestamp>, ApiError> {
    match text.trim() {
        "" | "infinite" | "indefinite" | "infinity" | "never" => Ok(None),
        t => triplespace_projections::feed::parse_timestamp(t)
            .map(|us| Some(Timestamp(us)))
            .ok_or_else(|| ApiError::new("invalidexpiry", format!("Invalid expiry time \"{t}\"."))),
    }
}

/// `action=protect`.
pub async fn protect(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require("protect")?;
    if ctx.params.flag("cascade") {
        return Err(ApiError::new(
            "ts-no-cascade",
            "There is no cascading protection.",
        ));
    }
    let site = Site::load(ctx).await?;
    let page = page_of(ctx, &site).await?;
    let (_, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let protections: Vec<(String, String)> = ctx
        .params
        .required("protections")?
        .split('|')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((a, g)) => Ok((a.trim().to_string(), g.trim().to_string())),
            None => Err(ApiError::new(
                "invalidprotection",
                format!("Invalid protection \"{p}\"."),
            )),
        })
        .collect::<Result<_, _>>()?;
    let expiries: Vec<&str> = ctx
        .params
        .get("expiry")
        .unwrap_or("infinite")
        .split('|')
        .collect();
    if expiries.len() != 1 && expiries.len() != protections.len() {
        return Err(ApiError::new(
            "toofewexpiries",
            format!(
                "{} expiry timestamps were provided where {} were needed.",
                expiries.len(),
                protections.len()
            ),
        ));
    }
    let target = Target::Page(page.page_id);
    let mut acl = current(ctx, &target)
        .await?
        .unwrap_or_else(|| Acl::protect(target.clone(), &[], "", None));
    let mut shown = Vec::new();
    for (k, (action, group)) in protections.iter().enumerate() {
        if !matches!(action.as_str(), "edit" | "move" | "read") {
            return Err(ApiError::new(
                "protect-invalidaction",
                format!("Invalid protection type \"{action}\"."),
            ));
        }
        let ex_text = expiries
            .get(k)
            .or_else(|| expiries.first())
            .copied()
            .unwrap_or("infinite");
        let ex = expiry(ex_text)?;
        if action == "read" && acl.read_kind == Some(ReadKind::Moderation) {
            return Err(ApiError::new(
                "ts-deleted",
                "The page is deleted; undelete it before restricting who may read it.",
            ));
        }
        if group == "all" || group.is_empty() {
            acl.restrictions.remove(action);
            if action == "read" {
                acl.read_kind = None;
            }
        } else {
            acl.restrictions
                .insert(action.clone(), Restriction::to_group(group, ex));
            if action == "read" {
                acl.read_kind = Some(ReadKind::Confidential);
            }
        }
        shown.push(json!({action.as_str(): group, "expiry": if ex.is_none() { "infinite".to_string() } else { ex_text.to_string() }}));
    }
    let acl = (!acl.restrictions.is_empty()).then_some(acl);
    if acl.is_none() && current(ctx, &target).await?.is_none() {
        // Nothing was protected and nothing is: there is nothing to write.
        return Ok(ApiResponse::ok(json!({"protect": {
            "title": site.prefixed(page.ns, &page.title),
            "reason": reason,
            "protections": shown,
        }})));
    }
    save(ctx, target, acl, &reason, &suppress_group).await?;
    Ok(ApiResponse::ok(json!({"protect": {
        "title": site.prefixed(page.ns, &page.title),
        "reason": reason,
        "protections": shown,
    }})))
}

/// The record a revision ID names: its partition and offset, and its graph.
async fn record_of(ctx: &Ctx, revid: u64) -> Result<Option<(u64, u64, String)>, ApiError> {
    let r = i64::try_from(revid).unwrap_or(i64::MAX);
    let row = ctx
        .db()
        .query_opt(
            "SELECT partition, \"offset\", source FROM view.activity
             WHERE tenant = $1 AND revid = $2 ORDER BY \"offset\" DESC LIMIT 1",
            &[&ctx.tenant.slug, &r],
        )
        .await?;
    Ok(row.map(|r| {
        (
            r.get::<_, i64>(0).cast_unsigned(),
            u64::try_from(r.get::<_, i64>(1)).unwrap_or(0),
            r.get::<_, String>(2),
        )
    }))
}

/// `action=revisiondelete`.
#[allow(clippy::too_many_lines)]
pub async fn revisiondelete(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    let kind = ctx.params.required("type")?.to_string();
    if !matches!(kind.as_str(), "revision" | "archive") {
        return Err(ApiError::new(
            "ts-not-supported",
            format!("type={kind} is not served yet."),
        ));
    }
    let names = |p: Option<&str>| -> Result<Vec<String>, ApiError> {
        p.unwrap_or_default()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(|s| match s {
                "content" | "comment" | "user" => Ok(s.to_string()),
                other => Err(ApiError::bad_value("hide", other)),
            })
            .collect()
    };
    let hide = names(ctx.params.get("hide"))?;
    let show = names(ctx.params.get("show"))?;
    if hide.is_empty() && show.is_empty() {
        return Err(ApiError::new(
            "revdelete-nooversight",
            "You must hide or show at least one part.",
        ));
    }
    if hide.iter().any(|h| show.contains(h)) {
        return Err(ApiError::new(
            "badparams",
            "A part cannot be both hidden and shown.",
        ));
    }
    let suppress = ctx.params.get("suppress").unwrap_or("nochange").to_string();
    let (delete_group, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let ids: Vec<u64> = ctx
        .params
        .required("ids")?
        .split('|')
        .map(|s| s.parse().map_err(|_| ApiError::bad_value("ids", s)))
        .collect::<Result<_, _>>()?;
    let mut items = Vec::new();
    for revid in ids {
        let Some((partition, offset, source)) = record_of(ctx, revid).await? else {
            items.push(json!({"id": revid, "status": "Fail", "errors": [{"code": "nosuchrevid"}]}));
            continue;
        };
        // The part a page's or a post's text is, or an entity change set's content.
        let content_part = if source == "pages" { "text" } else { "content" };
        let part = |n: &str| match n {
            "content" => content_part.to_string(),
            "comment" => "comment".to_string(),
            _ => "attestation".to_string(),
        };
        let target = Target::Record { partition, offset };
        let existing = current(ctx, &target).await?;
        let mut parts: Vec<String> = match &existing {
            Some(a) if a.parts.is_empty() => {
                vec![content_part.into(), "comment".into(), "attestation".into()]
            }
            Some(a) => a.parts.clone(),
            None => Vec::new(),
        };
        for h in &hide {
            let p = part(h);
            if !parts.contains(&p) {
                parts.push(p);
            }
        }
        parts.retain(|p| !show.iter().any(|s| part(s) == *p));
        parts.sort();
        let group = match suppress.as_str() {
            "yes" => suppress_group.clone(),
            "no" => delete_group.clone(),
            _ => existing
                .as_ref()
                .and_then(|a| a.restrictions.get(READ))
                .and_then(|r| r.group.clone())
                .unwrap_or_else(|| delete_group.clone()),
        };
        let acl = (!parts.is_empty()).then(|| {
            let mut a = Acl::read(target.clone(), ReadKind::Moderation, &group, None);
            a.parts.clone_from(&parts);
            a
        });
        if acl.is_none() && existing.is_none() {
            items.push(json!({"id": revid, "status": "Success"}));
            continue;
        }
        save(ctx, target, acl, &reason, &suppress_group).await?;
        let mut item = json!({"id": revid, "status": "Success"});
        for (flag, p) in [
            ("texthidden", content_part),
            ("commenthidden", "comment"),
            ("userhidden", "attestation"),
        ] {
            if parts.iter().any(|x| x == p) {
                item[flag] = Value::Bool(true);
            }
        }
        if group == suppress_group && !parts.is_empty() {
            item["suppressed"] = Value::Bool(true);
        }
        items.push(item);
    }
    Ok(ApiResponse::ok(json!({"revisiondelete": {
        "status": "Success",
        "type": kind,
        "items": items,
    }})))
}
