//! `action=compare` (compat `#mod-compare`; 18 §2.3; 19 §3.8): a line diff between two
//! revisions of document pages, as MediaWiki's table formatter writes it.
//!
//! Each side is named by `{from,to}rev`, `{from,to}id` (the page's latest revision) or
//! `{from,to}title`; `torelative=prev|next|cur` names the other side from `from`, and as
//! in MediaWiki `prev` diffs the previous revision against `from`. A first revision's
//! `prev` is the empty text, with MediaWiki's warning, so a creation reads as all
//! additions; the latest revision's `next` is itself. `prop` takes `diff`, `diffsize`,
//! `ids`, `title`, `user`, `comment`, `parsedcomment`, `size`, `timestamp` and `rel`;
//! `difftype` only `table`. The diff is [`scatter_pages::diff`]'s.
//!
//! The revisions are the page's rows of the activity feed, so a revision the viewer may
//! not see is not found, and one whose text is hidden or erased is `missingcontent`.
//! Entity revisions are not compared here yet: their diff is the REST route's
//! (`GET /entity/{id}/compare`), and `ts-entity-compare` says so. Supplied text
//! (`fromtext`, `fromslots`) is not supported yet.

use serde_json::{Map, Value, json};
use triplespace_projections::feed::{self, Direction, Feed, TargetSet, target_kind};

use crate::activity::{comment_text, user_id, viewer, visibility};
use crate::api::Ctx;
use crate::entity_json::iso8601;
use crate::pages::{self, Site};
use crate::response::{ApiError, ApiResponse};

/// A revision on one side of the comparison.
struct Rev {
    page_id: u64,
    ns: i32,
    title: String,
    row: feed::Row,
}

/// The feed of one page's revisions.
fn page_feed(page_id: u64) -> Feed {
    let mut f = Feed::everything(1);
    f.set = TargetSet::Target {
        kind: target_kind::PAGE,
        id: page_id.to_string(),
    };
    f.filters.kinds = vec!["edit".into()];
    f.filters.sources = vec!["pages".into()];
    f
}

/// One row of a page's revisions: the one with `revid`, or the one before or after
/// `from`.
async fn feed_row(
    ctx: &Ctx,
    page_id: u64,
    revid: Option<u64>,
    after: Option<(&feed::Row, Direction)>,
) -> Result<Option<feed::Row>, ApiError> {
    let mut f = page_feed(page_id);
    f.filters.revid = revid;
    if let Some((row, direction)) = after {
        f.cursor = Some(row.cursor());
        f.direction = direction;
    }
    let p = feed::query(ctx.db(), &ctx.tenant.slug, &f, &viewer(&ctx.caller))
        .await
        .map_err(ApiError::internal)?;
    Ok(p.rows.into_iter().next())
}

/// Whether a revision ID is an entity's, which this module does not compare.
async fn is_entity_revision(ctx: &Ctx, revid: u64) -> Result<bool, ApiError> {
    let row = ctx
        .db()
        .query_opt(
            "SELECT 1 FROM view.activity WHERE tenant = $1 AND revid = $2 AND target_kind = 1",
            &[&ctx.tenant.slug, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await?;
    Ok(row.is_some())
}

fn entity_compare() -> ApiError {
    ApiError::new(
        "ts-entity-compare",
        "Entity revisions are compared over REST: GET /w/rest.php/triplespace/v0/entity/{id}/compare.",
    )
}

/// A side as its parameters name it, or `None` when it names none.
async fn side(ctx: &Ctx, site: &Site, prefix: &str) -> Result<Option<Rev>, ApiError> {
    let tenant = ctx.tenant.slug.as_str();
    let p = |name: &str| ctx.params.get(&format!("{prefix}{name}"));
    if p("text").is_some() || p("slots").is_some() {
        return Err(ApiError::new(
            "ts-compare-text",
            "Comparing supplied text is not supported yet; name two revisions.",
        ));
    }
    let (page, revid) = if let Some(r) = p("rev") {
        let name = format!("{prefix}rev");
        let r: u64 = r.parse().map_err(|_| ApiError::bad_value(&name, r))?;
        match pages::by_revid(ctx.db(), tenant, r).await? {
            Some(page) => (page, r),
            None if is_entity_revision(ctx, r).await? => return Err(entity_compare()),
            None => {
                return Err(ApiError::new(
                    "nosuchrevid",
                    format!("There is no revision with ID {r}."),
                ));
            }
        }
    } else if let Some(id) = p("id") {
        let name = format!("{prefix}id");
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value(&name, id))?;
        let page = pages::by_id(ctx.db(), tenant, n).await?.ok_or_else(|| {
            ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
        })?;
        let r = page.latest_revid.unwrap_or(0);
        (page, r)
    } else if let Some(t) = p("title") {
        if pages::names_entity(site, t) {
            return Err(entity_compare());
        }
        let title = pages::resolve_title(site, t)
            .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{t}\".")))?;
        let page = pages::by_title(ctx.db(), tenant, title.namespace, &title.text)
            .await?
            .ok_or_else(|| {
                ApiError::new("missingtitle", "The page you specified doesn't exist.")
            })?;
        let r = page.latest_revid.unwrap_or(0);
        (page, r)
    } else {
        return Ok(None);
    };
    let row = feed_row(ctx, page.page_id, Some(revid), None)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                "nosuchrevid",
                format!("There is no revision with ID {revid}."),
            )
        })?;
    Ok(Some(Rev {
        page_id: page.page_id,
        ns: page.ns,
        title: page.title,
        row,
    }))
}

/// The text after a revision, or `missingcontent`.
async fn text_of(ctx: &Ctx, rev: &Rev) -> Result<String, ApiError> {
    let revid = rev.row.revid.unwrap_or(0);
    if rev.row.visibility & visibility::CONTENT != 0 {
        return Err(missing_content(revid));
    }
    pages::text_at(ctx.db(), &ctx.tenant.slug, rev.page_id, revid)
        .await?
        .ok_or_else(|| missing_content(revid))
}

fn missing_content(revid: u64) -> ApiError {
    ApiError::new(
        "missingcontent",
        format!("The content for revision {revid} is not available."),
    )
}

/// The fields of one side that `prop` asks for.
fn side_fields(
    out: &mut Map<String, Value>,
    prefix: &str,
    site: &Site,
    rev: &Rev,
    props: &[String],
    v2: bool,
) {
    let want = |p: &str| props.iter().any(|x| x == p);
    let key = |name: &str| format!("{prefix}{name}");
    let flag = |out: &mut Map<String, Value>, name: String| {
        out.insert(name, if v2 { json!(true) } else { json!("") });
    };
    let row = &rev.row;
    if want("ids") {
        out.insert(key("id"), json!(rev.page_id));
        out.insert(key("revid"), json!(row.revid.unwrap_or(0)));
    }
    if want("title") {
        out.insert(key("ns"), json!(rev.ns));
        out.insert(key("title"), json!(site.prefixed(rev.ns, &rev.title)));
    }
    if want("size") {
        out.insert(key("size"), json!(row.size.unwrap_or(0)));
    }
    if want("timestamp") {
        out.insert(key("timestamp"), json!(iso8601(row.time)));
    }
    if want("user") {
        if row.visibility & visibility::USER != 0 || row.actor.hidden {
            flag(out, key("userhidden"));
        } else {
            let name = row
                .actor
                .name
                .clone()
                .or_else(|| row.actor.key.clone())
                .unwrap_or_default();
            out.insert(key("user"), json!(name));
            out.insert(key("userid"), json!(user_id(row.actor.key.as_deref())));
        }
    }
    if want("comment") || want("parsedcomment") {
        if row.visibility & visibility::COMMENT != 0 {
            flag(out, key("commenthidden"));
        } else {
            let text = comment_text(row);
            if want("parsedcomment") {
                out.insert(key("parsedcomment"), json!(escape(&text)));
            }
            if want("comment") {
                out.insert(key("comment"), json!(text));
            }
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `action=compare`.
#[allow(clippy::too_many_lines)]
pub async fn run(ctx: &Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.require("read")?;
    if let Some(t) = ctx.params.get("difftype")
        && t != "table"
    {
        return Err(ApiError::bad_value("difftype", t));
    }
    let site = Site::load(ctx).await?;
    let v2 = ctx.formatversion == 2;
    let mut props = ctx.params.list("prop");
    if props.is_empty() {
        props = ["diff", "ids", "title"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    let mut response_warnings: Vec<String> = Vec::new();
    let named_from = side(ctx, &site, "from").await?.ok_or_else(|| {
        ApiError::new(
            "missingparam",
            "One of the parameters \"fromtitle\", \"fromid\", \"fromrev\", \"fromtext\" and \"fromslots\" is required.",
        )
    })?;
    // `from` and `to`; `None` for `from` is the empty text before a page's first revision.
    let (from, to): (Option<Rev>, Rev) = match ctx.params.get("torelative") {
        Some("prev") => {
            let prev = feed_row(
                ctx,
                named_from.page_id,
                None,
                Some((&named_from.row, Direction::Older)),
            )
            .await?;
            if prev.is_none() {
                response_warnings.push("There is no previous revision.".into());
            }
            let prev = prev.map(|row| Rev {
                page_id: named_from.page_id,
                ns: named_from.ns,
                title: named_from.title.clone(),
                row,
            });
            (prev, named_from)
        }
        Some("next") => {
            let next = feed_row(
                ctx,
                named_from.page_id,
                None,
                Some((&named_from.row, Direction::Newer)),
            )
            .await?;
            if next.is_none() {
                response_warnings.push("There is no next revision.".into());
            }
            let to = Rev {
                page_id: named_from.page_id,
                ns: named_from.ns,
                title: named_from.title.clone(),
                row: next.unwrap_or_else(|| named_from.row.clone()),
            };
            (Some(named_from), to)
        }
        Some("cur") => {
            let page = pages::by_id(ctx.db(), &ctx.tenant.slug, named_from.page_id)
                .await?
                .ok_or_else(|| {
                    ApiError::new("missingtitle", "The page you specified doesn't exist.")
                })?;
            let row = feed_row(ctx, page.page_id, page.latest_revid, None)
                .await?
                .ok_or_else(|| {
                    ApiError::new("missingtitle", "The page you specified doesn't exist.")
                })?;
            let to = Rev {
                page_id: page.page_id,
                ns: page.ns,
                title: page.title,
                row,
            };
            (Some(named_from), to)
        }
        Some(other) => return Err(ApiError::bad_value("torelative", other)),
        None => {
            let to = side(ctx, &site, "to").await?.ok_or_else(|| {
                ApiError::new(
                    "missingparam",
                    "One of the parameters \"totitle\", \"toid\", \"torev\", \"totext\", \"torelative\" and \"toslots\" is required.",
                )
            })?;
            (Some(named_from), to)
        }
    };
    let old = match &from {
        Some(r) => text_of(ctx, r).await?,
        None => String::new(),
    };
    let new = text_of(ctx, &to).await?;
    let hunks = scatter_pages::diff::diff(&old, &new);
    let body = scatter_pages::diff::table_rows(&hunks, |n| format!("Line {n}:"));

    let mut out = Map::new();
    if let Some(f) = &from {
        side_fields(&mut out, "from", &site, f, &props, v2);
    }
    side_fields(&mut out, "to", &site, &to, &props, v2);
    let want = |p: &str| props.iter().any(|x| x == p);
    if want("rel") {
        if let Some(f) = &from
            && let Some(prev) =
                feed_row(ctx, f.page_id, None, Some((&f.row, Direction::Older))).await?
        {
            out.insert("prev".into(), json!(prev.revid.unwrap_or(0)));
        }
        if let Some(next) =
            feed_row(ctx, to.page_id, None, Some((&to.row, Direction::Newer))).await?
        {
            out.insert("next".into(), json!(next.revid.unwrap_or(0)));
        }
    }
    if want("diffsize") {
        out.insert("diffsize".into(), json!(body.len()));
    }
    if want("diff") {
        out.insert(if v2 { "body" } else { "*" }.into(), json!(body));
    }
    let mut r = ApiResponse::ok(json!({"compare": Value::Object(out)}));
    for w in response_warnings {
        r.warn("compare", &w);
    }
    Ok(r)
}
