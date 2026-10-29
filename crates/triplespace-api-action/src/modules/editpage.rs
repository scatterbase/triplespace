//! `action=edit` (compat `#mod-edit`; 18 §2.3; 10 §3.2): creates or edits a document page
//! with text models only. An entity, thread or talk title is refused with MediaWiki's own
//! `no-direct-editing`.
//!
//! **The base** an edit is checked against is `baserevid` where given; else the revision
//! that was current at `basetimestamp`; else the page's latest, as MediaWiki takes an edit
//! with neither (0008 A40). A newer record than the base is `editconflict`, naming the
//! current revision, and the text is not saved.
//!
//! The text is `text`, or `appendtext`/`prependtext` around the current text; the
//! pre-save transform runs on `wikitext` (`~~~~`). Saving the text the page already has
//! is `nochange`, as in MediaWiki, and writes nothing. `createonly` and `nocreate` are
//! MediaWiki's; `section` is not supported yet. A User page may be created only for a
//! user who exists (namespaces.toml `creation_rule = "owner-exists"`); a Module page not
//! until Lua is built (`ts-namespace-disabled`, 11 §1.3).
//!
//! **ACLs** (09 §2, §4.8, §5.3): an edit is checked for `edit` against the page's chain;
//! a create for `edit` and `createpage` against the title's, which includes the ACL of a
//! reservation holding it, and takes the reserved page ID. A title held by a restriction
//! the editor does not satisfy, a reservation or a confidential page alike, is
//! `ts-restricted-title`; a protected page is `protectedpage`.

use scatter_ingest::IngestError;
use scatter_ingest::write::{Attestation, RawRecord, Request, write_raw};
use scatter_log::cbor::Value as Cbor;
use scatter_pages::{PAYLOAD_PAGE, PageOp, validate};
use scatter_projection::Backend as _;
use scatter_titles::Namespaces;
use serde_json::{Value, json};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::entity_json::iso8601;
use crate::modules::moderation;
use crate::modules::parse::pst;
use crate::pages::{self, Page, Site};
use crate::response::{ApiError, ApiResponse};

fn no_direct_editing(title: &str) -> ApiError {
    ApiError::new(
        "no-direct-editing",
        format!("Direct editing via API is not supported for content model of \"{title}\"."),
    )
}

/// The offset of a page's revision, for `baserevid`.
async fn offset_of_revid(ctx: &Ctx, page: &Page, revid: u64) -> Result<u64, ApiError> {
    let row = ctx
        .db()
        .query_opt(
            "SELECT \"offset\", target_id FROM view.activity
             WHERE tenant = $1 AND revid = $2 AND target_kind = 2",
            &[&ctx.tenant.slug, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await?
        .ok_or_else(|| {
            ApiError::new(
                "nosuchrevid",
                format!("There is no revision with ID {revid}."),
            )
        })?;
    if row.get::<_, Option<String>>(1) != Some(page.page_id.to_string()) {
        return Err(ApiError::new(
            "revwrongpage",
            format!("r{revid} is not a revision of {}.", page.title),
        ));
    }
    Ok(u64::try_from(row.get::<_, i64>(0)).unwrap_or(0))
}

/// The offset of the revision current at a time, for `basetimestamp`.
async fn offset_at(ctx: &Ctx, page: &Page, at: u64) -> Result<Option<u64>, ApiError> {
    // Timestamps are shown to the second, truncated (`iso8601`), and records are timed to
    // the microsecond: a whole-second `basetimestamp` names every record in that second,
    // or a client's base would resolve to the revision before the one it read.
    let at = if at.is_multiple_of(1_000_000) {
        at.saturating_add(999_999)
    } else {
        at
    };
    let row = ctx
        .db()
        .query_opt(
            "SELECT \"offset\" FROM view.activity
             WHERE tenant = $1 AND target_kind = 2 AND target_id = $2 AND time <= $3
             ORDER BY time DESC, \"offset\" DESC LIMIT 1",
            &[
                &ctx.tenant.slug,
                &page.page_id.to_string(),
                &triplespace_projections::common::time_of(at),
            ],
        )
        .await?;
    Ok(row.map(|r| u64::try_from(r.get::<_, i64>(0)).unwrap_or(0)))
}

/// Whether a user page's owner exists: the user named by the title's root.
async fn owner_exists(ctx: &Ctx, title: &str) -> Result<bool, ApiError> {
    let root = title.split('/').next().unwrap_or(title);
    let row = ctx
        .db()
        .query_opt(
            "SELECT 1 FROM view.actor WHERE issuer = $1 AND lower(name) = lower($2)
               AND kind IN ('registered', 'bot') AND status = 'active'",
            &[&ctx.tenant.issuer, &root],
        )
        .await?;
    Ok(row.is_some())
}

/// `action=edit`.
#[allow(clippy::too_many_lines)]
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require_somewhere("edit")?;
    let site = Site::load(ctx).await?;
    let tenant = ctx.tenant.slug.clone();
    let v2 = ctx.formatversion == 2;
    let flag = || if v2 { json!(true) } else { json!("") };
    // `watchlist` and `watchlistexpiry` are checked before anything is written.
    if let Some(w) = ctx.params.get("watchlist")
        && !matches!(w, "watch" | "unwatch" | "preferences" | "nochange")
    {
        return Err(ApiError::bad_value("watchlist", w));
    }
    if let Some(e) = ctx.params.get("watchlistexpiry") {
        super::moderation::expiry(e)?;
    }

    // The title.
    let (ns, title, existing) = if let Some(id) = ctx.params.get("pageid") {
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value("pageid", id))?;
        let page = pages::by_id(ctx.db(), &tenant, &pages::Hidden::NONE, n)
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
            })?;
        (page.ns, page.title.clone(), Some(page))
    } else {
        let name = ctx.params.required("title")?;
        if pages::names_entity(&site, name) {
            return Err(no_direct_editing(name));
        }
        let t = pages::resolve_title(&site, name)
            .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
        let page = pages::by_title(
            ctx.db(),
            &tenant,
            &pages::Hidden::NONE,
            t.namespace,
            &t.text,
        )
        .await?;
        (t.namespace, t.text, page)
    };
    let prefixed = site.prefixed(ns, &title);
    // Where a scoped group's rights would hold (09 §3.4): computed only for a caller who
    // has some.
    let scope = if ctx.caller.effective.scoped.is_empty() {
        Vec::new()
    } else {
        triplespace_projections::acl_read::title_scope(
            ctx.db(),
            &tenant,
            ns,
            &title,
            existing.as_ref().map(|p| p.page_id),
        )
        .await
        .map_err(ApiError::internal)?
    };
    let held = |permission: &str| {
        if ctx.caller.effective.holds_on(permission, &scope) {
            Ok(())
        } else {
            Err(ApiError::permission_denied(permission))
        }
    };
    held("edit")?;
    // Interface pages change what every reader sees (0095 §4), as MediaWiki's do.
    if ns == scatter_titles::INTERFACE {
        held("editinterface")?;
    }
    let namespace = Namespaces::embedded()
        .get(ns)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{prefixed}\".")))?;
    if !site.holds_pages(ns) {
        if namespace.enabled_by.is_some() {
            return Err(ApiError::new(
                "ts-namespace-disabled",
                format!(
                    "The {} namespace is disabled on this wiki.",
                    namespace.canonical
                ),
            ));
        }
        return Err(no_direct_editing(&prefixed));
    }
    if ctx.params.get("section").is_some_and(|s| !s.is_empty()) {
        return Err(ApiError::new(
            "ts-notsupported",
            "Editing a section is not supported yet; send the whole text.",
        ));
    }

    // The model.
    let models = scatter_pages::ContentModels::embedded();
    let current_model = existing.as_ref().map(|p| p.model.clone());
    let model = ctx
        .params
        .get("contentmodel")
        .map(str::to_string)
        .or_else(|| current_model.clone())
        .or_else(|| namespace.default_model.clone())
        .unwrap_or_else(|| "wikitext".into());
    let direct = models
        .get(&model)
        .is_some_and(|m| m.direct_editing && m.implemented);
    if !direct || !namespace.allows_model(&model) {
        return Err(no_direct_editing(&prefixed));
    }
    if current_model.as_deref().is_some_and(|m| m != model) {
        held("editcontentmodel")?;
    }

    // The text.
    let current_text = match &existing {
        Some(p) => pages::latest_text(ctx.db(), &tenant, p).await?,
        None => None,
    };
    let mut text = if let Some(t) = ctx.params.get("text") {
        t.to_string()
    } else if ctx.params.get("appendtext").is_some() || ctx.params.get("prependtext").is_some() {
        format!(
            "{}{}{}",
            ctx.params.get("prependtext").unwrap_or(""),
            current_text.as_deref().unwrap_or(""),
            ctx.params.get("appendtext").unwrap_or("")
        )
    } else {
        return Err(ApiError::missing_param("text"));
    };
    if model == "wikitext" {
        text = pst(ctx, &site, &text);
    }

    // Create or edit.
    let minor = ctx.params.flag("minor") && !ctx.params.flag("notminor");
    let (op, base) = if let Some(page) = &existing {
        if ctx.params.flag("createonly") {
            return Err(ApiError::new(
                "articleexists",
                "The article you tried to create has been created already.",
            ));
        }
        if current_text.as_deref() == Some(text.as_str())
            && current_model.as_deref() == Some(model.as_str())
        {
            let mut edit = serde_json::Map::new();
            edit.insert("result".into(), json!("Success"));
            edit.insert("pageid".into(), json!(page.page_id));
            edit.insert("title".into(), json!(prefixed));
            edit.insert("contentmodel".into(), json!(model));
            edit.insert("nochange".into(), flag());
            if let Some(actor) = ctx.caller.identity.actor_key.clone()
                && let Some(target) = crate::watching::target_of_page(ctx, page.page_id).await?
                && crate::watching::after_write(ctx, &actor, &target, crate::watching::Auto::Edit)
                    .await?
            {
                edit.insert("watched".into(), flag());
            }
            return Ok(ApiResponse::ok(json!({"edit": Value::Object(edit)})));
        }
        let base = if let Some(r) = ctx.params.get("baserevid") {
            let revid: u64 = r.parse().map_err(|_| ApiError::bad_value("baserevid", r))?;
            Some(offset_of_revid(ctx, page, revid).await?)
        } else if let Some(ts) = ctx.params.get("basetimestamp") {
            let at = triplespace_projections::feed::parse_timestamp(ts)
                .ok_or_else(|| ApiError::bad_value("basetimestamp", ts))?;
            offset_at(ctx, page, at).await?.or(page.latest_offset)
        } else {
            page.latest_offset
        };
        let op = PageOp::Edit {
            model: (current_model.as_deref() != Some(model.as_str())).then(|| model.clone()),
            minor,
            source: None,
        };
        (op, base)
    } else {
        if ctx.params.flag("nocreate") {
            return Err(ApiError::new(
                "missingtitle",
                "The page you specified doesn't exist.",
            ));
        }
        held("createpage")?;
        if ns == 2 && !owner_exists(ctx, &title).await? {
            return Err(ApiError::new(
                "ts-no-such-user",
                format!(
                    "A user page can be created only for an existing user; there is no user \"{}\".",
                    title.split('/').next().unwrap_or(&title)
                ),
            ));
        }
        let op = PageOp::Create {
            ns,
            title: title.clone(),
            model: model.clone(),
            minor,
            source: None,
        };
        (op, None)
    };
    validate(&op, Some(&text)).map_err(|e| ApiError::new("invalidtitle", e.to_string()))?;

    // The ACLs along the page's chain, or the title's while no page holds it.
    let at = scatter_actors::Timestamp(now_micros());
    let reserved = match &existing {
        Some(_) => None,
        None => triplespace_projections::acl_read::reservation(ctx.db(), &tenant, ns, &title)
            .await
            .map_err(ApiError::internal)?,
    };
    let chain = if let Some(p) = &existing {
        triplespace_projections::acl_read::page_chain(ctx.db(), &tenant, p.page_id).await
    } else {
        triplespace_projections::acl_read::title_chain(ctx.db(), &tenant, ns, &title, reserved)
            .await
    }
    .map_err(ApiError::internal)?;
    let who = &ctx.caller.effective;
    moderation::enforce("edit", who, &scope, &chain, at, &prefixed)?;
    if existing.is_none() {
        moderation::enforce("createpage", who, &scope, &chain, at, &prefixed).map_err(|e| {
            if e.code == "permissiondenied" {
                e
            } else {
                moderation::restricted_title(&prefixed)
            }
        })?;
    }

    // The record.
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("edit"))?;
    let mut attestation = Attestation::by(&actor);
    attestation.tags = ctx.params.list("tags");
    if ctx.params.flag("bot") && ctx.caller.effective.holds("bot") {
        attestation.tags.push("bot".into());
    }
    let now = now_micros();
    let mut request = Request::new(&tenant, attestation, now);
    request.comment = ctx
        .params
        .get("summary")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    request.base_offset = base;
    let store = ctx.app.store();
    let pipeline = ctx.app.pipeline();
    let mut cx = store.begin().await?;
    let raw = RawRecord {
        graph: "pages".into(),
        payload_type: PAYLOAD_PAGE.into(),
        key: existing.as_ref().map(|p| p.page_id.to_string()),
        page_id: existing.as_ref().map(|p| p.page_id).or(reserved),
        content: Cbor::from_json(&op.to_json()),
        extra: vec![Cbor::text(&text)],
    };
    let written = match write_raw(store, pipeline, &mut cx, &request, raw).await {
        Ok(w) => w,
        Err(IngestError::Conflict { latest, .. }) => {
            let _ = store.rollback(cx).await;
            return Err(ApiError::new("editconflict", "Edit conflict.")
                .with(
                    "currentrevid",
                    json!(existing.as_ref().and_then(|p| p.latest_revid)),
                )
                .with("latestoffset", json!(latest)));
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            let text = e.to_string();
            if text.contains("page_title") {
                return Err(ApiError::new(
                    "articleexists",
                    "The article you tried to create has been created already.",
                ));
            }
            return Err(ApiError::new("failed-save", text));
        }
    };
    store.commit(cx).await?;
    let h = written.record.header();
    let mut edit = serde_json::Map::new();
    if existing.is_none() {
        edit.insert("new".into(), flag());
    }
    edit.insert("result".into(), json!("Success"));
    edit.insert("pageid".into(), json!(h.page_id));
    edit.insert("title".into(), json!(prefixed));
    edit.insert("contentmodel".into(), json!(model));
    edit.insert(
        "oldrevid".into(),
        json!(existing.as_ref().and_then(|p| p.latest_revid).unwrap_or(0)),
    );
    edit.insert("newrevid".into(), json!(h.revid));
    edit.insert("newtimestamp".into(), json!(iso8601(now)));
    // The write is committed; the watch follows it (16 §4.3).
    let auto = if existing.is_none() {
        crate::watching::Auto::Create
    } else {
        crate::watching::Auto::Edit
    };
    if let Some(p) = h.page_id
        && let Some(target) = crate::watching::target_of_page(ctx, p).await?
        && crate::watching::after_write(ctx, &actor, &target, auto).await?
    {
        edit.insert("watched".into(), flag());
    }
    ctx.cache_tags.push(format!("p:{}", h.page_id.unwrap_or(0)));
    Ok(ApiResponse::ok(json!({"edit": Value::Object(edit)})))
}
