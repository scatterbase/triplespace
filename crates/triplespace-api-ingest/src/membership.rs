//! The one writer of membership records (09 §3.3; 0016 A35–A38): `action=userrights`
//! and the CLI.
//!
//! A change adds an account to some groups, each with an expiry, and removes it from
//! others. Each group is one `scatter:v0/membership` record in the tenant's `actors`
//! partition, keyed by the account, with the reason in the comment part and a log ID,
//! since each projects as a public `rights/rights` log event; all of them run their
//! projections in the caller's unit of work, so a refusal of one writes none.
//!
//! **Who may change what** is [`scatter_actors::may_change_membership`]: `owner` only by
//! its members; any other group with `userrights`; otherwise through the `adds`,
//! `removes`, `adds_self` and `removes_self` lists of the writer's groups. A group must
//! exist: in the tenant's registry, or with a member already (a group that only an ACL
//! names, 09 §4.2). **The last member of `owner`** leaves only when nothing needs it any
//! more ([`scatter_actors::owner_may_leave`]); the CLI, an instance act, may override
//! that (`force_owner`).

use std::collections::BTreeSet;

use scatter_actors::evaluate::Effective;
use scatter_actors::group::{GroupRegistry, OWNER};
use scatter_actors::membership::Membership;
use scatter_actors::{MembershipRefusal, Timestamp, may_change_membership, owner_may_leave};
use scatter_ingest::store::{IngestStore as _, Sequence};
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use triplespace_projections::PgCx;
use triplespace_projections::actors::payload;

use crate::PgIngest;

/// Why a membership change was refused. Nothing was written; the caller rolls back.
#[derive(Debug, thiserror::Error)]
pub enum MembershipError {
    /// The writer may not change this group's membership.
    #[error("{0}")]
    Refused(MembershipRefusal),
    /// No such group.
    #[error("there is no group `{0}`")]
    UnknownGroup(String),
    /// The last member of `owner` may not leave yet: the permission nobody else would hold.
    #[error("the last owner may not leave while no other group with a member holds `{0}`")]
    LastOwner(String),
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

fn store_err(e: impl std::fmt::Display) -> MembershipError {
    MembershipError::Store(e.to_string())
}

/// One change to one account's memberships.
#[derive(Debug, Clone, Default)]
pub struct Change {
    /// The account.
    pub account: String,
    /// Groups to add it to, each with an expiry.
    pub add: Vec<(String, Option<Timestamp>)>,
    /// Groups to remove it from.
    pub remove: Vec<String>,
    /// The reason.
    pub reason: String,
}

/// What a change did: the groups added to and removed from, those it already was or
/// was not in being left out, as MediaWiki reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changed {
    /// Added (or given a new expiry).
    pub added: Vec<String>,
    /// Removed.
    pub removed: Vec<String>,
}

/// The groups `account` is in on `tenant` now, explicit memberships only.
pub async fn current(cx: &PgCx, tenant: &str, account: &str) -> Result<BTreeSet<String>, String> {
    let rows = cx
        .conn()
        .query(
            "SELECT \"group\" FROM view.membership
             WHERE tenant = $1 AND actor_key = $2 AND layer = 'tenant'
               AND (expires IS NULL OR expires > $3)",
            &[&tenant, &account, &now_db()],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

fn now_db() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(i64::MAX)
}

/// The groups with a current explicit member on the tenant.
pub async fn populated(cx: &PgCx, tenant: &str) -> Result<BTreeSet<String>, String> {
    let rows = cx
        .conn()
        .query(
            "SELECT DISTINCT \"group\" FROM view.membership
             WHERE tenant = $1 AND layer = 'tenant' AND (expires IS NULL OR expires > $2)",
            &[&tenant, &now_db()],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// Who may write: an evaluated actor, or an instance act from the command line, which
/// may change any membership and may empty `owner` (09 §3.6).
#[derive(Debug, Clone, Copy)]
pub enum Writer<'a> {
    /// An actor on the tenant.
    Actor(&'a Effective),
    /// The CLI.
    Instance,
}

/// Writes a change (see the module documentation).
///
/// # Errors
///
/// A [`MembershipError`]; nothing is kept, and the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn change(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor: &str,
    writer: Writer<'_>,
    registry: &GroupRegistry,
    c: &Change,
    now: u64,
) -> Result<Changed, MembershipError> {
    let before = current(cx, tenant, &c.account)
        .await
        .map_err(MembershipError::Store)?;
    let mut groups_with_members = populated(cx, tenant)
        .await
        .map_err(MembershipError::Store)?;
    let own = actor == c.account;
    let mut out = Changed::default();
    let mut records = Vec::new();
    for (group, expires) in &c.add {
        if registry.by_name(group).is_none() && !groups_with_members.contains(group) {
            return Err(MembershipError::UnknownGroup(group.clone()));
        }
        if let Writer::Actor(who) = writer {
            may_change_membership(who, registry, group, true, own)
                .map_err(MembershipError::Refused)?;
        }
        records.push(Membership::add(group, *expires));
        out.added.push(group.clone());
    }
    for group in &c.remove {
        if !before.contains(group) {
            continue;
        }
        if let Writer::Actor(who) = writer {
            may_change_membership(who, registry, group, false, own)
                .map_err(MembershipError::Refused)?;
        }
        records.push(Membership::remove(group));
        out.removed.push(group.clone());
    }
    // The last owner (09 §3.6).
    if out.removed.iter().any(|g| g == OWNER) && matches!(writer, Writer::Actor(_)) {
        let owners = cx
            .conn()
            .query_one(
                "SELECT count(*) FROM view.membership
                 WHERE tenant = $1 AND \"group\" = $2 AND layer = 'tenant'
                   AND (expires IS NULL OR expires > $3)",
                &[&tenant, &OWNER, &now_db()],
            )
            .await
            .map_err(store_err)?
            .get::<_, i64>(0);
        if owners <= 1 {
            for g in &out.added {
                groups_with_members.insert(g.clone());
            }
            owner_may_leave(registry, &groups_with_members).map_err(MembershipError::LastOwner)?;
        }
    }
    let actors = store
        .partition(cx, tenant, "actors")
        .await
        .map_err(store_err)?
        .ok_or_else(|| store_err(format!("no `actors` partition for `{tenant}`")))?;
    for m in records {
        let logid = store
            .next_id(cx, tenant, &Sequence::Log)
            .await
            .map_err(store_err)?;
        let content = Value::from_json(&serde_json::to_value(&m).map_err(store_err)?);
        let draft = Draft {
            appended_at: now,
            payload_type: payload::MEMBERSHIP.into(),
            key: Some(c.account.clone()),
            revid: None,
            logid: Some(logid),
            page_id: None,
            body: Body::core(
                &content,
                &Value::text(&c.reason),
                &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
            )
            .map_err(store_err)?,
        };
        append_projected(store, pipeline, cx, actors, draft, Budget::NONE)
            .await
            .map_err(store_err)?;
    }
    Ok(out)
}
