//! `wbsearchentities` (compat §4.1; 0014 §8): the Postgres fallback — a prefix match on
//! `view.term` in the requested language (labels first, then aliases), the tenant's rows
//! and the shared rows together, in Wikibase's response shape. Its public form carries the
//! `Cache-Tag` of each entity it lists. Results filter by the caller's hidden set (09
//! §5.3): a confidential entity, one in a hidden set or namespace, and a deleted one are
//! not found. A graph restriction is not applied here (the fallback does not know which
//! graph serves a row); the resolver's reads do.

use serde_json::{Value, json};

use crate::api::Ctx;
use crate::entity_json::title_of;
use crate::response::{ApiError, ApiResponse};

/// Runs `wbsearchentities`.
pub async fn wbsearchentities(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.require("read")?;
    let search = ctx.params.required("search")?.to_string();
    let hidden = crate::pages::hidden(ctx).await?;
    if hidden.every_entity() {
        return Ok(nothing_found(&search));
    }
    find(ctx, &hidden).await
}

/// The `LIKE` pattern of a prefix search, its wildcards escaped.
fn prefix_pattern(search: &str) -> String {
    format!(
        "{}%",
        search
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

/// The search itself, for a caller who may read the tenant.
async fn find(
    ctx: &mut Ctx,
    hidden: &triplespace_projections::acl_read::Hidden,
) -> Result<ApiResponse, ApiError> {
    ctx.caller.require("read")?;
    let search = ctx.params.required("search")?.to_string();
    let language = ctx.params.required("language")?.to_string();
    let entity_type = ctx.params.get("type").unwrap_or("item").to_string();
    if !matches!(entity_type.as_str(), "item" | "property" | "domain") {
        return Err(ApiError::bad_value("type", &entity_type));
    }
    let limit = ctx.params.int("limit", 7, 1, 50)?;
    let offset = ctx.params.int("continue", 0, 0, 10_000)?;
    let strict = ctx.params.flag("strictlanguage");
    let languages: Vec<String> = if strict || language == "mul" {
        vec![language.clone()]
    } else {
        vec![language.clone(), "mul".into(), "en".into()]
    };
    let pattern = prefix_pattern(&search);
    // One row per matching entity: the best (label over alias, requested language over
    // fallbacks) term that matched.
    let rows = ctx
        .client
        .query(
            "SELECT DISTINCT ON (t.entity_id) t.entity_id, t.kind, t.lang, t.text, e.page_id
             FROM view.term t
             JOIN view.entity e ON e.tenant = t.tenant AND e.id = t.entity_id
             WHERE t.tenant IN ($1, '') AND t.kind IN (1, 3) AND t.lang = ANY($2)
               AND lower(t.text) LIKE $3 AND e.type = $4 AND NOT e.deleted
               AND NOT (e.id = ANY($7)) AND NOT (e.type = ANY($8))
             ORDER BY t.entity_id, t.kind, array_position($2, t.lang)
             LIMIT $5 OFFSET $6",
            &[
                &ctx.tenant.slug,
                &languages,
                &pattern,
                &entity_type,
                &i64::try_from(limit + 1).unwrap_or(51),
                &i64::try_from(offset).unwrap_or(0),
                &hidden.entity_ids(),
                &hidden.entity_types(),
            ],
        )
        .await?;
    let base = ctx.tenant.base.trim_end_matches('/');
    let public = ctx.tenant.public_base.trim_end_matches('/');
    let mut results = Vec::new();
    for r in rows.iter().take(usize::try_from(limit).unwrap_or(7)) {
        let id: String = r.get(0);
        let kind: i16 = r.get(1);
        let lang: String = r.get(2);
        let text: String = r.get(3);
        let page_id: i64 = r.get(4);
        let (label, description) = display_terms(ctx, &id, &languages).await?;
        // The public form names every entity it shows, so erasing one purges it (0014 §5).
        ctx.cache_tags.push(format!("entity:{id}"));
        let entity_id =
            scatter_wikibase_model::id::EntityId::parse(&id).map_err(ApiError::internal)?;
        let (_, title) = title_of(
            &entity_id,
            &scatter_wikibase_model::entity::EntityType::parse(&entity_type),
        );
        let mut entry = json!({
            "id": id,
            "title": title,
            "pageid": page_id,
            "repository": "",
            "url": format!("{public}/wiki/{}", title.replace(' ', "_")),
            "concepturi": format!("{base}/entity/{id}"),
            "display": {},
            "match": {
                "type": if kind == 1 { "label" } else { "alias" },
                "language": lang,
                "text": text,
            },
        });
        if let Some(l) = &label {
            entry["label"] = json!(l.1);
            entry["display"]["label"] = json!({"value": l.1, "language": l.0});
        }
        if let Some(d) = &description {
            entry["description"] = json!(d.1);
            entry["display"]["description"] = json!({"value": d.1, "language": d.0});
        }
        if kind == 3 {
            entry["aliases"] = json!([text]);
        }
        results.push(entry);
    }
    let mut body = json!({
        "searchinfo": {"search": search},
        "search": Value::Array(results),
        "success": 1,
    });
    if rows.len() > usize::try_from(limit).unwrap_or(7) {
        body["search-continue"] = json!(offset + limit);
    }
    Ok(ApiResponse::ok(body))
}

/// A search that finds nothing, in the shape of one that does: what a private tenant
/// answers those outside it (09 §4.7).
fn nothing_found(search: &str) -> ApiResponse {
    ApiResponse::ok(json!({
        "searchinfo": {"search": search},
        "search": [],
        "success": 1,
    }))
}

/// The label and description to display, in the first language that has one.
async fn display_terms(
    ctx: &Ctx,
    id: &str,
    languages: &[String],
) -> Result<(Option<(String, String)>, Option<(String, String)>), ApiError> {
    let rows = ctx
        .client
        .query(
            "SELECT kind, lang, text FROM view.term
             WHERE tenant IN ($1, '') AND entity_id = $2 AND kind IN (1, 2) AND lang = ANY($3)
             ORDER BY kind, array_position($3, lang), tenant DESC",
            &[&ctx.tenant.slug, &id, &languages],
        )
        .await?;
    let mut label = None;
    let mut description = None;
    for r in rows {
        let kind: i16 = r.get(0);
        let pair = (r.get::<_, String>(1), r.get::<_, String>(2));
        match kind {
            1 if label.is_none() => label = Some(pair),
            2 if description.is_none() => description = Some(pair),
            _ => {}
        }
    }
    Ok((label, description))
}

/// Runs `list=prefixsearch` (compat §3.3; 0010 §3; 0052 §6): the best-matching document
/// page titles for what a reader has typed, a case-insensitive prefix on `view.page`
/// (03 §11.2). `psnamespace` (a `|` list, `*` for every namespace that holds pages,
/// `0` by default) is ignored when `pssearch` begins with a namespace prefix, as in
/// MediaWiki. The entity namespaces hold no document pages, so they are never searched:
/// entities are found with `wbsearchentities`. An exact title comes first, then shorter
/// titles before longer, so that the closest match leads. Pages a confidential
/// restriction hides from the caller are not listed (09 §5.3).
pub async fn prefixsearch(
    ctx: &Ctx,
    query: &mut serde_json::Map<String, Value>,
    body: &mut serde_json::Map<String, Value>,
) -> Result<(), ApiError> {
    use scatter_titles::Namespaces;
    use scatter_wikitext::{Title, parse_target};

    ctx.caller.require("read")?;
    let search = ctx.params.required("pssearch")?.to_string();
    let limit = ctx.params.int("pslimit", 10, 1, 500)?;
    let offset = ctx.params.int("psoffset", 0, 0, 10_000)?;
    let site = crate::pages::Site::load(ctx).await?;
    let holds_pages = |ns: i32| {
        site.holds_pages(ns)
            && Namespaces::embedded()
                .get(ns)
                .is_some_and(|n| !n.is_entity_namespace())
    };
    let mut namespaces: Vec<i32> = match ctx.params.get("psnamespace") {
        None | Some("") => vec![0],
        Some("*") => Namespaces::embedded()
            .iter()
            .map(|n| n.id)
            .filter(|&ns| holds_pages(ns))
            .collect(),
        Some(list) => {
            let mut out = Vec::new();
            for n in list.split('|') {
                let ns: i32 = n
                    .parse()
                    .map_err(|_| ApiError::bad_value("psnamespace", n))?;
                out.push(ns);
            }
            out
        }
    };
    // A namespace prefix in the text picks that namespace; the text is what follows it.
    let prefix = match parse_target(&search, &site.config, &Title::new(0, "")) {
        Some(t) if t.title.interwiki.is_none() => {
            if t.title.namespace != 0 || search.trim_start().starts_with(':') {
                namespaces = vec![t.title.namespace];
            }
            t.title.text
        }
        // Not a title, or a title in another wiki: nothing here has it.
        _ => {
            query.insert("prefixsearch".into(), Value::Array(Vec::new()));
            return Ok(());
        }
    };
    let hidden = crate::pages::hidden(ctx).await?;
    namespaces.retain(|&ns| holds_pages(ns) && !hidden.namespaces.contains(&ns));
    namespaces.dedup();
    let rows = if hidden.every_page() || namespaces.is_empty() {
        Vec::new()
    } else {
        let lowered = prefix.to_lowercase();
        ctx.db()
            .query(
                "SELECT page_id, ns, title FROM view.page
                 WHERE tenant = $1 AND ns = ANY($2) AND NOT deleted AND latest_offset IS NOT NULL
                   AND lower(title) LIKE $3
                   AND NOT (page_id = ANY($4))
                 ORDER BY (lower(title) = $5) DESC, length(title), ns, title COLLATE \"C\"
                 LIMIT $6 OFFSET $7",
                &[
                    &ctx.tenant.slug,
                    &namespaces,
                    &prefix_pattern(&prefix),
                    &hidden.page_ids(),
                    &lowered,
                    &i64::try_from(limit + 1).unwrap_or(501),
                    &i64::try_from(offset).unwrap_or(0),
                ],
            )
            .await?
    };
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if i == usize::try_from(limit).unwrap_or(10) {
            body.insert(
                "continue".into(),
                json!({"psoffset": offset + limit, "continue": "-||"}),
            );
            break;
        }
        let ns: i32 = r.get(1);
        let title: String = r.get(2);
        out.push(json!({
            "ns": ns,
            "title": site.prefixed(ns, &title),
            "pageid": r.get::<_, i64>(0),
        }));
    }
    query.insert("prefixsearch".into(), Value::Array(out));
    Ok(())
}
