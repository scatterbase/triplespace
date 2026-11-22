//! Badges as a tenant reads them (0114; 06 §6): the tenant's badge registry, the
//! instance's with the tenant's `badges.local`, and the `class` badges an entity carries,
//! worked out when it is read and never stored.
//!
//! A `class` badge is on an entity that is an instance of its class: an `instance-of`
//! value that is the class, or reaches it through `subclass-of` values, to
//! `entities.closure_depth` steps over what the tenant's view holds (06 §6.2). The two
//! roles are read in Wikidata's mirrored form (`WDP31`, `WDP279`), as the site reads the
//! statement UI's roles, until the instance's role records are read.

use std::collections::{BTreeMap, BTreeSet};

use scatter_providers::Registry;
use scatter_wikibase_fusion::badges::{BadgeError, BadgeRegistry};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::Rank;
use scatter_wikibase_model::value::DataValue;

use crate::common::Partitions;
use crate::entities::EntityProjection;

/// `entities.closure_depth`'s default (0070 §2.3).
const DEFAULT_DEPTH: u32 = 6;

/// The most class entities one walk reads, so that a wide class tree cannot make a read
/// expensive: past it the walk stops with what it found.
pub const MAX_READS: usize = 200;

/// The value of the `site` setting `code` for `tenant`, else the instance's default for
/// it (23 §3.1).
async fn site_value<C: Partitions>(
    client: &C,
    tenant: &str,
    code: &str,
) -> Result<Option<serde_json::Value>, String> {
    let rows = client
        .query(
            "SELECT tenant, coalesce(config->'record'->'value', config->'value')
             FROM view.registry
             WHERE (tenant = $1 AND $1 <> '' AND kind = 'site' AND code = $2)
                OR (tenant = '' AND kind = 'template' AND code = 'site:' || $2)",
            &[&tenant, &code],
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut instance = None;
    let mut own = None;
    for r in &rows {
        let t: String = r.get(0);
        let v: Option<serde_json::Value> = r.get(1);
        if t.is_empty() {
            instance = v;
        } else {
            own = v;
        }
    }
    Ok(own.or(instance))
}

/// The tenant's badge registry: the instance's with the tenant's `badges.local`
/// (implements 0114 §3 (A1)). A `badges.local` that does not read is reported, and the
/// instance's registry stands.
///
/// # Errors
/// The database's.
pub async fn registry<C: Partitions>(
    client: &C,
    tenant: &str,
) -> Result<Result<BadgeRegistry, BadgeError>, String> {
    let local = site_value(client, tenant, "badges.local").await?;
    let base = BadgeRegistry::default_registry();
    Ok(match local {
        Some(v) => base.with_local(&v),
        None => Ok(base.clone()),
    })
}

/// The classes an entity names through `properties`: item values of statements that are
/// not deprecated.
fn values(entity: &Entity, properties: &[EntityId]) -> Vec<EntityId> {
    properties
        .iter()
        .filter_map(|p| entity.statements.get(p))
        .flatten()
        .filter(|s| s.rank != Rank::Deprecated)
        .filter_map(|s| match s.mainsnak.data_value() {
            Some(DataValue::EntityId(v)) => Some(v.id.clone()),
            _ => None,
        })
        .collect()
}

/// A walk up the class tree, for the `class` badges of one or more entities read in one
/// request: each class's superclasses are read once.
#[derive(Debug)]
pub struct ClassWalk<'a> {
    badges: &'a BadgeRegistry,
    instance_of: Vec<EntityId>,
    subclass_of: Vec<EntityId>,
    depth: u32,
    supers: BTreeMap<EntityId, Vec<EntityId>>,
    reads: usize,
}

impl<'a> ClassWalk<'a> {
    /// A walk for `tenant`, with its `entities.closure_depth`, over the roles in the
    /// mirrored form of the registry's Wikidata provider.
    ///
    /// # Errors
    /// The database's.
    pub async fn new<C: Partitions>(
        client: &C,
        tenant: &str,
        providers: &Registry,
        badges: &'a BadgeRegistry,
    ) -> Result<Self, String> {
        let depth = site_value(client, tenant, "entities.closure_depth")
            .await?
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(DEFAULT_DEPTH);
        let role = |p: &str| {
            providers
                .by_slug("wikidata")
                .and_then(|w| EntityId::parse(&format!("{}{p}", w.code)).ok())
                .into_iter()
                .collect::<Vec<_>>()
        };
        Ok(Self::with_roles(badges, role("P31"), role("P279"), depth))
    }

    /// A walk with the `instance-of` and `subclass-of` properties given.
    #[must_use]
    pub fn with_roles(
        badges: &'a BadgeRegistry,
        instance_of: Vec<EntityId>,
        subclass_of: Vec<EntityId>,
        depth: u32,
    ) -> Self {
        Self {
            badges,
            instance_of,
            subclass_of,
            depth,
            supers: BTreeMap::new(),
            reads: 0,
        }
    }

    /// The `class` badges `entity` carries, as badge items in drawing order (implements
    /// 0114 §2).
    ///
    /// # Errors
    /// The database's.
    pub async fn badges<C: Partitions>(
        &mut self,
        client: &C,
        order: &EntityProjection,
        tenant: &str,
        entity: &Entity,
    ) -> Result<Vec<EntityId>, String> {
        let wanted: BTreeMap<&EntityId, &EntityId> = self
            .badges
            .classes()
            .map(|(b, class)| (class, &b.item))
            .collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let mut found = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut frontier = values(entity, &self.instance_of);
        for step in 0..=self.depth {
            let mut next = Vec::new();
            for class in frontier {
                if !seen.insert(class.clone()) {
                    continue;
                }
                if let Some(item) = wanted.get(&class) {
                    found.insert((*item).clone());
                }
                if step < self.depth {
                    next.extend(self.superclasses(client, order, tenant, &class).await?);
                }
            }
            if next.is_empty() || found.len() == wanted.len() {
                break;
            }
            frontier = next;
        }
        Ok(self
            .badges
            .badges()
            .iter()
            .filter(|b| found.contains(&b.item))
            .map(|b| b.item.clone())
            .collect())
    }

    async fn superclasses<C: Partitions>(
        &mut self,
        client: &C,
        order: &EntityProjection,
        tenant: &str,
        class: &EntityId,
    ) -> Result<Vec<EntityId>, String> {
        if let Some(s) = self.supers.get(class) {
            return Ok(s.clone());
        }
        if self.reads >= MAX_READS {
            return Ok(Vec::new());
        }
        self.reads += 1;
        let supers = match crate::read::current(client, order, tenant, class).await? {
            Some(c) => values(&c.resolved.entity, &self.subclass_of),
            None => Vec::new(),
        };
        self.supers.insert(class.clone(), supers.clone());
        Ok(supers)
    }
}
