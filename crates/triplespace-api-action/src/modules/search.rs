//! Search without a search index (implements 0014 §8 (A31); 03 §11.6): the Postgres
//! fallback behind `wbsearchentities`, `list=prefixsearch`, `list=search` and the REST
//! API's `GET /suggest`.
//!
//! - **Entities** are found by a prefix of a label or alias in `view.term`, along a
//!   language chain, the tenant's rows and the shared rows (mirrored entities) together.
//!   A result is one per identity cluster, the member the local preference selects
//!   (`canonical_id`, 0004 §4) where the caller may read it. An exact match leads, then
//!   the entities most referred to (`view.entity_ref_count`, summed over the cluster:
//!   03 §4.2), then the shorter text.
//! - **Pages** are found by a prefix of their title on `view.page`, and by the words of
//!   their title and latest text in `view.page_text`.
//! - Every result filters by the caller's hidden set (09 §5.3; 03 §11.5): a confidential
//!   entity or page, one in a hidden set or namespace, and a deleted one are not found.
//!   A graph restriction is not applied here (the fallback does not know which graph
//!   serves a row); the resolver's reads do.
//!
//! The fallback has no `haswbstatement:`, no analysis (words are lower-cased and nothing
//! else) and no ranking beyond the above.

use serde_json::{Value, json};
use triplespace_projections::acl_read::Hidden;

use crate::api::Ctx;
use crate::entity_json::title_of;
use crate::pages::Site;
use crate::response::{ApiError, ApiResponse};

/// The most matching terms ranked for one entity search. A prefix as short as a letter
/// matches more than is worth counting references for: the exact and shortest matches
/// are kept, and the ranking is among them.
pub const CANDIDATES: i64 = 2000;

/// An entity a search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityHit {
    /// The entity's ID: its cluster's canonical member where the caller may read it.
    pub id: String,
    /// Its type: `item`, `property`, `domain`, …
    pub entity_type: String,
    /// Its page ID.
    pub page_id: i64,
    /// Whether an alias matched, rather than a label.
    pub alias: bool,
    /// The language of the text that matched.
    pub lang: String,
    /// The text that matched.
    pub text: String,
    /// The label to display, `(language, text)`, in the first language of the chain
    /// that has one.
    pub label: Option<(String, String)>,
    /// The description to display, likewise.
    pub description: Option<(String, String)>,
}

impl EntityHit {
    /// The page title: `Item:Q6`, `Domain:example.org`.
    #[must_use]
    pub fn title(&self) -> String {
        match scatter_wikibase_model::id::EntityId::parse(&self.id) {
            Ok(id) => {
                title_of(
                    &id,
                    &scatter_wikibase_model::entity::EntityType::parse(&self.entity_type),
                )
                .1
            }
            Err(_) => self.id.clone(),
        }
    }
}

/// A document page a search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHit {
    /// The page ID.
    pub page_id: i64,
    /// The namespace.
    pub ns: i32,
    /// The title within the namespace.
    pub title: String,
}

/// The languages a term search reads for a requested language (03 §11.6): itself, each
/// shorter prefix of its code (`de-at`, then `de`), then `mul` and English; itself alone
/// when `strict`, and for `mul`.
#[must_use]
pub fn language_chain(language: &str, strict: bool) -> Vec<String> {
    let mut out = vec![language.to_string()];
    if strict || language == "mul" {
        return out;
    }
    let mut code = language;
    while let Some((shorter, _)) = code.rsplit_once('-') {
        out.push(shorter.to_string());
        code = shorter;
    }
    for last in ["mul", "en"] {
        if !out.iter().any(|l| l == last) {
            out.push(last.to_string());
        }
    }
    out
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

/// The entities of `types` with a label or alias that begins with `search` in a language
/// of `chain`, from `offset`, at most `limit`; and whether there are more. One per
/// cluster, the best first (the module's ordering).
///
/// # Errors
///
/// The database's.
#[allow(clippy::too_many_arguments)]
pub async fn entity_hits(
    db: &tokio_postgres::Client,
    tenant: &str,
    hidden: &Hidden,
    chain: &[String],
    search: &str,
    types: &[String],
    limit: u32,
    offset: u32,
) -> Result<(Vec<EntityHit>, bool), tokio_postgres::Error> {
    if hidden.every_entity() || search.trim().is_empty() {
        return Ok((Vec::new(), false));
    }
    let rows = db
        .query(
            "WITH m AS (
               SELECT t.entity_id, t.kind, t.lang, t.text, lower(t.text) = $4 AS exact
               FROM view.term t
               WHERE t.tenant IN ($1, '') AND t.kind IN (1, 3) AND t.lang = ANY($2)
                 AND lower(t.text) LIKE $3
               ORDER BY exact DESC, length(t.text), t.entity_id
               LIMIT $9
             ), c AS (
               SELECT DISTINCT ON (r.id) r.id, r.type, r.page_id, m.kind, m.lang, m.text, m.exact,
                      e.cluster_id AS cluster
               FROM m
               JOIN view.entity e ON e.id = m.entity_id AND e.tenant IN ($1, '')
                 AND NOT e.deleted AND e.type = ANY($5)
                 AND NOT (e.id = ANY($7)) AND NOT (e.type = ANY($8))
               JOIN LATERAL (
                 SELECT x.id, x.type, x.page_id FROM view.entity x
                 WHERE x.tenant IN ($1, '') AND x.id IN (e.canonical_id, e.id) AND NOT x.deleted
                   AND NOT (x.id = ANY($7)) AND NOT (x.type = ANY($8))
                 ORDER BY (x.id = e.canonical_id) DESC, (x.tenant = $1) DESC
                 LIMIT 1) r ON true
               ORDER BY r.id, m.exact DESC, m.kind, array_position($2, m.lang), length(m.text)
             )
             SELECT c.id, c.type, c.page_id, c.kind, c.lang, c.text
             FROM c
             ORDER BY c.exact DESC,
                      (SELECT coalesce(sum(n.refs), 0) FROM view.entity_ref_count n
                       WHERE n.tenant IN ($1, '')
                         AND n.target_id = ANY(c.id || ARRAY(
                               SELECT x.id FROM view.entity x
                               WHERE x.tenant IN ($1, '') AND x.cluster_id = c.cluster))) DESC,
                      length(c.text), c.kind, c.id
             LIMIT $6 OFFSET $10",
            &[
                &tenant,
                &chain,
                &prefix_pattern(search),
                &search.to_lowercase(),
                &types,
                &i64::from(limit + 1),
                &hidden.entity_ids(),
                &hidden.entity_types(),
                &CANDIDATES,
                &i64::from(offset),
            ],
        )
        .await?;
    let more = rows.len() > limit as usize;
    let mut out = Vec::new();
    for r in rows.iter().take(limit as usize) {
        let id: String = r.get(0);
        let (label, description) = display_terms(db, tenant, &id, chain).await?;
        out.push(EntityHit {
            id,
            entity_type: r.get(1),
            page_id: r.get(2),
            alias: r.get::<_, i16>(3) == 3,
            lang: r.get(4),
            text: r.get(5),
            label,
            description,
        });
    }
    Ok((out, more))
}

/// The label and description to display, in the first language that has one.
async fn display_terms(
    db: &tokio_postgres::Client,
    tenant: &str,
    id: &str,
    languages: &[String],
) -> Result<(Option<(String, String)>, Option<(String, String)>), tokio_postgres::Error> {
    let rows = db
        .query(
            "SELECT kind, lang, text FROM view.term
             WHERE tenant IN ($1, '') AND entity_id = $2 AND kind IN (1, 2) AND lang = ANY($3)
             ORDER BY kind, array_position($3, lang), tenant DESC",
            &[&tenant, &id, &languages],
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

/// Runs `wbsearchentities` (compat §4.1): [`entity_hits`] in Wikibase's response shape.
/// Its public form carries the `Cache-Tag` of each entity it lists.
pub async fn wbsearchentities(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.require("read")?;
    let search = ctx.params.required("search")?.to_string();
    let hidden = crate::pages::hidden(ctx).await?;
    if hidden.every_entity() {
        return Ok(nothing_found(&search));
    }
    let language = ctx.params.required("language")?.to_string();
    let entity_type = ctx.params.get("type").unwrap_or("item").to_string();
    if !matches!(entity_type.as_str(), "item" | "property" | "domain") {
        return Err(ApiError::bad_value("type", &entity_type));
    }
    let limit = u32::try_from(ctx.params.int("limit", 7, 1, 50)?).unwrap_or(7);
    let offset = u32::try_from(ctx.params.int("continue", 0, 0, 10_000)?).unwrap_or(0);
    let chain = language_chain(&language, ctx.params.flag("strictlanguage"));
    let (hits, more) = entity_hits(
        ctx.db(),
        &ctx.tenant.slug,
        &hidden,
        &chain,
        &search,
        std::slice::from_ref(&entity_type),
        limit,
        offset,
    )
    .await?;
    let base = ctx.tenant.base.trim_end_matches('/').to_string();
    let public = ctx.tenant.public_base.trim_end_matches('/').to_string();
    let mut results = Vec::new();
    for h in &hits {
        // The public form names every entity it shows, so erasing one purges it (0014 §5).
        ctx.cache_tags.push(format!("entity:{}", h.id));
        let title = h.title();
        let mut entry = json!({
            "id": h.id,
            "title": title,
            "pageid": h.page_id,
            "repository": "",
            "url": format!("{public}/wiki/{}", title.replace(' ', "_")),
            "concepturi": format!("{base}/entity/{}", h.id),
            "display": {},
            "match": {
                "type": if h.alias { "alias" } else { "label" },
                "language": h.lang,
                "text": h.text,
            },
        });
        if let Some(l) = &h.label {
            entry["label"] = json!(l.1);
            entry["display"]["label"] = json!({"value": l.1, "language": l.0});
        }
        if let Some(d) = &h.description {
            entry["description"] = json!(d.1);
            entry["display"]["description"] = json!({"value": d.1, "language": d.0});
        }
        if h.alias {
            entry["aliases"] = json!([h.text]);
        }
        results.push(entry);
    }
    let mut body = json!({
        "searchinfo": {"search": search},
        "search": Value::Array(results),
        "success": 1,
    });
    if more {
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

/// The namespaces a page search covers and the text it looks for: `asked` (`None` for
/// the main namespace, an empty list for every namespace that holds document pages),
/// unless the text begins with a namespace prefix, which picks that namespace, as in
/// MediaWiki. `None` when the text is not a title here. The entity namespaces hold no
/// document pages and are never among them; nor is a namespace hidden from the caller.
fn page_scope(
    site: &Site,
    hidden: &Hidden,
    search: &str,
    asked: Option<Vec<i32>>,
) -> Option<(Vec<i32>, String)> {
    use scatter_wikitext::{Title, parse_target};

    // The tenant's table, its own namespaces included (0008 A54).
    let holds_pages = |ns: i32| {
        site.holds_pages(ns)
            && site
                .namespaces
                .get(ns)
                .is_some_and(|n| !n.is_entity_namespace())
    };
    let mut namespaces: Vec<i32> = match asked {
        None => vec![0],
        Some(list) if list.is_empty() => site.namespaces.iter().map(|n| n.id).collect(),
        Some(list) => list,
    };
    // A namespace prefix in the text picks that namespace; the text is what follows it.
    let t = parse_target(search, &site.config, &Title::new(0, ""))?;
    if t.title.interwiki.is_some() {
        return None;
    }
    if t.title.namespace != 0 || search.trim_start().starts_with(':') {
        namespaces = vec![t.title.namespace];
    }
    namespaces.retain(|&ns| holds_pages(ns) && !hidden.namespaces.contains(&ns));
    namespaces.sort_unstable();
    namespaces.dedup();
    Some((namespaces, t.title.text))
}

/// Whether `search` is the title of the page `hit`, whatever its case: what makes a page
/// a Go to (19 §2.1).
#[must_use]
pub fn names_page(site: &Site, search: &str, hit: &PageHit) -> bool {
    use scatter_wikitext::{Title, parse_target};
    parse_target(search, &site.config, &Title::new(0, "")).is_some_and(|t| {
        t.title.interwiki.is_none()
            && t.title.namespace == hit.ns
            && t.title.text.to_lowercase() == hit.title.to_lowercase()
    })
}

/// The document pages whose title begins with `search`, a case-insensitive prefix on
/// `view.page` (03 §11.2), from `offset`, at most `limit`; and whether there are more.
/// `namespaces` as `page_scope` reads them. An exact title comes first, then shorter
/// titles before longer, so that the closest match leads.
///
/// # Errors
///
/// The database's.
#[allow(clippy::too_many_arguments)]
pub async fn page_title_hits(
    db: &tokio_postgres::Client,
    tenant: &str,
    site: &Site,
    hidden: &Hidden,
    search: &str,
    namespaces: Option<Vec<i32>>,
    limit: u32,
    offset: u32,
) -> Result<(Vec<PageHit>, bool), tokio_postgres::Error> {
    let Some((namespaces, prefix)) = page_scope(site, hidden, search, namespaces) else {
        return Ok((Vec::new(), false));
    };
    if hidden.every_page() || namespaces.is_empty() {
        return Ok((Vec::new(), false));
    }
    let rows = db
        .query(
            "SELECT page_id, ns, title FROM view.page
             WHERE tenant = $1 AND ns = ANY($2) AND NOT deleted AND latest_offset IS NOT NULL
               AND lower(title) LIKE $3
               AND NOT (page_id = ANY($4))
             ORDER BY (lower(title) = $5) DESC, length(title), ns, title COLLATE \"C\"
             LIMIT $6 OFFSET $7",
            &[
                &tenant,
                &namespaces,
                &prefix_pattern(&prefix),
                &hidden.page_ids(),
                &prefix.to_lowercase(),
                &i64::from(limit + 1),
                &i64::from(offset),
            ],
        )
        .await?;
    let more = rows.len() > limit as usize;
    let hits = rows
        .iter()
        .take(limit as usize)
        .map(|r| PageHit {
            page_id: r.get(0),
            ns: r.get(1),
            title: r.get(2),
        })
        .collect();
    Ok((hits, more))
}

/// A `|` list of namespace numbers; `*` is the empty list, which stands for every
/// namespace; `None` when the parameter is absent.
fn namespace_param(ctx: &Ctx, name: &str) -> Result<Option<Vec<i32>>, ApiError> {
    match ctx.params.get(name) {
        None | Some("") => Ok(None),
        Some("*") => Ok(Some(Vec::new())),
        Some(list) => list
            .split('|')
            .map(|n| n.parse().map_err(|_| ApiError::bad_value(name, n)))
            .collect::<Result<Vec<i32>, _>>()
            .map(Some),
    }
}

/// Runs `list=prefixsearch` (compat §3.3; 0010 §3; 0052 §6): the best-matching document
/// page titles for what a reader has typed ([`page_title_hits`]). `psnamespace` (a `|`
/// list, `*` for every namespace that holds pages, `0` by default) is ignored when
/// `pssearch` begins with a namespace prefix, as in MediaWiki. The entity namespaces hold
/// no document pages, so they are never searched: entities are found with
/// `wbsearchentities`. Pages a confidential restriction hides from the caller are not
/// listed (09 §5.3).
pub async fn prefixsearch(
    ctx: &Ctx,
    query: &mut serde_json::Map<String, Value>,
    body: &mut serde_json::Map<String, Value>,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let search = ctx.params.required("pssearch")?.to_string();
    let limit = u32::try_from(ctx.params.int("pslimit", 10, 1, 500)?).unwrap_or(10);
    let offset = u32::try_from(ctx.params.int("psoffset", 0, 0, 10_000)?).unwrap_or(0);
    let namespaces = namespace_param(ctx, "psnamespace")?;
    let site = Site::load(ctx).await?;
    let hidden = crate::pages::hidden(ctx).await?;
    let (hits, more) = page_title_hits(
        ctx.db(),
        &ctx.tenant.slug,
        &site,
        &hidden,
        &search,
        namespaces,
        limit,
        offset,
    )
    .await?;
    if more {
        body.insert(
            "continue".into(),
            json!({"psoffset": offset + limit, "continue": "-||"}),
        );
    }
    let out = hits
        .iter()
        .map(|h| {
            json!({
                "ns": h.ns,
                "title": site.prefixed(h.ns, &h.title),
                "pageid": h.page_id,
            })
        })
        .collect();
    query.insert("prefixsearch".into(), Value::Array(out));
    Ok(())
}

/// The words of a search text, lower-cased: what a snippet marks.
fn words(search: &str) -> Vec<String> {
    search
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A page's stored wikitext as plain words, roughly, for a snippet: tags, link brackets
/// and targets (`[[a|b]]` is `b`), quote marks of emphasis, heading marks and template
/// braces are taken out. Not a rendering: a template call leaves its name and arguments.
#[must_use]
pub fn plain(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let starts = |i: usize, what: &str| {
        what.chars()
            .enumerate()
            .all(|(n, c)| chars.get(i + n) == Some(&c))
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '<' {
            // A tag, where it closes on the line and soon.
            let close = chars[i..]
                .iter()
                .take(300)
                .position(|&x| x == '>' || x == '\n');
            if let Some(n) = close.filter(|&n| chars[i + n] == '>') {
                i += n + 1;
                continue;
            }
        }
        if starts(i, "[[") {
            // What the link shows: the text after the last `|` before `]]`, or the target.
            let end = (i + 2..chars.len().saturating_sub(1))
                .take(300)
                .find(|&n| chars[n] == ']' && chars[n + 1] == ']');
            if let Some(end) = end {
                let inner: String = chars[i + 2..end].iter().collect();
                out.push_str(inner.rsplit('|').next().unwrap_or(&inner));
                i = end + 2;
                continue;
            }
        }
        if starts(i, "''") || starts(i, "{{") || starts(i, "}}") || starts(i, "==") {
            while i < chars.len() && chars[i] == c {
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// About how many characters of a text a snippet shows.
const SNIPPET_CHARS: usize = 200;

/// A snippet of `text` around the first of `words` it holds, as MediaWiki's: HTML, the
/// text escaped and each word found wrapped in `<span class="searchmatch">`. The start
/// of the text where it holds none. With `prefix`, a word that begins with one of `words`
/// is found, as a term or title that matched by its prefix was; without, only the word
/// itself, as the text index matches. The text is the stored source; the fallback does
/// not render it.
#[must_use]
pub fn snippet(text: &str, words: &[String], prefix: bool) -> String {
    // Each run of letters and digits with where it starts, in characters.
    let chars: Vec<char> = text.chars().collect();
    let is_word = |c: char| c.is_alphanumeric();
    let hit = |from: usize, to: usize| {
        let w: String = chars[from..to]
            .iter()
            .flat_map(|c| c.to_lowercase())
            .collect();
        if prefix {
            words.iter().any(|x| w.starts_with(x.as_str()))
        } else {
            words.contains(&w)
        }
    };
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_word(chars[i]) {
            let from = i;
            while i < chars.len() && is_word(chars[i]) {
                i += 1;
            }
            runs.push((from, i));
        } else {
            i += 1;
        }
    }
    let first = runs.iter().find(|(a, b)| hit(*a, *b)).map_or(0, |r| r.0);
    // A window that begins a little before the first word found, at a word's start.
    let mut from = first.saturating_sub(SNIPPET_CHARS / 4);
    while from > 0 && from < chars.len() && is_word(chars[from - 1]) {
        from -= 1;
    }
    let mut to = (from + SNIPPET_CHARS).min(chars.len());
    while to < chars.len() && is_word(chars[to]) {
        to += 1;
    }
    let escape = |out: &mut String, c: char| match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        '\n' | '\r' | '\t' => out.push(' '),
        c => out.push(c),
    };
    let mut out = String::new();
    let mut at = from;
    for &(a, b) in runs.iter().filter(|(a, b)| *a >= from && *b <= to) {
        if !hit(a, b) {
            continue;
        }
        chars[at..a].iter().for_each(|c| escape(&mut out, *c));
        out.push_str("<span class=\"searchmatch\">");
        chars[a..b].iter().for_each(|c| escape(&mut out, *c));
        out.push_str("</span>");
        at = b;
    }
    chars[at..to].iter().for_each(|c| escape(&mut out, *c));
    out.trim().to_string()
}

/// The namespace of each entity type the fallback searches.
const ENTITY_KINDS: [(&str, i32); 3] = [("item", 120), ("property", 122), ("domain", 210)];

/// Runs `list=search` (compat §3.3; 03 §11.2, §11.6): full text over document pages,
/// and entities by their terms.
///
/// - `srnamespace` (a `|` list, `*` for every namespace that holds document pages, `0`
///   by default) says where; a namespace prefix at the start of `srsearch` picks that
///   namespace for pages, as in MediaWiki.
/// - **Pages** match where their title or latest text holds every word of `srsearch`
///   (`view.page_text`; quoted phrases, `or` and `-word` as `websearch_to_tsquery` reads
///   them), or where their title begins with it. Redirects are not listed. An exact
///   title leads, then titles that begin with the text, then by how well the words
///   match, the title's words above the text's. `srwhat=title` matches titles alone,
///   `srwhat=text` the words alone.
/// - **Entities**, for an entity namespace named in `srnamespace` (`*` does not name
///   them, as in `list=prefixsearch`), match by a prefix of a label or alias in `uselang`
///   and its fallbacks ([`entity_hits`]), and are listed before pages; their snippet is
///   the label and description.
/// - A page's snippet is cut from its stored text with the commonest markup taken out
///   ([`plain`]); the fallback does not render the page.
/// - `srprop`: `size`, `wordcount`, `timestamp`, `snippet` (the default four) and
///   `titlesnippet`. `srinfo=totalhits` (the default) counts the pages that match.
/// - What a confidential restriction hides from the caller is not listed (09 §5.3).
#[allow(clippy::too_many_lines)]
pub async fn search(
    ctx: &Ctx,
    query: &mut serde_json::Map<String, Value>,
    body: &mut serde_json::Map<String, Value>,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let text = ctx.params.required("srsearch")?.to_string();
    let limit = u32::try_from(ctx.params.int("srlimit", 10, 1, 500)?).unwrap_or(10);
    let offset = u32::try_from(ctx.params.int("sroffset", 0, 0, 10_000)?).unwrap_or(0);
    let what = ctx.params.get("srwhat").unwrap_or("").to_string();
    if !matches!(what.as_str(), "" | "text" | "title" | "nearmatch") {
        return Err(ApiError::bad_value("srwhat", &what));
    }
    let props: Vec<String> = match ctx.params.get("srprop") {
        None => ["size", "wordcount", "timestamp", "snippet"]
            .map(String::from)
            .to_vec(),
        Some(p) => p.split('|').map(str::to_string).collect(),
    };
    let prop = |name: &str| props.iter().any(|p| p == name);
    let asked = namespace_param(ctx, "srnamespace")?;
    let site = Site::load(ctx).await?;
    let hidden = crate::pages::hidden(ctx).await?;
    let wanted = u32::try_from(u64::from(offset) + u64::from(limit) + 1).unwrap_or(u32::MAX);
    let mut results: Vec<Value> = Vec::new();
    let mut total: i64 = 0;

    // Entities, for the entity namespaces asked for.
    let types: Vec<String> = ENTITY_KINDS
        .iter()
        .filter(|(_, ns)| asked.as_ref().is_some_and(|list| list.contains(ns)))
        .map(|(t, _)| (*t).to_string())
        .collect();
    if !types.is_empty() && what != "text" {
        let chain = language_chain(ctx.params.get("uselang").unwrap_or(&site.lang), false);
        let (hits, _) = entity_hits(
            ctx.db(),
            &ctx.tenant.slug,
            &hidden,
            &chain,
            &text,
            &types,
            wanted,
            0,
        )
        .await?;
        total += i64::try_from(hits.len()).unwrap_or(0);
        for h in hits {
            let ns = ENTITY_KINDS
                .iter()
                .find(|(t, _)| *t == h.entity_type)
                .map_or(120, |(_, ns)| *ns);
            let mut entry = json!({"ns": ns, "title": h.title(), "pageid": h.page_id});
            let label = h.label.as_ref().map_or(h.text.as_str(), |l| l.1.as_str());
            if prop("snippet") {
                let shown = match &h.description {
                    Some(d) => format!("{label}: {}", d.1),
                    None => label.to_string(),
                };
                entry["snippet"] = json!(snippet(&shown, &words(&text), true));
            }
            if prop("titlesnippet") {
                entry["titlesnippet"] = json!(snippet(label, &words(&text), true));
            }
            results.push(entry);
        }
    }

    // Pages, by their words and by their title.
    let scope = page_scope(&site, &hidden, &text, asked.clone());
    if let Some((namespaces, rest)) = scope.filter(|(n, _)| !n.is_empty() && !hidden.every_page()) {
        // The words asked for: all of the text, or what follows a namespace prefix.
        let asked_text = if rest.to_lowercase() == text.trim().to_lowercase() {
            text.clone()
        } else {
            text.split_once(':')
                .map_or(text.as_str(), |(_, r)| r)
                .trim()
                .to_string()
        };
        let rows = ctx
            .db()
            .query(
                "SELECT p.page_id, p.ns, p.title, p.len, p.latest_at, count(*) OVER (),
                        (SELECT r.state FROM view.entity_revision r
                         WHERE r.tenant = p.tenant AND r.revid = p.latest_revid)
                 FROM view.page p
                 LEFT JOIN view.page_text x ON x.tenant = p.tenant AND x.page_id = p.page_id
                 CROSS JOIN websearch_to_tsquery('simple', $3) q
                 WHERE p.tenant = $1 AND p.ns = ANY($2) AND NOT p.deleted AND NOT p.is_redirect
                   AND p.latest_offset IS NOT NULL AND NOT (p.page_id = ANY($4))
                   AND (($8 AND x.tsv @@ q) OR ($9 AND lower(p.title) LIKE $5))
                 ORDER BY (lower(p.title) = $6) DESC, (lower(p.title) LIKE $5) DESC,
                          ts_rank('{0.1, 0.1, 0.1, 1.0}', x.tsv, q) DESC NULLS LAST,
                          p.ns, p.title COLLATE \"C\"
                 LIMIT $7",
                &[
                    &ctx.tenant.slug,
                    &namespaces,
                    &asked_text,
                    &hidden.page_ids(),
                    &prefix_pattern(&rest),
                    &rest.to_lowercase(),
                    &i64::from(wanted),
                    &(what != "title" && what != "nearmatch"),
                    &(what != "text"),
                ],
            )
            .await?;
        let marked = words(&asked_text);
        for r in &rows {
            let ns: i32 = r.get(1);
            let title: String = r.get(2);
            let stored: Option<Vec<u8>> = r.get(6);
            let stored = stored.map(|b| String::from_utf8_lossy(&b).into_owned());
            let mut entry = json!({
                "ns": ns,
                "title": site.prefixed(ns, &title),
                "pageid": r.get::<_, i64>(0),
            });
            if prop("size") {
                entry["size"] = json!(r.get::<_, Option<i32>>(3).unwrap_or(0));
            }
            if prop("wordcount") {
                entry["wordcount"] = json!(
                    stored
                        .as_deref()
                        .map_or(0, |t| t.split_whitespace().count())
                );
            }
            if prop("timestamp")
                && let Some(at) = r.get::<_, Option<std::time::SystemTime>>(4)
            {
                let micros = at
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(0));
                entry["timestamp"] = json!(crate::entity_json::iso8601(micros));
            }
            if prop("snippet") {
                entry["snippet"] = json!(snippet(
                    &plain(stored.as_deref().unwrap_or("")),
                    &marked,
                    false
                ));
            }
            if prop("titlesnippet") {
                entry["titlesnippet"] = json!(snippet(&site.prefixed(ns, &title), &marked, true));
            }
            results.push(entry);
        }
        total += rows.first().map_or(0, |r| r.get::<_, i64>(5));
    }

    let more = results.len() > (offset + limit) as usize;
    let page: Vec<Value> = results
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();
    if more {
        body.insert(
            "continue".into(),
            json!({"sroffset": offset + limit, "continue": "-||"}),
        );
    }
    if ctx
        .params
        .get("srinfo")
        .is_none_or(|i| i.split('|').any(|p| p == "totalhits"))
    {
        query.insert("searchinfo".into(), json!({"totalhits": total}));
    }
    query.insert("search".into(), Value::Array(page));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0014 §8 (03 §11.6): the requested language and its fallbacks.
    #[test]
    fn a_language_chain_ends_with_mul_and_english() {
        assert_eq!(language_chain("de-at", false), ["de-at", "de", "mul", "en"]);
        assert_eq!(language_chain("en", false), ["en", "mul"]);
        assert_eq!(language_chain("de", true), ["de"]);
        assert_eq!(language_chain("mul", false), ["mul"]);
    }

    #[test]
    fn a_prefix_pattern_escapes_its_wildcards() {
        assert_eq!(prefix_pattern("50%_A\\"), "50\\%\\_a\\\\%");
    }

    #[test]
    fn a_snippet_marks_the_words_and_escapes_the_rest() {
        let s = snippet(
            "An <b>open</b> library & more",
            &words("Open LIBRARY"),
            false,
        );
        assert_eq!(
            s,
            "An &lt;b&gt;<span class=\"searchmatch\">open</span>&lt;/b&gt; \
             <span class=\"searchmatch\">library</span> &amp; more"
        );
        // Words are whole: `open` does not mark `opened`.
        assert_eq!(snippet("opened", &words("open"), false), "opened");
        // Unless the match was by prefix, as a label's or a title's is.
        assert_eq!(
            snippet("Seven", &words("sev"), true),
            "<span class=\"searchmatch\">Seven</span>"
        );
        // A long text is cut around the first word found.
        let long = format!("{} needle {}", "hay ".repeat(200), "straw ".repeat(200));
        let s = snippet(&long, &words("needle"), false);
        assert!(
            s.contains("<span class=\"searchmatch\">needle</span>"),
            "{s}"
        );
        assert!(s.chars().count() < 2 * SNIPPET_CHARS, "{s}");
        assert_eq!(snippet("", &words("x"), false), "");
    }

    #[test]
    fn plain_text_drops_the_commonest_markup() {
        assert_eq!(
            plain(
                "== Goals ==\n* '''Bold''' and [[Project:Home|home]], [[Help]] <span style=\"x\">in</span> {{t|a}}; 1 < 2"
            ),
            " Goals \n* Bold and home, Help in t|a; 1 < 2"
        );
        assert_eq!(plain("[[unclosed"), "[[unclosed");
    }
}
