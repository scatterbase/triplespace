//! The thread routes (0019 §9; 0049 §11; 0069 §9; 18 §3.2): reading a thread and a talk
//! page's listing, and every write a person makes to a thread, through the one writer
//! ([`triplespace_api_ingest::threads`]).
//!
//! - `POST /thread`: `create`. Body: `subject`, `target` (`{kind: "page"|"actor", id}`),
//!   `text`, and optionally `also`, `status`, `pinned`.
//! - `GET /thread/{id}`: subject, title, status, home and listings, and the posts as
//!   `as:Note` objects in tree order, with the collapses that stand; `base`, the offset a
//!   later write names.
//! - `POST /thread/{id}/post`: `inReplyTo`, `text`, and optionally `status`, `collapse`
//!   (`{first, last}`) or `uncollapse`.
//! - `PUT /thread/{id}/post/{revid}`: `edit`, by the post's author: `text`, `base`.
//! - `POST /thread/{id}/rename`, `/move`, `/attach`, `/detach`, `/pin`, `/unpin`: each with
//!   `base` and an optional `summary`; `subject`, or `target` (and `keep` on a move).
//! - `GET /page/{id}/threads`: the threads attached to a talk page or board, pinned first.
//!
//! Creating, posting and editing need `edit`; the rest need `move` (09 §8.1). A write with
//! a session cookie carries its CSRF token as `token`. Foreign threads, `upstream-reply`,
//! ACL redaction of hidden posts and the per-target rights of 09 §8.1 are not served yet.

#![allow(
    clippy::result_large_err,
    reason = "the parsers return a ready response, as the async routes do"
)]

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use scatter_ingest::{Attestation, Request};
use scatter_log_postgres::log;
use scatter_threads::{
    Category, Invalid, Limits, Range, Statuses, Target, Thread, ThreadOp, Writer,
};
use serde_json::{Map, Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::Tenant;
use triplespace_api_ingest::threads::{ThreadError, ThreadWrite, write};
use triplespace_projections::common::partition_of;
use triplespace_projections::feed::iso8601;
use triplespace_projections::threads::{read_record, text};

use crate::entity_route::{error, internal};
use crate::settings::{check_token, prelude};

type Body = Map<String, Value>;

fn bad(message: impl Into<String>) -> Response {
    error(StatusCode::BAD_REQUEST, "badbody", message)
}

fn not_found(id: u64) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "ts-thread-not-found",
        format!("no thread has page ID {id}"),
    )
}

/// The API code and status of a refused record.
fn refusal(e: &Invalid) -> (StatusCode, &'static str) {
    let code = match e {
        Invalid::Create | Invalid::Text => "badbody",
        Invalid::Subject(_) => "ts-bad-subject",
        Invalid::Status(_) => "ts-unknown-status",
        Invalid::MediaType(_) => "ts-bad-media-type",
        Invalid::ImportOnly(_) | Invalid::Heading(_) => "ts-import-only",
        Invalid::NotAPost(_) => "ts-not-a-post",
        Invalid::CollapseOrder
        | Invalid::CollapseCrosses(_)
        | Invalid::CollapseReply
        | Invalid::CollapseAndUncollapse
        | Invalid::NotCollapsing(_) => "ts-bad-collapse",
        Invalid::NoTarget(_) => "ts-no-target",
        Invalid::NoHomes(_)
        | Invalid::ActorListing
        | Invalid::Attached(_)
        | Invalid::NotAListing(_)
        | Invalid::NotAttached(_)
        | Invalid::SameHome => "ts-bad-attachment",
        Invalid::TooManyAttachments | Invalid::TooManyPinned(_) => "ts-limit",
        Invalid::Pinned(_) | Invalid::NotPinned(_) => "ts-bad-pin",
    };
    let status = match e {
        Invalid::NoTarget(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, code)
}

fn refused(e: &ThreadError) -> Response {
    match e {
        ThreadError::Invalid(i) => {
            let (status, code) = refusal(i);
            error(status, code, i.to_string())
        }
        ThreadError::NoThread(id) => not_found(*id),
        ThreadError::NeedsBase(_) => {
            error(StatusCode::BAD_REQUEST, "ts-base-required", e.to_string())
        }
        ThreadError::Conflict { .. } => error(StatusCode::CONFLICT, "editconflict", e.to_string()),
        ThreadError::Store(_) => internal(e),
    }
}

fn target_of(v: Option<&Value>) -> Result<Target, Response> {
    let v = v.ok_or_else(|| bad("`target` is required"))?;
    match (v.get("kind").and_then(Value::as_str), v.get("id")) {
        (Some("page"), Some(id)) => id
            .as_u64()
            .or_else(|| id.as_str().and_then(|s| s.parse().ok()))
            .map(Target::Page)
            .ok_or_else(|| bad("a page target's `id` is a page ID")),
        (Some("actor"), Some(Value::String(key))) => Ok(Target::Actor(key.clone())),
        _ => Err(bad("`target` is `{kind: \"page\"|\"actor\", id}`")),
    }
}

fn target_json(t: &Target) -> Value {
    match t {
        Target::Page(id) => json!({"kind": "page", "id": id}),
        Target::Actor(key) => json!({"kind": "actor", "id": key}),
    }
}

fn string(body: &Body, key: &str) -> Result<Option<String>, Response> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(bad(format!("`{key}` is a string"))),
    }
}

fn required(body: &Body, key: &str) -> Result<String, Response> {
    string(body, key)?.ok_or_else(|| bad(format!("`{key}` is required")))
}

fn number(body: &Body, key: &str) -> Result<Option<u64>, Response> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| bad(format!("`{key}` is a non-negative integer"))),
    }
}

/// A write's tenant, author and body: the caller must hold `right` and have an account,
/// and a session's write must carry its token.
async fn begin_write(
    app: &App,
    origin: &Origin,
    headers: &HeaderMap,
    body: &Bytes,
    right: &str,
) -> Result<(Tenant, String, Body), Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let (tenant, caller) = prelude(app, &client, origin, headers).await?;
    if caller.require(right).is_err() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            format!("this needs the \"{right}\" right"),
        ));
    }
    let Some(actor) = caller.identity.actor_key.clone() else {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "writing to a thread needs an account",
        ));
    };
    let mut object: Body = match serde_json::from_slice(body) {
        Ok(Value::Object(o)) => o,
        _ => return Err(bad("the body is a JSON object")),
    };
    check_token(app, &caller, headers, &object)?;
    object.remove("token");
    Ok((tenant, actor, object))
}

/// `thread.max_attachments` and `thread.max_pinned`, as the tenant resolves them.
async fn limits(app: &App, tenant: &str) -> Limits {
    let mut l = Limits::default();
    let Ok(client) = app.pool().get().await else {
        return l;
    };
    if let Ok((s, _)) = triplespace_accounts::settings::load(&**client, Some(tenant)).await {
        let n = |k: &str| {
            s.value(k)
                .and_then(|v| v.as_u64())
                .and_then(|n| usize::try_from(n).ok())
        };
        if let Some(n) = n("thread.max_attachments") {
            l.max_attachments = n;
        }
        if let Some(n) = n("thread.max_pinned") {
            l.max_pinned = n;
        }
    }
    l
}

/// Writes one record and answers with the thread, the record and the next base.
async fn save(
    app: &App,
    tenant: &Tenant,
    actor: &str,
    thread: Option<u64>,
    op: ThreadOp,
    text: Option<String>,
    body: &Body,
) -> Response {
    let base = match number(body, "base") {
        Ok(b) => b,
        Err(r) => return r,
    };
    let summary = match string(body, "summary") {
        Ok(s) => s,
        Err(r) => return r,
    };
    let created = thread.is_none();
    let mut request = Request::new(
        &tenant.slug,
        Attestation::by(actor),
        triplespace_api_action::edit::now_micros(),
    );
    request.base_offset = base;
    request.comment = summary.filter(|_| op.needs_base());
    let w = ThreadWrite {
        thread,
        op,
        text,
        writer: Writer::Editor,
        limits: limits(app, &tenant.slug).await,
        date: None,
    };
    let store = app.store();
    let mut cx = match scatter_projection::Backend::begin(store).await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let out = match write(store, app.pipeline(), &mut cx, &request, w).await {
        Ok(out) => out,
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            return refused(&e);
        }
    };
    if let Err(e) = scatter_projection::Backend::commit(store, cx).await {
        return internal(&e);
    }
    let title = title_of(app, &tenant.slug, out.thread).await;
    let body = json!({
        "thread": out.thread,
        "title": title,
        "revid": out.revid,
        "base": out.written.offset(),
    });
    http_cache::respond(
        &HeaderMap::new(),
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}

async fn title_of(app: &App, tenant: &str, thread: u64) -> Option<String> {
    let client = app.pool().get().await.ok()?;
    let id = i64::try_from(thread).ok()?;
    client
        .query_opt(
            "SELECT title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .ok()
        .flatten()
        .map(|r| r.get(0))
}

/// `POST /thread`.
pub async fn create(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (tenant, actor, b) = match begin_write(&app, &origin, &headers, &body, "edit").await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let parsed = (|| {
        let name = required(&b, "subject")?;
        let text = required(&b, "text")?;
        let target = target_of(b.get("target"))?;
        let also = match b.get("also") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|t| target_of(Some(t)))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(bad("`also` is a list of targets")),
        };
        let pinned = b.get("pinned").and_then(Value::as_bool).unwrap_or(false);
        let op = ThreadOp::Create {
            name,
            target,
            also,
            status: string(&b, "status")?,
            pinned,
            media_type: string(&b, "mediaType")?,
            imported_from: None,
        };
        Ok((op, text))
    })();
    match parsed {
        Ok((op, text)) => save(&app, &tenant, &actor, None, op, Some(text), &b).await,
        Err(r) => r,
    }
}

/// `POST /thread/{id}/post`.
pub async fn post(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    body: Bytes,
) -> Response {
    let (tenant, actor, b) = match begin_write(&app, &origin, &headers, &body, "edit").await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let parsed = (|| {
        let text = required(&b, "text")?;
        let collapse = match b.get("collapse") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                serde_json::from_value::<Range>(v.clone())
                    .map_err(|_| bad("`collapse` is `{first, last}`, two revision IDs"))?,
            ),
        };
        let op = ThreadOp::Post {
            in_reply_to: number(&b, "inReplyTo")?,
            status: string(&b, "status")?,
            media_type: string(&b, "mediaType")?,
            heading: None,
            collapse,
            uncollapse: number(&b, "uncollapse")?,
            imported_from: None,
        };
        Ok((op, text))
    })();
    match parsed {
        Ok((op, text)) => save(&app, &tenant, &actor, Some(id), op, Some(text), &b).await,
        Err(r) => r,
    }
}

/// `PUT /thread/{id}/post/{revid}`: an edit, by the post's author.
pub async fn edit(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((id, revid)): Path<(u64, u64)>,
    body: Bytes,
) -> Response {
    let (tenant, actor, b) = match begin_write(&app, &origin, &headers, &body, "edit").await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let text = match required(&b, "text") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let author = async {
        let client = app.pool().get().await.ok()?;
        let (r, t) = (i64::try_from(revid).ok()?, i64::try_from(id).ok()?);
        client
            .query_opt(
                "SELECT actor FROM view.post WHERE tenant = $1 AND revid = $2 AND thread_id = $3",
                &[&tenant.slug, &r, &t],
            )
            .await
            .ok()
            .flatten()
            .map(|row| row.get::<_, Option<String>>(0))
    }
    .await;
    match author {
        None => {
            return error(
                StatusCode::NOT_FOUND,
                "ts-not-a-post",
                format!("revision {revid} is not a post of thread {id}"),
            );
        }
        Some(a) if a.as_deref() != Some(actor.as_str()) => {
            return error(
                StatusCode::FORBIDDEN,
                "permissiondenied",
                "only a post's author edits it",
            );
        }
        Some(_) => {}
    }
    let media_type = match string(&b, "mediaType") {
        Ok(m) => m,
        Err(r) => return r,
    };
    let op = ThreadOp::Edit {
        object: revid,
        media_type,
    };
    save(&app, &tenant, &actor, Some(id), op, Some(text), &b).await
}

/// The operations that change a thread's subject or attachments.
#[derive(Clone, Copy)]
pub(crate) enum Change {
    Rename,
    Move,
    Attach,
    Detach,
    Pin,
    Unpin,
}

async fn change(
    app: App,
    origin: Origin,
    headers: HeaderMap,
    id: u64,
    body: Bytes,
    what: Change,
) -> Response {
    let (tenant, actor, b) = match begin_write(&app, &origin, &headers, &body, "move").await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let op = (|| {
        Ok(match what {
            Change::Rename => ThreadOp::Rename {
                name: required(&b, "subject")?,
            },
            Change::Move => ThreadOp::Move {
                target: target_of(b.get("target"))?,
                keep: b.get("keep").and_then(Value::as_bool).unwrap_or(false),
            },
            Change::Attach => ThreadOp::Attach {
                target: target_of(b.get("target"))?,
            },
            Change::Detach => ThreadOp::Detach {
                target: target_of(b.get("target"))?,
            },
            Change::Pin => ThreadOp::Pin {
                target: target_of(b.get("target"))?,
            },
            Change::Unpin => ThreadOp::Unpin {
                target: target_of(b.get("target"))?,
            },
        })
    })();
    match op {
        Ok(op) => save(&app, &tenant, &actor, Some(id), op, None, &b).await,
        Err(r) => r,
    }
}

macro_rules! change_route {
    ($name:ident, $what:expr, $doc:literal) => {
        #[doc = $doc]
        pub async fn $name(
            State(app): State<App>,
            Extension(origin): Extension<Origin>,
            headers: HeaderMap,
            Path(id): Path<u64>,
            body: Bytes,
        ) -> Response {
            change(app, origin, headers, id, body, $what).await
        }
    };
}

change_route!(rename, Change::Rename, "`POST /thread/{id}/rename`.");
change_route!(move_to, Change::Move, "`POST /thread/{id}/move`.");
change_route!(attach, Change::Attach, "`POST /thread/{id}/attach`.");
change_route!(detach, Change::Detach, "`POST /thread/{id}/detach`.");
change_route!(pin, Change::Pin, "`POST /thread/{id}/pin`.");
change_route!(unpin, Change::Unpin, "`POST /thread/{id}/unpin`.");

fn respond(headers: &HeaderMap, method: &Method, caller: &Caller, body: &Value) -> Response {
    let credentialed = caller.has_session() || bearer(headers).is_some();
    let class = Class::Public.permitted(method, credentialed, false, false);
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &[],
    )
}

/// Talk page IDs with their titles, `{ns}:{title}` as `view.page` holds them.
async fn talk_title(client: &deadpool_postgres::Object, tenant: &str, talk: Option<u64>) -> Value {
    let Some(id) = talk.and_then(|t| i64::try_from(t).ok()) else {
        return Value::Null;
    };
    client
        .query_opt(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .ok()
        .flatten()
        .map_or(
            Value::Null,
            |r| json!({"ns": r.get::<_, i32>(0), "title": r.get::<_, String>(1)}),
        )
}

/// `GET /thread/{id}`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let partition = match partition_of(&*client, &tenant.slug, "pages").await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(id),
        Err(e) => return internal(&e),
    };
    let records = match log::read_by_key(&*client, partition, &id.to_string()).await {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let mut folded = Vec::new();
    let mut texts = std::collections::HashMap::new();
    let mut last_offset = 0;
    for (offset, r) in &records {
        if r.header().payload_type != scatter_threads::PAYLOAD_THREAD {
            continue;
        }
        last_offset = *offset;
        match read_record(r) {
            Ok(Some(rec)) => {
                if let Some(t) = text(r) {
                    texts.insert(rec.revid, t);
                }
                folded.push((rec, r.header().appended_at));
            }
            Ok(None) => {}
            Err(e) => return internal(&e),
        }
    }
    if folded.is_empty() {
        return not_found(id);
    }
    let appended: std::collections::HashMap<u64, u64> =
        folded.iter().map(|(r, at)| (r.revid, *at)).collect();
    let thread = match Thread::replay(folded.iter().map(|(r, _)| r)) {
        Ok(t) => t,
        Err(e) => return internal(&e),
    };
    let statuses = Statuses::embedded();
    let mut attachments = Vec::new();
    for a in thread.attachments() {
        attachments.push(json!({
            "target": target_json(&a.target),
            "talk_page_id": a.talk,
            "talk_page": talk_title(&client, &tenant.slug, a.talk).await,
            "attached": appended.get(&a.since).map(|t| iso8601(*t)),
            "pinned": a.pinned.and_then(|t| u64::try_from(t).ok()).map(iso8601),
        }));
    }
    let home = attachments.remove(0);
    let body = json!({
        "id": id,
        "title": thread.title,
        "subject": thread.subject,
        "status": thread.status(),
        "category": match thread.category(statuses) { Category::Open => "open", Category::Closed => "closed" },
        "home": home,
        "listings": attachments,
        "posts": notes(&thread, &texts, &appended),
        "base": last_offset,
    });
    respond(&headers, &method, &caller, &body)
}

/// The posts as `as:Note` objects in tree order: each post, then its replies.
fn notes(
    thread: &Thread,
    texts: &std::collections::HashMap<u64, String>,
    appended: &std::collections::HashMap<u64, u64>,
) -> Vec<Value> {
    let posts = thread.posts();
    let mut children: std::collections::HashMap<Option<u64>, Vec<usize>> =
        std::collections::HashMap::new();
    for (k, p) in posts.iter().enumerate() {
        let parent = p.parent.filter(|r| thread.post(*r).is_some());
        children.entry(parent).or_default().push(k);
    }
    let mut out = Vec::with_capacity(posts.len());
    let mut stack: Vec<(usize, usize)> = children
        .get(&None)
        .map(|v| v.iter().rev().map(|k| (*k, 0)).collect())
        .unwrap_or_default();
    while let Some((k, depth)) = stack.pop() {
        let p = &posts[k];
        let mut note = json!({
            "type": "Note",
            "id": p.revid,
            "inReplyTo": p.parent,
            "attributedTo": p.actor,
            "published": appended.get(&p.revid).map(|t| iso8601(*t)),
            "updated": p.edits.last().and_then(|r| appended.get(r)).map(|t| iso8601(*t)),
            "mediaType": p.media_type,
            "content": texts.get(&p.text),
            "depth": depth,
            "collapsed": thread.is_collapsed(p.revid),
        });
        if let Some(s) = &p.status {
            note["status"] = json!(s);
        }
        if let Some(h) = p.heading {
            note["heading"] = json!(h);
        }
        if let Some(r) = p.collapse {
            note["collapse"] = json!({"first": r.first, "last": r.last});
        }
        if p.imported {
            note["imported"] = json!(true);
        }
        out.push(note);
        if let Some(kids) = children.get(&Some(p.revid)) {
            stack.extend(kids.iter().rev().map(|c| (*c, depth + 1)));
        }
    }
    out
}

/// `GET /page/{id}/threads`: the threads attached to a talk page or board, pinned first in
/// the order they were pinned, then by last activity.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Ok(talk) = i64::try_from(id) else {
        return not_found(id);
    };
    let target = client
        .query_opt(
            "SELECT target_kind, target_id FROM view.talk_page WHERE tenant = $1 AND page_id = $2",
            &[&tenant.slug, &talk],
        )
        .await;
    let target = match target {
        Ok(Some(r)) => match r.get::<_, i16>(0) {
            1 => r.get::<_, String>(1).parse().map(Target::Page).ok(),
            _ => Some(Target::Actor(r.get(1))),
        },
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "ts-not-a-talk-page",
                format!("page {id} is not a talk page or board with threads"),
            );
        }
        Err(e) => return internal(&e),
    };
    let rows = client
        .query(
            "SELECT t.page_id, p.title, t.subject, t.status, a.home,
               to_char(a.pinned AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
               to_char(t.last_post AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
               t.post_count
             FROM view.thread_attachment a
             JOIN view.thread t ON t.tenant = a.tenant AND t.page_id = a.thread_id
             JOIN view.page p ON p.tenant = t.tenant AND p.page_id = t.page_id AND NOT p.deleted
             WHERE a.tenant = $1 AND a.talk_page_id = $2
             ORDER BY a.pinned IS NULL, a.pinned, t.last_post DESC, t.page_id DESC",
            &[&tenant.slug, &talk],
        )
        .await;
    let rows = match rows {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let statuses = Statuses::embedded();
    let threads: Vec<Value> = rows
        .iter()
        .map(|r| {
            let status: String = r.get(3);
            let category = statuses.get(&status).map_or("open", |s| match s.category {
                Category::Open => "open",
                Category::Closed => "closed",
            });
            json!({
                "id": r.get::<_, i64>(0),
                "title": r.get::<_, String>(1),
                "subject": r.get::<_, String>(2),
                "status": status,
                "category": category,
                "home": r.get::<_, bool>(4),
                "pinned": r.get::<_, Option<String>>(5),
                "last_post": r.get::<_, Option<String>>(6),
                "posts": r.get::<_, i32>(7),
            })
        })
        .collect();
    let body = json!({
        "talk_page_id": id,
        "target": target.as_ref().map(target_json),
        "threads": threads,
    });
    respond(&headers, &method, &caller, &body)
}
