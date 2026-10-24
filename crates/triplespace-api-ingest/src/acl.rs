//! The one writer of ACL records (0023 §3; 0016 §4; 0056 §4; 09 §4.3, §4.5): protection,
//! deletion, hiding and suppression, and their undoing, shared by the Action API's
//! moderation modules, `/acl` and the CLI.
//!
//! [`write()`] appends a `scatter:v0/acl` record keyed by the target key, with the reason in
//! the comment part, to the partition the target belongs in (the tenant `log`, its
//! `config` for graphs and the tenant, the instance `log` for blobs), with a log ID, since
//! every ACL record projects as a log event, and runs the inline projections in the
//! caller's unit of work. `None` retires the target's ACL.
//!
//! **Who may write it.** The right [`Acl::required_right`] names for the new ACL, and for
//! the ACL it replaces or retires, so that undoing a suppression needs what doing it
//! needed. A confidential `read` restriction may name only a group the writer is in
//! (0056 §4).
//!
//! **A suppression is marked** `suppress: true` in the record's attestation map: the ACL
//! written, or for a retirement the one retired, restricts to the tenant's suppression
//! group. The writer resolves that group from settings; the projections that draw the
//! suppressed bit and the `suppress/…` log events read the mark instead.
//!
//! **A reserved title** (09 §4.8): [`AclWrite::reserve`] takes a page ID from the
//! sequence for a title that has no page, and the ACL, keyed by that ID, carries the
//! namespace and title, so a `create` there takes the ID.

use scatter_actors::acl::ReservedTitle;
use scatter_actors::evaluate::Effective;
use scatter_actors::{Acl, AclPartition, Target, Timestamp};
use scatter_ingest::store::{IngestStore as _, Sequence};
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::record::Record;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use triplespace_projections::PgCx;
use triplespace_projections::actors::payload;

use crate::PgIngest;

/// Why an ACL write was refused. The caller rolls back.
#[derive(Debug, thiserror::Error)]
pub enum AclWriteError {
    /// The writer lacks the right the ACL needs.
    #[error("this needs the \"{0}\" right")]
    Denied(&'static str),
    /// The ACL is malformed.
    #[error("{0}")]
    Invalid(String),
    /// A confidential restriction names a group the writer is not in, or the write changes
    /// or lifts one whose group the writer is not in (09 §4.5; 0056 A18).
    #[error("a confidential restriction may be set, changed or lifted only by a member of `{0}`")]
    LockedOut(String),
    /// The tenancy policy does not allow the confidential restriction
    /// (`security.restrictions`, 0056 §11): `ts-policy`.
    #[error("{0}")]
    Policy(String),
    /// There is no ACL to retire.
    #[error("{0} has no ACL")]
    NothingToRetire(String),
    /// The store refused.
    #[error("store: {0}")]
    Store(String),
}

fn store_err(e: impl std::fmt::Display) -> AclWriteError {
    AclWriteError::Store(e.to_string())
}

/// One ACL write.
#[derive(Debug, Clone)]
pub struct AclWrite {
    /// The target.
    pub target: Target,
    /// The new ACL, or `None` to retire the current one.
    pub acl: Option<Acl>,
    /// The reason, for the comment part.
    pub reason: String,
    /// The tenant's suppression group, `moderation.suppress_group`.
    pub suppress_group: String,
    /// Reserve this title under a new page ID (create-protection): `target` and the
    /// ACL's target are replaced by `acl:page:{new ID}`.
    pub reserve: Option<ReservedTitle>,
    /// The namespaces and sets enclosing the target, against which a scoped group's
    /// rights are tested (09 §3.4); empty counts unscoped rights only.
    pub enclosing: Vec<Target>,
}

/// The target's current ACL.
pub async fn current(cx: &PgCx, tenant: &str, target: &Target) -> Result<Option<Acl>, String> {
    Ok(
        triplespace_projections::acl_read::current(cx, tenant, std::slice::from_ref(target))
            .await?
            .pop(),
    )
}

/// Writes or retires an ACL (see the module documentation), and returns the record.
#[allow(clippy::too_many_arguments)]
///
/// # Errors
///
/// An [`AclWriteError`]; the caller rolls back.
pub async fn write(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor: &str,
    who: &Effective,
    mut w: AclWrite,
    now: u64,
) -> Result<Record, AclWriteError> {
    if let Some(r) = w.reserve.take() {
        let Some(acl) = w.acl.as_mut() else {
            return Err(AclWriteError::Invalid(
                "a reserved title needs an ACL".into(),
            ));
        };
        let id = store
            .next_id(cx, tenant, &Sequence::Page)
            .await
            .map_err(store_err)?;
        w.target = Target::Page(id);
        acl.target = w.target.clone();
        acl.reserved = Some(r);
    }
    // A new set takes its ID from the page-ID sequence, as a reservation does (09 §4.7).
    if w.target == Target::Set(0) {
        let id = store
            .next_id(cx, tenant, &Sequence::Page)
            .await
            .map_err(store_err)?;
        w.target = Target::Set(id);
        if let Some(acl) = w.acl.as_mut() {
            acl.target = w.target.clone();
        }
    }
    let existing = current(cx, tenant, &w.target)
        .await
        .map_err(AclWriteError::Store)?;
    let mut rights = Vec::new();
    if let Some(acl) = &w.acl {
        if acl.target != w.target {
            return Err(AclWriteError::Invalid(format!(
                "the ACL names {} under {}",
                acl.target, w.target
            )));
        }
        acl.validate()
            .map_err(|e| AclWriteError::Invalid(e.to_string()))?;
        rights.push(acl.required_right(&w.suppress_group));
    }
    check_confidential(cx, existing.as_ref(), &w, who).await?;
    match &existing {
        Some(old) => rights.push(old.required_right(&w.suppress_group)),
        None if w.acl.is_none() => {
            return Err(AclWriteError::NothingToRetire(w.target.key()));
        }
        None => {}
    }
    if let Some(r) = rights.into_iter().find(|r| !who.holds_on(r, &w.enclosing)) {
        return Err(AclWriteError::Denied(r));
    }

    let (owner, graph) = match w.target.partition() {
        AclPartition::TenantLog => (tenant, "log"),
        AclPartition::TenantConfig => (tenant, "config"),
        AclPartition::InstanceLog => ("", "log"),
    };
    let partition = store
        .partition(cx, owner, graph)
        .await
        .map_err(store_err)?
        .ok_or_else(|| store_err(format!("no `{graph}` partition for `{owner}`")))?;
    let suppressed = w
        .acl
        .as_ref()
        .or(existing.as_ref())
        .is_some_and(|a| a.required_right(&w.suppress_group) == "suppressrevision");
    let content = match &w.acl {
        Some(acl) => Value::from_json(&serde_json::to_value(acl).map_err(store_err)?),
        None => Value::Null,
    };
    let logid = store
        .next_id(cx, tenant, &Sequence::Log)
        .await
        .map_err(store_err)?;
    let draft = Draft {
        appended_at: now,
        payload_type: payload::ACL.into(),
        key: Some(w.target.key()),
        revid: None,
        logid: Some(logid),
        page_id: None,
        body: Body::core(
            &content,
            &Value::text(&w.reason),
            &Value::map(if suppressed {
                vec![
                    (Value::text("actor"), Value::text(actor)),
                    (Value::text("suppress"), Value::Bool(true)),
                ]
            } else {
                vec![(Value::text("actor"), Value::text(actor))]
            }),
        )
        .map_err(store_err)?,
    };
    let (record, _) = append_projected(store, pipeline, cx, partition, draft, Budget::NONE)
        .await
        .map_err(store_err)?;
    Ok(record)
}

/// The rules for a confidential restriction (09 §4.5; 0056 §11, A18): only its group's
/// members may set, change or lift one, and one this write sets or changes must be one
/// the instance's tenancy policy allows; one it keeps as it was is not re-examined.
async fn check_confidential(
    cx: &PgCx,
    existing: Option<&Acl>,
    w: &AclWrite,
    who: &Effective,
) -> Result<(), AclWriteError> {
    scatter_actors::check_lock_out_change(existing, w.acl.as_ref(), who).map_err(|e| match e {
        scatter_actors::AclError::LockedOut(g) => AclWriteError::LockedOut(g),
        other => AclWriteError::Invalid(other.to_string()),
    })?;
    let new_confidential = w.acl.as_ref().and_then(Acl::confidential_read);
    if new_confidential.is_some() && new_confidential != existing.and_then(Acl::confidential_read) {
        let switches = triplespace_projections::tenants::tenancy(cx.conn())
            .await
            .map_err(store_err)?;
        let policy = scatter_actors::TenancyPolicy {
            preset: None,
            switches,
        };
        policy
            .check_restriction(&w.target)
            .map_err(|e| AclWriteError::Policy(e.to_string()))?;
    }
    Ok(())
}

/// Whether a deletion (a moderation `read` restriction) is in force on the ACL at `now`.
#[must_use]
pub fn deletes(acl: &Acl, now: u64) -> bool {
    acl.restriction_at(scatter_actors::acl::READ, Timestamp(now))
        .is_some()
        && acl.read_kind == Some(scatter_actors::ReadKind::Moderation)
}
