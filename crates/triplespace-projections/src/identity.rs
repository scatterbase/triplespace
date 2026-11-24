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
//! Mirror-derived links (tier 2, 0004 §3, A28) come from the instance's `reconcile:default`
//! record, whose `links` name, for each provider, the properties of its mirror whose values
//! name another provider's entity. A mirror's state of an entity replaces the tier-2 links
//! its statements make, as shared rows (`tenant = ''`); a change recomputes the shared
//! clusters around it, the clusters of every tenant whose links or rows reach them (a
//! tenant's clusters are computed from the shared links and its own), and resolves the
//! entities it reaches again. A tenant reads its own cluster rows for an ID its links or
//! blocks touch, and the shared ones otherwise.
//!
//! Not built yet: inferred links (tier 3), a tenant's link properties for its own sources,
//! aliases and tombstoned members (0004 A24, A27, A28).

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
            if info.tenant.is_empty() && info.is_mirror() && h.payload_type == PAYLOAD_CHANGESET {
                let properties = LinkProperties::load(cx).await?;
                return mirror_links(cx, &info.name, record, &properties).await;
            }
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

    /// A shared mirror's block reads the instance's link properties once; any other block
    /// is applied a record at a time.
    fn apply_many<'a>(
        &'a self,
        cx: &'a mut PgCx,
        records: &'a [Record],
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let mut out = Applied::default();
            let Some(first) = records.first() else {
                return Ok(out);
            };
            let info = partition_info(cx, first.header().partition).await?;
            if info.tenant.is_empty() && info.is_mirror() {
                let properties = LinkProperties::load(cx).await?;
                for record in records {
                    if record.header().payload_type == PAYLOAD_CHANGESET {
                        out.rows += mirror_links(cx, &info.name, record, &properties)
                            .await?
                            .rows;
                    }
                }
                return Ok(out);
            }
            for record in records {
                let applied = <Self as Projection<B>>::apply(self, cx, record).await?;
                out.rows += applied.rows;
                out.fanout.extend(applied.fanout);
            }
            Ok(out)
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
                let seeds: Vec<String> = touched
                    .iter()
                    .filter(|(x, _)| *x == t)
                    .map(|(_, id)| id.clone())
                    .collect();
                if t.is_empty() {
                    shared_change(cx, seeds, false).await?;
                } else {
                    recompute(cx, &t, seeds).await?;
                }
            }
            Ok(())
        })
    }
}

/// The instance's tier-2 link properties (0004 §3, A28), from `links` in its
/// `reconcile:default` record: for each provider code, the properties of its mirror, as
/// stored (`WDP10283` for Wikidata's `P10283`), whose values name an entity of a target
/// provider (`W123` as `OAW123`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkProperties(std::collections::BTreeMap<String, Vec<(String, String)>>);

impl LinkProperties {
    /// Reads `links` from a `reconcile` record's content: `{"WD": [{"property": "P10283",
    /// "target": "OA"}]}`. An entry without both fields is skipped.
    #[must_use]
    pub fn parse(config: &serde_json::Value) -> Self {
        let mut out: std::collections::BTreeMap<String, Vec<(String, String)>> =
            std::collections::BTreeMap::new();
        let Some(links) = config.get("links").and_then(serde_json::Value::as_object) else {
            return Self(out);
        };
        for (code, list) in links {
            let code = code.to_ascii_uppercase();
            for item in list.as_array().into_iter().flatten() {
                let field = |k: &str| item.get(k).and_then(serde_json::Value::as_str);
                let (Some(property), Some(target)) = (field("property"), field("target")) else {
                    continue;
                };
                let property = property.to_ascii_uppercase();
                let stored = if property.starts_with(&code) {
                    property
                } else {
                    format!("{code}{property}")
                };
                out.entry(code.clone())
                    .or_default()
                    .push((stored, target.to_ascii_uppercase()));
            }
        }
        Self(out)
    }

    /// The instance's, from `view.registry`; none without a record.
    ///
    /// # Errors
    ///
    /// The database's.
    pub async fn load<C: PgClient>(client: &C) -> Result<Self, String> {
        let row = client
            .query_opt(
                "SELECT config FROM view.registry
                  WHERE tenant = '' AND kind = 'reconcile' AND code = 'default'",
                &[],
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map_or_else(Self::default, |r| {
            Self::parse(&r.get::<_, serde_json::Value>(0))
        }))
    }

    /// The tier-2 links a provider's state of `subject` makes: `(a, b, statement)`, `a` the
    /// lesser ID. A deprecated statement makes none, and neither does a value that is no
    /// ID of the target provider.
    #[must_use]
    pub fn links_of(
        &self,
        code: &str,
        subject: &EntityId,
        entity: &scatter_wikibase_model::entity::Entity,
    ) -> Vec<(String, String, String)> {
        use scatter_wikibase_model::statement::{Rank, SnakKind};
        use scatter_wikibase_model::value::DataValue;
        let Some(properties) = self.0.get(code) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for statement in entity.all_statements() {
            if statement.rank == Rank::Deprecated {
                continue;
            }
            let property = statement.mainsnak.property.as_str();
            let SnakKind::Value(DataValue::String(value)) = &statement.mainsnak.kind else {
                continue;
            };
            for (p, target) in properties {
                if p != property {
                    continue;
                }
                let Ok(other) = EntityId::parse(&format!("{target}{}", value.trim())) else {
                    continue;
                };
                if other.form() != IdForm::Foreign
                    || !other.as_str().starts_with(target.as_str())
                    || other == *subject
                {
                    continue;
                }
                let (a, b) = if subject.as_str() <= other.as_str() {
                    (subject.as_str().to_string(), other.as_str().to_string())
                } else {
                    (other.as_str().to_string(), subject.as_str().to_string())
                };
                let source = statement.id.as_ref().map_or_else(
                    || format!("{subject}#{property}={value}"),
                    |i| i.as_str().to_string(),
                );
                out.push((a, b, source));
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// A shared mirror's record: the tier-2 links of the entity it puts, tombstones or
/// redirects are replaced with those its new state makes (0004 §3, A28). Nothing is read
/// or written for a provider the instance names no link properties for.
async fn mirror_links(
    cx: &PgCx,
    graph: &str,
    record: &Record,
    properties: &LinkProperties,
) -> Result<Applied, String> {
    let Some(code) = scatter_providers::Registry::default_registry()
        .providers()
        .iter()
        .find(|p| p.mirror_graph() == graph)
        .map(|p| p.code.clone())
    else {
        return Ok(Applied::rows(0));
    };
    if !properties.0.contains_key(&code) {
        return Ok(Applied::rows(0));
    }
    let Some(op) = operation(record)? else {
        return Ok(Applied::rows(0));
    };
    let (subject, new) = match op.as_ref() {
        Operation::Put { id, entity, .. } => (id.clone(), properties.links_of(&code, id, entity)),
        Operation::Tombstone { id, .. } => (id.clone(), Vec::new()),
        Operation::Redirect {
            from,
            upstream: Some(_),
            ..
        } => (from.clone(), Vec::new()),
        _ => return Ok(Applied::rows(0)),
    };
    let old: Vec<(String, String, String)> = cx
        .query(
            "SELECT a, b, source FROM view.link WHERE tenant = '' AND subject = $1 ORDER BY a, b, source",
            &[&subject.as_str()],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    if old == new {
        return Ok(Applied::rows(0));
    }
    cx.execute(
        "DELETE FROM view.link WHERE tenant = '' AND subject = $1",
        &[&subject.as_str()],
    )
    .await
    .map_err(|e| sql(&e))?;
    let partition = scatter_log_postgres::ids::partition_to_db(record.header().partition);
    let offset = offset_db(record)?;
    for (a, b, source) in &new {
        cx.execute(
            "INSERT INTO view.link (tenant, a, b, tier, source, status, \"partition\", \"offset\", subject)
             VALUES ('', $1, $2, 2, $3, 'applied', $4, $5, $6)
             ON CONFLICT (tenant, a, b, tier, source) DO NOTHING",
            &[a, b, source, &partition, &offset, &subject.as_str()],
        )
        .await
        .map_err(|e| sql(&e))?;
    }
    let seeds: Vec<String> = old
        .iter()
        .chain(&new)
        .flat_map(|(a, b, _)| [a.clone(), b.clone()])
        .collect();
    shared_change(cx, seeds, true).await
}

/// A change to the shared links around `seeds`: the shared clusters are recomputed, then
/// every tenant's whose links, blocks or clusters reach them, and with `resolve`, every
/// entity reached is resolved again, shared and in each tenant that holds a row of it.
async fn shared_change(cx: &PgCx, seeds: Vec<String>, resolve: bool) -> Result<Applied, String> {
    let reached = component(cx, "", seeds.clone()).await?;
    let mut rows = recompute(cx, "", seeds).await?.rows;
    let list: Vec<String> = reached.iter().cloned().collect();
    let tenants: Vec<String> = cx
        .query(
            "SELECT tenant FROM view.link WHERE tenant <> '' AND (a = ANY($1) OR b = ANY($1))
             UNION SELECT tenant FROM view.cluster_member WHERE tenant <> '' AND entity_id = ANY($1)
             UNION SELECT tenant FROM view.different_from WHERE tenant <> '' AND (a = ANY($1) OR b = ANY($1))",
            &[&list],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for t in &tenants {
        rows += recompute(cx, t, list.clone()).await?.rows;
    }
    if resolve {
        let entities = crate::entities::EntityProjection::default();
        for id in &list {
            let Ok(id) = EntityId::parse(id) else {
                continue;
            };
            let n = entities.resolve_subject(cx, "", &id, None).await?;
            rows += usize::try_from(n).unwrap_or(0);
            for t in crate::entities::EntityProjection::overlay_tenants(cx, &id).await? {
                let n = entities.resolve_subject(cx, &t, &id, None).await?;
                rows += usize::try_from(n).unwrap_or(0);
            }
        }
    }
    Ok(Applied::rows(rows))
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
              WHERE m1.tenant IN ($1, '') AND m1.entity_id = ANY($2)",
            &[&tenant, &list],
        )
        .await
        .map_err(|e| sql(&e))?;
    nodes.extend(rows.iter().map(|r| r.get::<_, String>(0)));
    let mut frontier: Vec<String> = nodes.iter().cloned().collect();
    while !frontier.is_empty() && nodes.len() < COMPONENT_LIMIT {
        // A tenant's clusters are made of the shared links and its own.
        let rows = cx
            .query(
                "SELECT a, b FROM view.link
                  WHERE tenant IN ($1, '') AND (a = ANY($2) OR b = ANY($2))",
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
            let (a, b, tier, source, owner): (String, String, i16, String, String) =
                (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
            // A shared link's status is the shared view's; a tenant's view of it is its
            // clusters.
            if owner != tenant {
                continue;
            }
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
            "SELECT a, b, tier, source, tenant FROM view.link
              WHERE tenant IN ($1, '') AND a = ANY($2) AND b = ANY($2)
              ORDER BY a, b, tier, source, tenant",
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

/// Which of `ids` a tenant's own cluster rows decide (0004 A28): those in one of its
/// clusters, or touched by one of its links or blocks. The shared rows decide the rest.
/// The shared view (`tenant = ''`) decides none of its own.
async fn governed<C: PgClient>(
    client: &C,
    tenant: &str,
    ids: &[String],
) -> Result<BTreeSet<String>, String> {
    if tenant.is_empty() || ids.is_empty() {
        return Ok(BTreeSet::new());
    }
    Ok(client
        .query(
            "SELECT x.id FROM unnest($2::text[]) AS x(id)
              WHERE EXISTS (SELECT 1 FROM view.cluster_member m WHERE m.tenant = $1 AND m.entity_id = x.id)
                 OR EXISTS (SELECT 1 FROM view.link l WHERE l.tenant = $1 AND (l.a = x.id OR l.b = x.id))
                 OR EXISTS (SELECT 1 FROM view.different_from d
                             WHERE d.tenant = $1 AND (d.a = x.id OR d.b = x.id))",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// `ids` split by the view that decides their clusters: the tenant's, then the shared.
async fn by_view<C: PgClient>(
    client: &C,
    tenant: &str,
    ids: &[String],
) -> Result<[(String, Vec<String>); 2], String> {
    let own = governed(client, tenant, ids).await?;
    let (mine, shared): (Vec<String>, Vec<String>) =
        ids.iter().cloned().partition(|id| own.contains(id));
    Ok([(tenant.to_string(), mine), (String::new(), shared)])
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
    let ids = [id.to_string()];
    let view = if governed(client, tenant, &ids).await?.is_empty() {
        ""
    } else {
        tenant
    };
    let rows = client
        .query(
            "SELECT c.cluster_id, c.canonical_id, m2.entity_id FROM view.cluster_member m1
               JOIN view.cluster c ON c.tenant = m1.tenant AND c.cluster_id = m1.cluster_id
               JOIN view.cluster_member m2 ON m2.tenant = m1.tenant AND m2.cluster_id = m1.cluster_id
              WHERE m1.tenant = $1 AND m1.entity_id = $2",
            &[&view, &id],
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
    for (view, ids) in by_view(client, tenant, ids).await? {
        if ids.is_empty() {
            continue;
        }
        let rows = client
            .query(
                "SELECT m1.entity_id, m2.entity_id FROM view.cluster_member m1
                   JOIN view.cluster_member m2 ON m2.tenant = m1.tenant AND m2.cluster_id = m1.cluster_id
                  WHERE m1.tenant = $1 AND m1.entity_id = ANY($2) AND m2.entity_id <> m1.entity_id",
                &[&view, &ids],
            )
            .await
            .map_err(|e| e.to_string())?;
        for r in &rows {
            out.entry(r.get(0)).or_default().push(r.get(1));
        }
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
    let mut out = std::collections::BTreeMap::new();
    for (view, ids) in by_view(client, tenant, ids).await? {
        if ids.is_empty() {
            continue;
        }
        let rows = client
            .query(
                "SELECT m.entity_id, c.canonical_id FROM view.cluster_member m
                   JOIN view.cluster c ON c.tenant = m.tenant AND c.cluster_id = m.cluster_id
                  WHERE m.tenant = $1 AND m.entity_id = ANY($2) AND m.entity_id <> c.canonical_id",
                &[&view, &ids],
            )
            .await
            .map_err(|e| e.to_string())?;
        out.extend(
            rows.iter()
                .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1))),
        );
    }
    Ok(out)
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
    let Some(m) = membership(client, tenant, id).await? else {
        return Ok(Vec::new());
    };
    let mut others: Vec<String> = m.members.into_iter().filter(|x| x != id).collect();
    others.sort_by(|a, b| (*b == m.canonical).cmp(&(*a == m.canonical)).then(a.cmp(b)));
    Ok(others)
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
