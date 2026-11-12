//! The backup key set's record (0103 A3, A5): a `config` record of kind `backup-keys`,
//! code `set`, in the instance `config` partition, holding the keys' public halves and the
//! threshold. The instance's `owner` registers or replaces it (`PUT /backup-keys`, the
//! instance right `ts-backup-keys`), or, on an instance with no owner account, the operator
//! through `triplespace backup-key register`. A new set applies to the backups made after
//! it; each backup names the record's offset, so an older backup keeps the set it was made
//! under.

use scatter_ingest::IngestError;
use scatter_integrity::backup_key::KeySet;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{PAYLOAD_CONFIG, config_key};
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use tokio_postgres::GenericClient;

use crate::PgIngest;
use triplespace_projections::PgCx;

/// The record's kind.
pub const KIND: &str = "backup-keys";

/// The record's code: one set at a time.
pub const CODE: &str = "set";

/// The set in force, with the offset of the record that registered it, which a backup
/// names, and who registered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    /// The set.
    pub set: KeySet,
    /// The offset of its record in the instance `config` partition.
    pub offset: u64,
    /// The actor whose record it is.
    pub actor: String,
}

/// The set in force, or `None` when none has been registered.
///
/// # Errors
///
/// The store's, or a record whose content is not a set.
pub async fn current<C: GenericClient>(client: &C) -> Result<Option<Registered>, String> {
    let row = client
        .query_opt(
            "SELECT config, \"offset\", actor FROM view.registry
             WHERE tenant = '' AND kind = $1 AND code = $2",
            &[&KIND, &CODE],
        )
        .await
        .map_err(|e| e.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let config: serde_json::Value = row.get(0);
    let offset: i64 = row.get(1);
    Ok(Some(Registered {
        set: KeySet::from_json(&config).map_err(|e| e.to_string())?,
        offset: u64::try_from(offset).map_err(|e| e.to_string())?,
        actor: row.get::<_, Option<String>>(2).unwrap_or_default(),
    }))
}

/// Whether the instance has an owner account: a member of the primary tenant's `owner`
/// group (0046 §1). Without one, the operator registers the set through the command line.
///
/// # Errors
///
/// The store's.
pub async fn has_owner<C: GenericClient>(client: &C) -> Result<bool, String> {
    let Some(primary) = triplespace_projections::tenants::primary(client)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let row = client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM view.membership
                            WHERE tenant = $1 AND \"group\" = 'owner'
                              AND (expires IS NULL OR expires > (extract(epoch FROM now()) * 1e6)::bigint))",
            &[&primary],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.get(0))
}

/// Appends the record that registers `set`, as `actor`, in the caller's unit of work, and
/// returns its offset.
///
/// # Errors
///
/// The store's or a projection's.
pub async fn register(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    set: &KeySet,
    actor: &str,
    comment: Option<&str>,
    now: u64,
) -> Result<u64, IngestError> {
    let draft = Draft {
        appended_at: now,
        payload_type: PAYLOAD_CONFIG.into(),
        key: Some(config_key(KIND, CODE)),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::from_json(&set.to_json()),
            &comment.map_or(Value::Null, Value::text),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(|e| IngestError::Store(format!("body: {e}")))?,
    };
    let (record, _) = scatter_ingest::write::append_projected(
        store,
        pipeline,
        cx,
        CONFIG_PARTITION,
        draft,
        Budget::NONE,
    )
    .await?;
    Ok(record.header().offset)
}
