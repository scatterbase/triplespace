//! `prop=info`, `prop=templates` and `list=allpages` (compat `#mod-query+info`,
//! `#mod-query+templates`, `#mod-query+allpages`; 18 §2.3).
//!
//! `prop=templates` lists the pages a document page transcludes, as its latest text
//! expands with the tenant's `wikitext.expansion` (11 §1.1): none while expansion is off,
//! and none for an entity page. They are found by expanding the text when asked, since no
//! projection keeps them yet; `tlnamespace` and `tltemplates` filter them. Pywikibot asks
//! for them before every save, to honour `{{nobots}}`.
//!
//! `prop=info` reports, for each page of the page set, its content model, language,
//! latest revision, length, when it last changed, and the `new` and `redirect` flags;
//! `inprop=protection` (none here yet), `url` and `displaytitle`. Combined with
//! `prop=revisions`, it adds its fields to the pages that one lists. `list=allpages` lists
//! a namespace's document pages in title order, with `apfrom`, `apto`, `apprefix`,
//! `apfilterredir`, `apminsize`, `apmaxsize`, `apdir`, `aplimit` and `apcontinue`; the
//! entity namespaces list nothing here, their pages being entities.

use serde_json::{Map, Value, json};

use crate::api::Ctx;
use crate::entity_json::iso8601;
use crate::modules::pageset::{self, PageAsk};
use crate::pages::Site;
use crate::response::{ApiError, ApiResponse};

const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 500;

async fn info_of(ctx: &Ctx, site: &Site, page: &PageAsk) -> Result<Map<String, Value>, ApiError> {
    let v2 = ctx.formatversion == 2;
    let props = ctx.params.list("inprop");
    let want = |p: &str| props.iter().any(|x| x == p);
    let mut m = Map::new();
    m.insert("pageid".into(), json!(page.page_id));
    m.insert("ns".into(), json!(page.ns));
    m.insert("title".into(), json!(page.title));
    m.insert("contentmodel".into(), json!(page.content_model()));
    m.insert("pagelanguage".into(), json!(site.lang));
    m.insert("pagelanguagehtmlcode".into(), json!(site.lang));
    m.insert("pagelanguagedir".into(), json!("ltr"));
    match (&page.doc, &page.id) {
        (Some(d), _) => {
            if let Some(t) = d.latest_at {
                m.insert("touched".into(), json!(iso8601(t)));
            }
            m.insert("lastrevid".into(), json!(d.latest_revid.unwrap_or(0)));
            m.insert("length".into(), json!(d.len));
            if d.is_redirect {
                m.insert("redirect".into(), if v2 { json!(true) } else { json!("") });
            }
            if d.revisions == 1 {
                m.insert("new".into(), if v2 { json!(true) } else { json!("") });
            }
        }
        (None, Some(id)) => {
            let row = ctx
                .db()
                .query_opt(
                    "SELECT local_revid, resolved_size FROM view.entity WHERE tenant IN ($1, '') AND id = $2
                     ORDER BY (tenant = $1) DESC LIMIT 1",
                    &[&ctx.tenant.slug, &id.as_str()],
                )
                .await?;
            if let Some(r) = row {
                m.insert(
                    "lastrevid".into(),
                    json!(r.get::<_, Option<i64>>(0).unwrap_or(0)),
                );
                m.insert("length".into(), json!(r.get::<_, i32>(1)));
            }
        }
        (None, None) => {}
    }
    if want("protection") {
        m.insert("protection".into(), json!([]));
        m.insert("restrictiontypes".into(), json!(["edit", "move"]));
    }
    if want("url") {
        let base = ctx.tenant.public_base.trim_end_matches('/');
        let encoded = scatter_wikitext::html::url_encode_title(&page.title);
        m.insert("fullurl".into(), json!(format!("{base}/wiki/{encoded}")));
        m.insert(
            "editurl".into(),
            json!(format!("{base}/w/index.php?title={encoded}&action=edit")),
        );
        m.insert(
            "canonicalurl".into(),
            json!(format!("{base}/wiki/{encoded}")),
        );
    }
    if want("displaytitle") {
        m.insert("displaytitle".into(), json!(page.title));
    }
    Ok(m)
}

/// `prop=info`: its own page list, or its fields added to the pages `prop=revisions`
/// already listed.
pub async fn info(ctx: &Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let mut set = pageset::resolve(ctx, &site).await?;
    let v2 = ctx.formatversion == 2;
    let mut infos = Vec::new();
    for p in &set.pages {
        infos.push(info_of(ctx, &site, p).await?);
    }
    merge_pages(query, &mut set, infos, v2);
    Ok(())
}

/// Puts a prop module's page entries into the query: added to the pages an earlier prop
/// module listed, or listed with the page set's report.
fn merge_pages(
    query: &mut Map<String, Value>,
    set: &mut pageset::PageSet,
    entries: Vec<Map<String, Value>>,
    v2: bool,
) {
    if let Some(existing) = query.get_mut("pages") {
        for entry in entries {
            let id = entry.get("pageid").cloned().unwrap_or(Value::Null);
            let target = match &mut *existing {
                Value::Array(list) => list.iter_mut().find(|p| p.get("pageid") == Some(&id)),
                Value::Object(map) => map.get_mut(&id.to_string()),
                _ => None,
            };
            match target {
                Some(Value::Object(m)) => m.extend(entry),
                _ => match &mut *existing {
                    Value::Array(list) => list.push(Value::Object(entry)),
                    Value::Object(map) => {
                        map.insert(id.to_string(), Value::Object(entry));
                    }
                    _ => {}
                },
            }
        }
        return;
    }
    let missing = std::mem::take(&mut set.missing);
    set.report(query);
    let list: Vec<Value> = entries
        .into_iter()
        .map(Value::Object)
        .chain(missing)
        .collect();
    query.insert(
        "pages".into(),
        if v2 { Value::Array(list) } else { keyed(list) },
    );
}

/// `prop=templates`: the pages each document page transcludes.
pub async fn templates(ctx: &Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let mut set = pageset::resolve(ctx, &site).await?;
    let v2 = ctx.formatversion == 2;
    let namespaces: Vec<i32> = ctx
        .params
        .list("tlnamespace")
        .iter()
        .map(|n| n.parse().map_err(|_| ApiError::bad_value("tlnamespace", n)))
        .collect::<Result<_, _>>()?;
    let only: Vec<String> = ctx
        .params
        .list("tltemplates")
        .iter()
        .filter_map(|t| crate::pages::resolve_title(&site, t))
        .map(|t| site.prefixed(t.namespace, &t.text))
        .collect();
    let tenant = ctx.tenant.slug.as_str();
    let mut entries = Vec::new();
    for p in &set.pages {
        let mut m = Map::new();
        m.insert("pageid".into(), json!(p.page_id));
        m.insert("ns".into(), json!(p.ns));
        m.insert("title".into(), json!(p.title));
        let used = match &p.doc {
            Some(doc) if site.expansion && doc.model == "wikitext" => {
                match crate::pages::latest_text(ctx.db(), tenant, doc).await? {
                    Some(text) => {
                        crate::pages::expand(ctx.db(), tenant, &site, &text)
                            .await?
                            .1
                    }
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        let mut list: Vec<Value> = Vec::new();
        for t in used {
            let title = site.prefixed(t.ns, &t.title);
            if (!namespaces.is_empty() && !namespaces.contains(&t.ns))
                || (!only.is_empty() && !only.contains(&title))
            {
                continue;
            }
            let entry = json!({"ns": t.ns, "title": title});
            if !list.contains(&entry) {
                list.push(entry);
            }
        }
        if !list.is_empty() {
            m.insert("templates".into(), Value::Array(list));
        }
        entries.push(m);
    }
    merge_pages(query, &mut set, entries, v2);
    Ok(())
}

/// Pages keyed by page ID, with missing ones under negative keys, as `formatversion=1`.
fn keyed(pages: Vec<Value>) -> Value {
    let mut out = Map::new();
    let mut negative = -1i64;
    for p in pages {
        let key = if let Some(id) = p.get("pageid").and_then(Value::as_u64) {
            id.to_string()
        } else {
            let k = negative.to_string();
            negative -= 1;
            k
        };
        out.insert(key, p);
    }
    Value::Object(out)
}

/// `list=allpages`.
#[allow(clippy::too_many_lines)]
pub async fn allpages(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let ns: i32 = match ctx.params.get("apnamespace") {
        Some(v) => v
            .parse()
            .map_err(|_| ApiError::bad_value("apnamespace", v))?,
        None => 0,
    };
    let limit = match ctx.params.get("aplimit") {
        None => DEFAULT_LIMIT,
        Some("max") => MAX_LIMIT,
        Some(v) => v
            .parse::<i64>()
            .map_err(|_| ApiError::bad_value("aplimit", v))?
            .clamp(1, MAX_LIMIT),
    };
    let title = |name: &str| name.replace('_', " ");
    let descending = match ctx.params.get("apdir").unwrap_or("ascending") {
        "ascending" | "newer" => false,
        "descending" | "older" => true,
        other => return Err(ApiError::bad_value("apdir", other)),
    };
    let from = ctx
        .params
        .get("apcontinue")
        .or_else(|| ctx.params.get("apfrom"))
        .map(title);
    let to = ctx.params.get("apto").map(title);
    let prefix = ctx.params.get("apprefix").map(title).unwrap_or_default();
    let redirects: Option<bool> = match ctx.params.get("apfilterredir").unwrap_or("all") {
        "all" => None,
        "redirects" => Some(true),
        "nonredirects" => Some(false),
        other => return Err(ApiError::bad_value("apfilterredir", other)),
    };
    let size_param = |name: &str| -> Result<Option<i32>, ApiError> {
        ctx.params
            .get(name)
            .map(|v| v.parse::<i32>().map_err(|_| ApiError::bad_value(name, v)))
            .transpose()
    };
    let (min, max) = (size_param("apminsize")?, size_param("apmaxsize")?);
    let (cmp_from, cmp_to, order) = if descending {
        ("<=", ">=", "DESC")
    } else {
        (">=", "<=", "ASC")
    };
    let sql = format!(
        "SELECT page_id, title FROM view.page
         WHERE tenant = $1 AND ns = $2 AND NOT deleted AND latest_offset IS NOT NULL
           AND ($3::text IS NULL OR title {cmp_from} $3)
           AND ($4::text IS NULL OR title {cmp_to} $4)
           AND title LIKE $5
           AND ($6::bool IS NULL OR is_redirect = $6)
           AND ($7::int IS NULL OR len >= $7)
           AND ($8::int IS NULL OR len <= $8)
         ORDER BY title COLLATE \"C\" {order} LIMIT $9"
    );
    let like = format!(
        "{}%",
        prefix
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = ctx
        .db()
        .query(
            &sql,
            &[
                &ctx.tenant.slug,
                &ns,
                &from,
                &to,
                &like,
                &redirects,
                &min,
                &max,
                &(limit + 1),
            ],
        )
        .await?;
    if scatter_titles::Namespaces::embedded()
        .get(ns)
        .is_some_and(scatter_titles::Namespace::is_entity_namespace)
    {
        response.warn(
            "allpages",
            "The entity namespaces list no document pages; list entities with wbsearchentities.",
        );
    }
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let t: String = r.get(1);
        if i64::try_from(i).unwrap_or(i64::MAX) == limit {
            body.insert(
                "continue".into(),
                json!({"apcontinue": t.replace(' ', "_"), "continue": "-||"}),
            );
            break;
        }
        out.push(json!({"pageid": r.get::<_, i64>(0), "ns": ns, "title": site.prefixed(ns, &t)}));
    }
    query.insert("allpages".into(), Value::Array(out));
    Ok(())
}
