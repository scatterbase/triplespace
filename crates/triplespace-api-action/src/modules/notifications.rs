//! Echo's modules over the inbox (0021 §7; 18 §2.4): `meta=notifications`,
//! `action=echomarkread` and `action=echomarkseen`, in Echo's shapes, reading and marking
//! the caller's own inbox ([`triplespace_notify::inbox`]).
//!
//! - **`meta=notifications`**: `notprop` (`list`, `count`, `seenTime`; `list` by default),
//!   `notfilter` (`read`, `!read`, both by default), `notsections` (`alert`, `message`),
//!   `notlimit` (20, at most 50) and `notcontinue`. Each item is Echo's model: `id`,
//!   `type`, `category`, `section`, `timestamp`, `title`, `agent`, `read`; Triplespace adds
//!   `ts-reason` (16 §5.2) and `ts-params`, the activity's parameters (for `rights`, the
//!   group, whether it was an addition or a removal, and the expiry). `rights` is Echo's
//!   `user-rights`, `talk` its `edit-user-talk` and `mention` its `mention`; reasons Echo
//!   lacks are `ts-{reason}`. As in Echo, `talk` is a message and every other reason an
//!   alert. An item about a page has its `title` and the `revid` of the record; one about a
//!   thread also `ts-thread`, the thread's subject and the talk page it is on.
//!   `notgroupbysection` gives each section its own list and counts, as Echo does.
//!
//!   **What the reader may no longer see is left out** (16 §5.2; 0021 A20): an item about a
//!   thread they may not read now, whose talk page is hidden from them (a confidential
//!   page's, or a hidden user's), or whose record is hidden in any part or erased. The
//!   counts leave the same items out, so they are counted up to 100.
//! - **`action=echomarkread`**: `list` (IDs), `unreadlist` (IDs to mark unread) or `all`,
//!   which `sections` may narrow to alerts or messages.
//! - **`action=echomarkseen`**: `type` (`alert`, `message`, `all`) sets the seen time;
//!   there is one seen time per account, whichever type is named.

use serde_json::{Map, Value, json};
use triplespace_notify::inbox::{self, Filter, Item, Reasons};
use triplespace_projections::common::target_kind;
use triplespace_projections::feed::{iso8601, mw_timestamp};

use crate::api::Ctx;
use crate::response::{ApiError, ApiResponse};

fn login_required() -> ApiError {
    ApiError::new(
        "login-required",
        "You must be logged in to view or mark notifications.",
    )
}

fn caller(ctx: &Ctx) -> Result<String, ApiError> {
    ctx.caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(login_required)
}

/// Echo's type for a reason.
fn echo_type(reason: &str) -> String {
    match reason {
        "rights" => "user-rights".into(),
        "mention" => "mention".into(),
        "talk" => "edit-user-talk".into(),
        other => format!("ts-{other}"),
    }
}

/// What an activity row says of an item: performer, parameters and revision ID, keyed by
/// `(partition, offset)`.
type Acts = std::collections::HashMap<(i64, i64), (Option<String>, Value, Option<i64>)>;

/// The reasons Echo files under `message`; every other reason is an `alert`.
const MESSAGES: &[&str] = &["talk"];

fn section(reason: &str) -> &'static str {
    if MESSAGES.contains(&reason) {
        "message"
    } else {
        "alert"
    }
}

/// The rows of one section.
fn reasons_of(section: &str) -> Reasons<'static> {
    if section == "message" {
        Reasons::Only(MESSAGES)
    } else {
        Reasons::Except(MESSAGES)
    }
}

/// The most unread items counted; Echo shows more as `99+`.
const COUNT_CAP: i64 = 100;

/// The items the caller may still see (16 §5.2): an item about a thread is left out when
/// the thread does not read to them now, or its record is hidden in any part or erased.
async fn visible(ctx: &Ctx, items: Vec<Item>) -> Result<Vec<Item>, ApiError> {
    let threads: Vec<&Item> = items
        .iter()
        .filter(|i| i.target_kind == Some(target_kind::PAGE))
        .collect();
    if threads.is_empty() {
        return Ok(items);
    }
    let parts: Vec<i64> = threads.iter().map(|i| i.partition).collect();
    let offs: Vec<i64> = threads.iter().map(|i| i.offset).collect();
    let hidden: std::collections::HashSet<(i64, i64)> = ctx
        .db()
        .query(
            "SELECT a.partition, a.\"offset\" FROM view.activity a
             JOIN unnest($1::bigint[], $2::bigint[]) AS k(p, o) ON a.partition = k.p AND a.\"offset\" = k.o
             WHERE a.visibility <> 0 OR a.kind = 'erased'",
            &[&parts, &offs],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    // What the reader may not see by page: a confidential page's, and a hidden user's
    // talk page, which a thread's home may be (09 §5.3, §6.3).
    let absent = triplespace_projections::acl_read::hidden(
        ctx.db(),
        &ctx.tenant.slug,
        &ctx.caller.effective,
        scatter_actors::Timestamp(crate::edit::now_micros()),
    )
    .await
    .map_err(ApiError::internal)?;
    let ids: Vec<i64> = threads
        .iter()
        .filter_map(|i| i.target_id.as_deref()?.parse().ok())
        .collect();
    let pages: std::collections::HashMap<String, Vec<(i32, i64)>> = ctx
        .db()
        .query(
            "SELECT p.page_id, p.ns, tp.ns, tp.page_id
             FROM view.page p
             LEFT JOIN view.thread t ON t.tenant = p.tenant AND t.page_id = p.page_id
             LEFT JOIN view.page tp ON tp.tenant = t.tenant AND tp.page_id = t.talk_page_id
             WHERE p.tenant = $1 AND p.page_id = ANY($2) AND NOT p.deleted",
            &[&ctx.tenant.slug, &ids],
        )
        .await?
        .iter()
        .map(|r| {
            let id: i64 = r.get(0);
            let mut v = vec![(r.get::<_, i32>(1), id)];
            if let (Some(ns), Some(t)) = (r.get::<_, Option<i32>>(2), r.get::<_, Option<i64>>(3)) {
                v.push((ns, t));
            }
            (id.to_string(), v)
        })
        .collect();
    let mut readable: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(items.len());
    for i in items {
        if i.target_kind == Some(target_kind::PAGE) {
            if hidden.contains(&(i.partition, i.offset)) {
                continue;
            }
            let id = i.target_id.clone().unwrap_or_default();
            let shown = pages.get(&id).is_some_and(|ps| {
                ps.iter()
                    .all(|(ns, p)| !absent.page(*ns, u64::try_from(*p).unwrap_or(0)))
            });
            if !shown {
                continue;
            }
            let ok = if let Some(ok) = readable.get(&id) {
                *ok
            } else {
                let ok = match id.parse::<u64>() {
                    Ok(thread) => triplespace_projections::acl_read::page_readable(
                        ctx.db(),
                        &ctx.tenant.slug,
                        &ctx.caller.effective,
                        thread,
                    )
                    .await
                    .map_err(ApiError::internal)?,
                    Err(_) => false,
                };
                readable.insert(id, ok);
                ok
            };
            if !ok {
                continue;
            }
        }
        out.push(i);
    }
    Ok(out)
}

/// The unread items of a section the caller may see, up to [`COUNT_CAP`].
async fn unread(ctx: &Ctx, me: &str, reasons: Reasons<'_>) -> Result<i64, ApiError> {
    let items = inbox::list(ctx.db(), me, Filter::Unread, reasons, COUNT_CAP, None).await?;
    Ok(i64::try_from(visible(ctx, items).await?.len()).unwrap_or(COUNT_CAP))
}

fn count_json(n: i64) -> Value {
    json!(if n > 99 {
        "99+".to_string()
    } else {
        n.to_string()
    })
}

/// What a thread item names: the thread's title and subject, and the talk page it is on.
async fn threads_of(
    ctx: &Ctx,
    items: &[Item],
) -> Result<std::collections::HashMap<String, Value>, ApiError> {
    let ids: Vec<i64> = items
        .iter()
        .filter(|i| i.target_kind == Some(target_kind::PAGE))
        .filter_map(|i| i.target_id.as_deref()?.parse().ok())
        .collect();
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let site = crate::pages::Site::load_for(ctx.db(), &ctx.tenant.slug).await?;
    let rows = ctx
        .db()
        .query(
            "SELECT p.page_id, p.ns, p.title, t.subject, tp.ns, tp.title
             FROM view.page p
             LEFT JOIN view.thread t ON t.tenant = p.tenant AND t.page_id = p.page_id
             LEFT JOIN view.page tp ON tp.tenant = t.tenant AND tp.page_id = t.talk_page_id
             WHERE p.tenant = $1 AND p.page_id = ANY($2)",
            &[&ctx.tenant.slug, &ids],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let id: i64 = r.get(0);
            let (ns, title): (i32, String) = (r.get(1), r.get(2));
            let talk = match (r.get::<_, Option<i32>>(4), r.get::<_, Option<String>>(5)) {
                (Some(ns), Some(t)) => json!({"full": site.prefixed(ns, &t), "namespace-key": ns}),
                _ => Value::Null,
            };
            (
                id.to_string(),
                json!({
                    "title": {
                        "full": site.prefixed(ns, &title),
                        "namespace": site.prefixed(ns, "x").trim_end_matches(":x"),
                        "namespace-key": ns,
                        "text": title,
                    },
                    "subject": r.get::<_, Option<String>>(3),
                    "talk": talk,
                }),
            )
        })
        .collect())
}

fn micros(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

fn timestamp(us: i64) -> Value {
    let secs = us / 1_000_000;
    json!({
        "utciso8601": iso8601(micros(us)),
        "utcunix": secs.to_string(),
        "unix": secs.to_string(),
        "utcmw": mw_timestamp(micros(us)),
        "mw": mw_timestamp(micros(us)),
    })
}

/// What the activity rows behind the items say: performer and parameters, keyed by
/// `(partition, offset)`; and the names of the accounts involved, those whose names the
/// caller may not see left out.
async fn activity_of(
    ctx: &Ctx,
    items: &[Item],
) -> Result<(Acts, std::collections::HashMap<String, String>), ApiError> {
    let parts: Vec<i64> = items.iter().map(|i| i.partition).collect();
    let offs: Vec<i64> = items.iter().map(|i| i.offset).collect();
    let rows = ctx
        .db()
        .query(
            "SELECT a.partition, a.\"offset\", a.actor_key, a.params, a.revid
             FROM view.activity a
             JOIN unnest($2::bigint[], $3::bigint[]) AS k(p, o) ON a.partition = k.p AND a.\"offset\" = k.o
             WHERE a.tenant = $1",
            &[&ctx.tenant.slug, &parts, &offs],
        )
        .await?;
    let mut acts = std::collections::HashMap::new();
    let mut keys: Vec<String> = items
        .iter()
        .filter(|i| i.target_kind == Some(target_kind::ACTOR))
        .filter_map(|i| i.target_id.clone())
        .collect();
    for r in &rows {
        let actor: Option<String> = r.get(2);
        if let Some(a) = &actor {
            keys.push(a.clone());
        }
        acts.insert(
            (r.get::<_, i64>(0), r.get::<_, i64>(1)),
            (
                actor,
                r.get::<_, Option<Value>>(3).unwrap_or(Value::Null),
                r.get::<_, Option<i64>>(4),
            ),
        );
    }
    keys.sort();
    keys.dedup();
    let effective = &ctx.caller.effective;
    let hidden = triplespace_projections::acl_read::hidden_actors(
        ctx.db(),
        &ctx.tenant.slug,
        &keys,
        |g| effective.reads_in(g),
        scatter_actors::Timestamp(crate::edit::now_micros()),
    )
    .await
    .map_err(ApiError::internal)?;
    let names = ctx
        .db()
        .query(
            "SELECT actor_key, name FROM view.actor WHERE tenant = $1 AND actor_key = ANY($2) AND name IS NOT NULL",
            &[&ctx.tenant.slug, &keys],
        )
        .await?
        .iter()
        .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
        .filter(|(k, _)| !hidden.contains(k))
        .collect();
    Ok((acts, names))
}

/// One item in Echo's model.
fn item_json(
    i: &Item,
    acts: &Acts,
    names: &std::collections::HashMap<String, String>,
    threads: &std::collections::HashMap<String, Value>,
    wiki: &str,
) -> Value {
    let (agent, params, revid) =
        acts.get(&(i.partition, i.offset))
            .cloned()
            .unwrap_or((None, Value::Null, None));
    let mut v = json!({
        "wiki": wiki,
        "id": i.id,
        "type": echo_type(&i.reason),
        "category": echo_type(&i.reason),
        "section": section(&i.reason),
        "timestamp": timestamp(i.published),
        "ts-reason": i.reason,
        "ts-params": params,
    });
    match i.target_kind {
        Some(target_kind::ACTOR) => {
            if let Some(name) = i.target_id.as_ref().and_then(|k| names.get(k)) {
                v["title"] = json!({
                    "full": format!("User:{name}"),
                    "namespace": "User",
                    "namespace-key": 2,
                    "text": name,
                });
            }
        }
        Some(target_kind::PAGE) => {
            if let Some(t) = i.target_id.as_ref().and_then(|k| threads.get(k)) {
                v["title"] = t["title"].clone();
                if !t["subject"].is_null() {
                    v["ts-thread"] = json!({"subject": t["subject"], "talk": t["talk"]});
                }
            }
            if let Some(r) = revid {
                v["revid"] = json!(r);
            }
        }
        _ => {}
    }
    v["agent"] = match agent.as_deref() {
        // An instance act names the operator, not an account (16 §5.2).
        Some(k) if k.starts_with("instance:") => {
            json!({"id": 0, "name": Value::Null, "ts-instance": true})
        }
        Some(k) => match names.get(k) {
            Some(n) => json!({"id": crate::activity::user_id(Some(k)), "name": n}),
            None => json!({"id": 0, "name": Value::Null, "hidden": true}),
        },
        None => json!({"id": 0, "name": Value::Null, "hidden": true}),
    };
    if let Some(r) = i.read {
        v["read"] = json!(mw_timestamp(micros(r)));
    }
    v
}

/// One section's list, or every section's, with its continuation.
async fn list_of(
    ctx: &Ctx,
    me: &str,
    filter: Filter,
    reasons: Reasons<'_>,
) -> Result<Map<String, Value>, ApiError> {
    let limit = ctx.params.int("notlimit", 20, 1, 50)?;
    let limit = usize::try_from(limit).unwrap_or(50);
    let before = match ctx.params.get("notcontinue") {
        Some(c) => Some(
            c.rsplit('|')
                .next()
                .and_then(|n| n.parse::<i64>().ok())
                .ok_or_else(|| ApiError::bad_value("notcontinue", c))?,
        ),
        None => None,
    };
    let mut items = inbox::list(
        ctx.db(),
        me,
        filter,
        reasons,
        i64::try_from(limit + 1).unwrap_or(51),
        before,
    )
    .await?;
    let more = items.len() > limit;
    items.truncate(limit);
    let tail = items.last().map(|l| (l.published, l.id));
    let items = visible(ctx, items).await?;
    let (acts, names) = activity_of(ctx, &items).await?;
    let threads = threads_of(ctx, &items).await?;
    let wiki = ctx.tenant.slug.clone();
    let list: Vec<Value> = items
        .iter()
        .map(|i| item_json(i, &acts, &names, &threads, &wiki))
        .collect();
    let mut out = Map::new();
    out.insert("list".into(), Value::Array(list));
    if more && let Some((published, id)) = tail {
        out.insert(
            "continue".into(),
            json!(format!("{}|{id}", published / 1_000_000)),
        );
    }
    Ok(out)
}

/// `meta=notifications`.
pub async fn meta(ctx: &mut Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    let me = caller(ctx)?;
    ctx.cache_class = crate::http_cache::Class::Private;
    // Addressing runs behind the write path; reading the inbox brings it up to date, so
    // the bell never lags a write the reader has just seen.
    ctx.app.catch_up_async().await.map_err(ApiError::internal)?;
    let mut props = ctx.params.list("notprop");
    if props.is_empty() {
        props.push("list".into());
    }
    let mut filter: Vec<String> = ctx.params.list("notfilter");
    filter.sort();
    let filter = match filter.as_slice() {
        [f] if f == "!read" => Filter::Unread,
        [f] if f == "read" => Filter::Read,
        _ => Filter::All,
    };
    let mut sections = ctx.params.list("notsections");
    for s in &sections {
        if s != "alert" && s != "message" {
            return Err(ApiError::bad_value("notsections", s));
        }
    }
    if sections.is_empty() {
        sections = vec!["alert".into(), "message".into()];
    }
    let both = sections.len() > 1;
    let reasons = if both {
        Reasons::All
    } else {
        reasons_of(&sections[0])
    };
    let wants = |p: &str| props.iter().any(|x| x == p);
    let seen = if wants("seenTime") {
        Some(
            inbox::seen_at(ctx.db(), &me)
                .await?
                .map_or(Value::Null, |t| json!(iso8601(micros(t)))),
        )
    } else {
        None
    };
    let mut out = Map::new();
    if ctx.params.flag("notgroupbysection") {
        let mut total = 0;
        for s in &sections {
            let mut group = if wants("list") {
                list_of(ctx, &me, filter, reasons_of(s)).await?
            } else {
                Map::new()
            };
            if wants("count") {
                let n = unread(ctx, &me, reasons_of(s)).await?;
                total += n;
                group.insert("rawcount".into(), json!(n));
                group.insert("count".into(), count_json(n));
            }
            if let Some(seen) = &seen {
                group.insert("seenTime".into(), seen.clone());
            }
            out.insert(s.clone(), Value::Object(group));
        }
        if wants("count") {
            out.insert("rawcount".into(), json!(total));
            out.insert("count".into(), count_json(total));
        }
    } else {
        if wants("list") {
            out.extend(list_of(ctx, &me, filter, reasons).await?);
        }
        if wants("count") {
            let n = unread(ctx, &me, reasons).await?;
            out.insert("rawcount".into(), json!(n));
            out.insert("count".into(), count_json(n));
        }
        if let Some(seen) = seen {
            out.insert(
                "seenTime".into(),
                json!({"alert": seen.clone(), "message": seen}),
            );
        }
    }
    query.insert("notifications".into(), Value::Object(out));
    Ok(())
}

fn ids(ctx: &Ctx, name: &str) -> Result<Vec<i64>, ApiError> {
    ctx.params
        .list(name)
        .iter()
        .map(|s| s.parse().map_err(|_| ApiError::bad_value(name, s)))
        .collect()
}

/// `action=echomarkread`.
pub async fn markread(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    let me = caller(ctx)?;
    if ctx.params.flag("all") {
        // `sections` narrows `all` to alerts or messages, as in Echo.
        let sections = ctx.params.list("sections");
        let reasons = match sections.as_slice() {
            [s] if s == "alert" || s == "message" => reasons_of(s),
            [] => Reasons::All,
            [s, t] if s != t && [s, t].iter().all(|x| *x == "alert" || *x == "message") => {
                Reasons::All
            }
            [s, ..] => return Err(ApiError::bad_value("sections", s)),
        };
        inbox::mark(ctx.db(), &me, None, reasons, true).await?;
    } else {
        let read = ids(ctx, "list")?;
        let unread = ids(ctx, "unreadlist")?;
        if read.is_empty() && unread.is_empty() {
            return Err(ApiError::new(
                "missingparam",
                "At least one of the parameters \"list\", \"unreadlist\" and \"all\" is required.",
            ));
        }
        inbox::mark(ctx.db(), &me, Some(&read), Reasons::All, true).await?;
        inbox::mark(ctx.db(), &me, Some(&unread), Reasons::All, false).await?;
    }
    let alerts = unread(ctx, &me, reasons_of("alert")).await?;
    let messages = unread(ctx, &me, reasons_of("message")).await?;
    let n = alerts + messages;
    Ok(ApiResponse::ok(json!({"echomarkread": {
        "result": "success",
        "count": count_json(n),
        "rawcount": n,
        "alert": {"count": count_json(alerts), "rawcount": alerts},
        "message": {"count": count_json(messages), "rawcount": messages},
    }})))
}

/// `action=echomarkseen`.
pub async fn markseen(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    let me = caller(ctx)?;
    match ctx.params.get("type") {
        Some("alert" | "message" | "all") => {}
        Some(other) => return Err(ApiError::bad_value("type", other)),
        None => return Err(ApiError::missing_param("type")),
    }
    let t = inbox::mark_seen(ctx.db(), &me).await?;
    Ok(ApiResponse::ok(json!({"query": {"echomarkseen": {
        "result": "success",
        "timestamp": iso8601(micros(t)),
    }}})))
}
