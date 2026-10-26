//! `prop=info`, `prop=templates` and `list=allpages` (compat `#mod-query+info`,
//! `#mod-query+templates`, `#mod-query+allpages`; 18 §2.3).
//!
//! `prop=templates` lists the pages a document page's latest text transcludes when it is
//! expanded (11 §1.1); an entity page and a page of another model transclude none. They are found by expanding the text when asked, since no
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
        m.insert("protection".into(), protection_of(ctx, page).await?);
        m.insert(
            "restrictiontypes".into(),
            json!(["edit", "move", "read", "create"]),
        );
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
    // The caller's own watch (16 §4.3): never anyone else's, and no watcher count.
    let target = match &page.id {
        Some(id) => Some((triplespace_accounts::watch::kind::ENTITY, id.to_string())),
        None if want("watched") || want("notificationtimestamp") => {
            crate::watching::target_of_page(ctx, page.page_id).await?
        }
        None => None,
    };
    watch_fields(ctx, target.as_ref(), &props, &mut m).await?;
    Ok(m)
}

/// `inprop=watched` and `inprop=notificationtimestamp` for the caller's own watch of a
/// target (16 §4.3): `watched`, `watchlistexpiry` and Triplespace's `ts-notify`; the time of
/// the first change they have not seen, or the empty string.
async fn watch_fields(
    ctx: &Ctx,
    target: Option<&(i16, String)>,
    props: &[String],
    m: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    let want = |p: &str| props.iter().any(|x| x == p);
    if !want("watched") && !want("notificationtimestamp") {
        return Ok(());
    }
    let Some(holder) = ctx.caller.identity.actor_key.clone() else {
        return Ok(());
    };
    let v2 = ctx.formatversion == 2;
    let yes = if v2 { json!(true) } else { json!("") };
    let w = match target {
        Some((k, i)) => triplespace_accounts::watch::get(ctx.db(), &holder, *k, i).await?,
        None => None,
    };
    if want("watched") {
        match &w {
            Some(w) => {
                m.insert("watched".into(), yes.clone());
                if let Some(e) = w.expires {
                    m.insert(
                        "watchlistexpiry".into(),
                        json!(iso8601(u64::try_from(e).unwrap_or(0))),
                    );
                }
                if w.notify {
                    m.insert("ts-notify".into(), yes.clone());
                }
            }
            None if v2 => {
                m.insert("watched".into(), json!(false));
            }
            None => {}
        }
    }
    if want("notificationtimestamp") {
        let t = match &w {
            Some(w) => crate::watching::first_unseen(ctx, &holder, w).await?,
            None => None,
        };
        m.insert(
            "notificationtimestamp".into(),
            json!(t.map_or(String::new(), |t| iso8601(u64::try_from(t).unwrap_or(0)))),
        );
    }
    Ok(())
}

/// `inprop=protection`, as MediaWiki lists it: one entry per restricted action along the
/// page's or entity's chain, `{type, level, expiry}` plus `source` for one an enclosing
/// target set (a namespace, a set, the tenant), which MediaWiki uses for cascading
/// protection. Only restrictions the caller already passed are listed, since the page was
/// resolved for it: a confidential `read` names the group the caller is in. A deletion's
/// `read` is not a protection and is left out; a confidential `read` that took the talk page
/// carries `ts-talk` (0056 A24).
async fn protection_of(ctx: &Ctx, page: &PageAsk) -> Result<Value, ApiError> {
    use scatter_actors::acl::{READ, ReadKind, Target};
    let tenant = &ctx.tenant.slug;
    let chain = match &page.id {
        Some(id) => {
            triplespace_projections::acl_read::entity_chain(ctx.db(), tenant, id.as_str(), None)
                .await
        }
        None => triplespace_projections::acl_read::page_chain(ctx.db(), tenant, page.page_id).await,
    }
    .map_err(ApiError::internal)?;
    let own = match &page.id {
        Some(id) => Target::Entity(id.as_str().to_string()),
        None => Target::Page(page.page_id),
    };
    let at = scatter_actors::Timestamp(crate::edit::now_micros());
    let mut out = Vec::new();
    for acl in &chain {
        for (permission, r) in &acl.restrictions {
            if !r.active_at(at) {
                continue;
            }
            if permission == READ && acl.read_kind == Some(ReadKind::Moderation) {
                continue;
            }
            let kind = match permission.as_str() {
                "createpage" => "create",
                p => p,
            };
            let mut e = json!({
                "type": kind,
                "level": r.group.clone().unwrap_or_default(),
                "expiry": match r.expires {
                    None => "infinity".to_string(),
                    Some(t) => iso8601(t.0),
                },
            });
            if acl.target != own {
                e["source"] = json!(acl.target.key());
            } else if permission == READ && acl.talk {
                // The restriction took the talk page with it (0056 A24).
                e["ts-talk"] = json!(true);
            }
            out.push(e);
        }
    }
    Ok(Value::Array(out))
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
    // A title with no page may still be restricted (`create`, `read`: 09 §4.8), and a
    // reserved one already is: `inprop=protection` says so, as the Protect form needs.
    if ctx.params.list("inprop").iter().any(|p| p == "protection") {
        for m in &mut set.missing {
            let Value::Object(entry) = m else { continue };
            if !entry.contains_key("missing") || !entry.contains_key("title") {
                continue;
            }
            entry.insert("restrictiontypes".into(), json!(["create", "read"]));
            let ns = entry["ns"].as_i64().unwrap_or(0);
            let prefixed = entry["title"].as_str().unwrap_or_default();
            let text = crate::pages::resolve_title(&site, prefixed).map(|t| t.text);
            let reserved = match (i32::try_from(ns), text) {
                (Ok(ns), Some(text)) => triplespace_projections::acl_read::reservation(
                    ctx.db(),
                    &ctx.tenant.slug,
                    ns,
                    &text,
                )
                .await
                .map_err(ApiError::internal)?,
                _ => None,
            };
            let protection = match reserved {
                Some(id) => {
                    let ask = PageAsk {
                        id: None,
                        doc: None,
                        page_id: id,
                        ns,
                        title: prefixed.to_string(),
                        only_revid: None,
                    };
                    protection_of(ctx, &ask).await?
                }
                None => json!([]),
            };
            entry.insert("protection".into(), protection);
        }
    }
    // A title with no page may be watched too (16 §4.3): the star on a talk page no thread
    // has made yet, or on a page to come.
    let props = ctx.params.list("inprop");
    if props
        .iter()
        .any(|p| p == "watched" || p == "notificationtimestamp")
    {
        for m in &mut set.missing {
            let Value::Object(entry) = m else { continue };
            let Some(title) = entry
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            if entry.contains_key("invalid") {
                continue;
            }
            let target = crate::watching::target_of_title(ctx, &site, &title).await?;
            watch_fields(ctx, target.as_ref(), &props, entry).await?;
        }
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
    let hidden = crate::pages::hidden(ctx).await?;
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
            Some(doc) if doc.model == "wikitext" => {
                match crate::pages::latest_text(ctx.db(), tenant, doc).await? {
                    Some(text) => {
                        crate::pages::expand(ctx.db(), tenant, &site, &hidden, &text)
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
           AND NOT (page_id = ANY($10))
         ORDER BY title COLLATE \"C\" {order} LIMIT $9"
    );
    let like = format!(
        "{}%",
        prefix
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    // Pages a confidential restriction hides from the caller are not listed (09 §5.3).
    let hidden = crate::pages::hidden(ctx).await?;
    let rows = if hidden.every_page() || hidden.namespaces.contains(&ns) {
        Vec::new()
    } else {
        ctx.db()
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
                    &hidden.page_ids(),
                ],
            )
            .await?
    };
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
