//! Saving settings (0085 §3): the one writer of `site`, `template:site:` and `tenancy`
//! records that `PATCH /settings` and `triplespace-cli settings` share.
//!
//! A save reads the configuration and plans against it inside the unit of work that
//! appends, under a transaction-scoped advisory lock, so two saves cannot both pass the
//! conflict check against the same state. Every record lands in one transaction with its
//! projections, each carrying the summary in its comment part, followed by one
//! `settings/change` log event naming the keys with their old and new values (16 §2.3).

use scatter_ingest::IngestError;
use scatter_ingest::store::{IngestStore as _, Sequence};
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::registry::PAYLOAD_CONFIG;
use scatter_log::store::Draft;
use scatter_projection::{Backend as _, Budget, Pipeline};
use triplespace_accounts::settings::plan::{Change, Rights, Target, Write};
use triplespace_accounts::settings::{Patch, Refusal, load, plan};
use triplespace_projections::common::PAYLOAD_LOGEVENT;

use crate::PgIngest;

/// The advisory lock every save holds for its transaction.
const SETTINGS_LOCK: i64 = 0x0085_5e77;

/// What a save did.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    /// The changes, as the log event reports them.
    pub changes: Vec<Change>,
    /// The `{partition, offset}` of every record appended, the log event last.
    pub records: Vec<(u64, u64)>,
    /// The written `config`'s next offset after the save: the base of the next one.
    pub base: u64,
}

/// Why a save failed.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// The validator refused it; nothing was written.
    #[error("refused: {}", .0.iter().map(|r| r.message.as_str()).collect::<Vec<_>>().join("; "))]
    Refused(Vec<Refusal>),
    /// The store failed; nothing was written.
    #[error("store: {0}")]
    Store(String),
}

fn store_error(e: impl std::fmt::Display) -> SaveError {
    SaveError::Store(e.to_string())
}

fn draft(
    payload_type: &str,
    key: &str,
    content: Option<&serde_json::Value>,
    comment: Option<&str>,
    actor: &str,
    now: u64,
) -> Result<Draft, SaveError> {
    Ok(Draft {
        appended_at: now,
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &content.map_or(Value::Null, Value::from_json),
            &comment.map_or(Value::Null, Value::text),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(store_error)?,
    })
}

/// Saves a patch: at a tenant's base (`tenant = Some(slug)`, `Target::Tenant`), or at
/// the farm base (`Target::Instance`, with the primary tenant's slug or `None`).
/// `rights` answers for the scope written.
#[allow(clippy::too_many_arguments)]
pub async fn save<R: Rights + Sync + ?Sized>(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    tenant: Option<&str>,
    target: Target,
    patch: &Patch,
    actor: &str,
    rights: &R,
    now: u64,
) -> Result<Saved, SaveError> {
    let mut cx = store.begin().await.map_err(store_error)?;
    let result = save_in(
        store, pipeline, &mut cx, tenant, target, patch, actor, rights, now,
    )
    .await;
    match result {
        Ok(saved) => {
            store.commit(cx).await.map_err(store_error)?;
            Ok(saved)
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn save_in<R: Rights + Sync + ?Sized>(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut triplespace_projections::PgCx,
    tenant: Option<&str>,
    target: Target,
    patch: &Patch,
    actor: &str,
    rights: &R,
    now: u64,
) -> Result<Saved, SaveError> {
    cx.conn()
        .execute("SELECT pg_advisory_xact_lock($1)", &[&SETTINGS_LOCK])
        .await
        .map_err(store_error)?;
    let snapshot_tenant = match target {
        Target::Tenant => tenant,
        Target::Instance => None,
    };
    let (snap, parts) = load(cx.conn(), snapshot_tenant)
        .await
        .map_err(store_error)?;
    let planned = plan(&snap, target, patch, rights).map_err(SaveError::Refused)?;
    let config = match target {
        Target::Tenant => parts
            .tenant_config
            .ok_or_else(|| SaveError::Store("the tenant has no config partition".into()))?,
        Target::Instance => parts.instance_config,
    };
    let comment = planned.summary.as_deref();
    let mut records = Vec::new();
    let mut base = match target {
        Target::Tenant => parts.tenant_next.unwrap_or(0),
        Target::Instance => parts.instance_next,
    };
    for Write { key, content, .. } in &planned.writes {
        let d = draft(PAYLOAD_CONFIG, key, content.as_ref(), comment, actor, now)?;
        let (record, _) = append_projected(store, pipeline, cx, config, d, Budget::NONE)
            .await
            .map_err(|e: IngestError| store_error(e))?;
        records.push((config, record.header().offset));
        base = record.header().offset + 1;
    }
    if let Some(event) = planned.event(target) {
        let (log, seq_tenant) = match target {
            Target::Tenant => (parts.tenant_log, tenant.unwrap_or("")),
            Target::Instance => (parts.instance_log, ""),
        };
        if let Some(log) = log {
            let mut d = draft(
                PAYLOAD_LOGEVENT,
                "settings",
                Some(&event),
                comment,
                actor,
                now,
            )?;
            d.logid = Some(
                store
                    .next_id(cx, seq_tenant, &Sequence::Log)
                    .await
                    .map_err(store_error)?,
            );
            let (record, _) = append_projected(store, pipeline, cx, log, d, Budget::NONE)
                .await
                .map_err(store_error)?;
            records.push((log, record.header().offset));
        }
    }
    Ok(Saved {
        changes: planned.changes,
        records,
        base,
    })
}
