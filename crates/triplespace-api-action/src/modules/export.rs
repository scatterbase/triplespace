//! `action=query&export` and `exportnowrap` (0047 §8.1, §11; 21 §4.2; compat §5.4): the
//! current revisions of the page set (`titles=`, `pageids=`, `revids=`) as `Special:Export`
//! writes them, in MediaWiki XML, export schema 0.11. With `export` alone the XML is the
//! `query.export` string (under `*` in `formatversion=1`, as MediaWiki writes it); with
//! `exportnowrap` it is the whole response, as `text/xml`. `exportschema` is accepted;
//! 0.11 is the one written, and asking for 0.10 draws a warning. Export needs only
//! `read`, and the XML shows each revision as the export scope shows it to the caller
//! (01 §8). A page the caller may not know of is listed as missing, as one that does not
//! exist is.
//!
//! Also `list=categorymembers` (0038 §13), the listing `Special:Export`'s `addcat` adds
//! pages from: `cmtitle` (`Category:Name`, or the name), `cmlimit`; the members by sort
//! key, those the caller may read, as `view.page_category` keeps them. `cmtype`,
//! `cmsort`, continuation and the rest of MediaWiki's parameters are not built.

use serde_json::{Map, Value, json};
use triplespace_projections::pages::TenantNames;
use triplespace_rdf::bundle::Reader;
use triplespace_rdf::records::BuildError;
use triplespace_rdf::xml;

use crate::api::Ctx;
use crate::modules::pageset;
use crate::response::{ApiError, ApiResponse};

/// `export`: the XML of the page set's current revisions.
pub async fn export(
    ctx: &mut Ctx,
    query: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    if let Some(schema) = ctx.params.get("exportschema")
        && schema != "0.11"
    {
        response.warn(
            "query",
            &format!("Export schema {schema} is not written; the export is schema 0.11."),
        );
    }
    let site = crate::pages::Site::load(ctx).await?;
    let mut set = pageset::resolve(ctx, &site).await?;
    set.report(query);
    let mut selection = xml::Selection::default();
    for page in &set.pages {
        match &page.id {
            Some(id) => selection
                .entities
                .push((id.as_str().to_string(), page.title.clone())),
            None => selection.pages.push((page.page_id, page.title.clone())),
        }
    }
    let reader = Reader::new(ctx.db(), &ctx.tenant.slug, ctx.caller.effective.clone())
        .await
        .map_err(|_| {
            ApiError::new(
                "readapidenied",
                "You need read permission to use this module.",
            )
        })?;
    let (settings, _) =
        triplespace_accounts::settings::load(ctx.db(), Some(&ctx.tenant.slug)).await?;
    let text = |key: &str, or: &str| {
        settings
            .value(key)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| or.to_string())
    };
    let sitename = text("site.name", &ctx.tenant.slug);
    let lang = text("site.language", "en");
    let limit = settings
        .value("export.sync_revisions")
        .and_then(|v| v.as_u64())
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(5000);
    let mut names = TenantNames::load(ctx.db(), &ctx.tenant.slug)
        .await
        .map_err(ApiError::internal)?;
    names.site_name = Some(sitename.clone());
    let base = format!(
        "{}/wiki/{}",
        ctx.tenant.public_base.trim_end_matches('/'),
        super::query::MAIN_PAGE.replace(' ', "_")
    );
    let site_info = xml::site_info(
        &names,
        &lang,
        &sitename,
        &ctx.tenant.slug.replace('-', "_"),
        &base,
        &ctx.app.config().generator,
    );
    let export = match xml::build(
        ctx.db(),
        &ctx.tenant.slug,
        &reader,
        &selection,
        site_info,
        limit,
    )
    .await
    {
        Ok(x) => x,
        Err(e @ BuildError::TooLarge { .. }) => {
            return Err(ApiError::new("export-too-large", e.to_string()));
        }
        Err(e) => return Err(ApiError::internal(e)),
    };
    let xml = String::from_utf8_lossy(&export.xml).into_owned();
    ctx.cache_class = crate::http_cache::Class::Private;
    if ctx.params.flag("exportnowrap") {
        response.raw = Some(("text/xml; charset=utf-8", xml));
        return Ok(());
    }
    if ctx.formatversion == 2 {
        query.insert("export".into(), json!(xml));
    } else {
        query.insert("export".into(), json!({"*": xml}));
    }
    Ok(())
}

/// `list=categorymembers`.
pub async fn categorymembers(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let title = ctx.params.required("cmtitle")?;
    let limit = match ctx.params.get("cmlimit") {
        None => 10,
        Some("max") => 500,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| ApiError::bad_value("cmlimit", v))?
            .clamp(1, 500),
    };
    let site = crate::pages::Site::load(ctx).await?;
    let hidden = crate::pages::hidden(ctx).await?;
    let members = xml::pages_in_category(ctx.db(), &ctx.tenant.slug, title)
        .await
        .map_err(ApiError::internal)?;
    let mut rows: Vec<Value> = Vec::new();
    let mut more = false;
    for (page_id, ns, title) in members {
        if hidden.page(ns, page_id) {
            continue;
        }
        if rows.len() == limit {
            more = true;
            break;
        }
        rows.push(json!({
            "pageid": page_id,
            "ns": ns,
            "title": site.prefixed(ns, &title),
        }));
    }
    if more {
        body.insert(
            "continue".into(),
            json!({"cmcontinue": "", "continue": "-||"}),
        );
    }
    query.insert("categorymembers".into(), Value::Array(rows));
    Ok(())
}
