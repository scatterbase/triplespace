//! `action=move` (18 §2.3; 10 §2.5): renames a document page. A move is one `move`
//! record on the page and, where the old title may hold `wikitext`, a `create` record for
//! a new redirect page at the old title, both in one transaction with one base check
//! against the page's latest record. The log event is the `move/move` the `move` record
//! projects to, titled by the old title with the new one in its parameters.
//!
//! - `from` or `fromid`, `to`, `reason`, `tags`.
//! - `noredirect` leaves no redirect; needs `suppressredirect`. Where the old title cannot
//!   hold `wikitext` (a Table or Scope title, a Module's code page) no redirect is left
//!   either way, and the event says so.
//! - `movetalk` is accepted and does nothing: a talk page is attached to its subject by
//!   identifier, so its threads are already the new title's (0019 §2).
//! - `movesubpages` moves every subpage under the old title in the same transaction, each
//!   leaving its own redirect; needs `move-subpages`, and a namespace with subpages.
//!
//! **Which pages move.** A page whose model is a directly edited text model, to a title
//! in a namespace that allows that model (10 §1.4). An entity's, a thread's, a board's or
//! a talk page's title is not a page's to change: `immobilenamespace`, as MediaWiki
//! answers for a namespace that disallows moves.
//!
//! **ACLs at both ends** (09 §3.4, §4.8, §5.3). The source is checked as a moderation
//! module checks its page: a confidential page the caller may not read is missing, and
//! `move` and `edit` are tested against its chain and the scope of the caller's groups.
//! The destination is checked as a create is: a title another page holds is
//! `articleexists`, whether or not the caller may see that page; a title held by a
//! reservation or restriction the caller does not satisfy is `ts-restricted-title`; and
//! `createpage` and `edit` are tested against the title's chain and scope. A scoped group
//! therefore cannot carry a page out of its scope. MediaWiki's own gates hold as well:
//! `movefile` in the File namespace, `move-rootuserpages` for a user's root page,
//! `move-categorypages` for a category.
//!
//! **Moving over a redirect.** A title occupied by a redirect with one revision that
//! points at the page being moved is freed first: the redirect is deleted (a moderation
//! `read` ACL on its page ID, logged as `delete/delete` with the move's reason) in the
//! same transaction; needs `delete-redirect`. Any other occupant is `articleexists`.

use scatter_actors::acl::{Acl, READ, ReadKind, Restriction, Target};
use scatter_ingest::IngestError;
use scatter_ingest::store::{IngestStore as _, Sequence};
use scatter_ingest::write::{Attestation, RawRecord, Request, write_raw};
use scatter_log::cbor::Value as Cbor;
use scatter_pages::{PAYLOAD_PAGE, PageOp, validate};
use scatter_projection::Backend as _;
use scatter_titles::Namespaces;
use serde_json::{Value, json};
use triplespace_api_ingest::acl::{AclWrite, write as write_acl};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::modules::moderation;
use crate::pages::{self, Page, Site};
use crate::response::{ApiError, ApiResponse};

/// MediaWiki's refusal for a title whose namespace disallows moves.
fn immobile(title: &str) -> ApiError {
    ApiError::new(
        "immobilenamespace",
        format!("The page \"{title}\" cannot be moved: its namespace does not allow it."),
    )
}

/// Whether a title may hold a `wikitext` page, so that a redirect may be left there
/// (10 §2.5): the namespace's default model is `wikitext`, or it allows `wikitext` at
/// `/doc` subpages (Module, Table, Query, Scope).
fn takes_redirect(ns: i32, title: &str) -> bool {
    Namespaces::embedded().get(ns).is_some_and(|n| {
        n.default_model.as_deref() == Some("wikitext")
            || (n.allows_model("wikitext") && n.subpages && title.ends_with("/doc"))
    })
}

/// Whether a page of `model` may be moved: a directly edited text model, which excludes
/// entities, threads, boards and talk pages.
fn movable(ns: i32, model: &str) -> bool {
    let models = scatter_pages::ContentModels::embedded();
    models
        .get(model)
        .is_some_and(|m| m.direct_editing && m.implemented)
        && Namespaces::embedded()
            .get(ns)
            .is_some_and(|n| n.holds_pages(|_| true) && !n.is_entity_namespace())
}

/// One move to make: a page and where it goes.
struct Step {
    page: Page,
    ns: i32,
    title: String,
    /// The one-revision redirect at the destination to delete first, if any.
    displace: Option<u64>,
}

/// The occupant of a destination title, if any: `articleexists`, unless it is a redirect
/// with one revision pointing at `page`, which the move may displace with
/// `delete-redirect` (10 §2.5).
async fn occupant(
    ctx: &Ctx,
    page: &Page,
    ns: i32,
    title: &str,
    to: &str,
) -> Result<Option<u64>, ApiError> {
    let tenant = &ctx.tenant.slug;
    let Some(there) = pages::by_title(ctx.db(), tenant, &pages::Hidden::NONE, ns, title).await?
    else {
        return Ok(None);
    };
    let exists = || {
        ApiError::new(
            "articleexists",
            format!(
                "A page of that name already exists, or the name you have chosen is not valid: \"{to}\"."
            ),
        )
    };
    if !there.is_redirect || there.revisions != 1 {
        return Err(exists());
    }
    let points_here = ctx
        .db()
        .query_opt(
            "SELECT 1 FROM view.redirect WHERE tenant = $1 AND page_id = $2 AND target_page_id = $3",
            &[
                tenant,
                &i64::try_from(there.page_id).unwrap_or(i64::MAX),
                &i64::try_from(page.page_id).unwrap_or(i64::MAX),
            ],
        )
        .await?
        .is_some();
    if !points_here {
        return Err(exists());
    }
    if !ctx.caller.effective.holds("delete-redirect") {
        return Err(exists());
    }
    Ok(Some(there.page_id))
}

/// The subpages of a title, in a namespace with subpages: what `movesubpages` carries
/// along.
async fn subpages(ctx: &Ctx, ns: i32, title: &str) -> Result<Vec<Page>, ApiError> {
    let pattern = format!(
        "{}/%",
        title
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = ctx
        .db()
        .query(
            "SELECT page_id FROM view.page
             WHERE tenant = $1 AND ns = $2 AND title LIKE $3 AND NOT deleted AND latest_offset IS NOT NULL
             ORDER BY title",
            &[&ctx.tenant.slug, &ns, &pattern],
        )
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let id = u64::try_from(r.get::<_, i64>(0)).unwrap_or(0);
        if let Some(p) = pages::by_id(ctx.db(), &ctx.tenant.slug, &pages::Hidden::NONE, id).await? {
            out.push(p);
        }
    }
    Ok(out)
}

/// Checks one move at both ends (module docs), and finds what it displaces.
#[allow(clippy::too_many_lines)]
async fn check(
    ctx: &Ctx,
    site: &Site,
    page: Page,
    ns: i32,
    title: String,
) -> Result<Step, ApiError> {
    use scatter_actors::acl::{ReadDecision, read_decision};
    let tenant = ctx.tenant.slug.clone();
    let who = &ctx.caller.effective;
    let at = scatter_actors::Timestamp(now_micros());
    let from = site.prefixed(page.ns, &page.title);
    let to = site.prefixed(ns, &title);
    if !movable(page.ns, &page.model) {
        return Err(immobile(&from));
    }
    let dest = Namespaces::embedded()
        .get(ns)
        .filter(|n| site.holds_pages(ns) && !n.is_entity_namespace())
        .ok_or_else(|| immobile(&to))?;
    if !dest.allows_model(&page.model) {
        return Err(ApiError::new(
            "ts-model-not-allowed",
            format!(
                "The {} namespace does not hold {} pages.",
                if dest.canonical.is_empty() {
                    "main"
                } else {
                    &dest.canonical
                },
                page.model
            ),
        ));
    }
    if page.ns == ns && page.title == title {
        return Err(ApiError::new("selfmove", "Cannot move a page to itself."));
    }

    // The source: missing when confidential to the caller; then `move` and `edit`.
    let source_chain =
        triplespace_projections::acl_read::page_chain(ctx.db(), &tenant, page.page_id)
            .await
            .map_err(ApiError::internal)?;
    if read_decision(who, &source_chain, at) == ReadDecision::Absent {
        return Err(moderation_missing(ctx));
    }
    let source_scope = moderation::enclosing(ctx, &Target::Page(page.page_id), None).await?;
    let held = |permission: &str, scope: &[Target]| {
        if who.holds_on(permission, scope) {
            Ok(())
        } else {
            Err(ApiError::permission_denied(permission))
        }
    };
    held("move", &source_scope)?;
    held("edit", &source_scope)?;
    if page.ns == 6 || ns == 6 {
        held("movefile", &source_scope)?;
    }
    if page.ns == 2 && !page.title.contains('/') {
        held("move-rootuserpages", &source_scope)?;
    }
    if page.ns == 14 || ns == 14 {
        held("move-categorypages", &source_scope)?;
    }
    moderation::enforce("move", who, &source_scope, &source_chain, at, &from)?;
    moderation::enforce("edit", who, &source_scope, &source_chain, at, &from)?;

    // The destination: taken, restricted, or open to a create.
    let displace = occupant(ctx, &page, ns, &title, &to).await?;
    let reserved = triplespace_projections::acl_read::reservation(ctx.db(), &tenant, ns, &title)
        .await
        .map_err(ApiError::internal)?;
    let dest_scope = if who.scoped.is_empty() {
        Vec::new()
    } else {
        triplespace_projections::acl_read::title_scope(ctx.db(), &tenant, ns, &title, None)
            .await
            .map_err(ApiError::internal)?
    };
    held("move", &dest_scope)?;
    held("createpage", &dest_scope)?;
    held("edit", &dest_scope)?;
    let dest_chain =
        triplespace_projections::acl_read::title_chain(ctx.db(), &tenant, ns, &title, reserved)
            .await
            .map_err(ApiError::internal)?;
    // A reservation the caller does not satisfy, or a confidential title, is restricted;
    // one the caller satisfies is still a title taken, since the reservation holds its
    // own page ID (09 §4.8).
    for permission in ["createpage", "edit"] {
        moderation::enforce(permission, who, &dest_scope, &dest_chain, at, &to).map_err(|e| {
            if e.code == "permissiondenied" {
                e
            } else {
                moderation::restricted_title(&to)
            }
        })?;
    }
    if reserved.is_some() {
        return Err(moderation::restricted_title(&to));
    }
    let op = PageOp::Move {
        ns,
        title: title.clone(),
        redirect: None,
    };
    validate(&op, None).map_err(|e| ApiError::new("invalidtitle", e.to_string()))?;
    Ok(Step {
        page,
        ns,
        title,
        displace,
    })
}

/// The refusal for a source the caller may not know of.
fn moderation_missing(ctx: &Ctx) -> ApiError {
    match ctx.params.get("fromid") {
        Some(n) => ApiError::new("nosuchpageid", format!("There is no page with ID {n}.")),
        None => ApiError::new("missingtitle", "The page you specified doesn't exist."),
    }
}

/// `action=move`.
#[allow(clippy::too_many_lines)]
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require_somewhere("move")?;
    let site = Site::load(ctx).await?;
    let tenant = ctx.tenant.slug.clone();
    let v2 = ctx.formatversion == 2;
    let flag = || if v2 { json!(true) } else { json!("") };

    // The page, and where it goes.
    let page = if let Some(id) = ctx.params.get("fromid") {
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value("fromid", id))?;
        pages::by_id(ctx.db(), &tenant, &pages::Hidden::NONE, n)
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
            })?
    } else {
        let name = ctx.params.required("from")?;
        if pages::names_entity(&site, name) {
            return Err(immobile(name));
        }
        let t = pages::resolve_title(&site, name)
            .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
        pages::by_title(
            ctx.db(),
            &tenant,
            &pages::Hidden::NONE,
            t.namespace,
            &t.text,
        )
        .await?
        .ok_or_else(|| ApiError::new("missingtitle", "The page you specified doesn't exist."))?
    };
    let to_name = ctx.params.required("to")?;
    if pages::names_entity(&site, to_name) {
        return Err(immobile(to_name));
    }
    let to = pages::resolve_title(&site, to_name)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{to_name}\".")))?;
    let noredirect = ctx.params.flag("noredirect");
    if noredirect && !ctx.caller.effective.holds("suppressredirect") {
        return Err(ApiError::permission_denied("suppressredirect"));
    }
    let from_prefixed = site.prefixed(page.ns, &page.title);
    let to_prefixed = site.prefixed(to.namespace, &to.text);
    let with_subpages = ctx.params.flag("movesubpages");
    if with_subpages && !ctx.caller.effective.holds("move-subpages") {
        return Err(ApiError::permission_denied("move-subpages"));
    }
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();

    // Every move, checked before any is written.
    let (from_ns, from_title) = (page.ns, page.title.clone());
    let mut steps = vec![check(ctx, &site, page, to.namespace, to.text.clone()).await?];
    let mut skipped: Vec<Value> = Vec::new();
    if with_subpages
        && Namespaces::embedded()
            .get(from_ns)
            .is_some_and(|n| n.subpages)
    {
        for sub in subpages(ctx, from_ns, &from_title).await? {
            let rest = sub.title[from_title.len()..].to_string();
            let new_title = format!("{}{rest}", to.text);
            let old = site.prefixed(sub.ns, &sub.title);
            match check(ctx, &site, sub, to.namespace, new_title).await {
                Ok(step) => steps.push(step),
                Err(e) => {
                    skipped.push(json!({"from": old, "error": {"code": e.code, "info": e.info}}));
                }
            }
        }
    }

    // The records, in one transaction: for each page the redirect's deletion where one is
    // displaced, the `move`, and the redirect's `create`.
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("move"))?;
    let (delete_group, suppress_group) = moderation::groups(ctx).await?;
    // What deleting a displaced redirect needs, read before the transaction: its scope,
    // and its ACL where it has one, which keeps its other restrictions (0023 §8).
    let mut displaced = Vec::new();
    for step in &steps {
        if let Some(id) = step.displace {
            let target = Target::Page(id);
            let mut acl = moderation::current(ctx, &target).await?.unwrap_or_else(|| {
                Acl::read(target.clone(), ReadKind::Moderation, &delete_group, None)
            });
            if acl.read_kind == Some(ReadKind::Confidential) {
                acl.confidential = acl.restrictions.remove(READ);
                acl.confidential_talk = acl.talk;
            }
            acl.restrictions
                .insert(READ.into(), Restriction::to_group(&delete_group, None));
            acl.read_kind = Some(ReadKind::Moderation);
            displaced.push((id, acl, moderation::enclosing(ctx, &target, None).await?));
        }
    }
    let store = ctx.app.store();
    let pipeline = ctx.app.pipeline();
    let mut cx = store.begin().await?;
    let mut moved: Vec<Value> = Vec::new();
    let mut first_redirect: Option<u64> = None;
    let mut redirect_count = 0;
    for (i, step) in steps.iter().enumerate() {
        let now = now_micros();
        let old_prefixed = site.prefixed(step.page.ns, &step.page.title);
        let new_prefixed = site.prefixed(step.ns, &step.title);
        if let Some((id, acl, enclosing)) = step
            .displace
            .and_then(|id| displaced.iter().find(|(d, _, _)| *d == id))
            .map(|(id, acl, e)| (*id, acl.clone(), e.clone()))
        {
            let out = write_acl(
                store,
                pipeline,
                &mut cx,
                &tenant,
                &actor,
                &ctx.caller.effective,
                AclWrite {
                    target: Target::Page(id),
                    acl: Some(acl),
                    reason: format!("Moved over redirect: {old_prefixed} to {new_prefixed}"),
                    suppress_group: suppress_group.clone(),
                    reserve: None,
                    enclosing,
                },
                now,
            )
            .await;
            if let Err(e) = out {
                let _ = store.rollback(cx).await;
                return Err(ApiError::new("failed-save", e.to_string()));
            }
        }
        let leaves_redirect = !noredirect && takes_redirect(step.page.ns, &step.page.title);
        let redirect_id = if leaves_redirect {
            match store.next_id(&mut cx, &tenant, &Sequence::Page).await {
                Ok(id) => Some(id),
                Err(e) => {
                    let _ = store.rollback(cx).await;
                    return Err(ApiError::new("failed-save", e.to_string()));
                }
            }
        } else {
            None
        };
        let mut attestation = Attestation::by(&actor);
        attestation.tags = ctx.params.list("tags");
        let mut request = Request::new(&tenant, attestation, now);
        request.comment = Some(reason.clone()).filter(|r| !r.is_empty());
        request.base_offset = step.page.latest_offset;
        // A move is also a log event (10 §2.5), so its record takes a log ID.
        request.logid = match store.next_id(&mut cx, &tenant, &Sequence::Log).await {
            Ok(id) => Some(id),
            Err(e) => {
                let _ = store.rollback(cx).await;
                return Err(ApiError::new("failed-save", e.to_string()));
            }
        };
        let op = PageOp::Move {
            ns: step.ns,
            title: step.title.clone(),
            redirect: redirect_id,
        };
        let raw = RawRecord {
            graph: "pages".into(),
            payload_type: PAYLOAD_PAGE.into(),
            key: Some(step.page.page_id.to_string()),
            page_id: Some(step.page.page_id),
            content: Cbor::from_json(&op.to_json()),
            extra: Vec::new(),
        };
        if let Err(e) = write_raw(store, pipeline, &mut cx, &request, raw).await {
            let _ = store.rollback(cx).await;
            return Err(match e {
                IngestError::Conflict { .. } => ApiError::new("editconflict", "Edit conflict."),
                e if e.to_string().contains("page_title") => ApiError::new(
                    "articleexists",
                    format!("A page of that name already exists: \"{new_prefixed}\"."),
                ),
                e => ApiError::new("failed-save", e.to_string()),
            });
        }
        if let Some(rid) = redirect_id {
            let text = format!("#REDIRECT [[{new_prefixed}]]");
            let op = PageOp::Create {
                ns: step.page.ns,
                title: step.page.title.clone(),
                model: "wikitext".into(),
                minor: false,
                source: None,
            };
            let attestation = Attestation::by(&actor);
            let mut request = Request::new(&tenant, attestation, now);
            request.comment = Some(format!("{old_prefixed} moved to {new_prefixed}"));
            let raw = RawRecord {
                graph: "pages".into(),
                payload_type: PAYLOAD_PAGE.into(),
                key: None,
                page_id: Some(rid),
                content: Cbor::from_json(&op.to_json()),
                extra: vec![Cbor::text(&text)],
            };
            if let Err(e) = write_raw(store, pipeline, &mut cx, &request, raw).await {
                let _ = store.rollback(cx).await;
                return Err(ApiError::new("failed-save", e.to_string()));
            }
            redirect_count += 1;
            if i == 0 {
                first_redirect = Some(rid);
            }
        }
        ctx.cache_tags.push(format!("p:{}", step.page.page_id));
        if i > 0 {
            moved.push(json!({"from": old_prefixed, "to": new_prefixed}));
        }
    }
    store.commit(cx).await?;

    let mut out = serde_json::Map::new();
    out.insert("from".into(), json!(from_prefixed));
    out.insert("to".into(), json!(to_prefixed));
    out.insert("reason".into(), json!(reason));
    if first_redirect.is_some() {
        out.insert("redirectcreated".into(), flag());
    }
    if steps[0].displace.is_some() {
        out.insert("moveoverredirect".into(), flag());
    }
    if ctx.params.flag("movetalk") {
        // Talk pages follow their subject by identifier (10 §2.5): nothing moved.
        out.insert("talkmove-error-code".into(), json!("ts-talk-follows"));
        out.insert(
            "talkmove-error-info".into(),
            json!("A talk page follows its subject; it was not moved separately."),
        );
    }
    if with_subpages {
        moved.extend(skipped);
        out.insert("subpages".into(), Value::Array(moved));
    }
    out.insert("ts-redirects".into(), json!(redirect_count));
    Ok(ApiResponse::ok(json!({"move": Value::Object(out)})))
}
