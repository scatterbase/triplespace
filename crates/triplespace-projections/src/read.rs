//! Reading an entity back (0012 §2.1; 0013 §5.1): the same contributions the projection
//! resolves, with what a reader needs beside the state — the page ID, the latest revision
//! and its time, which graph asserts each statement, and the local offset a base revision
//! names. The API's `wbgetentities` and every write module read through here.

use std::collections::BTreeMap;

use scatter_log::header::Header;
use scatter_wikibase_changeset::Correction as LocalCorrection;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::id::StatementId;
use scatter_wikibase_resolve::{Contribution, Correction, Resolved, resolve};

use crate::common::Partitions;
use crate::entities::EntityProjection;

/// An entity as it stands.
#[derive(Debug, Clone)]
pub struct Current {
    /// The resolved view, with per-statement provenance.
    pub resolved: Resolved,
    /// The contributions, local first then mirrors in provider order.
    pub contributions: Vec<Contribution>,
    /// The local corrections (rank overrides and suppressions).
    pub corrections: BTreeMap<StatementId, Correction>,
    /// The page ID (0013 §6).
    pub page_id: u64,
    /// The latest revision ID: the local graph's where it has one, else the newest
    /// mirror record's (provider-ranged, 0018 §2).
    pub lastrevid: Option<u64>,
    /// When the latest record was appended, µs since the epoch.
    pub modified: u64,
    /// The latest local record's offset: what a base revision stands for (0006 §8).
    pub local_offset: Option<u64>,
    /// The latest local record's revision ID.
    pub local_revid: Option<u64>,
}

impl Current {
    /// Whether the local graph asserts the statement with this GUID.
    #[must_use]
    pub fn is_local_statement(&self, id: &StatementId) -> bool {
        self.resolved
            .statements
            .iter()
            .any(|s| s.members.contains(id) && s.graphs.iter().any(|g| g == "local"))
    }

    /// The local graph's own state, if it contributes.
    #[must_use]
    pub fn local(&self) -> Option<&scatter_wikibase_model::entity::Entity> {
        self.contributions
            .iter()
            .find(|c| c.graph == "local")
            .map(|c| &c.entity)
    }
}

impl Current {
    /// The statements of the resolved view that say exactly what an upstream says, which RDF
    /// names as the upstream names them (05 §1.5; 0002 A43): every member asserted by a
    /// mirror or entity source graph, and no local correction on any member. A statement the
    /// local graph or a derived graph asserts, or whose rank or membership a correction
    /// changes, is the instance's. Each is given with the entity its upstream names it
    /// after: the subject of its ID, as a foreign ID, or, for a key-mapped provider's
    /// statement under a keyed subject, the item the graph's state was mapped from, which
    /// the contribution carries (0002 A44). A statement fused from two mirrors is the
    /// first member's, in provider order. One whose upstream the registry cannot name (a
    /// keyed subject no item was mapped onto, a source entity) is left out: it is named
    /// under the instance's base.
    #[must_use]
    pub fn upstream_statements(
        &self,
        registry: &scatter_providers::Registry,
    ) -> BTreeMap<StatementId, scatter_providers::UpstreamRef> {
        let mirrors: std::collections::HashSet<String> = registry
            .providers()
            .iter()
            .chain(registry.sources())
            .map(scatter_providers::Provider::mirror_graph)
            .collect();
        let mapped_from = |graph: &str| -> Option<scatter_providers::UpstreamRef> {
            let item = self
                .contributions
                .iter()
                .find(|c| c.graph == graph)?
                .upstream_id
                .as_deref()?;
            let provider = registry
                .providers()
                .iter()
                .find(|p| p.mirror_graph() == graph)?;
            scatter_providers::UpstreamRef::mapped(provider, item)
        };
        self.resolved
            .entity
            .all_statements()
            .zip(&self.resolved.statements)
            .filter(|(_, prov)| {
                !prov.graphs.is_empty()
                    && prov.graphs.iter().all(|g| mirrors.contains(g))
                    && !prov
                        .members
                        .iter()
                        .any(|m| self.corrections.contains_key(m))
            })
            .filter_map(|(s, prov)| {
                let id = s.id.clone()?;
                let subject = id.entity_id()?;
                let upstream = match subject.form() {
                    scatter_wikibase_model::id::IdForm::Foreign => registry
                        .parse_foreign_id(subject.as_str())
                        .ok()
                        .map(|f| scatter_providers::UpstreamRef::of(&f))?,
                    scatter_wikibase_model::id::IdForm::Keyed => mapped_from(&prov.graphs[0])?,
                    scatter_wikibase_model::id::IdForm::Local => return None,
                };
                Some((id, upstream))
            })
            .collect()
    }
}

/// Adds a keyed entity's derived label and alias (0009 §5) where the resolved state has
/// no `mul` label of its own.
pub fn add_derived_terms(entity: &mut scatter_wikibase_model::entity::Entity) {
    let Some((t, key)) = entity
        .id
        .keyed_parts()
        .map(|(t, k)| (t.to_string(), k.to_string()))
    else {
        return;
    };
    let Some(kt) = scatter_normalize::KeyedRegistry::default_registry().by_name(&t) else {
        return;
    };
    let Some(label) = kt.derived_label(&key) else {
        return;
    };
    if !entity.labels.contains_key("mul") {
        entity.labels.insert("mul".into(), label.clone());
    }
    if label != key {
        let aliases = entity.aliases.entry("mul".into()).or_default();
        if !aliases.contains(&key) {
            aliases.push(key);
        }
    }
}

/// The keyed entity a key-mapped provider's own item ID is an alias of (05 §4.1): `XDQ9`
/// → `domain:example.org` when `view.keyed_map` says the provider wrote `Q9` under that
/// key, for the tenant or the instance. `None` for anything else: a local or keyed ID, a
/// foreign ID of an unregistered provider, or one the map does not hold, which is then
/// an ordinary foreign item or missing.
pub async fn mapped_alias<C: Partitions>(
    client: &C,
    registry: &scatter_providers::Registry,
    tenant: &str,
    id: &EntityId,
) -> Result<Option<EntityId>, String> {
    if id.form() != scatter_wikibase_model::id::IdForm::Foreign {
        return Ok(None);
    }
    let Ok(foreign) = registry.parse_foreign_id(id.as_str()) else {
        return Ok(None);
    };
    let row = client
        .query_opt(
            "SELECT keyed_type, key FROM view.keyed_map
             WHERE provider = $1 AND upstream_id = $2 AND (tenant = $3 OR tenant = '')
             ORDER BY tenant DESC LIMIT 1",
            &[
                &foreign.provider().slug.as_str(),
                &foreign.upstream_id().as_str(),
                &tenant,
            ],
        )
        .await
        .map_err(|e| e.to_string())?;
    row.map(|r| {
        let keyed_type: String = r.get(0);
        let key: String = r.get(1);
        EntityId::parse(&format!("{keyed_type}:{key}")).map_err(|e| e.to_string())
    })
    .transpose()
}

/// Reads an entity's current state on a tenant: `None` when no graph holds it.
pub async fn current<C: Partitions>(
    client: &C,
    order: &EntityProjection,
    tenant: &str,
    id: &EntityId,
) -> Result<Option<Current>, String> {
    let mut sources = EntityProjection::sources(client, tenant, id).await?;
    if sources.is_empty() {
        return Ok(None);
    }
    sources.sort_by_key(|s| {
        (
            s.graph != "local",
            order.mirror_rank(&s.graph),
            s.graph.clone(),
        )
    });
    let has_local = !tenant.is_empty()
        && sources
            .iter()
            .any(|s| s.graph == "local" && s.tenant == tenant);
    let local = if has_local {
        EntityProjection::local(client, tenant, id).await?
    } else {
        crate::entities::Local::default()
    };
    let mut contributions = Vec::new();
    let mut headers: Vec<Header> = local.latest.iter().cloned().collect();
    if let Some(e) = &local.state.entity {
        contributions.push(Contribution {
            graph: "local".to_string(),
            entity: e.clone(),
            upstream_id: None,
        });
    }
    for s in sources
        .iter()
        .filter(|s| s.graph.starts_with("mirror/") || s.graph.starts_with("source/"))
    {
        if let Some((entity, header, upstream_id)) =
            EntityProjection::mirror(client, s, None).await?
        {
            headers.push(header);
            contributions.push(Contribution {
                graph: s.graph.clone(),
                entity,
                upstream_id,
            });
        }
    }
    let corrections: BTreeMap<StatementId, Correction> = local
        .state
        .corrections
        .iter()
        .map(|(k, c)| {
            (
                k.clone(),
                match c {
                    LocalCorrection::Rank(r) => Correction::Rank(*r),
                    LocalCorrection::Suppress => Correction::Suppress,
                },
            )
        })
        .collect();
    let Some(resolved) = resolve(&contributions, &corrections) else {
        return Ok(None);
    };
    let page_id = headers
        .iter()
        .filter_map(|h| h.page_id)
        .max()
        .ok_or_else(|| format!("no record for `{id}` carries a page ID (0013 §6)"))?;
    let local_revid = local.latest.as_ref().and_then(|h| h.revid);
    let newest = headers.iter().max_by_key(|h| h.appended_at);
    Ok(Some(Current {
        lastrevid: local_revid.or_else(|| newest.and_then(|h| h.revid)),
        modified: newest.map_or(0, |h| h.appended_at),
        local_offset: local.latest.as_ref().map(|h| h.offset),
        local_revid,
        page_id,
        resolved,
        contributions,
        corrections,
    }))
}

/// One graph's cursor for an entity (0002 §8.4), as the provenance response shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphState {
    /// The graph: `local`, `mirror/wikidata`, …
    pub graph: String,
    /// When the graph's current record for the entity was appended or synced.
    pub synced_at: std::time::SystemTime,
    /// The upstream version that record carries (Wikidata's `lastrevid`), for a mirror.
    pub upstream_version: Option<String>,
    /// The job that wrote it, if a job did.
    pub job_id: Option<i64>,
    /// The graph's history policy, `full` or `latest` (0002 §2), from its registry entry.
    pub history: Option<String>,
}

/// What the serving tables say about an entity beside its state (0012 §5, provenance).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Standing {
    /// When the instance first held the entity.
    pub first_seen: Option<std::time::SystemTime>,
    /// The retention policy (0002 §5), where one is set.
    pub retention: Option<String>,
    /// The canonical ID of its identity cluster (0004 §4); itself when it is in none.
    pub canonical_id: Option<String>,
    /// The cluster, where it is in one.
    pub cluster_id: Option<i64>,
    /// Whether a `read` ACL has deleted it (0023 §4).
    pub deleted: bool,
    /// Each graph that holds a record for it.
    pub graphs: Vec<GraphState>,
}

/// Reads an entity's [`Standing`]: the tenant's own rows over the shared ones.
pub async fn standing<C: Partitions>(
    client: &C,
    tenant: &str,
    id: &EntityId,
) -> Result<Standing, String> {
    let err = |e: tokio_postgres::Error| e.to_string();
    let entity = client
        .query_opt(
            "SELECT first_seen, retention, canonical_id, cluster_id, deleted FROM view.entity
             WHERE id = $1 AND tenant IN ($2, '') ORDER BY tenant DESC LIMIT 1",
            &[&id.as_str(), &tenant],
        )
        .await
        .map_err(err)?;
    let mut out = Standing::default();
    if let Some(r) = entity {
        out.first_seen = r.get(0);
        out.retention = r.get(1);
        out.canonical_id = r.get(2);
        out.cluster_id = r.get(3);
        out.deleted = r.get(4);
    }
    let rows = client
        .query(
            "SELECT s.graph, s.synced_at, s.upstream_version, s.job_id,
                    (SELECT g.config->>'history' FROM view.registry g
                     WHERE g.kind = 'graph' AND g.code = s.graph AND g.tenant IN ($2, '')
                     ORDER BY g.tenant DESC LIMIT 1)
             FROM view.entity_source s
             WHERE s.entity_id = $1 AND s.tenant IN ($2, '')
             ORDER BY s.graph <> 'local', s.graph",
            &[&id.as_str(), &tenant],
        )
        .await
        .map_err(err)?;
    out.graphs = rows
        .iter()
        .map(|r| GraphState {
            graph: r.get(0),
            synced_at: r.get(1),
            upstream_version: r.get(2),
            job_id: r.get(3),
            history: r.get(4),
        })
        .collect();
    Ok(out)
}

/// `YYYY-MM-DDTHH:MM:SSZ` from microseconds since the epoch.
#[must_use]
pub fn iso8601(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}
