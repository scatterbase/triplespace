//! `view.entity_ref` and its counter (03 §4.2, §4.3; 0013 A108): which entities an
//! entity's resolved statements refer to, and how many entities refer to each target.
//!
//! The rows are tables of the entity projection (step 4), as `term` and `identifier`
//! are: [`rows`] reads them off the resolved state, and [`write()`] replaces a source's
//! rows in the statement that composition's other writes go down with. A target is an
//! ID, in source form, whether or not any graph holds the entity yet.
//!
//! The count is kept apart from the write. [`write()`] appends what it added and removed to
//! `view.entity_ref_delta` and queues one item of work; the worker's [`RefCountConsumer`]
//! folds a tenant's deltas into `view.entity_ref_count` in one statement. So composition
//! never updates a count: two writers that refer to the same target (every entity refers
//! to the same few properties) do not wait on one another, and a bootstrap's counts are
//! one aggregate when the worker next runs.

use std::collections::BTreeMap;

use scatter_log_postgres::PgClient;
use scatter_projection::{Backend, BoxFuture, Consumer};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::DataValue;

use crate::backend::PgCx;

/// The consumer's name in `ops.projection_work` and lag reports.
pub const CONSUMER: &str = "ref_count";

/// The role bit of a main snak.
pub const MAIN: i16 = 1;
/// The role bit of a qualifier.
pub const QUALIFIER: i16 = 2;
/// The role bit of a reference's snak.
pub const REFERENCE: i16 = 4;

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// What a snak refers to: its property, an entity value, and the entity a quantity's
/// unit, a coordinate's globe or a time's calendar model names (0004 A20); a `"1"` unit
/// or an IRI kept as written names none.
fn snak_targets(snak: &Snak, role: i16, out: &mut BTreeMap<String, i16>) {
    let mut add = |id: &str| *out.entry(id.to_string()).or_default() |= role;
    add(snak.property.as_str());
    let SnakKind::Value(value) = &snak.kind else {
        return;
    };
    let named = match value {
        DataValue::EntityId(v) => {
            add(v.id.as_str());
            return;
        }
        DataValue::Quantity(q) => &q.unit,
        DataValue::GlobeCoordinate(g) => &g.globe,
        DataValue::Time(t) => &t.calendarmodel,
        _ => return,
    };
    if let Ok(id) = EntityId::parse(named) {
        add(id.as_str());
    }
}

/// An entity's `view.entity_ref` rows: each entity its statements refer to, with the
/// roles it is referred to in, in target order. The entity itself is left out by
/// [`write()`], which knows the ID its rows are written under.
#[must_use]
pub fn rows(entity: &Entity) -> Vec<(String, i16)> {
    let mut out = BTreeMap::new();
    for s in entity.all_statements() {
        snak_targets(&s.mainsnak, MAIN, &mut out);
        for snak in s.qualifiers.values().flatten() {
            snak_targets(snak, QUALIFIER, &mut out);
        }
        for snak in s.references.iter().flat_map(|r| r.snaks.values().flatten()) {
            snak_targets(snak, REFERENCE, &mut out);
        }
    }
    out.into_iter().collect()
}

/// Replaces the `view.entity_ref` rows of `sources` in `tenant` with `rows`
/// (`(source, target, roles)`, every source among `sources`): one statement, which
/// deletes the rows no longer there, writes the rest, appends the change to each
/// target's count to `view.entity_ref_delta`, and queues the fold when there is one. A
/// source with no row in `rows` is left with none, which is how its rows are deleted.
///
/// # Errors
///
/// The database's.
pub async fn write(
    cx: &PgCx,
    tenant: &str,
    sources: &[&str],
    rows: &[(&str, &str, i16)],
) -> Result<u64, String> {
    if sources.is_empty() {
        return Ok(0);
    }
    let sources: Vec<&str> = sources.to_vec();
    let from: Vec<&str> = rows.iter().map(|r| r.0).collect();
    let to: Vec<&str> = rows.iter().map(|r| r.1).collect();
    let roles: Vec<i16> = rows.iter().map(|r| r.2).collect();
    // `gone` and `put` touch different rows, so their order does not matter; a row
    // `put` inserted, as against one whose roles it changed, has no `xmax`.
    cx.execute(
        "WITH new AS (
             SELECT t.source_id, t.target_id, t.roles
             FROM unnest($3::text[], $4::text[], $5::smallint[]) AS t(source_id, target_id, roles)
             WHERE t.source_id <> t.target_id
         ),
         gone AS (
             DELETE FROM view.entity_ref r
             WHERE r.tenant = $1 AND r.source_id = ANY($2)
               AND NOT EXISTS (SELECT 1 FROM new n
                               WHERE n.source_id = r.source_id AND n.target_id = r.target_id)
             RETURNING r.target_id
         ),
         put AS (
             INSERT INTO view.entity_ref (tenant, target_id, source_id, roles)
             SELECT $1, target_id, source_id, roles FROM new
             ON CONFLICT (tenant, target_id, source_id) DO UPDATE SET roles = EXCLUDED.roles
               WHERE view.entity_ref.roles <> EXCLUDED.roles
             RETURNING target_id, (xmax = 0) AS added
         ),
         delta AS (
             INSERT INTO view.entity_ref_delta (tenant, target_id, d)
             SELECT $1, x.target_id, sum(x.d)
             FROM (SELECT target_id, -1 AS d FROM gone
                   UNION ALL SELECT target_id, 1 FROM put WHERE added) x
             GROUP BY x.target_id HAVING sum(x.d) <> 0
             RETURNING 1
         )
         INSERT INTO ops.projection_work (projection, tenant, partition, \"offset\", key)
         SELECT $6, $1, 0, 0, 'fold' WHERE EXISTS (SELECT 1 FROM delta)",
        &[&tenant, &sources, &from, &to, &roles, &CONSUMER],
    )
    .await
    .map_err(|e| sql(&e))?;
    Ok(u64::try_from(rows.len()).unwrap_or(u64::MAX))
}

/// [`write()`] for one entity: its rows as [`rows`] reads them.
///
/// # Errors
///
/// The database's.
pub async fn write_one(
    cx: &PgCx,
    tenant: &str,
    id: &EntityId,
    targets: &[(String, i16)],
) -> Result<u64, String> {
    let rows: Vec<(&str, &str, i16)> = targets
        .iter()
        .map(|(target, roles)| (id.as_str(), target.as_str(), *roles))
        .collect();
    write(cx, tenant, &[id.as_str()], &rows).await
}

/// Folds a tenant's deltas into its counts: one statement. A count that reaches nothing
/// is removed.
///
/// # Errors
///
/// The database's.
pub async fn fold(cx: &PgCx, tenant: &str) -> Result<u64, String> {
    let n = cx
        .execute(
            "WITH taken AS (
                 DELETE FROM view.entity_ref_delta WHERE tenant = $1 RETURNING target_id, d
             ),
             summed AS (
                 SELECT target_id, sum(d)::bigint AS d FROM taken GROUP BY target_id
             )
             INSERT INTO view.entity_ref_count (tenant, target_id, refs)
             SELECT $1, target_id, d FROM summed WHERE d <> 0
             ON CONFLICT (tenant, target_id) DO UPDATE
               SET refs = view.entity_ref_count.refs + EXCLUDED.refs",
            &[&tenant],
        )
        .await
        .map_err(|e| sql(&e))?;
    cx.execute(
        "DELETE FROM view.entity_ref_count WHERE tenant = $1 AND refs <= 0",
        &[&tenant],
    )
    .await
    .map_err(|e| sql(&e))?;
    Ok(n)
}

/// Removes every row, delta and count of a tenant: what a reset of the entity projection
/// does before its records are applied again.
///
/// # Errors
///
/// The database's.
pub async fn reset(cx: &PgCx, tenant: &str) -> Result<(), String> {
    for table in [
        "view.entity_ref",
        "view.entity_ref_delta",
        "view.entity_ref_count",
    ] {
        cx.execute(
            &format!("DELETE FROM {table} WHERE tenant = $1"),
            &[&tenant],
        )
        .await
        .map_err(|e| sql(&e))?;
    }
    Ok(())
}

/// The `ref_count` consumer for the worker (03 §4.2, §6.1): folds a tenant's deltas.
/// One worker applies a tenant's work at a time, so no two folds update the same count.
pub struct RefCountConsumer;

impl<B: Backend<Cx = PgCx>> Consumer<B> for RefCountConsumer {
    fn name(&self) -> &'static str {
        CONSUMER
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        tenant: &'a str,
        _keys: &'a [String],
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let n = fold(&*cx, tenant).await?;
            Ok(usize::try_from(n).unwrap_or(usize::MAX))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 03 §4.3: a row per referred entity, with the roles it is referred to in.
    #[test]
    fn rows_name_properties_values_and_units_with_their_roles() {
        let entity = Entity::from_value(serde_json::json!({
                "type": "item", "id": "Q1",
                "claims": {
                    "P1": [{
                        "type": "statement", "rank": "normal", "id": "Q1$00000000-0000-0000-0000-00000000000A",
                        "mainsnak": {"snaktype": "value", "property": "P1", "datatype": "wikibase-item",
                            "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "WDQ5"}}},
                        "qualifiers": {"P2": [{"snaktype": "value", "property": "P2", "datatype": "quantity",
                            "datavalue": {"type": "quantity", "value": {"amount": "+1", "unit": "WDQ11573"}}}]},
                        "qualifiers-order": ["P2"],
                        "references": [{"snaks": {"P1": [{"snaktype": "value", "property": "P1", "datatype": "wikibase-item",
                            "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "Q1"}}}]},
                            "snaks-order": ["P1"]}]
                    }],
                    "P3": [{
                        "type": "statement", "rank": "normal", "id": "Q1$00000000-0000-0000-0000-00000000000B",
                        "mainsnak": {"snaktype": "value", "property": "P3", "datatype": "quantity",
                            "datavalue": {"type": "quantity", "value": {"amount": "+2", "unit": "1"}}}
                    }]
                }
        }))
        .unwrap()
        .entity;
        assert_eq!(
            rows(&entity),
            vec![
                ("P1".to_string(), MAIN | REFERENCE),
                ("P2".to_string(), QUALIFIER),
                ("P3".to_string(), MAIN),
                // The entity itself, which `write` leaves out.
                ("Q1".to_string(), REFERENCE),
                ("WDQ11573".to_string(), QUALIFIER),
                ("WDQ5".to_string(), MAIN),
            ]
        );
    }
}
