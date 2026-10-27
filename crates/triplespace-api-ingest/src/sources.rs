//! The one writer of `entity-source` records (0078 §1, §6.3, §6.4; 0078 §1–§4): a tenant
//! declares a provider of its own.
//!
//! A source is a `config` record keyed `entity-source:{name}` in the tenant's `config`
//! partition, written with `ts-config`. The first record gets the lowest free number in
//! 2^12..2^13 on the tenant, which is never reused, and opens the tenant partition
//! `source/{name}`, recorded as a `graph:` record beside it (0015 §5). A later record keeps
//! the number and the partition; `null` retires the record (the partition and its records
//! stay, as a retired provider's do).
//!
//! The name may not be one the registry has (0078 §3 rule 1): a provider slug, a keyed
//! type, a notation scheme. `ts-source-name-taken` is the refusal.

use scatter_actors::evaluate::Effective;
use scatter_ingest::store::IngestStore as _;
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::registry::{
    GraphEntry, GraphRegistry, HASH_SHA256, History, Integrity, PAYLOAD_CONFIG,
};
use scatter_log::store::Draft;
use scatter_log::tree::Segments;
use scatter_log_postgres::log;
use scatter_log_postgres::partition::PartitionInfo;
use scatter_projection::{Budget, Pipeline};
use scatter_providers::{EntitySource, Registry, SourceError};
use triplespace_projections::PgCx;
use triplespace_projections::entity_sources::{KIND, declared, declared_one};

use crate::PgIngest;

/// The segment exponent of a source partition, as the tenant's others (0001 §4).
const SEGMENT_K: u8 = 16;

/// Why a source write was refused; the caller rolls back.
#[derive(Debug, thiserror::Error)]
pub enum SourceWriteError {
    /// The writer lacks a right.
    #[error("this needs the \"{0}\" right")]
    Denied(&'static str),
    /// The declaration breaks a rule.
    #[error("{0}")]
    Invalid(#[from] SourceError),
    /// The name is a keyed type's or a notation scheme's (0078 §3 rule 1).
    #[error("`{0}` is a name the registry has (ts-source-name-taken)")]
    NameTaken(String),
    /// The tenant has every number in the range.
    #[error("no source number is free on `{0}`")]
    NoNumber(String),
    /// There is no record to retire.
    #[error("source `{0}` is not declared")]
    NothingToRetire(String),
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

fn store_err(e: impl std::fmt::Display) -> SourceWriteError {
    SourceWriteError::Store(e.to_string())
}

/// Writes (`Some`) or retires (`None`) a tenant's source, and returns the record as
/// written, number assigned. `base` is the tenant's base URI, for the partition's graph
/// IRI (0015 §5).
///
/// # Errors
///
/// A [`SourceWriteError`].
#[allow(clippy::too_many_arguments)]
pub async fn write(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    base: &str,
    actor: &str,
    who: &Effective,
    name: &str,
    source: Option<EntitySource>,
    reason: &str,
    now: u64,
) -> Result<Option<EntitySource>, SourceWriteError> {
    if !who.holds("ts-config") {
        return Err(SourceWriteError::Denied("ts-config"));
    }
    let existing = declared_one(cx.conn(), tenant, name)
        .await
        .map_err(store_err)?;
    let Some(mut source) = source else {
        if existing.is_none() {
            return Err(SourceWriteError::NothingToRetire(name.to_string()));
        }
        append_record(
            store,
            pipeline,
            cx,
            tenant,
            &format!("{KIND}:{name}"),
            Value::Null,
            actor,
            reason,
            now,
        )
        .await?;
        return Ok(None);
    };
    source.name = name.to_string();
    // The number is the server's: the existing one, else the lowest free on the tenant.
    source.number = Some(match existing.as_ref().and_then(|e| e.number) {
        Some(n) => n,
        None => next_number(cx, tenant).await?,
    });
    source.validate(Registry::default_registry())?;
    if scatter_normalize::KeyedRegistry::default_registry()
        .by_name(name)
        .is_some()
        || scatter_normalize::SchemeRegistry::default_registry()
            .by_name(name)
            .is_some()
    {
        return Err(SourceWriteError::NameTaken(name.to_string()));
    }

    open_partition(store, pipeline, cx, tenant, base, name, actor, reason, now).await?;

    let content = Value::from_json(&serde_json::to_value(&source).map_err(store_err)?);
    append_record(
        store,
        pipeline,
        cx,
        tenant,
        &format!("{KIND}:{name}"),
        content,
        actor,
        reason,
        now,
    )
    .await?;
    Ok(Some(source))
}

/// The lowest free source number on the tenant (0078 §4).
async fn next_number(cx: &mut PgCx, tenant: &str) -> Result<u32, SourceWriteError> {
    let taken = declared(cx.conn(), tenant)
        .await
        .map_err(store_err)?
        .into_iter()
        .filter_map(|s| s.number);
    EntitySource::next_number(taken).ok_or_else(|| SourceWriteError::NoNumber(tenant.to_string()))
}

/// Opens `source/{name}` on the first record (0078 §4), with its `graph:` record.
#[allow(clippy::too_many_arguments)]
async fn open_partition(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    base: &str,
    name: &str,
    actor: &str,
    reason: &str,
    now: u64,
) -> Result<(), SourceWriteError> {
    let graph_name = format!("source/{name}");
    if store
        .partition(cx, tenant, &graph_name)
        .await
        .map_err(store_err)?
        .is_none()
    {
        let registry = GraphRegistry::embedded();
        let (graph, _) = registry
            .lookup(&graph_name)
            .ok_or_else(|| store_err("`source/{name}` is not a registered graph"))?;
        let partition = random_partition()?;
        let base = base.trim_end_matches('/');
        log::create_partition(
            cx.conn(),
            partition,
            Segments::new(SEGMENT_K).expect("valid exponent"),
            &PartitionInfo {
                tenant: Some(tenant.to_string()),
                name: graph_name.clone(),
                graph_iri: format!("{base}/graph/{graph_name}"),
                history: graph.history.unwrap_or(History::Latest),
                integrity: graph.integrity.unwrap_or(Integrity::Hashed),
                export: graph.export,
                hash: HASH_SHA256.into(),
            },
        )
        .await
        .map_err(store_err)?;
        let entry = GraphEntry::source(graph, graph_name.clone(), partition, SEGMENT_K);
        let content = Value::from_json(&serde_json::to_value(&entry).map_err(store_err)?);
        append_record(
            store,
            pipeline,
            cx,
            tenant,
            &entry.key(),
            content,
            actor,
            reason,
            now,
        )
        .await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn append_record(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    key: &str,
    content: Value,
    actor: &str,
    reason: &str,
    now: u64,
) -> Result<(), SourceWriteError> {
    let config = store
        .partition(cx, tenant, "config")
        .await
        .map_err(store_err)?
        .ok_or_else(|| store_err(format!("no `config` partition for `{tenant}`")))?;
    let draft = Draft {
        appended_at: now,
        payload_type: PAYLOAD_CONFIG.into(),
        key: Some(key.to_string()),
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
    append_projected(store, pipeline, cx, config, draft, Budget::NONE)
        .await
        .map_err(store_err)?;
    Ok(())
}

/// A random 64-bit partition ID (0018 §2), never 0.
fn random_partition() -> Result<u64, SourceWriteError> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| store_err(format!("random partition ID: {e}")))?;
    Ok(u64::from_le_bytes(bytes).max(1))
}
