//! `batch=` on a write (0104 §1, §2, §6; 16 §2.2; 18 §2.3): the batch value a write names,
//! joined inside the write's own unit of work through [`triplespace_api_ingest::batch`].
//! The write then carries the job's ID in its attestation, the response carries it as
//! `batch`, and a write naming a rejected batch is refused with `ts-batch-rejected`.
//! The value is any non-empty string; the hash the log keeps is bound to the tenant and
//! the submitter's operator, so the same string under two operators is two batches.

use std::sync::atomic::Ordering;

use scatter_ingest::write::Attestation;
use serde_json::json;
use triplespace_api_ingest::batch::{self, BatchError};
use triplespace_projections::PgCx;

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::response::ApiError;

/// The batch value the request names, if any.
#[must_use]
pub fn value(ctx: &Ctx) -> Option<&str> {
    ctx.params
        .get("batch")
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// Joins the request's batch, if it names one, inside `cx`: the job ID the write's
/// records carry, which the response reports as `batch`. `None` for a write that names
/// no batch.
///
/// # Errors
///
/// `ts-batch-rejected` for a rejected batch; the store's.
pub async fn join(ctx: &Ctx, cx: &mut PgCx) -> Result<Option<u64>, ApiError> {
    let Some(value) = value(ctx) else {
        return Ok(None);
    };
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("edit"))?;
    let tenant = &ctx.tenant.slug;
    let operator = batch::operator_of(cx.conn(), tenant, &actor).await?;
    let hash = batch::hash(tenant, &operator, value);
    let attached = batch::attach(
        ctx.app.store(),
        ctx.app.pipeline(),
        cx,
        tenant,
        &actor,
        &operator,
        &hash,
        now_micros(),
    )
    .await
    .map_err(|e| match e {
        BatchError::Rejected { job } => ApiError::new(
            "ts-batch-rejected",
            format!("This batch was rejected as job {job}; writes naming it are refused."),
        )
        .with("job", json!(job)),
        BatchError::Ingest(e) => ApiError::internal(e),
    })?;
    ctx.batch.store(attached.job, Ordering::Relaxed);
    Ok(Some(attached.job))
}

/// [`join`], writing the job into an attestation.
///
/// # Errors
///
/// As [`join`].
pub async fn attach(
    ctx: &Ctx,
    cx: &mut PgCx,
    attestation: &mut Attestation,
) -> Result<(), ApiError> {
    if let Some(job) = join(ctx, cx).await? {
        attestation.job = Some(job);
    }
    Ok(())
}
