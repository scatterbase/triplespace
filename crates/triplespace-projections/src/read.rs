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

/// Reads an entity's current state on a tenant: `None` when no graph holds it.
pub async fn current(
    client: &tokio_postgres::Client,
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
        });
    }
    for s in sources.iter().filter(|s| s.graph.starts_with("mirror/")) {
        if let Some((entity, header)) = EntityProjection::mirror(client, s).await? {
            headers.push(header);
            contributions.push(Contribution {
                graph: s.graph.clone(),
                entity,
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
