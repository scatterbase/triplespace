//! The watch set's own modules (0020 §5; 18 §2.3–2.4), in MediaWiki's shapes:
//!
//! - **`action=watch`**: `titles` or `pageids`; `unwatch`; `expiry` (`infinite`, a time,
//!   or a span such as `1 week`); and Triplespace's `tsnotify` (`1` or `0`), the *notify
//!   me* flag of 16 §4.3, kept when not given. A `watch` token. A talk page is watched as
//!   its subject, so watching either watches both.
//! - **`action=setnotificationtimestamp`**: `entirewatchlist`, or `titles`/`pageids`:
//!   the holder has seen every change to them, up to now.
//! - **`list=watchlistraw`**: the holder's watch set as titles, by namespace and title,
//!   with `wrnamespace`, `wrprop=changed`, `wrshow=changed|!changed`, `wrlimit`,
//!   `wrcontinue` and `wrdir`. Its list is outside `query`, as MediaWiki puts it.
//!
//! Every one reads or writes the caller's own watch set: logged in, with `editmywatchlist`
//! to change it and `viewmywatchlist` to read it.

use serde_json::{Map, Value, json};
use triplespace_accounts::watch;

use crate::api::Ctx;
use crate::entity_json::iso8601;
use crate::pages::Site;
use crate::response::{ApiError, ApiResponse};
use crate::watching;

fn not_logged_in() -> ApiError {
    ApiError::new(
        "notloggedin",
        "Please log in to view or edit your watchlist.",
    )
}

/// The titles a request names, by `titles` or by `pageids`.
async fn titles(ctx: &Ctx, site: &Site) -> Result<Vec<String>, ApiError> {
    let mut out = ctx.params.list("titles");
    for id in ctx.params.list("pageids") {
        let n: u64 = id
            .parse()
            .map_err(|_| ApiError::bad_value("pageids", &id))?;
        let hidden = crate::pages::hidden(ctx).await?;
        if let Some(p) = crate::pages::by_id(ctx.db(), &ctx.tenant.slug, &hidden, n).await? {
            out.push(site.prefixed(p.ns, &p.title));
        }
    }
    if out.is_empty() {
        return Err(ApiError::new(
            "missingparam",
            "One of the parameters \"titles\" and \"pageids\" is required.",
        ));
    }
    Ok(out)
}

/// `action=watch`.
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_token(&ctx.app, &ctx.params, "watch")?;
    let holder = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(not_logged_in)?;
    ctx.caller.require("editmywatchlist")?;
    let site = Site::load(ctx).await?;
    let unwatch = ctx.params.flag("unwatch");
    let expires = match ctx.params.get("expiry") {
        Some(e) => super::moderation::expiry(e)?.map(|t| i64::try_from(t.0).unwrap_or(i64::MAX)),
        None => None,
    };
    let notify = match ctx.params.get("tsnotify") {
        None => None,
        Some("1" | "true") => Some(true),
        Some("0" | "false" | "") => Some(false),
        Some(other) => return Err(ApiError::bad_value("tsnotify", other)),
    };
    let v2 = ctx.formatversion == 2;
    let yes = if v2 { json!(true) } else { json!("") };
    let mut out = Vec::new();
    for title in titles(ctx, &site).await? {
        let Some(target) = watching::target_of_title(ctx, &site, &title).await? else {
            out.push(json!({"title": title, "invalid": yes}));
            continue;
        };
        let mut entry = Map::new();
        entry.insert("title".into(), json!(title));
        if unwatch {
            watch::unwatch(ctx.db(), &holder, target.0, &target.1).await?;
            entry.insert("unwatched".into(), yes.clone());
        } else {
            let w = watch::watch(ctx.db(), &holder, target.0, &target.1, expires, notify).await?;
            entry.insert("watched".into(), yes.clone());
            entry.insert(
                "watchlistexpiry".into(),
                w.expires.map_or(json!("infinity"), |e| {
                    json!(iso8601(u64::try_from(e).unwrap_or(0)))
                }),
            );
            if w.notify {
                entry.insert("ts-notify".into(), yes.clone());
            }
        }
        out.push(Value::Object(entry));
    }
    Ok(ApiResponse::ok(json!({"batchcomplete": yes, "watch": out})))
}

/// `action=setnotificationtimestamp`: the holder has seen every change, up to now, to
/// the titles named or to the whole watch set. `timestamp`, `torevid` and `newerthanrevid`
/// are not served: a watch's seen time only moves forward, to now.
pub async fn setnotificationtimestamp(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    let holder = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(not_logged_in)?;
    ctx.caller.require("editmywatchlist")?;
    for p in ["timestamp", "torevid", "newerthanrevid"] {
        if ctx.params.get(p).is_some() {
            return Err(ApiError::new(
                "ts-not-supported",
                format!("\"{p}\" is not served: a watch's seen time moves only to now."),
            ));
        }
    }
    let now = i64::try_from(crate::edit::now_micros()).unwrap_or(i64::MAX);
    let v2 = ctx.formatversion == 2;
    let yes = if v2 { json!(true) } else { json!("") };
    if ctx.params.flag("entirewatchlist") {
        watch::mark_seen(ctx.db(), &holder, None, now).await?;
        return Ok(ApiResponse::ok(json!({"batchcomplete": yes,
            "setnotificationtimestamp": [{"notificationtimestamp": ""}]})));
    }
    let site = Site::load(ctx).await?;
    let mut out = Vec::new();
    for title in titles(ctx, &site).await? {
        match watching::target_of_title(ctx, &site, &title).await? {
            Some(t) => {
                watch::mark_seen(ctx.db(), &holder, Some((t.0, &t.1)), now).await?;
                out.push(json!({"title": title, "notificationtimestamp": ""}));
            }
            None => out.push(json!({"title": title, "invalid": yes})),
        }
    }
    Ok(ApiResponse::ok(
        json!({"batchcomplete": yes, "setnotificationtimestamp": out}),
    ))
}

/// `list=watchlistraw`, into the response body beside `query`.
///
/// # Errors
///
/// Not logged in; a bad parameter; a database failure.
pub async fn raw(ctx: &Ctx, body: &mut Map<String, Value>) -> Result<(), ApiError> {
    let holder = super::changes::watch_holder(ctx)?;
    let site = Site::load(ctx).await?;
    let props = ctx.params.list("wrprop");
    let changed_prop = props.iter().any(|p| p == "changed");
    let show = ctx.params.list("wrshow");
    let only_changed = show.iter().any(|s| s == "changed");
    let only_unchanged = show.iter().any(|s| s == "!changed");
    let namespaces: Vec<i32> = ctx
        .params
        .list("wrnamespace")
        .iter()
        .map(|n| n.parse().map_err(|_| ApiError::bad_value("wrnamespace", n)))
        .collect::<Result<_, _>>()?;
    let limit = match ctx.params.get("wrlimit") {
        None => 10,
        Some("max") => 500,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| ApiError::bad_value("wrlimit", v))?
            .clamp(1, 500),
    };
    let descending = ctx.params.get("wrdir") == Some("descending");
    let from = ctx.params.get("wrcontinue").map(|c| {
        let (ns, t) = c.split_once('|').unwrap_or(("0", c));
        (ns.parse::<i32>().unwrap_or(0), t.replace('_', " "))
    });
    let watches = watch::list(ctx.db(), &holder, watching::MAX_WATCHES, None).await?;
    let mut entries: Vec<(i32, String, Option<i64>)> = Vec::new();
    for w in &watches {
        let Some((ns, title)) = watching::title_of(ctx, &site, w).await? else {
            continue;
        };
        if !namespaces.is_empty() && !namespaces.contains(&ns) {
            continue;
        }
        let changed = if changed_prop || only_changed || only_unchanged {
            watching::first_unseen(ctx, &holder, w).await?
        } else {
            None
        };
        if (only_changed && changed.is_none()) || (only_unchanged && changed.is_some()) {
            continue;
        }
        entries.push((ns, title, changed));
    }
    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    if descending {
        entries.reverse();
    }
    if let Some((ns, t)) = &from {
        entries.retain(|(n, title, _)| {
            let key = (*n, title.as_str());
            if descending {
                key <= (*ns, t.as_str())
            } else {
                key >= (*ns, t.as_str())
            }
        });
    }
    let more = entries.len() > limit;
    let next = entries.get(limit).map(|(ns, t, _)| format!("{ns}|{t}"));
    entries.truncate(limit);
    let list: Vec<Value> = entries
        .into_iter()
        .map(|(ns, title, changed)| {
            let mut m = Map::new();
            m.insert("ns".into(), json!(ns));
            m.insert("title".into(), json!(title));
            if changed_prop && let Some(c) = changed {
                m.insert(
                    "changed".into(),
                    json!(iso8601(u64::try_from(c).unwrap_or(0))),
                );
            }
            Value::Object(m)
        })
        .collect();
    body.insert("watchlistraw".into(), Value::Array(list));
    if more && let Some(n) = next {
        let c = body
            .entry("continue")
            .or_insert_with(|| json!({"continue": "-||"}));
        c["wrcontinue"] = json!(n);
    }
    Ok(())
}
