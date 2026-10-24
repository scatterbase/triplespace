//! `list=protectedpages`, `list=protectedtitles` and `list=protectedsets` (0016 §7; 0023
//! §8; 0056 §13; 18 §2.4): the restrictions in force on a tenant, filtered to those whose
//! targets the caller may read (09 §5.3), so that a confidential page is listed only to
//! its group. `Special:ProtectedPages` is served from these.
//!
//! - **`protectedpages`**: pages and entities with a restriction on `edit`, `move`,
//!   `read` or `createpage`, `prtype` to choose (`read` included, Triplespace's), `prlevel`
//!   for a group, `prnamespace`, `prlimit`. Each entry is MediaWiki's: `pageid`, `ns`,
//!   `title`, `type`, `level`, `expiry`, with `source` for an enclosing namespace, set or
//!   tenant restriction. Deletions are not protections and are left out.
//! - **`protectedtitles`**: reserved titles (09 §4.8), as MediaWiki lists create-protected
//!   titles: `ns`, `title`, `level`, `expiry`.
//! - **`protectedsets`** *(new)*: sets with `id`, `name`, their restrictions and their
//!   members, each member the caller may read; a set whose restriction the caller does not
//!   satisfy is absent.

use scatter_actors::Timestamp;
use scatter_actors::acl::{Acl, READ, ReadKind, Target};
use serde_json::{Map, Value, json};

use crate::api::Ctx;
use crate::entity_json::iso8601;
use crate::pages::{self, Site};
use crate::response::ApiError;

const MAX: usize = 500;

fn limit(ctx: &Ctx, name: &str) -> Result<usize, ApiError> {
    Ok(match ctx.params.get(name) {
        None => 10,
        Some("max") => MAX,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| ApiError::bad_value(name, v))?
            .clamp(1, MAX),
    })
}

fn expiry(t: Option<Timestamp>) -> Value {
    match t {
        None => json!("infinity"),
        Some(t) => json!(iso8601(t.0)),
    }
}

/// The ACLs of a tenant that restrict something, with a confidential or protecting
/// restriction in force now.
async fn acls(ctx: &Ctx) -> Result<Vec<Acl>, ApiError> {
    let rows = ctx
        .db()
        .query(
            "SELECT target, restrictions, read_kind, extra FROM view.acl WHERE tenant = $1
             ORDER BY \"offset\"",
            &[&ctx.tenant.slug],
        )
        .await?;
    let mut out = Vec::new();
    for r in &rows {
        let (_, acl) = triplespace_projections::acl_read::acl_of(r).map_err(ApiError::internal)?;
        out.push(acl);
    }
    Ok(out)
}

/// The title of a page or entity target the caller may read, with its namespace and page
/// ID; `None` where it may not, or the target is of another kind.
async fn readable_title(
    ctx: &Ctx,
    site: &Site,
    hidden: &pages::Hidden,
    target: &Target,
) -> Result<Option<(u64, i64, String)>, ApiError> {
    match target {
        Target::Page(id) => Ok(pages::by_id(ctx.db(), &ctx.tenant.slug, hidden, *id)
            .await?
            .map(|p| (p.page_id, i64::from(p.ns), site.prefixed(p.ns, &p.title)))),
        Target::Entity(id) => {
            let parsed = crate::modules::entities::parse_id(id)?;
            Ok(super::pageset::entity_ask(ctx, &parsed)
                .await?
                .map(|a| (a.page_id, a.ns, a.title)))
        }
        _ => Ok(None),
    }
}

/// `list=protectedpages`.
pub async fn pages(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let hidden = pages::hidden(ctx).await?;
    let types = ctx.params.list("prtype");
    let levels = ctx.params.list("prlevel");
    let namespaces: Vec<i64> = ctx
        .params
        .list("prnamespace")
        .iter()
        .map(|n| n.parse().map_err(|_| ApiError::bad_value("prnamespace", n)))
        .collect::<Result<_, _>>()?;
    let limit = limit(ctx, "prlimit")?;
    let offset: usize = match ctx.params.get("prcontinue") {
        Some(c) => c
            .parse()
            .map_err(|_| ApiError::bad_value("prcontinue", c))?,
        None => 0,
    };
    let at = Timestamp(crate::edit::now_micros());
    let mut out = Vec::new();
    for acl in acls(ctx).await? {
        if !matches!(acl.target, Target::Page(_) | Target::Entity(_)) {
            continue;
        }
        let Some((page_id, ns, title)) = readable_title(ctx, &site, &hidden, &acl.target).await?
        else {
            continue;
        };
        if !namespaces.is_empty() && !namespaces.contains(&ns) {
            continue;
        }
        for (permission, r) in &acl.restrictions {
            if !r.active_at(at)
                || (permission == READ && acl.read_kind == Some(ReadKind::Moderation))
            {
                continue;
            }
            let kind = if permission == "createpage" {
                "create"
            } else {
                permission.as_str()
            };
            if !types.is_empty() && !types.iter().any(|t| t == kind) {
                continue;
            }
            let level = r.group.clone().unwrap_or_default();
            if !levels.is_empty() && !levels.contains(&level) {
                continue;
            }
            out.push(json!({
                "pageid": page_id,
                "ns": ns,
                "title": title,
                "type": kind,
                "level": level,
                "expiry": expiry(r.expires),
            }));
        }
    }
    let page: Vec<Value> = out.iter().skip(offset).take(limit).cloned().collect();
    if out.len() > offset + limit {
        body.insert(
            "continue".into(),
            json!({"prcontinue": (offset + limit).to_string(), "continue": "-||"}),
        );
    }
    query.insert("protectedpages".into(), Value::Array(page));
    Ok(())
}

/// `list=protectedtitles`: reserved titles the caller may know of.
pub async fn titles(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let limit = limit(ctx, "ptlimit")?;
    let offset: usize = match ctx.params.get("ptcontinue") {
        Some(c) => c
            .parse()
            .map_err(|_| ApiError::bad_value("ptcontinue", c))?,
        None => 0,
    };
    let at = Timestamp(crate::edit::now_micros());
    let who = &ctx.caller.effective;
    let mut out = Vec::new();
    for acl in acls(ctx).await? {
        let (Target::Page(id), Some(reserved)) = (&acl.target, &acl.reserved) else {
            continue;
        };
        // Still a reservation (no revision has filled it), and known to the caller.
        let chain = triplespace_projections::acl_read::page_chain(ctx.db(), &ctx.tenant.slug, *id)
            .await
            .map_err(ApiError::internal)?;
        if scatter_actors::read_decision(who, &chain, at) == scatter_actors::ReadDecision::Absent {
            continue;
        }
        let ns = i32::try_from(reserved.namespace).unwrap_or_default();
        let held = triplespace_projections::acl_read::reservation(
            ctx.db(),
            &ctx.tenant.slug,
            ns,
            &reserved.title,
        )
        .await
        .map_err(ApiError::internal)?
        .is_some();
        if !held {
            continue;
        }
        let Some(r) = acl
            .restrictions
            .get("createpage")
            .filter(|r| r.active_at(at))
        else {
            continue;
        };
        out.push(json!({
            "ns": reserved.namespace,
            "title": site.prefixed(ns, &reserved.title),
            "level": r.group.clone().unwrap_or_default(),
            "expiry": expiry(r.expires),
        }));
    }
    let page: Vec<Value> = out.iter().skip(offset).take(limit).cloned().collect();
    if out.len() > offset + limit {
        body.insert(
            "continue".into(),
            json!({"ptcontinue": (offset + limit).to_string(), "continue": "-||"}),
        );
    }
    query.insert("protectedtitles".into(), Value::Array(page));
    Ok(())
}

/// `list=protectedsets`.
pub async fn sets(ctx: &Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let hidden = pages::hidden(ctx).await?;
    let at = Timestamp(crate::edit::now_micros());
    let who = &ctx.caller.effective;
    let mut out = Vec::new();
    for acl in acls(ctx).await? {
        let Target::Set(id) = acl.target else {
            continue;
        };
        if scatter_actors::read_decision(who, std::slice::from_ref(&acl), at)
            != scatter_actors::ReadDecision::Readable
        {
            continue;
        }
        let mut members = Vec::new();
        for m in &acl.members {
            let target = match m {
                scatter_actors::acl::SetMember::Page(p) => Target::Page(*p),
                scatter_actors::acl::SetMember::Entity(e) => Target::Entity(e.clone()),
            };
            if let Some((page_id, ns, title)) = readable_title(ctx, &site, &hidden, &target).await?
            {
                members.push(json!({"pageid": page_id, "ns": ns, "title": title}));
            }
        }
        let restrictions: Vec<Value> = acl
            .restrictions
            .iter()
            .filter(|(_, r)| r.active_at(at))
            .map(|(p, r)| {
                json!({"type": if p == "createpage" { "create" } else { p.as_str() },
                       "level": r.group.clone().unwrap_or_default(), "expiry": expiry(r.expires)})
            })
            .collect();
        out.push(json!({
            "id": id,
            "name": acl.name,
            "protection": restrictions,
            "members": members,
        }));
    }
    query.insert("protectedsets".into(), Value::Array(out));
    Ok(())
}
