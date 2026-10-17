//! `prop=revisions` (compat §3.2; 0012 §4; 18 §2.3): an entity page's local revisions,
//! in MediaWiki's shape, with the entity's state at any of them as `content`.
//!
//! The page set is `titles=` (entity titles in any form the Action API accepts),
//! `pageids=` or `revids=`. The rows are the local graph's change-set records for the
//! entity — the one-target feed of [`triplespace_projections::feed`] narrowed to
//! `source=local`, `kind=edit` — so a mirror's syncs do not appear here (they are
//! `rckind=sync` rows of the feed and the History tab's). `rvprop` takes MediaWiki's
//! `ids`, `flags`, `timestamp`, `user`, `userid`, `size`, `comment`, `parsedcomment`,
//! `tags`, `content`, `contentmodel` and the Triplespace additions `source`, `job`,
//! `operator` and `record` (the revision's `{partition, offset}`, which the site's diff
//! page resolves a revision ID through); `rvslots=main` (or `*`) puts the content under `slots.main` as
//! MediaWiki does since 1.32, otherwise under `*`/`content` by `formatversion`.
//! `rvlimit`, `rvdir`, `rvstart`, `rvend`, `rvstartid`, `rvendid` and `rvcontinue`
//! page as `list=recentchanges` does. `rvprop=content` at a revision is the local
//! graph's state after it, read through [`triplespace_projections::revisions`]: the
//! fold for now, one `view.entity_revision` row once 0083's tier-1 state lands. An
//! erased part sets `texthidden`, `commenthidden` or `userhidden`, and `erased`.
//!
//! Several pages in one request are served in turn; `rvlimit` then applies per page,
//! as MediaWiki applies it only to a single page, and continuation names the page.

use scatter_wikibase_model::entity::PageInfo;
use scatter_wikibase_model::id::EntityId;
use serde_json::{Map, Value, json};
use triplespace_projections::feed::{self, Cursor, Direction, Feed, Flag, TargetSet, target_kind};
use triplespace_projections::read;
use triplespace_projections::revisions;

use crate::activity::{comment_text, mw_row, viewer, visibility};
use crate::api::Ctx;
use crate::entity_json::{add_derived_terms, hasher_for, iso8601, title_of};
use crate::modules::entities::parse_id;
use crate::response::{ApiError, ApiResponse};

const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 500;

/// The content model of an entity page, by its namespace (namespaces.toml).
fn content_model(ns: i64) -> &'static str {
    match ns {
        122 => "wikibase-property",
        146 => "wikibase-lexeme",
        210 => "triplespace-domain",
        212 => "triplespace-keyword",
        216 => "triplespace-notation",
        _ => "wikibase-item",
    }
}

/// One page of the request: the entity, its page ID, and the revision the request
/// named for it (`revids=`), if one.
struct PageAsk {
    id: EntityId,
    page_id: u64,
    ns: i64,
    title: String,
    only_revid: Option<u64>,
}

/// Resolves `titles=`, `pageids=` or `revids=` to entity pages; missing ones are reported
/// as MediaWiki reports them.
async fn page_set(ctx: &Ctx, query: &mut Map<String, Value>) -> Result<Vec<PageAsk>, ApiError> {
    let mut pages = Vec::new();
    let mut missing = Vec::new();
    let titles = ctx.params.list("titles");
    let pageids = ctx.params.list("pageids");
    let revids = ctx.params.list("revids");
    if titles.is_empty() && pageids.is_empty() && revids.is_empty() {
        return Err(ApiError::new(
            "missingparam",
            "One of the parameters \"titles\", \"pageids\" and \"revids\" is required.",
        ));
    }
    let mut wanted: Vec<(EntityId, Option<u64>, Option<String>)> = Vec::new();
    for t in &titles {
        match parse_id(t) {
            Ok(id) => wanted.push((id, None, Some(t.clone()))),
            Err(_) => missing.push(json!({"title": t, "invalid": true})),
        }
    }
    for p in &pageids {
        let n: i64 = p.parse().map_err(|_| ApiError::bad_value("pageids", p))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT id FROM view.entity WHERE tenant IN ($1, '') AND page_id = $2 AND NOT deleted
                 ORDER BY (tenant = $1) DESC LIMIT 1",
                &[&ctx.tenant.slug, &n],
            )
            .await?;
        match row.and_then(|r| EntityId::parse(&r.get::<_, String>(0)).ok()) {
            Some(id) => wanted.push((id, None, None)),
            None => missing.push(json!({"pageid": n, "missing": true})),
        }
    }
    for r in &revids {
        let n: u64 = r.parse().map_err(|_| ApiError::bad_value("revids", r))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT target_id FROM view.activity
                 WHERE tenant = $1 AND revid = $2 AND source = 'local' AND target_kind = 1",
                &[&ctx.tenant.slug, &i64::try_from(n).unwrap_or(i64::MAX)],
            )
            .await?;
        match row.and_then(|r| EntityId::parse(&r.get::<_, String>(0)).ok()) {
            Some(id) => wanted.push((id, Some(n), None)),
            None => missing.push(json!({"revid": n, "missing": true})),
        }
    }
    for (id, only_revid, title) in wanted {
        let standing = read::standing(ctx.db(), &ctx.tenant.slug, &id)
            .await
            .map_err(ApiError::internal)?;
        let current = read::current(ctx.db(), ctx.app.order(), &ctx.tenant.slug, &id)
            .await
            .map_err(ApiError::internal)?;
        match current {
            Some(c) if !standing.deleted => {
                let (ns, t) = title_of(&id, &c.resolved.entity.entity_type);
                pages.push(PageAsk {
                    id,
                    page_id: c.page_id,
                    ns,
                    title: t,
                    only_revid,
                });
            }
            _ => {
                let t = title.unwrap_or_else(|| id.as_str().to_string());
                missing.push(json!({"title": t, "missing": true}));
            }
        }
    }
    if !missing.is_empty() {
        query.insert("pages".into(), Value::Array(missing));
    }
    Ok(pages)
}

/// The feed of one page's local revisions, from the request's `rv*` parameters.
fn feed_for(ctx: &Ctx, page: &PageAsk) -> Result<Feed, ApiError> {
    let mut f = Feed::everything(DEFAULT_LIMIT);
    f.set = TargetSet::Target {
        kind: target_kind::ENTITY,
        id: page.id.as_str().to_string(),
    };
    f.filters.kinds = vec!["edit".into(), "erased".into()];
    f.filters.sources = vec!["local".into()];
    f.direction = match ctx.params.get("rvdir").unwrap_or("older") {
        "older" => Direction::Older,
        "newer" => Direction::Newer,
        other => return Err(ApiError::bad_value("rvdir", other)),
    };
    let time = |name: &str| -> Result<Option<u64>, ApiError> {
        match ctx.params.get(name) {
            None => Ok(None),
            Some(v) => feed::parse_timestamp(v)
                .map(Some)
                .ok_or_else(|| ApiError::bad_value(name, v)),
        }
    };
    let (start, end) = (time("rvstart")?, time("rvend")?);
    match f.direction {
        Direction::Older => {
            f.filters.until = start;
            f.filters.since = end;
        }
        Direction::Newer => {
            f.filters.since = start;
            f.filters.until = end;
        }
    }
    f.limit = match ctx.params.get("rvlimit") {
        None => DEFAULT_LIMIT,
        Some("max") => MAX_LIMIT,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| ApiError::bad_value("rvlimit", v))?
            .clamp(1, MAX_LIMIT),
    };
    if let Some(c) = ctx.params.get("rvcontinue") {
        f.cursor = Some(Cursor::parse(c).ok_or_else(|| ApiError::bad_value("rvcontinue", c))?);
    }
    if let Some(u) = ctx.params.get("rvuser") {
        f.filters.actor = Some(u.to_string());
    }
    if ctx.params.list("rvshow").iter().any(|s| s == "minor") {
        f.filters.minor = Flag::Only;
    }
    Ok(f)
}

/// The content of a revision: the entity's local state after it, in Wikibase's JSON.
async fn content_at(ctx: &Ctx, page: &PageAsk, offset: u64) -> Result<Option<String>, ApiError> {
    let Some((rev, state)) =
        revisions::local_state_at(ctx.db(), &ctx.tenant.slug, &page.id, offset)
            .await
            .map_err(ApiError::internal)?
    else {
        return Ok(None);
    };
    let Some(mut entity) = state.entity else {
        return Ok(Some(String::new()));
    };
    add_derived_terms(&mut entity);
    let info = PageInfo {
        pageid: Some(page.page_id),
        ns: Some(page.ns),
        title: Some(page.title.clone()),
        lastrevid: rev.revid(),
        modified: Some(iso8601(rev.header.appended_at)),
    };
    Ok(Some(entity.to_wikibase_json(
        &hasher_for(&page.id, ctx.app.registry()),
        Some(&info),
    )))
}

/// `prop=revisions`.
#[allow(clippy::too_many_lines)]
pub async fn run(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let pages = page_set(ctx, query).await?;
    let mut props = ctx.params.list("rvprop");
    if props.is_empty() {
        props = ["ids", "timestamp", "flags", "comment", "user"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    let want = |p: &str| props.iter().any(|x| x == p);
    let slots = ctx.params.list("rvslots");
    let in_slots = slots.iter().any(|s| s == "main" || s == "*");
    if want("content") && !in_slots {
        response.warn(
            "revisions",
            "Because \"rvslots\" was not specified, a legacy format has been used for the output. This format is deprecated, and in the future the new format will always be used.",
        );
    }
    let v2 = ctx.formatversion == 2;
    let mut out: Vec<Value> = query
        .remove("pages")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let viewer = viewer(&ctx.caller);
    for page in &pages {
        let mut feed = feed_for(ctx, page)?;
        if let Some(r) = page.only_revid {
            // `revids=`: the one revision, whatever the window says.
            feed.filters.revid = Some(r);
            feed.filters.since = None;
            feed.filters.until = None;
            feed.cursor = None;
            feed.limit = 1;
        }
        let p = feed::query(ctx.db(), &ctx.tenant.slug, &feed, &viewer)
            .await
            .map_err(ApiError::internal)?;
        let mut revs = Vec::with_capacity(p.rows.len());
        for row in &p.rows {
            let r = revision_row(ctx, page, row, &props, in_slots, v2);
            revs.push(with_content(ctx, page, r, &props, in_slots, v2).await?);
        }
        let mut pg = page_json(page);
        pg["revisions"] = Value::Array(revs);
        out.push(pg);
        if let Some(next) = p.next {
            body.insert(
                "continue".into(),
                json!({"rvcontinue": next.token(), "continue": "||"}),
            );
            break;
        }
    }
    query.insert("pages".into(), pages_value(out, v2));
    Ok(())
}

/// `pages` as `formatversion` shapes it: an array, or an object keyed by page ID.
fn pages_value(pages: Vec<Value>, v2: bool) -> Value {
    if v2 {
        return Value::Array(pages);
    }
    let mut m = Map::new();
    let mut negative = -1i64;
    for p in pages {
        let key = if let Some(id) = p.get("pageid").and_then(Value::as_u64) {
            id.to_string()
        } else {
            let k = negative.to_string();
            negative -= 1;
            k
        };
        m.insert(key, p);
    }
    Value::Object(m)
}

fn page_json(page: &PageAsk) -> Value {
    json!({"pageid": page.page_id, "ns": page.ns, "title": page.title})
}

/// A revision row, from the activity row: MediaWiki's fields.
fn revision_row(
    ctx: &Ctx,
    page: &PageAsk,
    row: &feed::Row,
    props: &[String],
    in_slots: bool,
    v2: bool,
) -> Value {
    let mut m = Map::new();
    let flag = |m: &mut Map<String, Value>, name: &str, on: bool| {
        if v2 {
            m.insert(name.into(), json!(on));
        } else if on {
            m.insert(name.into(), json!(""));
        }
    };
    let want = |p: &str| props.iter().any(|x| x == p);
    if want("ids") {
        m.insert("revid".into(), json!(row.revid.unwrap_or(0)));
        m.insert("parentid".into(), json!(0));
    }
    if want("flags") {
        flag(&mut m, "minor", row.minor);
    }
    if want("user") || want("userid") {
        if row.visibility & visibility::USER != 0 || row.actor.hidden {
            flag(&mut m, "userhidden", true);
        } else {
            if want("user") {
                m.insert(
                    "user".into(),
                    json!(
                        row.actor
                            .name
                            .clone()
                            .or_else(|| row.actor.key.clone())
                            .unwrap_or_else(|| "(erased)".into())
                    ),
                );
            }
            if want("userid") {
                m.insert(
                    "userid".into(),
                    json!(crate::activity::user_id(row.actor.key.as_deref())),
                );
            }
        }
    }
    if want("timestamp") {
        m.insert("timestamp".into(), json!(iso8601(row.time)));
    }
    if want("size") {
        m.insert("size".into(), json!(row.size.unwrap_or(0)));
    }
    if want("comment") || want("parsedcomment") {
        if row.visibility & visibility::COMMENT != 0 {
            flag(&mut m, "commenthidden", true);
        } else {
            let text = comment_text(row);
            if want("comment") {
                m.insert("comment".into(), json!(text));
            }
            if want("parsedcomment") {
                m.insert("parsedcomment".into(), json!(crate::html_escape(&text)));
            }
        }
    }
    if want("tags") {
        m.insert("tags".into(), json!(row.tags));
    }
    if want("contentmodel") && !in_slots {
        m.insert("contentmodel".into(), json!(content_model(page.ns)));
        m.insert("contentformat".into(), json!("application/json"));
    }
    if row.kind == "erased" {
        flag(&mut m, "texthidden", true);
        flag(&mut m, "erased", true);
    }
    if want("record") {
        m.insert(
            "record".into(),
            json!({"partition": row.partition, "offset": row.offset}),
        );
    }
    // Triplespace's additions, as on `list=recentchanges`.
    let extra = mw_row(ctx, row, props, "revisions");
    for key in ["source", "job", "operator"] {
        if let Some(v) = extra.get(key) {
            m.insert(key.into(), v.clone());
        }
    }
    m.insert("_offset".into(), json!(row.offset));
    Value::Object(m)
}

/// Adds the content to a revision row when asked for, and drops the private `_offset`.
async fn with_content(
    ctx: &Ctx,
    page: &PageAsk,
    mut row: Value,
    props: &[String],
    in_slots: bool,
    v2: bool,
) -> Result<Value, ApiError> {
    let offset = row["_offset"].as_u64().unwrap_or(0);
    let m = row.as_object_mut().expect("an object");
    m.remove("_offset");
    let want = |p: &str| props.iter().any(|x| x == p);
    if !want("content") || m.contains_key("texthidden") {
        return Ok(row);
    }
    let Some(text) = content_at(ctx, page, offset).await? else {
        return Ok(row);
    };
    let model = content_model(page.ns);
    if in_slots {
        let mut slot = Map::new();
        slot.insert("contentmodel".into(), json!(model));
        slot.insert("contentformat".into(), json!("application/json"));
        if v2 {
            slot.insert("content".into(), json!(text));
        } else {
            slot.insert("*".into(), json!(text));
        }
        m.insert("slots".into(), json!({"main": Value::Object(slot)}));
    } else {
        m.insert("contentmodel".into(), json!(model));
        m.insert("contentformat".into(), json!("application/json"));
        if v2 {
            m.insert("content".into(), json!(text));
        } else {
            m.insert("*".into(), json!(text));
        }
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_models_follow_the_namespace() {
        assert_eq!(content_model(120), "wikibase-item");
        assert_eq!(content_model(122), "wikibase-property");
        assert_eq!(content_model(210), "triplespace-domain");
    }

    #[test]
    fn pages_are_keyed_by_id_in_formatversion_1() {
        let v = pages_value(
            vec![
                json!({"pageid": 7, "title": "Item:Q7"}),
                json!({"title": "Item:Q404", "missing": true}),
            ],
            false,
        );
        assert_eq!(v["7"]["title"], "Item:Q7");
        assert_eq!(v["-1"]["missing"], true);
        let v = pages_value(vec![json!({"pageid": 7})], true);
        assert!(v.is_array());
    }
}
