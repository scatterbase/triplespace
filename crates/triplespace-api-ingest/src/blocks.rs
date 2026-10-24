//! The one writer of block records (09 §3.3; 0016 §3): `action=block` and
//! `action=unblock`.
//!
//! A block is a `scatter:v0/block` record in the tenant's `actors` partition, keyed by the
//! blocked account, naming what it removes (by default `edit` and `createpage`, or
//! everything but `read`) and an expiry, with the reason in the comment part and a log ID,
//! since it projects as `block/block`, `block/reblock` or `block/unblock`. Writing one
//! needs `block`. A block on an account already blocked is refused unless it says it
//! replaces the block (`reblock`); an unblock of an account not blocked is refused.
//! Hiding the name is not this writer's: it is an `actor` ACL, written beside the block
//! through [`crate::acl`].

use scatter_actors::evaluate::Effective;
use scatter_actors::membership::Block;
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

/// Why a block write was refused; the caller rolls back.
#[derive(Debug, thiserror::Error)]
pub enum BlockError {
    /// The writer lacks `block`.
    #[error("this needs the \"block\" right")]
    Denied,
    /// The account is blocked already and the write does not say it replaces the block.
    #[error("the account is already blocked")]
    AlreadyBlocked,
    /// The account is not blocked.
    #[error("the account is not blocked")]
    NotBlocked,
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

fn store_err(e: impl std::fmt::Display) -> BlockError {
    BlockError::Store(e.to_string())
}

/// Whether the account has a block in force in the tenant's layer.
pub async fn blocked(cx: &PgCx, tenant: &str, account: &str, now: u64) -> Result<bool, String> {
    Ok(cx
        .conn()
        .query_opt(
            "SELECT 1 FROM view.block
             WHERE tenant = $1 AND actor_key = $2 AND layer = 'tenant'
               AND (expires IS NULL OR expires > $3)",
            &[&tenant, &account, &i64::try_from(now).unwrap_or(i64::MAX)],
        )
        .await
        .map_err(|e| e.to_string())?
        .is_some())
}

/// Writes a block (`Block::Block`) or an unblock, and returns the record.
///
/// # Errors
///
/// A [`BlockError`].
#[allow(clippy::too_many_arguments)]
pub async fn write(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor: &str,
    who: &Effective,
    account: &str,
    block: &Block,
    reblock: bool,
    reason: &str,
    now: u64,
) -> Result<Record, BlockError> {
    if !who.holds("block") {
        return Err(BlockError::Denied);
    }
    let is_blocked = blocked(cx, tenant, account, now)
        .await
        .map_err(BlockError::Store)?;
    match block {
        Block::Block { .. } if is_blocked && !reblock => return Err(BlockError::AlreadyBlocked),
        Block::Unblock if !is_blocked => return Err(BlockError::NotBlocked),
        _ => {}
    }
    let actors = store
        .partition(cx, tenant, "actors")
        .await
        .map_err(store_err)?
        .ok_or_else(|| store_err(format!("no `actors` partition for `{tenant}`")))?;
    let logid = store
        .next_id(cx, tenant, &Sequence::Log)
        .await
        .map_err(store_err)?;
    let content = Value::from_json(&serde_json::to_value(block).map_err(store_err)?);
    let draft = Draft {
        appended_at: now,
        payload_type: payload::BLOCK.into(),
        key: Some(account.to_string()),
        revid: None,
        logid: Some(logid),
        page_id: None,
        body: Body::core(
            &content,
            &Value::text(reason),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(store_err)?,
    };
    let (record, _) = append_projected(store, pipeline, cx, actors, draft, Budget::NONE)
        .await
        .map_err(store_err)?;
    Ok(record)
}
