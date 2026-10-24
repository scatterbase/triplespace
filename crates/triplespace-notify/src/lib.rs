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
//!
//! **Nobody is notified of their own action.** The preference matrix
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
        header.payload_type == payload::MEMBERSHIP || header.payload_type == payload::BLOCK
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
            let performer = attested(record).actor;
            let account = h.key.clone().unwrap_or_default();
            let published = i64::try_from(h.appended_at).unwrap_or(i64::MAX);
            let mut rows = 0u64;
            for d in Self::deliveries(cx, &tenant, record).await? {
                if performer.as_deref() == Some(d.recipient.as_str()) {
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
                            &partition_db(h.partition),
                            &i64::try_from(h.offset).unwrap_or(i64::MAX),
                            &d.reason,
                            &target_kind::ACTOR,
                            &account,
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
