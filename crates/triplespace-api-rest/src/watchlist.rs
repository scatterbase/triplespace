//! The watch set's REST routes (0020 §5; 18 §3.2; 16 §4.3), for the caller's own watches:
//!
//! - `GET /watchlist`: every watch, `{kind, id, ns, title, added, expires, notify,
//!   unseen}`; `unseen` is when the first change the holder has not seen happened, or null.
//!   A watch whose title the holder may not see now has `title` null.
//! - `PUT /watchlist/{kind}/{id}`: watch `entity`, `page`, `actor` or `title`
//!   (`{ns}:{title}`), with `expires` (`infinite`, a time or a span) and `notify`; a page
//!   is watched as its subject, so `page/{talk page ID}` watches the subject.
//! - `DELETE /watchlist/{kind}/{id}`: stop watching.
//! - `POST /watchlist/seen`: `{target: {kind, id}}`, or nothing for every watch: the
//!   holder has seen every change up to now.
//!
//! An account and `viewmywatchlist` to read, `editmywatchlist` and the CSRF token of a
//! session to write. Responses are private.

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{Map, Value, json};
use triplespace_accounts::watch::{self, Watch};
use triplespace_api_action::App;
use triplespace_api_action::api::Ctx;
use triplespace_api_action::entity_json::iso8601;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::pages::Site;
use triplespace_api_action::watching;

use crate::acl::{context, ok, refused, write_body};
use crate::entity_route::error;

fn kind_name(k: i16) -> &'static str {
    match k {
        watch::kind::ENTITY => "entity",
        watch::kind::PAGE => "page",
        watch::kind::ACTOR => "actor",
        _ => "title",
    }
}

fn time(us: Option<i64>) -> Value {
    us.map_or(Value::Null, |t| {
        json!(iso8601(u64::try_from(t).unwrap_or(0)))
    })
}

/// The holder, after the right is checked.
#[allow(clippy::result_large_err)]
fn holder(ctx: &Ctx, right: &str) -> Result<String, Response> {
    let Some(h) = ctx.caller.identity.actor_key.clone() else {
        return Err(error(
            StatusCode::FORBIDDEN,
            "notloggedin",
            "a watchlist needs an account",
        ));
    };
    if ctx.caller.require(right).is_err() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            format!("this needs the \"{right}\" right"),
        ));
    }
    Ok(h)
}

/// What a route's `{kind}/{id}` is watched as.
async fn target_of(ctx: &Ctx, kind: &str, id: &str) -> Result<(i16, String), Response> {
    let bad = || {
        error(
            StatusCode::BAD_REQUEST,
            "bad-target",
            format!("\"{kind}/{id}\" is not a watchable target"),
        )
    };
    let none = || {
        error(
            StatusCode::NOT_FOUND,
            "ts-no-target",
            format!("there is no {kind} {id}"),
        )
    };
    match kind {
        "entity" => Ok((watch::kind::ENTITY, id.to_string())),
        "actor" => Ok((watch::kind::ACTOR, id.to_string())),
        "page" => {
            let n: u64 = id.parse().map_err(|_| bad())?;
            watching::target_of_page(ctx, n)
                .await
                .map_err(|e| refused(&e))?
                .ok_or_else(none)
        }
        "title" => {
            let site = Site::load(ctx).await.map_err(|e| refused(&e))?;
            watching::target_of_title(ctx, &site, id)
                .await
                .map_err(|e| refused(&e))?
                .ok_or_else(bad)
        }
        _ => Err(bad()),
    }
}

async fn entry(ctx: &Ctx, site: &Site, holder: &str, w: &Watch) -> Result<Value, Response> {
    let title = watching::title_of(ctx, site, w)
        .await
        .map_err(|e| refused(&e))?;
    let unseen = watching::first_unseen(ctx, holder, w)
        .await
        .map_err(|e| refused(&e))?;
    Ok(json!({
        "kind": kind_name(w.kind),
        "id": w.id,
        "ns": title.as_ref().map(|t| t.0),
        "title": title.map(|t| t.1),
        "added": time(Some(w.added)),
        "expires": time(w.expires),
        "notify": w.notify,
        "unseen": time(unseen),
    }))
}

/// `GET /watchlist`.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let h = match holder(&ctx, "viewmywatchlist") {
        Ok(h) => h,
        Err(r) => return r,
    };
    let site = match Site::load(&ctx).await {
        Ok(s) => s,
        Err(e) => return refused(&e),
    };
    let watches = match watch::list(ctx.db(), &h, watching::MAX_WATCHES, None).await {
        Ok(w) => w,
        Err(e) => return refused(&e.into()),
    };
    let mut out = Vec::with_capacity(watches.len());
    for w in &watches {
        match entry(&ctx, &site, &h, w).await {
            Ok(v) => out.push(v),
            Err(r) => return r,
        }
    }
    ok(&headers, &json!({"watches": out}))
}

/// `PUT /watchlist/{kind}/{id}`.
pub async fn put(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let b: Map<String, Value> = match write_body(&app, &ctx, &headers, &body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let h = match holder(&ctx, "editmywatchlist") {
        Ok(h) => h,
        Err(r) => return r,
    };
    let target = match target_of(&ctx, &kind, &id).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let expires = match b.get("expires").and_then(Value::as_str) {
        Some(e) => match triplespace_api_action::modules::moderation::expiry(e) {
            Ok(t) => t.map(|t| i64::try_from(t.0).unwrap_or(i64::MAX)),
            Err(e) => return refused(&e),
        },
        None => None,
    };
    let notify = b.get("notify").and_then(Value::as_bool);
    let w = match watch::watch(ctx.db(), &h, target.0, &target.1, expires, notify).await {
        Ok(w) => w,
        Err(e) => return refused(&e.into()),
    };
    let site = match Site::load(&ctx).await {
        Ok(s) => s,
        Err(e) => return refused(&e),
    };
    match entry(&ctx, &site, &h, &w).await {
        Ok(v) => ok(&headers, &v),
        Err(r) => r,
    }
}

/// `DELETE /watchlist/{kind}/{id}`.
pub async fn delete(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = write_body(&app, &ctx, &headers, &body) {
        return r;
    }
    let h = match holder(&ctx, "editmywatchlist") {
        Ok(h) => h,
        Err(r) => return r,
    };
    let target = match target_of(&ctx, &kind, &id).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    match watch::unwatch(ctx.db(), &h, target.0, &target.1).await {
        Ok(removed) => ok(&headers, &json!({"removed": removed})),
        Err(e) => refused(&e.into()),
    }
}

/// `POST /watchlist/seen`.
pub async fn seen(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let ctx = match context(&app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let fields = match write_body(&app, &ctx, &headers, &body) {
        Ok(f) => f,
        Err(r) => return r,
    };
    let account = match holder(&ctx, "editmywatchlist") {
        Ok(a) => a,
        Err(r) => return r,
    };
    let target = match fields.get("target") {
        None | Some(Value::Null) => None,
        Some(asked) => {
            let (Some(kind), Some(id)) = (asked["kind"].as_str(), asked["id"].as_str()) else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "badbody",
                    "`target` is `{kind, id}`",
                );
            };
            match target_of(&ctx, kind, id).await {
                Ok(found) => Some(found),
                Err(r) => return r,
            }
        }
    };
    let now = i64::try_from(triplespace_api_action::edit::now_micros()).unwrap_or(i64::MAX);
    match watch::mark_seen(
        ctx.db(),
        &account,
        target.as_ref().map(|(kind, id)| (*kind, id.as_str())),
        now,
    )
    .await
    {
        Ok(n) => ok(&headers, &json!({"marked": n})),
        Err(e) => refused(&e.into()),
    }
}
