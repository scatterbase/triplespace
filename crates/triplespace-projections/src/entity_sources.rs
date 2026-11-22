//! A tenant's entity sources as declared (0078 §1): the `entity-source` rows of
//! `view.registry`, and the tenant's table of names built from them (0078 §3): the
//! provider registry with the tenant's sources added, which is what every lookup of a
//! colon-form ID on the tenant goes through.

use scatter_log_postgres::PgClient;
use scatter_providers::{EntitySource, Registry};

/// The config record kind.
pub const KIND: &str = "entity-source";

/// A tenant's declared sources, in name order. A record that does not parse is skipped:
/// the writer validates, so one that does not is from another version.
///
/// # Errors
///
/// If the store fails.
pub async fn declared<C: PgClient>(client: &C, tenant: &str) -> Result<Vec<EntitySource>, String> {
    let rows = client
        .query(
            "SELECT config FROM view.registry WHERE tenant = $1 AND kind = $2 ORDER BY code",
            &[&tenant, &KIND],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .filter_map(|r| serde_json::from_value(r.get::<_, serde_json::Value>(0)).ok())
        .collect())
}

/// A tenant's sources as `GET /entity-sources` and `siprop=triplespace`'s `entity_sources`
/// report them (0078 §10), in name order: each source's name, label, number, types with
/// their IRI templates, endpoints, licence, `promoted_to` and `retired`. Nothing in a
/// record is private: it is the tenant's public configuration, and a reader on another
/// instance reads it to rewrite references by IRI (05 §6.7).
#[must_use]
pub fn public(sources: &[EntitySource]) -> serde_json::Value {
    serde_json::Value::Array(
        sources
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "label": s.label,
                    "number": s.number,
                    "types": s.types.iter().map(|t| serde_json::json!({
                        "upstream_prefix": t.upstream_prefix,
                        "entity_type": t.entity_type,
                        "id_grammar": t.id_grammar,
                        "iri": t.iri,
                    })).collect::<Vec<_>>(),
                    "api": s.api,
                    "entity_data": s.entity_data,
                    "revision_ids": s.revision_ids,
                    "licence": s.licence,
                    "promoted_to": s.promoted_to.as_deref().filter(|c| !c.is_empty()),
                    "retired": s.retired,
                })
            })
            .collect(),
    )
}

/// One declared source by name.
///
/// # Errors
///
/// If the store fails.
pub async fn declared_one<C: PgClient>(
    client: &C,
    tenant: &str,
    name: &str,
) -> Result<Option<EntitySource>, String> {
    Ok(crate::registry::entry(client, tenant, KIND, name)
        .await
        .map_err(|e| e.to_string())?
        .and_then(|v| serde_json::from_value(v).ok()))
}

/// The tenant's table of names (0078 §3): the registry with the tenant's numbered sources
/// as providers. A source without a number yet (none, once written) is left out.
///
/// # Errors
///
/// If the store fails, or a source clashes with the registry (which the writer prevents).
pub async fn registry_for<C: PgClient>(client: &C, tenant: &str) -> Result<Registry, String> {
    let sources = declared(client, tenant)
        .await?
        .iter()
        .filter_map(EntitySource::to_provider)
        .collect();
    Registry::default_registry()
        .with_sources(sources)
        .map_err(|e| e.to_string())
}
