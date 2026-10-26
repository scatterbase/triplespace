//! Notifications (0021; 16 §5): the addressing projection that delivers activity to
//! inboxes, and the inbox itself, `private.inbox` and `private.inbox_state`.
//!
//! **Addressing is a projection** ([`AddressingProjection`], step `Addressing`): it reads
//! each record after the activity projection has written its row, decides who is
//! addressed and why (16 §5.2), and inserts one inbox row per (activity, recipient,
//! reason), idempotently, so a rebuild refills the inbox (read state excepted). The reasons
//! built so far:
//!
//! - **`rights`**: an account's memberships changed, or it was blocked or unblocked. A
//!   subsidiary's operator is addressed in its place.
//! - **`group-join`**: an account joined a group whose record sets `notify_joins`, by any
//!   route; each other member is addressed. Inclusion passes nothing on (0021 A19). A join
//!   by an account whose name is hidden (an `actor` ACL in force) is delivered to nobody,
//!   so that the notification cannot carry the name past the restriction.
//! - **`talk`**, **`reply`**, **`mention`** and **`thread-status`** (0019 §8; 0021 A20): a
//!   thread record, as [`triplespace_projections::threads::addressed`] reads it against the
//!   thread before it. `talk` addresses the owner of the user talk page that is the thread's
//!   home, for a new thread or post there; `reply`, the author of the post a post answers;
//!   `mention`, each local account whose user page the text links to, and for an edit only
//!   those it did not link before; `thread-status`, whoever started the thread, when a post
//!   moves it from an open status to a closed one. An account is addressed once per record,
//!   for the first of those reasons that applies, in that order. Imported threads and posts
//!   address nobody.
//!
//! - **`watch`** (0021 A21): a change reaches a target an account watches with `notify`
//!   (`private.watch`, read through [`triplespace_accounts::watch::notified`]): a new
//!   thread or post reaches the thread and the subject of every page it is attached to; any
//!   record of a document page reaches the page's target, and its title's for a watch
//!   made before the page existed ([`triplespace_projections::watched`]). It comes last:
//!   an account a thread record already addresses for another reason is not addressed
//!   again. Entities wait for a decision on mirror syncs.
//!
//! **Only local accounts have inboxes**, and a subsidiary's go to its operator; a thread
//! notification goes to a registered account, or a bot's operator, and to no surrogate or
//! temporary account. **A thread the recipient may not read addresses them not at all**, and
//! a record already hidden addresses nobody. **What is hidden later leaves the inbox**: a
//! `record` ACL that hides any part of a record deletes every row it was delivered as, and a
//! `read` ACL on a thread deletes its rows from the inboxes of those who may no longer read
//! it. Restrictions that reach a thread through its talk page, its namespace or the tenant
//! are checked when the inbox is read ([`inbox`]'s caller), not purged.
//!
//! **Nobody is notified of their own action**, nor of their subsidiary's. The preference matrix
//! (`notifications.{reason}.{channel}`) and channels other than the web inbox are not
//! built yet: every reason is delivered to the inbox.
//!
//! [`inbox`] reads and marks an account's inbox for the Echo-shaped modules.

#![forbid(unsafe_code)]

pub mod inbox;

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use triplespace_projections::PgCx;
use triplespace_projections::actors::payload;
use triplespace_projections::common::{attested, content, target_kind, tenant_of, to_jsonb};
use triplespace_projections::threads::{self, Addressed};

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// A partition ID as its `bigint`, bit for bit, as `view.activity` stores it.
#[must_use]
#[allow(clippy::cast_possible_wrap)]
pub fn partition_db(partition: u64) -> i64 {
    partition as i64
}

/// The addressing projection (see the crate documentation).
#[derive(Debug, Default, Clone, Copy)]
pub struct AddressingProjection;

/// One delivery.
struct Delivery<'a> {
    recipient: String,
    reason: &'a str,
    target_kind: i16,
    target_id: String,
}

/// The inbox a thread notification for `key` goes to (16 §5.2): a registered account's
/// own, a bot's operator's; `None` for a surrogate, a temporary account, a provider or an
/// actor with no row.
async fn local_inbox(cx: &PgCx, tenant: &str, key: &str) -> Result<Option<String>, String> {
    let row = cx
        .conn()
        .query_opt(
            "SELECT kind, operator FROM view.actor WHERE tenant = $1 AND actor_key = $2",
            &[&tenant, &key],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.and_then(|r| {
        let kind: String = r.get(0);
        let operator: Option<String> = r.get(1);
        match kind.as_str() {
            "registered" => Some(key.to_string()),
            "bot" => operator,
            _ => None,
        }
    }))
}

/// The local accounts with these names on the tenant, by name.
async fn by_name(cx: &PgCx, tenant: &str, names: &[String]) -> Result<Vec<String>, String> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    Ok(cx
        .conn()
        .query(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = ANY($2)
               AND kind IN ('registered', 'bot')",
            &[&tenant, &names],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// Whether the activity row of a record is hidden in any part, or erased.
async fn activity_hidden(cx: &PgCx, partition: i64, offset: i64) -> Result<bool, String> {
    Ok(cx
        .conn()
        .query_opt(
            "SELECT 1 FROM view.activity WHERE partition = $1 AND \"offset\" = $2
               AND (visibility <> 0 OR kind = 'erased')",
            &[&partition, &offset],
        )
        .await
        .map_err(|e| sql(&e))?
        .is_some())
}

/// Whether an account may read a page or thread now.
async fn may_read(cx: &PgCx, tenant: &str, account: &str, thread: u64) -> Result<bool, String> {
    let Some(who) = triplespace_accounts::identity::effective_of(cx.conn(), tenant, account)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    triplespace_projections::acl_read::page_readable(cx.conn(), tenant, &who, thread).await
}

/// Adds a `watch` delivery to `out` for every account that watches one of `reached` with
/// `notify`, unless `out` addresses it already; the notification is about `page`.
async fn watchers(
    cx: &PgCx,
    tenant: &str,
    reached: &[(i16, String)],
    page: u64,
    out: &mut Vec<Delivery<'static>>,
) -> Result<(), String> {
    let holders = triplespace_accounts::watch::notified(cx.conn(), reached)
        .await
        .map_err(|e| sql(&e))?;
    for (holder, _, _) in holders {
        let Some(to) = local_inbox(cx, tenant, &holder).await? else {
            continue;
        };
        if out.iter().any(|d| d.recipient == to) {
            continue;
        }
        out.push(Delivery {
            recipient: to,
            reason: "watch",
            target_kind: target_kind::PAGE,
            target_id: page.to_string(),
        });
    }
    Ok(())
}

/// Who a thread record addresses, and why, before the read checks (16 §5.2). An account
/// appears once, for its first reason in the order `talk`, `thread-status`, `reply`,
/// `mention`.
async fn thread_deliveries(
    cx: &PgCx,
    tenant: &str,
    a: &Addressed,
) -> Result<Vec<Delivery<'static>>, String> {
    let mut candidates: Vec<(String, &'static str)> = Vec::new();
    if a.new_post
        && let Some(owner) = &a.home_actor
    {
        candidates.push((owner.clone(), "talk"));
    }
    if let Some(starter) = &a.closed_for {
        candidates.push((starter.clone(), "thread-status"));
    }
    if let Some(parent) = &a.replied_to {
        candidates.push((parent.clone(), "reply"));
    }
    for key in by_name(cx, tenant, &a.mentions).await? {
        candidates.push((key, "mention"));
    }
    let mut out: Vec<Delivery<'static>> = Vec::new();
    for (key, reason) in candidates {
        let Some(to) = local_inbox(cx, tenant, &key).await? else {
            continue;
        };
        if out.iter().any(|d| d.recipient == to) {
            continue;
        }
        out.push(Delivery {
            recipient: to,
            reason,
            target_kind: target_kind::PAGE,
            target_id: a.thread.to_string(),
        });
    }
    Ok(out)
}

/// The account whose inbox receives what is addressed to `key`: its operator for a
/// subsidiary (16 §5.2), itself otherwise; `None` for an actor that has no row.
async fn inbox_of(cx: &PgCx, tenant: &str, key: &str) -> Result<Option<String>, String> {
    let row = cx
        .conn()
        .query_opt(
            "SELECT operator FROM view.actor WHERE tenant = $1 AND actor_key = $2",
            &[&tenant, &key],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.map(|r| {
        let operator: Option<String> = r.get(0);
        operator.unwrap_or_else(|| key.to_string())
    }))
}

/// Whether an `actor` ACL hides the account's name now.
async fn name_hidden(cx: &PgCx, tenant: &str, key: &str) -> Result<bool, String> {
    Ok(cx
        .conn()
        .query_opt(
            "SELECT 1 FROM view.acl WHERE tenant IN ($1, '') AND target = $2",
            &[&tenant, &format!("acl:actor:{key}")],
        )
        .await
        .map_err(|e| sql(&e))?
        .is_some())
}

/// The members of a group on the tenant as of a membership record in the `actors`
/// partition: those whose membership was recorded earlier in that partition and had not
/// lapsed at `at` (microseconds). Addressing runs behind the write path, so the current
/// rows also hold later joins, which are left out.
async fn members(
    cx: &PgCx,
    tenant: &str,
    group: &str,
    before: i64,
    at: i64,
) -> Result<Vec<String>, String> {
    Ok(cx
        .conn()
        .query(
            "SELECT actor_key FROM view.membership
             WHERE tenant = $1 AND \"group\" = $2 AND layer = 'tenant'
               AND \"offset\" < $3 AND (expires IS NULL OR expires > $4)",
            &[&tenant, &group, &before, &at],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

impl AddressingProjection {
    async fn deliveries(
        cx: &PgCx,
        tenant: &str,
        record: &Record,
    ) -> Result<Vec<Delivery<'static>>, String> {
        let h = record.header();
        let Some(account) = h.key.clone() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        if let Some(to) = inbox_of(cx, tenant, &account).await? {
            out.push(Delivery {
                recipient: to,
                reason: "rights",
                target_kind: target_kind::ACTOR,
                target_id: account.clone(),
            });
        }
        if h.payload_type == payload::MEMBERSHIP
            && let Some(v) = content(record)?
        {
            let m = to_jsonb(&v);
            let added = m.get("action").and_then(serde_json::Value::as_str) == Some("add");
            let group = m
                .get("group")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if added && !name_hidden(cx, tenant, &account).await? {
                let registry = triplespace_accounts::groups::registry(cx.conn(), tenant)
                    .await
                    .map_err(|e| sql(&e))?;
                if registry.by_name(group).is_some_and(|g| g.notify_joins) {
                    let at = i64::try_from(h.appended_at).unwrap_or(i64::MAX);
                    let before = i64::try_from(h.offset).unwrap_or(i64::MAX);
                    for member in members(cx, tenant, group, before, at).await? {
                        if member != account {
                            out.push(Delivery {
                                recipient: member,
                                reason: "group-join",
                                target_kind: target_kind::ACTOR,
                                target_id: account.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

impl<B: Backend<Cx = PgCx>> Projection<B> for AddressingProjection {
    fn name(&self) -> &'static str {
        "addressing"
    }

    fn step(&self) -> Step {
        Step::Addressing
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == payload::MEMBERSHIP
            || header.payload_type == payload::BLOCK
            || header.payload_type == scatter_threads_payload::THREAD
            || header.payload_type == scatter_threads_payload::PAGE
            || header.payload_type == payload::ACL
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx, h.partition).await?;
            if tenant.is_empty() {
                // Instance-layer memberships and blocks: no tenant inbox to address yet.
                return Ok(Applied::rows(0));
            }
            if h.payload_type == payload::ACL {
                return purge(cx, &tenant, record).await;
            }
            let performer = attested(record).actor;
            // A subsidiary's action is its operator's own (16 §5.2).
            let performer_inbox = match &performer {
                Some(p) => inbox_of(cx, &tenant, p).await?,
                None => None,
            };
            let published = i64::try_from(h.appended_at).unwrap_or(i64::MAX);
            let part = partition_db(h.partition);
            let offset = i64::try_from(h.offset).unwrap_or(i64::MAX);
            let deliveries = if h.payload_type == scatter_threads_payload::THREAD {
                if activity_hidden(cx, part, offset).await? {
                    return Ok(Applied::rows(0));
                }
                match threads::addressed(cx, &tenant, record).await? {
                    Some(a) => {
                        let mut found = thread_deliveries(cx, &tenant, &a).await?;
                        if a.new_post {
                            let reached = triplespace_projections::watched::reached_by_thread(
                                cx.conn(),
                                &tenant,
                                a.thread,
                            )
                            .await?;
                            watchers(cx, &tenant, &reached, a.thread, &mut found).await?;
                        }
                        let mut out = Vec::new();
                        for d in found {
                            if may_read(cx, &tenant, &d.recipient, a.thread).await? {
                                out.push(d);
                            }
                        }
                        out
                    }
                    None => Vec::new(),
                }
            } else if h.payload_type == scatter_threads_payload::PAGE {
                if activity_hidden(cx, part, offset).await? || content(record)?.is_none() {
                    return Ok(Applied::rows(0));
                }
                let Some(page) = h.page_id else {
                    return Ok(Applied::rows(0));
                };
                let reached =
                    triplespace_projections::watched::reached_by_page(cx.conn(), &tenant, page)
                        .await?;
                let mut found = Vec::new();
                watchers(cx, &tenant, &reached, page, &mut found).await?;
                let mut out = Vec::new();
                for d in found {
                    if may_read(cx, &tenant, &d.recipient, page).await? {
                        out.push(d);
                    }
                }
                out
            } else {
                Self::deliveries(cx, &tenant, record).await?
            };
            let mut rows = 0u64;
            for d in deliveries {
                if performer.as_deref() == Some(d.recipient.as_str())
                    || performer_inbox.as_deref() == Some(d.recipient.as_str())
                {
                    continue;
                }
                rows += cx
                    .conn()
                    .execute(
                        "INSERT INTO private.inbox
                           (actor_key, partition, \"offset\", reason, target_kind, target_id, published)
                         VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::bigint::double precision / 1e6))
                         ON CONFLICT (actor_key, partition, \"offset\", reason) DO NOTHING",
                        &[
                            &d.recipient,
                            &part,
                            &offset,
                            &d.reason,
                            &d.target_kind,
                            &d.target_id,
                            &published,
                        ],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            cx.conn()
                .execute(
                    "DELETE FROM private.inbox WHERE partition = $1",
                    &[&partition_db(partition)],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(())
        })
    }
}

/// The payload type of thread records, as `scatter-threads` names it.
mod scatter_threads_payload {
    pub const THREAD: &str = triplespace_projections::threads::PAYLOAD;
    pub const PAGE: &str = triplespace_projections::pages::PAYLOAD;
}

/// What an ACL record takes out of inboxes (16 §5.2; 03 §9.6 item 5): a `record` ACL
/// that leaves the record's activity row hidden in any part deletes every row delivered
/// for it; an ACL on a thread deletes its rows from the inboxes of those who may no longer
/// read it.
async fn purge(cx: &PgCx, tenant: &str, record: &Record) -> Result<Applied, String> {
    let key = record.header().key.clone().unwrap_or_default();
    let mut rows = 0u64;
    if let Some(rest) = key.strip_prefix("acl:record:") {
        let mut it = rest.splitn(2, ':');
        let (Some(p), Some(o)) = (
            it.next().and_then(|p| p.parse::<u64>().ok()),
            it.next().and_then(|o| o.parse::<i64>().ok()),
        ) else {
            return Ok(Applied::rows(0));
        };
        let p = partition_db(p);
        if activity_hidden(cx, p, o).await? {
            rows += cx
                .conn()
                .execute(
                    "DELETE FROM private.inbox WHERE partition = $1 AND \"offset\" = $2",
                    &[&p, &o],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
    } else if let Some(page) = key
        .strip_prefix("acl:page:")
        .and_then(|p| p.parse::<u64>().ok())
    {
        let id = page.to_string();
        let holders: Vec<String> = cx
            .conn()
            .query(
                "SELECT DISTINCT actor_key FROM private.inbox WHERE target_kind = $1 AND target_id = $2",
                &[&target_kind::PAGE, &id],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| r.get(0))
            .collect();
        for holder in holders {
            if !may_read(cx, tenant, &holder, page).await? {
                rows += cx
                    .conn()
                    .execute(
                        "DELETE FROM private.inbox WHERE actor_key = $1 AND target_kind = $2 AND target_id = $3",
                        &[&holder, &target_kind::PAGE, &id],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }
        }
    }
    Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
}

/// The pipeline with addressing registered after everything it reads.
///
/// # Errors
///
/// As [`scatter_projection::Pipeline::with`].
pub fn with_addressing<B: Backend<Cx = PgCx>>(
    pipeline: scatter_projection::Pipeline<B>,
) -> Result<scatter_projection::Pipeline<B>, scatter_projection::ProjectionError> {
    pipeline.with(AddressingProjection)
}
