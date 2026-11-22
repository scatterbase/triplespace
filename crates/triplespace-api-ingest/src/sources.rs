//! The one writer of `entity-source` records (0078 §1, §6.3, §6.4; 0078 §1–§4): a tenant
//! declares a provider of its own.
//!
//! A source is a `config` record keyed `entity-source:{name}` in the tenant's `config`
//! partition, written with `ts-config`. The first record gets the lowest free number in
//! 2^12..2^13 on the tenant, which is never reused, and opens the tenant partition
//! `source/{name}`, recorded as a `graph:` record beside it (0015 §5). A later record keeps
//! the number and the partition, and may not change what the first fixed (05 §6.4,
//! `ts-source-binding`). Retiring writes the record again flagged `retired`: the name, the
//! number and the templates stay reserved, the partition and its records stay, and the IDs
//! keep resolving. A record written `null` by an older version is read as no record.
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

/// What the registry means for one source, as lines `registry check` prints (0078 §3 rule
/// 2, §9): the names it shadows, and
/// the provider it can be promoted to.
#[must_use]
pub fn findings(source: &scatter_providers::EntitySource, registry: &Registry) -> Vec<String> {
    let name = &source.name;
    let mut out = Vec::new();
    if let Some(p) = registry.by_slug(name).filter(|p| !p.source) {
        out.push(format!(
            "`{name}` shadows provider `{}`, which is reached as {} on this tenant",
            p.slug, p.code
        ));
    }
    if scatter_keyed::KeyedRegistry::default_registry()
        .by_name(name)
        .is_some()
    {
        out.push(format!("`{name}` shadows the keyed type `{name}`"));
    }
    if scatter_normalize::ResolverRegistry::default_registry()
        .by_name(name)
        .is_some()
    {
        out.push(format!("`{name}` shadows the resolver `{name}`"));
    }
    if scatter_keyed::SchemeRegistry::default_registry()
        .by_name(name)
        .is_some()
    {
        out.push(format!("`{name}` shadows the notation scheme `{name}`"));
    }
    if !source.retired
        && let Some(p) = source.promotable_to(registry)
    {
        out.push(format!(
            "`{name}` can be promoted to {} (`{}`), which has its IRI templates",
            p.code, p.slug
        ));
    }
    out
}

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
    /// There is no current record to retire.
    #[error("source `{0}` is not declared, or is retired already")]
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
        // Retiring keeps the record, flagged, so that the name, the number and the
        // templates stay reserved and the IDs keep resolving (05 §6.4).
        let Some(mut retired) = existing.filter(|e| !e.retired) else {
            return Err(SourceWriteError::NothingToRetire(name.to_string()));
        };
        retired.retired = true;
        let content = Value::from_json(&serde_json::to_value(&retired).map_err(store_err)?);
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
        return Ok(None);
    };
    source.name = name.to_string();
    // Declared again, a retired source is current again, under its own number.
    source.retired = false;
    // The number is the server's: the existing one, else the lowest free on the tenant.
    source.number = Some(match existing.as_ref().and_then(|e| e.number) {
        Some(n) => n,
        None => next_number(cx, tenant).await?,
    });
    // The registry's names are checked when a source is first declared: a source keeps a
    // name a registry update adds later (05 §6.3 rule 2).
    if let Some(earlier) = &existing {
        source.validate_shape()?;
        source.check_binding(earlier)?;
    } else {
        source.validate(Registry::default_registry())?;
    }
    let registry_name = scatter_keyed::KeyedRegistry::default_registry()
        .by_name(name)
        .is_some()
        || scatter_keyed::SchemeRegistry::default_registry()
            .by_name(name)
            .is_some();
    if existing.is_none() && registry_name {
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
                // Fixed here, under the tenant's host at creation (01 §4.2; 0006 A29).
                origin: Some(scatter_log_postgres::partition::origin_under(
                    base,
                    Some(tenant),
                    &graph_name,
                )),
            },
        )
        .await
        .map_err(store_err)?;
        // A source partition is never the accounts partition (0013 §4).
        triplespace_db::grants::grant_partition(cx.conn(), partition, false)
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

#[cfg(test)]
mod registry_check_tests {
    use super::*;
    use scatter_providers::EntitySource;

    fn source(name: &str, iri: &str) -> EntitySource {
        serde_json::from_value(serde_json::json!({
            "name": name, "label": name,
            "types": [{"upstream_prefix": "Q", "entity_type": "item", "iri": iri}],
        }))
        .unwrap()
    }

    // 0078 §3 rule 2, §9: a name the registry has is shadowed, and a source a provider
    // covers is promotable; a source of its own graph is neither.
    #[test]
    fn a_check_lists_shadowed_names_and_promotable_sources() {
        let registry = Registry::default_registry();
        let lb = findings(
            &source("wikidata", "https://librarybase.org/entity/{upstream_id}"),
            registry,
        );
        assert_eq!(lb.len(), 2, "{lb:?}");
        assert!(lb[0].contains("shadows provider `wikidata`") && lb[0].contains("WD"));
        assert!(lb[1].contains("promoted to LB"));
        assert!(
            findings(&source("doi", "https://x.example/{upstream_id}"), registry)
                .iter()
                .any(|l| l.contains("resolver") || l.contains("keyed type"))
        );
        assert!(
            findings(
                &source("mhc", "https://mhc.example/{upstream_id}"),
                registry
            )
            .is_empty()
        );
        let mut retired = source("lb", "https://librarybase.org/entity/{upstream_id}");
        retired.retired = true;
        assert!(findings(&retired, registry).is_empty());
    }
}
