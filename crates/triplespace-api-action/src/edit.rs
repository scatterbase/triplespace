//! From a Wikibase edit to log records (0002 §8.1: the Action API submits a batch of one;
//! 0006 §8; 0013 §7). A write module computes the entity as the editor wants it, and this
//! module turns the difference from the current state into operations:
//!
//! - a new entity is one `create`;
//! - additions of terms, statements, sitelinks are one `add`;
//! - retractions are one `remove` (statements by GUID, terms by language, aliases by
//!   value, sitelinks by site) — of what the local graph asserts;
//! - a changed statement is a `remove` of its GUID and an `add` of the new statement under
//!   the same GUID, in that order, in one unit of work;
//! - a mirrored statement the editor removes or changes is not the tenant's to retract, so
//!   it is suppressed with an `override` (0004 §8), and a changed one re-added under a
//!   fresh local GUID.
//!
//! All the records of one edit go in one transaction with the base-offset check carried
//! from record to record, so a client sees either the whole edit or an `editconflict`.
//! The revision the module reports is the last record's.

use scatter_ingest::IngestError;
use scatter_ingest::write::{Attestation, Request, write_operation};
use scatter_projection::Backend as _;
use scatter_wikibase_changeset::Operation;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use serde_json::json;
pub use triplespace_api_ingest::diff::{diff, operations_from};
use triplespace_projections::read::{self, Current};

use crate::api::Ctx;
use crate::response::ApiError;

/// Microseconds since the epoch, now.
#[must_use]
pub fn now_micros() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}

/// What every write module carries beside its own parameters (compat §4.1).
#[derive(Debug, Clone, Default)]
pub struct EditMeta {
    /// `summary`.
    pub summary: Option<String>,
    /// `bot`.
    pub bot: bool,
    /// `baserevid`.
    pub baserevid: Option<u64>,
    /// `tags`.
    pub tags: Vec<String>,
}

impl EditMeta {
    /// From the request.
    pub fn from_params(ctx: &Ctx) -> Result<Self, ApiError> {
        let baserevid = match ctx.params.get("baserevid") {
            Some(v) => Some(v.parse::<u64>().map_err(|_| {
                ApiError::new(
                    "badinteger",
                    format!("Invalid value \"{v}\" for integer parameter \"baserevid\"."),
                )
            })?),
            None => None,
        };
        Ok(Self {
            summary: ctx.params.get("summary").map(str::to_string),
            bot: ctx.params.flag("bot"),
            baserevid,
            tags: ctx.params.list("tags"),
        })
    }
}

/// Reads the entity's current state.
pub async fn current(ctx: &Ctx, id: &EntityId) -> Result<Option<Current>, ApiError> {
    read::current(&ctx.client, ctx.app.order(), &ctx.tenant.slug, id)
        .await
        .map_err(ApiError::internal)
}

/// The operations that take `before` to `after`: a `create` for a new entity, else the
/// difference against the resolved state.
#[must_use]
pub fn operations(before: Option<&Current>, after: &Entity) -> Vec<Operation> {
    let Some(before) = before else {
        return vec![Operation::Create {
            id: Some(after.id.clone()),
            temporary_ref: None,
            entity: after.clone(),
            match_key: None,
            via: None,
        }];
    };
    diff(
        &before.resolved.entity,
        &|g| before.is_local_statement(g),
        after,
    )
}

/// What a committed edit reports.
#[derive(Debug, Clone)]
pub struct Saved {
    /// The last record's revision ID.
    pub lastrevid: Option<u64>,
    /// How many records were written.
    pub records: usize,
}

/// The local offset a `baserevid` names (0006 §8): the record with that revision ID in
/// the tenant's `local` partition.
async fn base_offset(ctx: &Ctx, revid: u64) -> Result<Option<u64>, ApiError> {
    let row = ctx
        .client
        .query_opt(
            "SELECT a.\"offset\" FROM view.activity a JOIN log.partition p ON p.partition = a.partition
             WHERE a.tenant = $1 AND a.revid = $2 AND p.name = 'local'",
            &[&ctx.tenant.slug, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await?;
    Ok(row.map(|r| u64::try_from(r.get::<_, i64>(0)).unwrap_or(0)))
}

/// Writes the operations of one edit in one unit of work.
pub async fn commit(
    ctx: &Ctx,
    ops: &[Operation],
    meta: &EditMeta,
    before: Option<&Current>,
) -> Result<Saved, ApiError> {
    if ops.is_empty() {
        // The entity is already as the edit asks (18 §1.5).
        ctx.unchanged("The entity is already as this edit would leave it.");
        return Ok(Saved {
            lastrevid: before.and_then(|c| c.lastrevid),
            records: 0,
        });
    }
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("edit"))?;
    let mut attestation = Attestation::by(&actor);
    attestation.tags.clone_from(&meta.tags);
    if meta.bot && ctx.caller.effective.holds("bot") {
        attestation.tags.push("bot".into());
    }
    let mut request = Request::new(&ctx.tenant.slug, attestation, now_micros());
    request.comment.clone_from(&meta.summary);
    request.base_offset = match (meta.baserevid, before) {
        (Some(revid), Some(_)) => Some(base_offset(ctx, revid).await?.ok_or_else(|| {
            ApiError::new(
                "nosuchrevid",
                format!("There is no revision with ID {revid}."),
            )
        })?),
        _ => None,
    };
    let store = ctx.app.store();
    let pipeline = ctx.app.pipeline();
    let mut cx = store.begin().await?;
    // The batch the edit names, joined in the same unit of work (0104 §1).
    if let Err(e) = crate::batch::attach(ctx, &mut cx, &mut request.attestation).await {
        let _ = store.rollback(cx).await;
        return Err(e);
    }
    let mut lastrevid = None;
    let mut records = 0;
    for op in ops {
        match write_operation(store, pipeline, &mut cx, &request, op, ctx.app.registry()).await {
            Ok(written) => {
                lastrevid = written.record.header().revid;
                request.base_offset = Some(written.offset());
                records += 1;
            }
            Err(IngestError::Conflict { latest, .. }) => {
                let _ = store.rollback(cx).await;
                let current_revid = before.and_then(|c| c.local_revid);
                return Err(ApiError::new(
                    "editconflict",
                    format!(
                        "Edit conflict: the entity changed since revision {}.",
                        meta.baserevid.unwrap_or(0)
                    ),
                )
                .with("currentrevid", json!(current_revid))
                .with("latestoffset", json!(latest)));
            }
            Err(IngestError::Invalid(problems)) => {
                let _ = store.rollback(cx).await;
                return Err(ApiError::new(
                    "modification-failed",
                    problems
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
            Err(e) => {
                let _ = store.rollback(cx).await;
                return Err(ApiError::new("failed-save", e.to_string()));
            }
        }
    }
    store.commit(cx).await?;
    Ok(Saved { lastrevid, records })
}
