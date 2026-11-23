//! Linked entities queued for their mirror (0070 §2.2, A10; 05 §5.3): a tenant's write that
//! names a foreign entity, or an entity of one of its sources, that no mirror holds yet adds
//! it to `ops.entity_fetch`, for the follower of its provider or source to fetch
//! (0070 A9). The write does not wait for it.
//!
//! An entity is named by being a statement's, a qualifier's or a reference's property or
//! value, or a value's unit, globe or calendar model (0004 A20), the subject of a local
//! statement, override or removal, or an ID of a `same-as` or `retain`, in a change set on any graph of a tenant other than a mirror (`local`,
//! `pages`, a derived graph). It is queued when its provider or source has an `api`, the
//! tenant's `entities.mirror` for its provider and type is `linked` or `all` (`linked`
//! when unset), and `view.entity_source` holds no row for it. A replay queues again only
//! what is still not mirrored, which the fetch then writes or finds unchanged, so the
//! projection keeps no rows of its own.

use std::collections::{BTreeMap, BTreeSet};

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log_postgres::PgClient;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_providers::Registry;
use scatter_wikibase_changeset::Operation;
use scatter_wikibase_ids::IdForm;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Reference, SnakGroups, SnakKind, Statement};
use scatter_wikibase_model::value::DataValue;

use crate::backend::PgCx;
use crate::common::{PAYLOAD_CHANGESET, operation, partition_info};
use scatter_wikibase_ids::TriplespaceId as _;

/// The fetch-queue projection.
#[derive(Debug, Clone, Default)]
pub struct FetchQueueProjection;

/// The entity IDs an operation names that a mirror could hold: foreign and colon-form IDs.
#[must_use]
pub fn named(op: &Operation) -> BTreeSet<EntityId> {
    let mut out = BTreeSet::new();
    let mut id = |id: &EntityId| {
        if id.form() != IdForm::Local {
            out.insert(id.clone());
        }
    };
    let mut ids: Vec<&EntityId> = Vec::new();
    match op {
        Operation::Create { entity, .. } | Operation::CreateOrAdd { entity, .. } => {
            entity_ids(entity, &mut ids);
        }
        Operation::Adopt {
            entity: Some(e), ..
        } => entity_ids(e, &mut ids),
        Operation::Add {
            id: subject,
            claims,
            references,
            qualifiers,
            entity,
            ..
        } => {
            ids.extend(subject.iter());
            for (property, statements) in claims {
                ids.push(property);
                for s in statements {
                    statement_ids(s, &mut ids);
                }
            }
            for refs in references.values() {
                for r in refs {
                    reference_ids(r, &mut ids);
                }
            }
            for groups in qualifiers.values() {
                group_ids(groups, &mut ids);
            }
            if let Some(e) = entity {
                entity_ids(e, &mut ids);
            }
        }
        Operation::Remove { id: subject, .. } | Operation::Override { id: subject, .. } => {
            ids.extend(subject.iter());
        }
        Operation::Retain {
            id: subject,
            ids: others,
            ..
        }
        | Operation::SameAs {
            id: subject,
            ids: others,
            ..
        } => {
            ids.extend(subject.iter());
            ids.extend(others.iter());
        }
        _ => {}
    }
    for i in ids {
        id(i);
    }
    for i in value_ids(op) {
        id(&i);
    }
    out
}

fn entity_ids<'a>(entity: &'a Entity, out: &mut Vec<&'a EntityId>) {
    for (property, statements) in &entity.statements {
        out.push(property);
        for s in statements {
            statement_ids(s, out);
        }
    }
}

fn statement_ids<'a>(s: &'a Statement, out: &mut Vec<&'a EntityId>) {
    snak_ids(&s.mainsnak.property, &s.mainsnak.kind, out);
    group_ids(&s.qualifiers, out);
    for r in &s.references {
        reference_ids(r, out);
    }
}

fn reference_ids<'a>(r: &'a Reference, out: &mut Vec<&'a EntityId>) {
    group_ids(&r.snaks, out);
}

fn group_ids<'a>(groups: &'a SnakGroups, out: &mut Vec<&'a EntityId>) {
    for (property, snaks) in groups {
        out.push(property);
        for snak in snaks {
            snak_ids(&snak.property, &snak.kind, out);
        }
    }
}

fn snak_ids<'a>(property: &'a EntityId, kind: &'a SnakKind, out: &mut Vec<&'a EntityId>) {
    out.push(property);
    if let SnakKind::Value(DataValue::EntityId(v)) = kind {
        out.push(&v.id);
    }
}

/// The entity a value's unit, globe or calendar model names, in its stored form (0004
/// A20); `None` for `"1"`, an IRI kept as written, or another kind of value.
fn value_id(kind: &SnakKind) -> Option<EntityId> {
    let s = match kind {
        SnakKind::Value(DataValue::Quantity(q)) => &q.unit,
        SnakKind::Value(DataValue::GlobeCoordinate(g)) => &g.globe,
        SnakKind::Value(DataValue::Time(t)) => &t.calendarmodel,
        _ => return None,
    };
    EntityId::parse(s).ok()
}

/// The units, globes and calendar models `op` names.
fn value_ids(op: &Operation) -> Vec<EntityId> {
    let mut out = Vec::new();
    let mut statement = |s: &Statement| {
        out.extend(s.snaks().filter_map(|n| value_id(&n.kind)));
    };
    match op {
        Operation::Create { entity, .. }
        | Operation::CreateOrAdd { entity, .. }
        | Operation::Adopt {
            entity: Some(entity),
            ..
        } => entity.all_statements().for_each(&mut statement),
        Operation::Add {
            claims,
            references,
            qualifiers,
            entity,
            ..
        } => {
            claims.values().flatten().for_each(&mut statement);
            if let Some(e) = entity {
                e.all_statements().for_each(&mut statement);
            }
            for r in references.values().flatten() {
                for snaks in r.snaks.values() {
                    out.extend(snaks.iter().filter_map(|n| value_id(&n.kind)));
                }
            }
            for groups in qualifiers.values() {
                for snaks in groups.values() {
                    out.extend(snaks.iter().filter_map(|n| value_id(&n.kind)));
                }
            }
        }
        _ => {}
    }
    out
}

/// The tenant's `entities.mirror` (0070 §2.1; 05 §5.2), or the instance's default for it
/// (`template:site:entities.mirror`) where the tenant sets none: `{code or source name:
/// {entity type: value}}`.
async fn mirror_setting(cx: &PgCx, tenant: &str) -> Result<serde_json::Value, String> {
    let rows = cx
        .query(
            "SELECT tenant, coalesce(config->'record'->'value', config->'value')
             FROM view.registry
             WHERE (tenant = $1 AND kind = 'site' AND code = 'entities.mirror')
                OR (tenant = '' AND kind = 'template' AND code = 'site:entities.mirror')",
            &[&tenant],
        )
        .await
        .map_err(|e| crate::common::describe(&e))?;
    let mut tenant_value = None;
    let mut instance_value = None;
    for r in &rows {
        let t: String = r.get(0);
        let v: Option<serde_json::Value> = r.get(1);
        if t.is_empty() {
            instance_value = v;
        } else {
            tenant_value = v;
        }
    }
    Ok(tenant_value
        .or(instance_value)
        .unwrap_or(serde_json::Value::Null))
}

/// Whether `setting` mirrors linked entities of `provider`'s `entity_type`: `linked` or
/// `all`, and `linked` when unset.
fn mirrors_linked(setting: &serde_json::Value, provider: &str, entity_type: &str) -> bool {
    match setting
        .get(provider)
        .and_then(|p| p.get(entity_type))
        .and_then(serde_json::Value::as_str)
    {
        None => true,
        Some(v) => matches!(v, "linked" | "all"),
    }
}

/// What to queue for `ids`, named on `tenant`: `(queue tenant, provider or source,
/// upstream ID, stored ID)`, for those whose provider or source can be fetched from and
/// whose setting mirrors them.
fn wanted(
    registry: &Registry,
    setting: &serde_json::Value,
    tenant: &str,
    ids: &BTreeSet<EntityId>,
) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for id in ids {
        let Ok(f) = registry.parse_foreign_id(id.as_str()) else {
            // A keyed ID, or a name no source of the tenant has.
            continue;
        };
        let p = f.provider();
        if p.api.is_none() || p.retired {
            continue;
        }
        let key = if p.source { &p.slug } else { &p.code };
        if !mirrors_linked(setting, key, &f.entity_type().entity_type) {
            continue;
        }
        let queue_tenant = if p.source {
            tenant.to_string()
        } else {
            String::new()
        };
        out.push((
            queue_tenant,
            p.slug.clone(),
            f.upstream_id(),
            id.as_str().to_string(),
        ));
    }
    out
}

impl FetchQueueProjection {
    async fn apply_records(cx: &PgCx, records: &[Record]) -> Result<Applied, String> {
        let info = partition_info(cx, records[0].header().partition).await?;
        if info.tenant.is_empty() || info.is_mirror() {
            return Ok(Applied::default());
        }
        let mut ids = BTreeSet::new();
        for record in records {
            if let Some(op) = operation(record)? {
                ids.extend(named(&op));
            }
        }
        if ids.is_empty() {
            return Ok(Applied::default());
        }
        let registry = crate::entity_sources::registry_for(cx, &info.tenant).await?;
        let setting = mirror_setting(cx, &info.tenant).await?;
        let wanted = wanted(&registry, &setting, &info.tenant, &ids);
        if wanted.is_empty() {
            return Ok(Applied::default());
        }
        let mut columns: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (t, p, u, s) in wanted {
            for (i, v) in [t, p, u, s].into_iter().enumerate() {
                columns.entry(i).or_default().push(v);
            }
        }
        let n = cx
            .execute(
                "INSERT INTO ops.entity_fetch (tenant, provider, entity_id, reason)
                 SELECT w.tenant, w.provider, w.upstream, 'linked'
                 FROM unnest($1::text[], $2::text[], $3::text[], $4::text[])
                      AS w(tenant, provider, upstream, stored)
                 WHERE NOT EXISTS (SELECT 1 FROM view.entity_source s
                                   WHERE s.tenant IN ('', $5) AND s.entity_id = w.stored)
                 ON CONFLICT (tenant, provider, entity_id) DO NOTHING",
                &[
                    &columns[&0],
                    &columns[&1],
                    &columns[&2],
                    &columns[&3],
                    &info.tenant,
                ],
            )
            .await
            .map_err(|e| crate::common::describe(&e))?;
        Ok(Applied::rows(usize::try_from(n).unwrap_or(usize::MAX)))
    }
}

impl<B: Backend<Cx = PgCx>> Projection<B> for FetchQueueProjection {
    fn name(&self) -> &'static str {
        "fetch_queue"
    }

    /// After resolution: it reads the registry and `view.entity_source` (step 2).
    fn step(&self) -> Step {
        Step::Activity
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CHANGESET
    }

    /// Reads no rows a record of a tenant's own graphs changes.
    fn repeats_in_block(&self, _earlier: &[&Record], _later: &Record) -> bool {
        true
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move { Self::apply_records(cx, std::slice::from_ref(record)).await })
    }

    /// The block's named entities, queued in one statement.
    fn apply_many<'a>(
        &'a self,
        cx: &'a mut PgCx,
        records: &'a [Record],
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move { Self::apply_records(cx, records).await })
    }

    /// Nothing to drop: the queue is not a projection's table (03 §5), and a replay queues
    /// only what is still not mirrored.
    fn reset<'a>(
        &'a self,
        _cx: &'a mut PgCx,
        _partition: u64,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 0070 §2.1: `linked` when unset; `linked` and `all` queue, `off` and `on-demand` do not.
    #[test]
    fn the_setting_decides_per_provider_and_type() {
        let s = json!({"WD": {"item": "on-demand", "property": "all"}, "lb": {"item": "off"}});
        assert!(!mirrors_linked(&s, "WD", "item"));
        assert!(mirrors_linked(&s, "WD", "property"));
        assert!(mirrors_linked(&s, "WD", "lexeme"));
        assert!(!mirrors_linked(&s, "lb", "item"));
        assert!(mirrors_linked(&serde_json::Value::Null, "WD", "item"));
    }
}
