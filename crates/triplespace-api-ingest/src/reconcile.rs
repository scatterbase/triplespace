//! Writing a `reconcile:default` record (0004 §9, A29; 04 §4.10; 23 §2.3): the instance's
//! policy record, under which every shared row is computed, or a tenant's, which narrows
//! it and configures the tenant's own entity sources.
//!
//! The record is validated before it is appended: a tenant's may hold only what a tenant
//! may set ([`TenantPolicy::refusals`], refused with `ts-instance-policy`). The projections
//! read it from `view.registry`: the tier-2 link properties and the inference properties
//! apply to each entity's next state (or to all at a rebuild), a tenant's `infer` at once.

use scatter_actors::evaluate::Effective;
use scatter_ingest::store::IngestStore as _;
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::PAYLOAD_CONFIG;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use triplespace_projections::PgCx;
use triplespace_projections::entity_sources::declared;
use triplespace_projections::identity::{INSTANCE_POLICY, TenantPolicy};

use crate::PgIngest;

/// The record's key.
pub const KEY: &str = "reconcile:default";

/// Why a `reconcile` record was not written.
#[derive(Debug, thiserror::Error)]
pub enum ReconcileWriteError {
    /// The writer lacks a right.
    #[error("this needs the \"{0}\" right")]
    Denied(&'static str),
    /// The record is not an object.
    #[error("a `reconcile` record is a JSON object")]
    NotAnObject,
    /// A tenant's record holds what only the instance may set.
    #[error("{INSTANCE_POLICY}: {}", .0.join("; "))]
    InstancePolicy(Vec<String>),
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

impl ReconcileWriteError {
    /// The error's code, as an API reports it.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Denied(_) => "permissiondenied",
            Self::NotAnObject => "ts-invalid-setting",
            Self::InstancePolicy(_) => INSTANCE_POLICY,
            Self::Store(_) => "internal_api_error_Exception",
        }
    }
}

fn store_err(e: impl std::fmt::Display) -> ReconcileWriteError {
    ReconcileWriteError::Store(e.to_string())
}

/// Writes (`Some`) or retires (`None`) the `reconcile:default` record of `tenant`, or of
/// the instance when `tenant` is empty, and returns the record as written. It needs
/// `ts-config`.
///
/// # Errors
///
/// A [`ReconcileWriteError`].
#[allow(clippy::too_many_arguments)]
pub async fn write(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor: &str,
    who: &Effective,
    record: Option<serde_json::Value>,
    reason: &str,
    now: u64,
) -> Result<Option<serde_json::Value>, ReconcileWriteError> {
    if !who.holds("ts-config") {
        return Err(ReconcileWriteError::Denied("ts-config"));
    }
    let record = match record {
        None => None,
        Some(serde_json::Value::Object(mut map)) => {
            map.insert("kind".into(), "reconcile".into());
            map.insert("name".into(), "default".into());
            Some(serde_json::Value::Object(map))
        }
        Some(_) => return Err(ReconcileWriteError::NotAnObject),
    };
    if let (false, Some(record)) = (tenant.is_empty(), &record) {
        let sources = declared(cx.conn(), tenant).await.map_err(store_err)?;
        let names: Vec<&str> = sources.iter().map(|s| s.name.as_str()).collect();
        let refusals = TenantPolicy::refusals(record, &names);
        if !refusals.is_empty() {
            return Err(ReconcileWriteError::InstancePolicy(refusals));
        }
    }
    let partition = if tenant.is_empty() {
        CONFIG_PARTITION
    } else {
        store
            .partition(cx, tenant, "config")
            .await
            .map_err(store_err)?
            .ok_or_else(|| store_err(format!("no `config` partition for `{tenant}`")))?
    };
    let content = record.as_ref().map_or(Value::Null, Value::from_json);
    let draft = Draft {
        appended_at: now,
        payload_type: PAYLOAD_CONFIG.into(),
        key: Some(KEY.to_string()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &content,
            &Value::text(reason),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(store_err)?,
    };
    append_projected(store, pipeline, cx, partition, draft, Budget::NONE)
        .await
        .map_err(store_err)?;
    Ok(record)
}
