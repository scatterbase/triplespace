//! Identity clusters (0004; chapter 04 §4, 03 §4.4): the links a tenant's own graphs assert
//! (tier 1), its `different-from` blocks, and the clusters they form.
//!
//! A `same-as`, a `convert` or an `equivalent-property` in a change set of a tenant graph that is not a mirror adds a
//! tier-1 row to `view.link`; a `different-from` a row to `view.different_from`; a `remove`
//! of either deletes it, and an erased record deletes what it wrote. Each change recomputes
//! the clusters around the IDs it names with `scatter-identity`: the connected links and
//! blocks are read, the clusters written to `view.cluster` and `view.cluster_member`, and
//! each link marked applied or held.
//!
//! An entity's ACL that deletes or undeletes it recomputes its clusters: a link touching a
//! deleted entity is kept, `inactive`, and left out (0004 A27).
//!
//! Built so far: tier 1 only, per tenant. Mirror-derived and inferred links (tiers 2 and
//! 3), the shared rows, aliases and tombstoned members are not yet handled (0004 A24, A27).

use std::cmp::Ordering;
use std::collections::BTreeSet;

use scatter_identity::{Link as IdLink, compute, stable_id};
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log_postgres::PgClient;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_wikibase_changeset::rank::ProviderOrder;
use scatter_wikibase_changeset::{Link, Operation};
use scatter_wikibase_ids::{IdForm, TriplespaceId as _};
use scatter_wikibase_model::id::EntityId;

use crate::backend::PgCx;
use crate::common::{PAYLOAD_CHANGESET, offset_db, operation, partition_info};

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// At most this many IDs are read around a change: a cluster holds one member per
/// namespace (0004 §2), so a component of links is small unless something is wrong.
const COMPONENT_LIMIT: usize = 1000;

/// The namespace an ID belongs to (04 §4.1): `local`, a provider code, a keyed type's
/// name, or an entity source's name.
#[must_use]
pub fn namespace(id: &str) -> String {
    let Ok(e) = EntityId::parse(id) else {
        return id.to_string();
    };
    match e.form() {
        IdForm::Local => "local".into(),
        IdForm::Foreign => e.as_str()[..2].to_string(),
        IdForm::Keyed => e
            .keyed_parts()
            .map_or_else(|| id.to_string(), |(name, _)| name.to_ascii_lowercase()),
    }
}

/// Ranks two IDs for the canonical member (04 §4.4), highest first.
fn order(a: &String, b: &String) -> Ordering {
    match (EntityId::parse(a), EntityId::parse(b)) {
        (Ok(x), Ok(y)) => ProviderOrder::default().compare(&x, &y),
        _ => a.cmp(b),
    }
}

/// The projection.
#[derive(Debug, Default, Clone, Copy)]
pub struct IdentityProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for IdentityProjection {
    fn name(&self) -> &'static str {
        "identity"
    }

    fn step(&self) -> Step {
        Step::Clusters
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CHANGESET
            || header.payload_type == crate::actors::payload::ACL
    }

    /// Each record's rows depend only on the links before it, applied in order.
    fn repeats_in_block(&self, _earlier: &[&Record], _later: &Record) -> bool {
        true
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let info = partition_info(cx, h.partition).await?;
            if info.tenant.is_empty() || info.is_mirror() {
                return Ok(Applied::rows(0));
            }
            let tenant = info.tenant.as_str();
            if h.payload_type == crate::actors::payload::ACL {
                return deletion(cx, tenant, h.key.as_deref()).await;
            }
            let partition = scatter_log_postgres::ids::partition_to_db(h.partition);
            let offset = offset_db(record)?;
            let Some(op) = operation(record)? else {
                // Erased: what the record wrote goes with it.
                let mut seeds = Vec::new();
                for table in ["view.link", "view.different_from"] {
                    let rows = cx
                        .query(
                            &format!(
                                "DELETE FROM {table} WHERE tenant = $1 AND \"partition\" = $2
                                   AND \"offset\" = $3 RETURNING a, b"
                            ),
                            &[&tenant, &partition, &offset],
                        )
                        .await
                        .map_err(|e| sql(&e))?;
                    seeds.extend(rows.iter().flat_map(|r| [r.get::<_, String>(0), r.get(1)]));
                }
                return recompute(cx, tenant, seeds).await;
            };
            let (a, b) = match op.as_ref() {
                Operation::SameAs { ids, .. }
                | Operation::DifferentFrom { ids, .. }
                | Operation::EquivalentProperty { ids, .. }
                | Operation::Remove {
                    link:
                        Some(
                            Link::SameAs { ids }
                            | Link::DifferentFrom { ids }
                            | Link::EquivalentProperty { ids },
                        ),
                    ..
                } => match ids.as_slice() {
                    [a, b] if a != b => (a.as_str().to_string(), b.as_str().to_string()),
                    _ => return Ok(Applied::rows(0)),
                },
                Operation::Convert { id, local } => {
                    (id.as_str().to_string(), local.as_str().to_string())
                }
                _ => return Ok(Applied::rows(0)),
            };
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            let source = offset.to_string();
            let result = match op.as_ref() {
                Operation::SameAs { .. }
                | Operation::Convert { .. }
                | Operation::EquivalentProperty { .. } => {
                    cx.execute(
                        "INSERT INTO view.link (tenant, a, b, tier, source, status, \"partition\", \"offset\")
                         VALUES ($1, $2, $3, 1, $4, 'applied', $5, $6)
                         ON CONFLICT (tenant, a, b, tier, source) DO NOTHING",
                        &[&tenant, &a, &b, &source, &partition, &offset],
                    )
                    .await
                }
                Operation::DifferentFrom { .. } => {
                    cx.execute(
                        "INSERT INTO view.different_from (tenant, a, b, \"partition\", \"offset\")
                         VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT (tenant, a, b) DO UPDATE
                           SET \"partition\" = EXCLUDED.\"partition\", \"offset\" = EXCLUDED.\"offset\"",
                        &[&tenant, &a, &b, &partition, &offset],
                    )
                    .await
                }
                Operation::Remove {
                    link: Some(Link::SameAs { .. } | Link::EquivalentProperty { .. }),
                    ..
                } => {
                    cx.execute(
                        "DELETE FROM view.link WHERE tenant = $1 AND a = $2 AND b = $3 AND tier = 1",
                        &[&tenant, &a, &b],
                    )
                    .await
                }
                _ => {
                    cx.execute(
                        "DELETE FROM view.different_from WHERE tenant = $1 AND a = $2 AND b = $3",
                        &[&tenant, &a, &b],
                    )
                    .await
                }
            };
            result.map_err(|e| sql(&e))?;
            recompute(cx, tenant, vec![a, b]).await
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let partition = scatter_log_postgres::ids::partition_to_db(partition);
            let mut touched: Vec<(String, String)> = Vec::new();
            for table in ["view.link", "view.different_from"] {
                let rows = cx
                    .query(
                        &format!(
                            "DELETE FROM {table} WHERE \"partition\" = $1 RETURNING tenant, a, b"
                        ),
                        &[&partition],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
                for r in &rows {
                    let t: String = r.get(0);
                    touched.push((t.clone(), r.get(1)));
                    touched.push((t, r.get(2)));
                }
            }
            let tenants: BTreeSet<String> = touched.iter().map(|(t, _)| t.clone()).collect();
            for t in tenants {
                let seeds = touched
                    .iter()
                    .filter(|(x, _)| *x == t)
                    .map(|(_, id)| id.clone())
                    .collect();
                recompute(cx, &t, seeds).await?;
            }
            Ok(())
        })
    }
}

/// Whether an operation writes or removes a link between entities: what changes clusters.
#[must_use]
pub fn is_link(op: &Operation) -> bool {
    matches!(
        op,
        Operation::SameAs { .. }
            | Operation::DifferentFrom { .. }
            | Operation::Convert { .. }
            | Operation::EquivalentProperty { .. }
            | Operation::Remove { link: Some(_), .. }
    )
}

/// The IDs connected to `seeds` on `tenant`: those in a cluster with one (a removed link may
/// leave them apart), and those the links reach from there, breadth first.
pub(crate) async fn component(
    cx: &PgCx,
    tenant: &str,
    seeds: Vec<String>,
) -> Result<BTreeSet<String>, String> {
    let mut nodes: BTreeSet<String> = seeds.into_iter().collect();
    if nodes.is_empty() {
        return Ok(nodes);
    }
    let list: Vec<String> = nodes.iter().cloned().collect();
    let rows = cx
        .query(
            "SELECT m2.entity_id FROM view.cluster_member m1
               JOIN view.cluster_member m2 ON m2.tenant = m1.tenant AND m2.cluster_id = m1.cluster_id
              WHERE m1.tenant = $1 AND m1.entity_id = ANY($2)",
            &[&tenant, &list],
        )
        .await
        .map_err(|e| sql(&e))?;
    nodes.extend(rows.iter().map(|r| r.get::<_, String>(0)));
    let mut frontier: Vec<String> = nodes.iter().cloned().collect();
    while !frontier.is_empty() && nodes.len() < COMPONENT_LIMIT {
        let rows = cx
            .query(
                "SELECT a, b FROM view.link WHERE tenant = $1 AND (a = ANY($2) OR b = ANY($2))",
                &[&tenant, &frontier],
            )
            .await
            .map_err(|e| sql(&e))?;
        frontier = rows
            .iter()
            .flat_map(|r| [r.get::<_, String>(0), r.get(1)])
            .filter(|id| nodes.insert(id.clone()))
            .collect();
    }
    Ok(nodes)
}

/// An entity's ACL may delete or undelete it (04 §4.4): its clusters are recomputed, and the
/// entities they reach resolved again, since the entities projection reads no ACL.
async fn deletion(cx: &PgCx, tenant: &str, key: Option<&str>) -> Result<Applied, String> {
    let Some(Ok(scatter_actors::acl::Target::Entity(id))) =
        key.map(scatter_actors::acl::Target::parse)
    else {
        return Ok(Applied::rows(0));
    };
    let linked = cx
        .query(
            "SELECT 1 FROM view.link WHERE tenant = $1 AND (a = $2 OR b = $2) LIMIT 1",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?;
    if linked.is_empty() {
        return Ok(Applied::rows(0));
    }
    let reached = component(cx, tenant, vec![id.clone()]).await?;
    let mut applied = recompute(cx, tenant, vec![id]).await?;
    let entities = crate::entities::EntityProjection::default();
    for other in reached {
        if let Ok(other) = EntityId::parse(&other) {
            applied.rows +=
                usize::try_from(entities.resolve_subject(cx, tenant, &other, None).await?)
                    .unwrap_or(0);
        }
    }
    Ok(applied)
}

/// Which of `ids` are deleted on `tenant`: its own row's flag, else the shared row's.
async fn deleted_among(
    cx: &PgCx,
    tenant: &str,
    ids: &[String],
) -> Result<BTreeSet<String>, String> {
    Ok(cx
        .query(
            "SELECT e.id FROM view.entity e
              WHERE e.id = ANY($2) AND e.deleted
                AND (e.tenant = $1 OR (e.tenant = '' AND NOT EXISTS (
                  SELECT 1 FROM view.entity o WHERE o.tenant = $1 AND o.id = e.id)))",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// Writes each link's status: held in its conflict, `inactive` where it touches a deleted
/// entity, applied otherwise.
async fn mark_links(
    cx: &PgCx,
    tenant: &str,
    list: &[String],
    deleted: BTreeSet<String>,
    conflicts: &[scatter_identity::Conflict<String>],
    link_rows: &[tokio_postgres::Row],
) -> Result<usize, String> {
    let mut n = 0usize;
    cx.execute(
        "UPDATE view.link SET status = 'applied', conflict_id = NULL
          WHERE tenant = $1 AND a = ANY($2) AND b = ANY($2) AND status <> 'applied'",
        &[&tenant, &list],
    )
    .await
    .map_err(|e| sql(&e))?;
    let deleted: Vec<String> = deleted.into_iter().collect();
    cx.execute(
        "UPDATE view.link SET status = 'inactive', conflict_id = NULL
          WHERE tenant = $1 AND (a = ANY($2) OR b = ANY($2))",
        &[&tenant, &deleted],
    )
    .await
    .map_err(|e| sql(&e))?;
    for conflict in conflicts {
        let conflict_id = stable_id(&conflict.key);
        for &i in &conflict.links {
            let r = &link_rows[i];
            let (a, b, tier, source): (String, String, i16, String) =
                (r.get(0), r.get(1), r.get(2), r.get(3));
            cx.execute(
                "UPDATE view.link SET status = 'held', conflict_id = $6
                  WHERE tenant = $1 AND a = $2 AND b = $3 AND tier = $4 AND source = $5",
                &[&tenant, &a, &b, &tier, &source, &conflict_id],
            )
            .await
            .map_err(|e| sql(&e))?;
            n += 1;
        }
    }
    Ok(n)
}

/// Recomputes the clusters around `seeds` on `tenant`: reads every link and block connected
/// to them, works out the clusters, and writes them and each link's status.
async fn recompute(cx: &PgCx, tenant: &str, seeds: Vec<String>) -> Result<Applied, String> {
    let nodes = component(cx, tenant, seeds).await?;
    if nodes.is_empty() {
        return Ok(Applied::rows(0));
    }
    let list: Vec<String> = nodes.iter().cloned().collect();
    // A deleted member leaves its cluster: its links are kept, and not applied while it is
    // deleted (04 §4.4).
    let deleted = deleted_among(cx, tenant, &list).await?;
    let link_rows: Vec<tokio_postgres::Row> = cx
        .query(
            "SELECT a, b, tier, source FROM view.link
              WHERE tenant = $1 AND a = ANY($2) AND b = ANY($2)
              ORDER BY a, b, tier, source",
            &[&tenant, &list],
        )
        .await
        .map_err(|e| sql(&e))?
        .into_iter()
        .filter(|r| {
            !deleted.contains(&r.get::<_, String>(0)) && !deleted.contains(&r.get::<_, String>(1))
        })
        .collect();
    let links: Vec<IdLink<String>> = link_rows
        .iter()
        .map(|r| IdLink {
            a: r.get(0),
            b: r.get(1),
            tier: u8::try_from(r.get::<_, i16>(2)).unwrap_or(u8::MAX),
        })
        .collect();
    let blocks: Vec<(String, String)> = cx
        .query(
            "SELECT a, b FROM view.different_from WHERE tenant = $1 AND a = ANY($2) AND b = ANY($2)",
            &[&tenant, &list],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let out = compute(&links, &blocks, |id| namespace(id), order);

    // The old clusters of these IDs go; the new ones are written in their place.
    cx.execute(
        "DELETE FROM view.cluster c USING view.cluster_member m
          WHERE c.tenant = $1 AND m.tenant = $1 AND m.cluster_id = c.cluster_id
            AND m.entity_id = ANY($2)",
        &[&tenant, &list],
    )
    .await
    .map_err(|e| sql(&e))?;
    cx.execute(
        "DELETE FROM view.cluster_member WHERE tenant = $1 AND cluster_id IN
           (SELECT cluster_id FROM view.cluster_member WHERE tenant = $1 AND entity_id = ANY($2))",
        &[&tenant, &list],
    )
    .await
    .map_err(|e| sql(&e))?;
    let mut rows = 0usize;
    for members in &out.clusters {
        let canonical = &members[0];
        let id = stable_id(canonical);
        cx.execute(
            "INSERT INTO view.cluster (tenant, cluster_id, canonical_id, version)
             VALUES ($1, $2, $3, COALESCE((SELECT max(version) FROM view.cluster WHERE tenant = $1), 0) + 1)",
            &[&tenant, &id, canonical],
        )
        .await
        .map_err(|e| sql(&e))?;
        for m in members {
            cx.execute(
                "INSERT INTO view.cluster_member (tenant, cluster_id, namespace, entity_id)
                 VALUES ($1, $2, $3, $4)",
                &[&tenant, &id, &namespace(m), m],
            )
            .await
            .map_err(|e| sql(&e))?;
        }
        rows += 1 + members.len();
    }
    rows += mark_links(cx, tenant, &list, deleted, &out.conflicts, &link_rows).await?;
    Ok(Applied::rows(rows))
}

/// The cluster an entity is a member of on a tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    /// The cluster's number.
    pub cluster_id: i64,
    /// Its canonical member (0004 §4).
    pub canonical: String,
    /// Every member, the canonical first, then by rank.
    pub members: Vec<String>,
}

/// The cluster `id` is a member of on `tenant`, where it is in one.
///
/// # Errors
///
/// The database's.
pub async fn membership<C: PgClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<Option<Membership>, String> {
    let rows = client
        .query(
            "SELECT c.cluster_id, c.canonical_id, m2.entity_id FROM view.cluster_member m1
               JOIN view.cluster c ON c.tenant = m1.tenant AND c.cluster_id = m1.cluster_id
               JOIN view.cluster_member m2 ON m2.tenant = m1.tenant AND m2.cluster_id = m1.cluster_id
              WHERE m1.tenant = $1 AND m1.entity_id = $2",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| e.to_string())?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let mut members: Vec<String> = rows.iter().map(|r| r.get(2)).collect();
    members.sort_by(order);
    Ok(Some(Membership {
        cluster_id: first.get(0),
        canonical: first.get(1),
        members,
    }))
}

/// For each of `ids` that is in a cluster on `tenant`, the other members of its cluster:
/// the exact matches a response may name it by (04 §4.12). Every cluster is of tier-1
/// links so far, each an exact match; inferred links, when they are built, will not be.
///
/// # Errors
///
/// The database's.
pub async fn exact_matches<C: PgClient>(
    client: &C,
    tenant: &str,
    ids: &[String],
) -> Result<std::collections::BTreeMap<String, Vec<String>>, String> {
    let mut out: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    if ids.is_empty() || tenant.is_empty() {
        return Ok(out);
    }
    let rows = client
        .query(
            "SELECT m1.entity_id, m2.entity_id FROM view.cluster_member m1
               JOIN view.cluster_member m2 ON m2.tenant = m1.tenant AND m2.cluster_id = m1.cluster_id
              WHERE m1.tenant = $1 AND m1.entity_id = ANY($2) AND m2.entity_id <> m1.entity_id",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| e.to_string())?;
    for r in &rows {
        out.entry(r.get(0)).or_default().push(r.get(1));
    }
    Ok(out)
}

/// For each of `ids` that is a non-canonical member of a cluster on `tenant`, its cluster's
/// canonical member: the canon resolution compares properties and values by (04 §4.6–§4.8).
///
/// # Errors
///
/// The database's.
pub async fn canonical_map<C: PgClient>(
    client: &C,
    tenant: &str,
    ids: &[String],
) -> Result<std::collections::BTreeMap<String, String>, String> {
    if ids.is_empty() || tenant.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let rows = client
        .query(
            "SELECT m.entity_id, c.canonical_id FROM view.cluster_member m
               JOIN view.cluster c ON c.tenant = m.tenant AND c.cluster_id = m.cluster_id
              WHERE m.tenant = $1 AND m.entity_id = ANY($2) AND m.entity_id <> c.canonical_id",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// The other members of `id`'s cluster on `tenant`, the canonical member first, then in
/// text order; empty when `id` is in no cluster.
///
/// # Errors
///
/// The database's.
pub async fn members<C: PgClient>(
    client: &C,
    tenant: &str,
    id: &str,
) -> Result<Vec<String>, String> {
    let rows = client
        .query(
            "SELECT m2.entity_id FROM view.cluster_member m1
               JOIN view.cluster_member m2 ON m2.tenant = m1.tenant AND m2.cluster_id = m1.cluster_id
               JOIN view.cluster c ON c.tenant = m1.tenant AND c.cluster_id = m1.cluster_id
              WHERE m1.tenant = $1 AND m1.entity_id = $2 AND m2.entity_id <> $2
              ORDER BY m2.entity_id = c.canonical_id DESC, m2.entity_id",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 04 §4.1: the namespace is whoever minted the ID, not the type code.
    #[test]
    fn namespaces_are_who_minted_the_id() {
        assert_eq!(namespace("Q5"), "local");
        assert_eq!(namespace("P5"), "local");
        assert_eq!(namespace("WDQ5"), "WD");
        assert_eq!(namespace("OAW5"), "OA");
        assert_eq!(namespace("OAA5"), "OA");
        assert_eq!(namespace("domain:example.org"), "domain");
        assert_eq!(namespace("mhc:Q1"), "mhc");
    }

    // 04 §4.4: keyed, then local, then providers, then sources.
    #[test]
    fn the_canonical_order() {
        let ids = ["mhc:Q1", "WDQ1", "Q1", "domain:example.org"].map(String::from);
        let mut v = ids.to_vec();
        v.sort_by(order);
        assert_eq!(v, ["domain:example.org", "Q1", "WDQ1", "mhc:Q1"]);
    }
}
