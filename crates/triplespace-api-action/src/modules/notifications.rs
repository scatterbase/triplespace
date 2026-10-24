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
//!   `user-rights`; reasons Echo lacks are `ts-{reason}`. Every reason built so far is an
//!   alert.
//! - **`action=echomarkread`**: `list` (IDs), `unreadlist` (IDs to mark unread) or `all`.
//! - **`action=echomarkseen`**: `type` (`alert`, `message`, `all`) sets the seen time;
//!   there is one seen time per account, whichever type is named.

use serde_json::{Map, Value, json};
use triplespace_notify::inbox::{self, Filter, Item};
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

fn section(reason: &str) -> &'static str {
    match reason {
        "talk" | "mention" | "reply" => "message",
        _ => "alert",
    }
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
) -> Result<
    (
        std::collections::HashMap<(i64, i64), (Option<String>, Value)>,
        std::collections::HashMap<String, String>,
    ),
    ApiError,
> {
    let parts: Vec<i64> = items.iter().map(|i| i.partition).collect();
    let offs: Vec<i64> = items.iter().map(|i| i.offset).collect();
    let rows = ctx
        .db()
        .query(
            "SELECT a.partition, a.\"offset\", a.actor_key, a.params
             FROM view.activity a
             JOIN unnest($2::bigint[], $3::bigint[]) AS k(p, o) ON a.partition = k.p AND a.\"offset\" = k.o
             WHERE a.tenant = $1",
            &[&ctx.tenant.slug, &parts, &offs],
        )
        .await?;
    let mut acts = std::collections::HashMap::new();
    let mut keys: Vec<String> = items.iter().filter_map(|i| i.target_id.clone()).collect();
    for r in &rows {
        let actor: Option<String> = r.get(2);
        if let Some(a) = &actor {
            keys.push(a.clone());
        }
        acts.insert(
            (r.get::<_, i64>(0), r.get::<_, i64>(1)),
            (actor, r.get::<_, Option<Value>>(3).unwrap_or(Value::Null)),
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
    acts: &std::collections::HashMap<(i64, i64), (Option<String>, Value)>,
    names: &std::collections::HashMap<String, String>,
    wiki: &str,
) -> Value {
    let (agent, params) = acts
        .get(&(i.partition, i.offset))
        .cloned()
        .unwrap_or((None, Value::Null));
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
    if let Some(name) = i.target_id.as_ref().and_then(|k| names.get(k)) {
        v["title"] = json!({
            "full": format!("User:{name}"),
            "namespace": "User",
            "namespace-key": 2,
            "text": name,
        });
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
    let sections = ctx.params.list("notsections");
    let wants_alerts = sections.is_empty() || sections.iter().any(|s| s == "alert");
    let mut out = Map::new();
    if props.iter().any(|p| p == "list") {
        let limit = ctx.params.int("notlimit", 20, 1, 50)?;
        let before = match ctx.params.get("notcontinue") {
            Some(c) => Some(
                c.rsplit('|')
                    .next()
                    .and_then(|n| n.parse::<i64>().ok())
                    .ok_or_else(|| ApiError::bad_value("notcontinue", c))?,
            ),
            None => None,
        };
        let mut items = if wants_alerts {
            inbox::list(
                ctx.db(),
                &me,
                filter,
                i64::try_from(limit + 1).unwrap_or(51),
                before,
            )
            .await?
        } else {
            Vec::new()
        };
        let more = items.len() > usize::try_from(limit).unwrap_or(50);
        items.truncate(usize::try_from(limit).unwrap_or(50));
        let (acts, names) = activity_of(ctx, &items).await?;
        let wiki = ctx.tenant.slug.clone();
        let list: Vec<Value> = items
            .iter()
            .filter(|i| sections.is_empty() || sections.iter().any(|s| s == section(&i.reason)))
            .map(|i| item_json(i, &acts, &names, &wiki))
            .collect();
        out.insert("list".into(), Value::Array(list));
        if more && let Some(last) = items.last() {
            out.insert(
                "continue".into(),
                json!(format!("{}|{}", last.published / 1_000_000, last.id)),
            );
        }
    }
    if props.iter().any(|p| p == "count") {
        let n = if wants_alerts {
            inbox::unread(ctx.db(), &me).await?
        } else {
            0
        };
        out.insert("rawcount".into(), json!(n));
        out.insert(
            "count".into(),
            json!(if n > 99 { "99+".into() } else { n.to_string() }),
        );
    }
    if props.iter().any(|p| p == "seenTime") {
        let seen = inbox::seen_at(ctx.db(), &me)
            .await?
            .map_or(Value::Null, |t| json!(iso8601(micros(t))));
        out.insert("seenTime".into(), json!({"alert": seen, "message": seen}));
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
        inbox::mark(ctx.db(), &me, None, true).await?;
    } else {
        let read = ids(ctx, "list")?;
        let unread = ids(ctx, "unreadlist")?;
        if read.is_empty() && unread.is_empty() {
            return Err(ApiError::new(
                "missingparam",
                "At least one of the parameters \"list\", \"unreadlist\" and \"all\" is required.",
            ));
        }
        inbox::mark(ctx.db(), &me, Some(&read), true).await?;
        inbox::mark(ctx.db(), &me, Some(&unread), false).await?;
    }
    let n = inbox::unread(ctx.db(), &me).await?;
    Ok(ApiResponse::ok(json!({"echomarkread": {
        "result": "success",
        "count": n.to_string(),
        "rawcount": n,
        "alert": {"count": n.to_string(), "rawcount": n},
        "message": {"count": "0", "rawcount": 0},
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
