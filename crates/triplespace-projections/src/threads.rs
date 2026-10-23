//! Threads (0019 §11; 03 §4.8; 14 §1): what a `scatter:v0/thread` record in a tenant's
//! `pages` partition projects to.
//!
//! - **`view.page`**: a row for the thread, in the `Thread` namespace with the title its
//!   `create` or `rename` minted, and a row for each talk page a record minted, in the
//!   subject's paired talk namespace, or `User talk` for an actor.
//! - **`view.talk_page`**: the target each talk page belongs to.
//! - **`view.thread`**: the home, subject, status, and the post count and times.
//! - **`view.thread_attachment`**: the home and the listings, with their pins.
//! - **`view.post`**: one row per post, with the record holding its current text, and an
//!   imported post's DiscussionTools name, ID and original time.
//!
//! The records are trusted as validated: the write path checks each against the thread's
//! fold before appending it. [`read_record`] gives the fold's view of a log record, which
//! the write path uses to replay a thread.

use scatter_log::cbor::Value;
use scatter_log::record::Record;
use scatter_log_postgres::PgClient;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_threads::{
    ImportedFrom, Minted, NS_THREAD, PAYLOAD_THREAD, TEXT_PART, Target, ThreadOp,
};
use scatter_titles::Namespaces;

use crate::backend::PgCx;
use crate::common::{attestation, attested, content, partition_info, time_of, to_jsonb};

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// The `target_kind` of a page target.
pub const TARGET_PAGE: i16 = 1;
/// The `target_kind` of an actor target.
pub const TARGET_ACTOR: i16 = 2;

/// A target as `(target_kind, target_id)`.
#[must_use]
pub fn target_columns(target: &Target) -> (i16, String) {
    match target {
        Target::Page(id) => (TARGET_PAGE, id.to_string()),
        Target::Actor(key) => (TARGET_ACTOR, key.clone()),
    }
}

/// Whether a page in this namespace is its own talk page: a thread or a board, whose paired
/// talk namespace is virtual and forwards to it (14 §1.4, §3.2).
#[must_use]
pub fn own_talk_page(ns: i32) -> bool {
    let reg = Namespaces::embedded();
    reg.get(ns)
        .and_then(|n| n.talk)
        .and_then(|t| reg.get(t))
        .is_some_and(|t| t.kind == scatter_titles::Kind::Virtual)
}

/// What the record's attestation map says the server minted.
#[must_use]
pub fn minted(record: &Record) -> Minted {
    let Some(a) = attestation(record) else {
        return Minted::default();
    };
    Minted {
        title: a.get("title").and_then(Value::as_text).map(str::to_owned),
        talk: a.get("talk").and_then(Value::as_u64),
        also: match a.get("also") {
            Some(Value::Array(items)) => items.iter().filter_map(Value::as_u64).collect(),
            _ => Vec::new(),
        },
    }
}

/// A log record as the thread fold reads it; `None` when its content part is erased.
pub fn read_record(record: &Record) -> Result<Option<scatter_threads::Record>, String> {
    let Some(v) = content(record)? else {
        return Ok(None);
    };
    let op = ThreadOp::from_json(to_jsonb(&v))?;
    let h = record.header();
    Ok(Some(scatter_threads::Record {
        revid: h.revid.ok_or("a thread record carries a revision ID")?,
        actor: attested(record).actor.unwrap_or_default(),
        time: i64::try_from(h.appended_at).map_err(|e| e.to_string())?,
        has_text: op.has_text(),
        op,
        minted: minted(record),
    }))
}

/// The record's text part, where it has one.
#[must_use]
pub fn text(record: &Record) -> Option<String> {
    record
        .body()
        .part(TEXT_PART)
        .and_then(|p| p.value().ok().flatten())
        .and_then(|v| match v {
            Value::Text(t) => Some(t),
            _ => None,
        })
}

fn i64_of(n: u64) -> Result<i64, String> {
    i64::try_from(n).map_err(|e| e.to_string())
}

/// The `view` rows of threads and talk pages.
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadProjection;

struct At<'a> {
    tenant: &'a str,
    thread: i64,
    revid: i64,
    offset: i64,
    time: std::time::SystemTime,
}

impl<B: Backend<Cx = PgCx>> Projection<B> for ThreadProjection {
    fn name(&self) -> &'static str {
        "thread"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &scatter_log::header::Header) -> bool {
        header.payload_type == PAYLOAD_THREAD
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let info = partition_info(cx, h.partition).await?;
            let Some(rec) = read_record(record)? else {
                return Ok(Applied::rows(0));
            };
            let at = At {
                tenant: info.tenant.as_str(),
                thread: i64_of(h.page_id.ok_or("a thread record carries its page ID")?)?,
                revid: i64_of(rec.revid)?,
                offset: i64_of(h.offset)?,
                time: time_of(h.appended_at),
            };
            let rows = apply(cx, &at, &rec).await?;
            Ok(Applied::rows(rows))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            for q in [
                "DELETE FROM view.post WHERE tenant = $1",
                "DELETE FROM view.thread_attachment WHERE tenant = $1",
                "DELETE FROM view.thread WHERE tenant = $1",
                "DELETE FROM view.talk_page WHERE tenant = $1",
            ] {
                cx.execute(q, &[&info.tenant]).await.map_err(|e| sql(&e))?;
            }
            Ok(())
        })
    }
}

#[allow(clippy::too_many_lines)]
async fn apply(cx: &PgCx, at: &At<'_>, rec: &scatter_threads::Record) -> Result<usize, String> {
    let tenant = at.tenant;
    let mut rows = 0;
    let talk = rec.minted.talk.map(i64_of).transpose()?;
    match &rec.op {
        ThreadOp::Create {
            name,
            target,
            also,
            status,
            pinned,
            imported_from,
            ..
        } => {
            let title = rec
                .minted
                .title
                .clone()
                .unwrap_or_else(|| at.thread.to_string());
            rows += cx
                .execute(
                    "INSERT INTO view.page (tenant, page_id, ns, title, content_model, latest_offset,
                       latest_revid, latest_at, revisions, reserved)
                     VALUES ($1, $2, $3, $4, 'triplespace-thread', $5, $6, $7, 1, false)
                     ON CONFLICT (tenant, page_id) DO UPDATE SET ns = EXCLUDED.ns, title = EXCLUDED.title,
                       content_model = EXCLUDED.content_model, latest_offset = EXCLUDED.latest_offset,
                       latest_revid = EXCLUDED.latest_revid, latest_at = EXCLUDED.latest_at,
                       revisions = 1, reserved = false",
                    &[&tenant, &at.thread, &NS_THREAD, &title, &at.offset, &at.revid, &at.time],
                )
                .await
                .map_err(|e| sql(&e))?;
            let subject = scatter_threads::normalize_subject(name).unwrap_or_else(|_| name.clone());
            let (kind, id) = target_columns(target);
            rows += cx
                .execute(
                    "INSERT INTO view.thread (tenant, page_id, target_kind, target_id, talk_page_id, subject,
                       status, created, last_post, post_count)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, 1)
                     ON CONFLICT (tenant, page_id) DO NOTHING",
                    &[
                        &tenant,
                        &at.thread,
                        &kind,
                        &id,
                        &talk,
                        &subject,
                        &status.as_deref().unwrap_or(scatter_threads::OPEN),
                        &at.time,
                    ],
                )
                .await
                .map_err(|e| sql(&e))?;
            if let Some(t) = talk {
                rows += talk_page(cx, tenant, t, target, at).await?;
                let pin = pinned.then_some(at.time);
                rows += attach(cx, at, t, true, pin).await?;
            }
            for (k, t) in also.iter().enumerate() {
                if let Some(id) = rec.minted.also.get(k).copied().map(i64_of).transpose()? {
                    rows += talk_page(cx, tenant, id, t, at).await?;
                    rows += attach(cx, at, id, false, None).await?;
                }
            }
            rows += post(cx, at, &rec.actor, None, imported_from.as_ref()).await?;
        }
        ThreadOp::Post {
            in_reply_to,
            status,
            imported_from,
            ..
        } => {
            let parent = in_reply_to.map(i64_of).transpose()?;
            rows += post(cx, at, &rec.actor, parent, imported_from.as_ref()).await?;
            rows += cx
                .execute(
                    "UPDATE view.thread SET post_count = post_count + 1,
                       last_post = GREATEST(last_post, $3), status = COALESCE($4, status)
                     WHERE tenant = $1 AND page_id = $2",
                    &[&tenant, &at.thread, &at.time, status],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        ThreadOp::Edit { object, .. } => {
            let object = i64_of(*object)?;
            rows += cx
                .execute(
                    "UPDATE view.post SET updated = $3, latest_offset = $4
                     WHERE tenant = $1 AND revid = $2 AND latest_offset < $4",
                    &[&tenant, &object, &at.time, &at.offset],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        ThreadOp::Rename { name } => {
            let subject = scatter_threads::normalize_subject(name).unwrap_or_else(|_| name.clone());
            rows += cx
                .execute(
                    "UPDATE view.thread SET subject = $3 WHERE tenant = $1 AND page_id = $2",
                    &[&tenant, &at.thread, &subject],
                )
                .await
                .map_err(|e| sql(&e))?;
            if let Some(title) = &rec.minted.title {
                rows += cx
                    .execute(
                        "UPDATE view.page SET title = $3 WHERE tenant = $1 AND page_id = $2",
                        &[&tenant, &at.thread, title],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }
        }
        ThreadOp::Move { target, keep } => {
            let Some(t) = talk else {
                return Err("a move's attestation carries the new home's talk page".into());
            };
            rows += talk_page(cx, tenant, t, target, at).await?;
            rows += if *keep {
                cx.execute(
                    "UPDATE view.thread_attachment SET home = false, attached_revid = $3
                     WHERE tenant = $1 AND thread_id = $2 AND home",
                    &[&tenant, &at.thread, &at.revid],
                )
                .await
            } else {
                cx.execute(
                    "DELETE FROM view.thread_attachment WHERE tenant = $1 AND thread_id = $2 AND home",
                    &[&tenant, &at.thread],
                )
                .await
            }
            .map_err(|e| sql(&e))?;
            // A listing promoted to home keeps its pin.
            let promoted = cx
                .execute(
                    "UPDATE view.thread_attachment SET home = true, attached = $4, attached_revid = $5
                     WHERE tenant = $1 AND thread_id = $2 AND talk_page_id = $3",
                    &[&tenant, &at.thread, &t, &at.time, &at.revid],
                )
                .await
                .map_err(|e| sql(&e))?;
            if promoted == 0 {
                rows += attach(cx, at, t, true, None).await?;
            }
            let (kind, id) = target_columns(target);
            rows += cx
                .execute(
                    "UPDATE view.thread SET target_kind = $3, target_id = $4, talk_page_id = $5
                     WHERE tenant = $1 AND page_id = $2",
                    &[&tenant, &at.thread, &kind, &id, &t],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        ThreadOp::Attach { target } => {
            let t = talk.ok_or("an attach's attestation carries the talk page")?;
            rows += talk_page(cx, tenant, t, target, at).await?;
            rows += attach(cx, at, t, false, None).await?;
        }
        ThreadOp::Detach { .. } => {
            let t = talk.ok_or("a detach's attestation carries the talk page")?;
            rows += cx
                .execute(
                    "DELETE FROM view.thread_attachment
                     WHERE tenant = $1 AND thread_id = $2 AND talk_page_id = $3 AND NOT home",
                    &[&tenant, &at.thread, &t],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        ThreadOp::Pin { .. } | ThreadOp::Unpin { .. } => {
            let t = talk.ok_or("a pin's attestation carries the talk page")?;
            let pin = matches!(rec.op, ThreadOp::Pin { .. }).then_some(at.time);
            rows += cx
                .execute(
                    "UPDATE view.thread_attachment SET pinned = $4
                     WHERE tenant = $1 AND thread_id = $2 AND talk_page_id = $3",
                    &[&tenant, &at.thread, &t, &pin],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
    }
    if !matches!(rec.op, ThreadOp::Create { .. }) {
        rows += cx
            .execute(
                "UPDATE view.page SET latest_offset = $3, latest_revid = $4, latest_at = $5,
                   revisions = revisions + 1
                 WHERE tenant = $1 AND page_id = $2 AND (latest_offset IS NULL OR latest_offset < $3)",
                &[&tenant, &at.thread, &at.offset, &at.revid, &at.time],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    Ok(usize::try_from(rows).unwrap_or(usize::MAX))
}

async fn attach(
    cx: &PgCx,
    at: &At<'_>,
    talk: i64,
    home: bool,
    pinned: Option<std::time::SystemTime>,
) -> Result<u64, String> {
    cx.execute(
        "INSERT INTO view.thread_attachment (tenant, thread_id, talk_page_id, home, attached,
           attached_revid, pinned)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (tenant, thread_id, talk_page_id) DO UPDATE SET home = EXCLUDED.home,
           attached = EXCLUDED.attached, attached_revid = EXCLUDED.attached_revid",
        &[
            &at.tenant, &at.thread, &talk, &home, &at.time, &at.revid, &pinned,
        ],
    )
    .await
    .map_err(|e| sql(&e))
}

async fn post(
    cx: &PgCx,
    at: &At<'_>,
    actor: &str,
    parent: Option<i64>,
    from: Option<&ImportedFrom>,
) -> Result<u64, String> {
    let original = from
        .and_then(|f| f.timestamp.as_deref())
        .and_then(crate::feed::parse_timestamp)
        .map(time_of);
    let actor = (!actor.is_empty()).then_some(actor);
    cx.execute(
        "INSERT INTO view.post (tenant, revid, thread_id, parent_revid, actor, published,
           latest_offset, dt_name, dt_id, original)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (tenant, revid) DO NOTHING",
        &[
            &at.tenant,
            &at.revid,
            &at.thread,
            &parent,
            &actor,
            &at.time,
            &at.offset,
            &from.and_then(|f| f.name.clone()),
            &from.and_then(|f| f.id.clone()),
            &original,
        ],
    )
    .await
    .map_err(|e| sql(&e))
}

/// Records a talk page for a target, and its `view.page` row where its title can be named:
/// the subject's title in the paired talk namespace, or the actor's name in `User talk`.
async fn talk_page(
    cx: &PgCx,
    tenant: &str,
    talk: i64,
    target: &Target,
    at: &At<'_>,
) -> Result<u64, String> {
    let (kind, id) = target_columns(target);
    let n = cx
        .execute(
            "INSERT INTO view.talk_page (tenant, page_id, target_kind, target_id)
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            &[&tenant, &talk, &kind, &id],
        )
        .await
        .map_err(|e| sql(&e))?;
    if n == 0 || matches!(target, Target::Page(p) if i64::try_from(*p).ok() == Some(talk)) {
        return Ok(n);
    }
    let named: Option<(i32, String)> = match target {
        Target::Page(page) => {
            let page = i64_of(*page)?;
            cx.query_opt(
                "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
                &[&tenant, &page],
            )
            .await
            .map_err(|e| sql(&e))?
            .and_then(|r| {
                let ns: i32 = r.get(0);
                Namespaces::embedded()
                    .get(ns)
                    .and_then(|n| n.talk)
                    .filter(|t| *t != ns)
                    .map(|t| (t, r.get::<_, String>(1)))
            })
        }
        Target::Actor(key) => cx
            .query_opt(
                "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2 AND name IS NOT NULL",
                &[&tenant, key],
            )
            .await
            .map_err(|e| sql(&e))?
            .map(|r| (3, r.get::<_, String>(0))),
    };
    let Some((ns, title)) = named else {
        return Ok(n);
    };
    let m = cx
        .execute(
            "INSERT INTO view.page (tenant, page_id, ns, title, content_model, latest_offset,
               latest_revid, latest_at, revisions, reserved)
             VALUES ($1, $2, $3, $4, 'triplespace-talk', $5, $6, $7, 0, false)
             ON CONFLICT DO NOTHING",
            &[&tenant, &talk, &ns, &title, &at.offset, &at.revid, &at.time],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(n + m)
}
