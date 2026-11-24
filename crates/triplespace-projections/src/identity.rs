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
//! A tenant's `reconcile:default` record names link properties for its own entity sources,
//! whose links are the tenant's rows, an order among those sources, and may decline inferred
//! links (`"infer": false`); the instance's record sets the provider order, by entity type
//! where `order_by_type` says so (0004 §4, A29).
//!
//! Retired IDs (`view.retired`): an ID a redirect retired, upstream's in a mirror or a
//! local one, is an alias of its target and is resolved to it before clusters are built
//! (0004 §1); an entity upstream deleted leaves its clusters unless the tenant retains it
//! (0004 §4). A mirror-derived link whose statement the tenant deprecates or suppresses is
//! left out of that tenant's clusters (0004 §3).
//!
//! Inferred links (tier 3) are worked out by [`crate::inference`], from the identifier
//! values of a shared mirror's entities.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use scatter_identity::{Link as IdLink, compute, stable_id};
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log_postgres::PgClient;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_wikibase_changeset::rank::ProviderOrder;
use scatter_wikibase_changeset::{Link, Operation, Retention};
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

/// Ranks two IDs by the default order: keyed, local, the providers, then sources.
fn order(a: &String, b: &String) -> Ordering {
    match (EntityId::parse(a), EntityId::parse(b)) {
        (Ok(x), Ok(y)) => ProviderOrder::default().compare(&x, &y),
        _ => a.cmp(b),
    }
}

/// The instance's policy record for clusters, `reconcile:default` (0004 §9, A28, A29;
/// 23 §2.3): the provider order, the tier-2 link properties and the tier-3 inference
/// properties.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// `order`: provider codes, highest-ranked first; empty for the default.
    order: Vec<String>,
    /// `order_by_type`: an order for one entity type (`item`, `property`, …).
    order_by_type: BTreeMap<String, Vec<String>>,
    /// `links`: for each provider code, the properties of its mirror, as stored
    /// (`WDP10283` for Wikidata's `P10283`), whose values name an entity of a target
    /// provider (`W123` as `OAW123`).
    links: BTreeMap<String, Vec<(String, String)>>,
    /// `infer`: the identifier properties, as stored, a shared value of which links two
    /// entities of different namespaces (tier 3).
    infer: Vec<String>,
}

/// A text array field of a record, upper-cased.
fn codes(config: &serde_json::Value, field: &str) -> Vec<String> {
    config
        .get(field)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_ascii_uppercase)
        .collect()
}

impl Policy {
    /// Reads a `reconcile` record's content: `{"order": ["OA", "WD"], "order_by_type":
    /// {"property": ["WD"]}, "links": {"WD": [{"property": "P10283", "target": "OA"}]},
    /// "infer": ["WDP356"]}`. A link entry without both fields is skipped.
    #[must_use]
    pub fn parse(config: &serde_json::Value) -> Self {
        let mut links: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for (code, list) in config
            .get("links")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
        {
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
                links
                    .entry(code.clone())
                    .or_default()
                    .push((stored, target.to_ascii_uppercase()));
            }
        }
        let order_by_type = config
            .get("order_by_type")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
            .map(|(t, _)| (t.to_ascii_lowercase(), codes(&config["order_by_type"], t)))
            .collect();
        Self {
            order: codes(config, "order"),
            order_by_type,
            links,
            infer: codes(config, "infer"),
        }
    }

    /// The instance's, from `view.registry`; the defaults without a record.
    ///
    /// # Errors
    ///
    /// The database's.
    pub async fn load<C: PgClient>(client: &C) -> Result<Self, String> {
        Ok(record(client, "")
            .await?
            .map_or_else(Self::default, |v| Self::parse(&v)))
    }

    /// The inference properties (tier 3), as stored.
    #[must_use]
    pub fn infer(&self) -> &[String] {
        &self.infer
    }

    /// The provider order for an entity type: `order_by_type`'s, else `order`, else the
    /// default of 0004 §4.
    fn provider_order(&self, entity_type: Option<&str>) -> ProviderOrder {
        let codes = entity_type
            .and_then(|t| self.order_by_type.get(t))
            .unwrap_or(&self.order);
        if codes.is_empty() {
            return ProviderOrder::default();
        }
        let codes: Vec<&str> = codes.iter().map(String::as_str).collect();
        ProviderOrder::new(&codes, scatter_providers::Registry::default_registry())
    }

    /// The mirror graphs' provider slugs in the instance's order for an entity type, where
    /// the record sets one: what the entity projection orders an entity's graphs by.
    #[must_use]
    pub fn slugs(&self, entity_type: Option<&str>) -> Option<Vec<String>> {
        let codes = entity_type
            .and_then(|t| self.order_by_type.get(t))
            .unwrap_or(&self.order);
        if codes.is_empty() {
            return None;
        }
        let registry = scatter_providers::Registry::default_registry();
        Some(
            codes
                .iter()
                .filter_map(|c| registry.by_code(c).map(|p| p.slug.clone()))
                .collect(),
        )
    }

    /// The tier-2 links a provider's state of `subject` makes ([`links_of`]).
    #[must_use]
    pub fn links_of(
        &self,
        code: &str,
        subject: &EntityId,
        entity: &scatter_wikibase_model::entity::Entity,
    ) -> Vec<(String, String, String)> {
        self.links
            .get(code)
            .map_or_else(Vec::new, |p| links_of(p, subject, entity))
    }
}

/// A tenant's `reconcile:default` record (0004 §3, §9, A29): it narrows the instance's and
/// configures the tenant's own entity sources, nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantPolicy {
    /// `links`: for each of the tenant's sources, by name in lower case, the properties
    /// of its graph, as stored (`mhc:P5`), whose values name an entity of a target: a
    /// provider code, `local`, or another source's name.
    links: BTreeMap<String, Vec<(String, String)>>,
    /// `order`: the tenant's sources, highest-ranked first, after every provider.
    order: Vec<String>,
    /// `infer`: `false` declines inferred links in the tenant's view.
    infer: bool,
}

impl Default for TenantPolicy {
    fn default() -> Self {
        Self {
            links: BTreeMap::new(),
            order: Vec::new(),
            infer: true,
        }
    }
}

impl TenantPolicy {
    /// Reads a tenant's `reconcile` record: `{"links": {"mhc": [{"property": "P5",
    /// "target": "WD"}]}, "order": ["mhc"], "infer": false}`. What a tenant may not set
    /// ([`Self::refusals`]) is not read.
    #[must_use]
    pub fn parse(config: &serde_json::Value) -> Self {
        let mut links: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for (name, list) in config
            .get("links")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
        {
            let name = name.to_ascii_lowercase();
            for item in list.as_array().into_iter().flatten() {
                let field = |k: &str| item.get(k).and_then(serde_json::Value::as_str);
                let (Some(property), Some(target)) = (field("property"), field("target")) else {
                    continue;
                };
                let stored = if property.contains(':') {
                    property.to_string()
                } else {
                    format!("{name}:{}", property.to_ascii_uppercase())
                };
                links
                    .entry(name.clone())
                    .or_default()
                    .push((stored, target.to_string()));
            }
        }
        Self {
            links,
            order: config
                .get("order")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_ascii_lowercase)
                .collect(),
            infer: config.get("infer") != Some(&serde_json::Value::Bool(false)),
        }
    }

    /// A tenant's; the defaults for the shared view and for a tenant without a record.
    ///
    /// # Errors
    ///
    /// The database's.
    pub async fn load<C: PgClient>(client: &C, tenant: &str) -> Result<Self, String> {
        if tenant.is_empty() {
            return Ok(Self::default());
        }
        Ok(record(client, tenant)
            .await?
            .map_or_else(Self::default, |v| Self::parse(&v)))
    }

    /// The tenant's sources in its order, by name in lower case.
    #[must_use]
    pub fn source_order(&self) -> &[String] {
        &self.order
    }

    /// Whether the tenant takes inferred links (tier 3) in its view.
    #[must_use]
    pub fn infers(&self) -> bool {
        self.infer
    }

    /// Why a tenant may not write `config` as its `reconcile` record, each a
    /// `ts-instance-policy` refusal (04 §4.10; 23 §2.3): a tenant narrows the instance's
    /// policy and configures its own entity sources, named in `sources`; it does not
    /// reorder shared providers, add inference properties, or set a shared provider's
    /// link properties. Empty when the record may be written.
    #[must_use]
    pub fn refusals(config: &serde_json::Value, sources: &[&str]) -> Vec<String> {
        let own = |name: &str| sources.iter().any(|s| s.eq_ignore_ascii_case(name));
        let mut out = Vec::new();
        for name in config
            .get("order")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = name.as_str().unwrap_or_default();
            if !own(name) {
                out.push(format!(
                    "`order` names `{name}`, which is not one of the tenant's entity sources: a tenant orders its own sources, not shared providers"
                ));
            }
        }
        if config.get("order_by_type").is_some() {
            out.push(
                "`order_by_type` reorders shared providers, which is the instance's to set".into(),
            );
        }
        for name in config
            .get("links")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
            .map(|(name, _)| name)
        {
            if !own(name) {
                out.push(format!(
                    "`links` names `{name}`, which is not one of the tenant's entity sources: a shared provider's link properties are the instance's to set"
                ));
            }
        }
        match config.get("infer") {
            None | Some(serde_json::Value::Bool(_)) => {}
            Some(_) => out.push(
                "`infer` may only be `false` (or `true`) for a tenant: the inference properties are the instance's to set"
                    .into(),
            ),
        }
        out
    }
}

/// The error code of [`TenantPolicy::refusals`].
pub const INSTANCE_POLICY: &str = "ts-instance-policy";

/// A scope's `reconcile:default` record.
async fn record<C: PgClient>(
    client: &C,
    tenant: &str,
) -> Result<Option<serde_json::Value>, String> {
    Ok(client
        .query_opt(
            "SELECT config FROM view.registry
              WHERE tenant = $1 AND kind = 'reconcile' AND code = 'default'",
            &[&tenant],
        )
        .await
        .map_err(|e| e.to_string())?
        .map(|r| r.get(0)))
}

/// The canonical order of a view (04 §4.4): keyed, local, the providers in the instance's
/// order, by entity type where the record says so, then the tenant's sources in its own.
pub(crate) struct CanonicalOrder {
    policy: Policy,
    sources: Vec<String>,
}

impl CanonicalOrder {
    pub(crate) async fn load<C: PgClient>(client: &C, tenant: &str) -> Result<Self, String> {
        Ok(Self {
            policy: Policy::load(client).await?,
            sources: TenantPolicy::load(client, tenant).await?.order,
        })
    }

    /// Ranks two IDs, the higher-ranked first.
    pub(crate) fn compare(&self, a: &String, b: &String) -> Ordering {
        let (Ok(x), Ok(y)) = (EntityId::parse(a), EntityId::parse(b)) else {
            return a.cmp(b);
        };
        let order = self.policy.provider_order(x.implied_entity_type());
        let (rx, ry) = (order.rank(&x), order.rank(&y));
        let source = |id: &String| {
            let ns = namespace(id);
            self.sources
                .iter()
                .position(|s| *s == ns)
                .unwrap_or(usize::MAX)
        };
        rx.cmp(&ry)
            .then_with(|| {
                // Sources tie in the provider order; the tenant's own order decides.
                if rx.0 == 4 {
                    source(a).cmp(&source(b))
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| a.cmp(b))
    }
}

/// The projection.
#[derive(Debug, Default, Clone, Copy)]
pub struct IdentityProjection;

/// The policies a record is applied under: the instance's, and the record's tenant's.
struct Policies {
    instance: Policy,
    tenant: TenantPolicy,
}

/// What a block's records share: which of its keys are retired, and the inference values
/// of its entities ([`crate::inference::Values`]).
#[derive(Default)]
struct Block {
    retired: BTreeSet<String>,
    values: crate::inference::Values,
}

impl Block {
    /// Reads both for `records` of one partition, in at most two statements.
    async fn load(
        cx: &PgCx,
        info: &crate::common::PartitionInfo,
        records: &[&Record],
        policies: Option<&Policies>,
    ) -> Result<Self, String> {
        let keys: Vec<&str> = records
            .iter()
            .filter_map(|r| r.header().key.as_deref())
            .collect();
        let retired = cx
            .query(
                "SELECT entity_id FROM view.retired WHERE tenant = $1 AND entity_id = ANY($2)",
                &[&info.tenant, &keys],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| r.get(0))
            .collect();
        let infer = policies.map_or(&[][..], |p| p.instance.infer());
        let values = if info.tenant.is_empty() && !infer.is_empty() {
            let mut subjects = Vec::new();
            for record in records {
                if record.header().payload_type != PAYLOAD_CHANGESET {
                    continue;
                }
                if let Some(
                    Operation::Put { id, .. }
                    | Operation::Tombstone { id, .. }
                    | Operation::Redirect { from: id, .. },
                ) = operation(record)?.as_deref()
                {
                    subjects.push(id.as_str().to_string());
                }
            }
            crate::inference::held(cx, &subjects, infer).await?
        } else {
            crate::inference::Values::new()
        };
        Ok(Self { retired, values })
    }
}

impl Policies {
    async fn load(cx: &PgCx, tenant: &str) -> Result<Self, String> {
        Ok(Self {
            instance: Policy::load(cx).await?,
            tenant: TenantPolicy::load(cx, tenant).await?,
        })
    }
}

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
            || (header.payload_type == scatter_log::registry::PAYLOAD_CONFIG
                && header.key.as_deref() == Some("reconcile:default"))
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
            if h.payload_type == scatter_log::registry::PAYLOAD_CONFIG {
                return policy_changed(cx, &info.tenant).await;
            }
            if info.is_mirror() {
                if h.payload_type != PAYLOAD_CHANGESET {
                    return Ok(Applied::rows(0));
                }
                let policies = Policies::load(cx, &info.tenant).await?;
                let mut block = Block::load(cx, &info, &[record], Some(&policies)).await?;
                return mirror_record(cx, &info, record, &policies, &mut block).await;
            }
            if info.tenant.is_empty() {
                return Ok(Applied::rows(0));
            }
            let mut block = Block::load(cx, &info, &[record], None).await?;
            local_record(cx, &info.tenant, record, &mut block).await
        })
    }

    /// A block reads the policy records once, and which of its keys are retired once, so
    /// that a record that names no link and no retired ID costs nothing more.
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
            if !info.is_mirror() && info.tenant.is_empty() {
                for record in records {
                    out.rows += <Self as Projection<B>>::apply(self, cx, record).await?.rows;
                }
                return Ok(out);
            }
            // The block's keys that are retired as it begins; a record that retires one
            // adds it, so that a later whole state of the same key brings it back.
            let policies = if info.is_mirror() {
                Some(Policies::load(cx, &info.tenant).await?)
            } else {
                None
            };
            let all: Vec<&Record> = records.iter().collect();
            let mut block = Block::load(cx, &info, &all, policies.as_ref()).await?;
            for record in records {
                let h = record.header();
                let applied = if h.payload_type == scatter_log::registry::PAYLOAD_CONFIG {
                    policy_changed(cx, &info.tenant).await?
                } else if let Some(policies) = &policies {
                    if h.payload_type == PAYLOAD_CHANGESET {
                        mirror_record(cx, &info, record, policies, &mut block).await?
                    } else {
                        Applied::rows(0)
                    }
                } else {
                    local_record(cx, &info.tenant, record, &mut block).await?
                };
                out.rows += applied.rows;
            }
            Ok(out)
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let partition = scatter_log_postgres::ids::partition_to_db(partition);
            let mut touched: Vec<(String, String)> = Vec::new();
            for (table, ends) in [
                ("view.link", "a, b"),
                ("view.different_from", "a, b"),
                ("view.retired", "entity_id, COALESCE(target, entity_id)"),
            ] {
                let rows = cx
                    .query(
                        &format!(
                            "DELETE FROM {table} WHERE \"partition\" = $1 RETURNING tenant, {ends}"
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

/// A record of a tenant's own graph: a link, a block, their removal, a redirect, a
/// correction of a mirrored statement a link comes from, an ACL that deletes or undeletes
/// an entity, or an erased record.
#[allow(clippy::too_many_lines)]
async fn local_record(
    cx: &PgCx,
    tenant: &str,
    record: &Record,
    block: &mut Block,
) -> Result<Applied, String> {
    let h = record.header();
    if h.payload_type == crate::actors::payload::ACL {
        return deletion(cx, tenant, h.key.as_deref()).await;
    }
    let partition = scatter_log_postgres::ids::partition_to_db(h.partition);
    let offset = offset_db(record)?;
    let Some(op) = operation(record)? else {
        // Erased: what the record wrote goes with it.
        let mut seeds = Vec::new();
        for (table, ends) in [
            ("view.link", "a, b"),
            ("view.different_from", "a, b"),
            ("view.retired", "entity_id, COALESCE(target, entity_id)"),
        ] {
            let rows = cx
                .query(
                    &format!(
                        "DELETE FROM {table} WHERE tenant = $1 AND \"partition\" = $2
                           AND \"offset\" = $3 RETURNING {ends}"
                    ),
                    &[&tenant, &partition, &offset],
                )
                .await
                .map_err(|e| sql(&e))?;
            seeds.extend(rows.iter().flat_map(|r| [r.get::<_, String>(0), r.get(1)]));
        }
        return changed(cx, tenant, seeds).await;
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
        Operation::Convert { id, local } => (id.as_str().to_string(), local.as_str().to_string()),
        // A local merge, or an adopted one: the retired ID is an alias of its target
        // (04 §4.1), and what was linked to it is linked to the target.
        Operation::Redirect {
            from,
            to,
            upstream: None,
        }
        | Operation::Adopt {
            id: from,
            redirect: Some(to),
            ..
        } => {
            retire(cx, tenant, from.as_str(), Some(to.as_str()), record).await?;
            block.retired.insert(from.as_str().to_string());
            let seeds = vec![from.as_str().to_string(), to.as_str().to_string()];
            return if linked(cx, tenant, &seeds).await? {
                changed(cx, tenant, seeds).await
            } else {
                Ok(Applied::rows(1))
            };
        }
        // A whole state of an ID a redirect had retired: it stands for itself again.
        Operation::Adopt {
            id,
            entity: Some(_),
            ..
        } => {
            if !block.retired.remove(id.as_str()) {
                return Ok(Applied::rows(0));
            }
            let back = unretire(cx, tenant, id.as_str()).await?;
            return if back.is_empty() {
                Ok(Applied::rows(0))
            } else {
                changed(cx, tenant, back).await
            };
        }
        // A correction of a mirrored statement a shared link comes from: while the tenant
        // deprecates or suppresses it, the tenant declines the link, a row of its own
        // beside the shared one (04 §4.3).
        Operation::Override { statement, .. } => {
            return correction(cx, tenant, statement.as_str(), record).await;
        }
        _ => return Ok(Applied::rows(0)),
    };
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    let source = offset.to_string();
    let result = match op.as_ref() {
        Operation::SameAs { .. } | Operation::Convert { .. } | Operation::EquivalentProperty { .. } => {
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
    // The entity projection resolves what a link record reaches.
    recompute(cx, tenant, vec![a, b]).await
}

/// Marks `id` retired on `tenant` (`''` for a shared mirror): an alias of `target`, or with
/// none, deleted upstream.
async fn retire(
    cx: &PgCx,
    tenant: &str,
    id: &str,
    target: Option<&str>,
    record: &Record,
) -> Result<(), String> {
    let partition = scatter_log_postgres::ids::partition_to_db(record.header().partition);
    cx.execute(
        "INSERT INTO view.retired (tenant, entity_id, target, \"partition\", \"offset\")
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (tenant, entity_id) DO UPDATE
           SET target = EXCLUDED.target, \"partition\" = EXCLUDED.\"partition\",
               \"offset\" = EXCLUDED.\"offset\"",
        &[&tenant, &id, &target, &partition, &offset_db(record)?],
    )
    .await
    .map_err(|e| sql(&e))?;
    Ok(())
}

/// Takes `id` out of the retired IDs of `tenant`; the IDs the change reaches (`id` and
/// what it was an alias of), empty when it was not retired.
async fn unretire(cx: &PgCx, tenant: &str, id: &str) -> Result<Vec<String>, String> {
    Ok(cx
        .query(
            "DELETE FROM view.retired WHERE tenant = $1 AND entity_id = $2
             RETURNING entity_id, COALESCE(target, entity_id)",
            &[&tenant, &id],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .flat_map(|r| [r.get::<_, String>(0), r.get(1)])
        .collect())
}

/// Whether any link or cluster of `tenant`'s view touches one of `ids`; for the shared view
/// (`''`), of any view, since every tenant's clusters build on the shared links.
async fn linked(cx: &PgCx, tenant: &str, ids: &[String]) -> Result<bool, String> {
    Ok(!cx
        .query(
            "SELECT 1 FROM view.link
              WHERE ($1 = '' OR tenant IN ($1, '')) AND (a = ANY($2) OR b = ANY($2))
             UNION ALL
             SELECT 1 FROM view.cluster_member
              WHERE ($1 = '' OR tenant IN ($1, '')) AND entity_id = ANY($2)
             UNION ALL
             SELECT 1 FROM view.retired
              WHERE ($1 = '' OR tenant IN ($1, '')) AND target = ANY($2)
             LIMIT 1",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| sql(&e))?
        .is_empty())
}

/// A change around `seeds` in a view: a tenant's clusters are recomputed and the entities
/// reached resolved again; the shared view's go through [`shared_change`].
async fn changed(cx: &PgCx, tenant: &str, seeds: Vec<String>) -> Result<Applied, String> {
    if seeds.is_empty() {
        return Ok(Applied::rows(0));
    }
    if tenant.is_empty() {
        return shared_change(cx, seeds, true).await;
    }
    let reached = component(cx, tenant, seeds.clone()).await?;
    let mut applied = recompute(cx, tenant, seeds).await?;
    let entities = crate::entities::EntityProjection::default();
    for id in reached {
        if let Ok(id) = EntityId::parse(&id) {
            applied.rows += usize::try_from(entities.resolve_subject(cx, tenant, &id, None).await?)
                .unwrap_or(0);
        }
    }
    Ok(applied)
}

/// The tier-2 links a state of `subject` makes under `properties`, each a stored property
/// and its target: `(a, b, statement)`, `a` the lesser ID. The target is a provider code
/// (`W123` as `OAW123`), `local` (the value is a local ID) or an entity source's name
/// (`Q1` as `mhc:Q1`). A deprecated statement makes none, and neither does a value that is
/// no ID of the target, nor one that would put a keyword or a notation into a cluster
/// (04 §4.1).
#[must_use]
pub fn links_of(
    properties: &[(String, String)],
    subject: &EntityId,
    entity: &scatter_wikibase_model::entity::Entity,
) -> Vec<(String, String, String)> {
    use scatter_wikibase_model::statement::{Rank, SnakKind};
    use scatter_wikibase_model::value::DataValue;
    let clusters = |id: &EntityId| {
        id.keyed_parts().is_none_or(|(name, _)| {
            scatter_keyed::KeyedRegistry::default_registry()
                .by_name(name)
                .is_none_or(|t| t.clusters)
        })
    };
    let mut out = Vec::new();
    if !clusters(subject) {
        return out;
    }
    let registry = scatter_providers::Registry::default_registry();
    for statement in entity.all_statements() {
        if statement.rank == Rank::Deprecated {
            continue;
        }
        let property = statement.mainsnak.property.as_str();
        let SnakKind::Value(DataValue::String(value)) = &statement.mainsnak.kind else {
            continue;
        };
        let value = value.trim();
        for (p, target) in properties {
            if !p.eq_ignore_ascii_case(property) {
                continue;
            }
            let (text, expected) = if target.eq_ignore_ascii_case("local") {
                (value.to_string(), "local".to_string())
            } else if registry.by_code(&target.to_ascii_uppercase()).is_some() {
                let code = target.to_ascii_uppercase();
                (format!("{code}{value}"), code)
            } else {
                (format!("{target}:{value}"), target.to_ascii_lowercase())
            };
            let Ok(other) = EntityId::parse(&text) else {
                continue;
            };
            // The value is an ID of the target as its own grammar has it.
            let well_formed = match other.form() {
                IdForm::Foreign => registry.parse_foreign_id(other.as_str()).is_ok(),
                IdForm::Local => other.numeric_id().is_some(),
                IdForm::Keyed => true,
            };
            if !well_formed
                || namespace(other.as_str()) != expected
                || namespace(subject.as_str()) == expected
                || !clusters(&other)
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

/// A record of a mirror or of a tenant's entity source: the tier-2 links of the entity it
/// puts are replaced with those its new state makes (0004 §3, A28), an entity it tombstones
/// or redirects is retired and its links go, and a state of a retired ID brings it back. A
/// shared mirror's record also recomputes the inferred links of its entity's identifier
/// values (tier 3). A shared mirror's rows are shared; a tenant's source's are the tenant's.
#[allow(clippy::too_many_lines)]
async fn mirror_record(
    cx: &PgCx,
    info: &crate::common::PartitionInfo,
    record: &Record,
    policies: &Policies,
    block: &mut Block,
) -> Result<Applied, String> {
    let tenant = info.tenant.as_str();
    let properties: &[(String, String)] = if let Some(name) = info.name.strip_prefix("source/") {
        policies
            .tenant
            .links
            .get(&name.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    } else {
        scatter_providers::Registry::default_registry()
            .providers()
            .iter()
            .find(|p| p.mirror_graph() == info.name)
            .and_then(|p| policies.instance.links.get(&p.code))
            .map_or(&[], Vec::as_slice)
    };
    let infer: &[String] = if tenant.is_empty() {
        policies.instance.infer()
    } else {
        &[]
    };
    let Some(op) = operation(record)? else {
        return Ok(Applied::rows(0));
    };
    let mut seeds: Vec<String> = Vec::new();
    let (subject, new, values) = match op.as_ref() {
        Operation::Put { id, entity, .. } => {
            if block.retired.remove(id.as_str()) {
                seeds.extend(unretire(cx, tenant, id.as_str()).await?);
            }
            if properties.is_empty() && infer.is_empty() && seeds.is_empty() {
                return Ok(Applied::rows(0));
            }
            (
                id.clone(),
                links_of(properties, id, entity),
                crate::inference::values_of(entity, infer),
            )
        }
        Operation::Tombstone { id, .. } => {
            retire(cx, tenant, id.as_str(), None, record).await?;
            block.retired.insert(id.as_str().to_string());
            seeds.push(id.as_str().to_string());
            (id.clone(), Vec::new(), BTreeSet::new())
        }
        Operation::Redirect {
            from,
            to,
            upstream: Some(_),
        } => {
            retire(cx, tenant, from.as_str(), Some(to.as_str()), record).await?;
            block.retired.insert(from.as_str().to_string());
            seeds.extend([from.as_str().to_string(), to.as_str().to_string()]);
            (from.clone(), Vec::new(), BTreeSet::new())
        }
        _ => return Ok(Applied::rows(0)),
    };
    let mut changed_links = false;
    // The entity's mirror-derived links: read where it may have any.
    if !properties.is_empty() || !matches!(op.as_ref(), Operation::Put { .. }) {
        let old: Vec<(String, String, String)> = cx
            .query(
                "SELECT a, b, source FROM view.link
                  WHERE tenant = $1 AND tier = 2 AND subject = $2 AND status <> 'declined'
                  ORDER BY a, b, source",
                &[&tenant, &subject.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect();
        if old != new {
            changed_links = true;
            cx.execute(
                "DELETE FROM view.link WHERE tenant = $1 AND tier = 2 AND subject = $2
                    AND status <> 'declined'",
                &[&tenant, &subject.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
            let partition = scatter_log_postgres::ids::partition_to_db(record.header().partition);
            let offset = offset_db(record)?;
            for (a, b, source) in &new {
                cx.execute(
                    "INSERT INTO view.link (tenant, a, b, tier, source, status, \"partition\", \"offset\", subject)
                     VALUES ($1, $2, $3, 2, $4, 'applied', $5, $6, $7)
                     ON CONFLICT (tenant, a, b, tier, source) DO NOTHING",
                    &[&tenant, a, b, source, &partition, &offset, &subject.as_str()],
                )
                .await
                .map_err(|e| sql(&e))?;
            }
            seeds.extend(
                old.iter()
                    .chain(&new)
                    .flat_map(|(a, b, _)| [a.clone(), b.clone()]),
            );
        }
    }
    if !infer.is_empty() {
        let inferred =
            crate::inference::relink(cx, subject.as_str(), values, &mut block.values, record)
                .await?;
        changed_links |= !inferred.is_empty();
        seeds.extend(inferred);
    }
    if seeds.is_empty() {
        return Ok(Applied::rows(0));
    }
    // A retired ID nothing links to changes no cluster.
    if !changed_links && !linked(cx, tenant, &seeds).await? {
        return Ok(Applied::rows(1));
    }
    changed(cx, tenant, seeds).await
}

/// The tenants that decline inferred links (`"infer": false` in their `reconcile:default`).
async fn declining(cx: &PgCx) -> Result<Vec<String>, String> {
    Ok(cx
        .query(
            "SELECT tenant FROM view.registry
              WHERE tenant <> '' AND kind = 'reconcile' AND code = 'default'
                AND config->'infer' = 'false'::jsonb",
            &[],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// A change to the shared links around `seeds`: the shared clusters are recomputed, then
/// every tenant's whose links, blocks or clusters reach them, and with `resolve`, every
/// entity reached is resolved again, shared and in each tenant that holds a row of it. A
/// tenant that declines inferred links has its own clusters, and its own rows of the
/// entities, wherever an inferred link reaches (0004 A29).
async fn shared_change(cx: &PgCx, seeds: Vec<String>, resolve: bool) -> Result<Applied, String> {
    let before = component(cx, "", seeds.clone()).await?;
    let mut rows = recompute(cx, "", seeds.clone()).await?.rows;
    let mut reached = component(cx, "", seeds).await?;
    reached.extend(before);
    let list: Vec<String> = reached.iter().cloned().collect();
    let mut tenants: BTreeSet<String> = cx
        .query(
            "SELECT tenant FROM view.link WHERE tenant <> '' AND (a = ANY($1) OR b = ANY($1))
             UNION SELECT tenant FROM view.cluster_member WHERE tenant <> '' AND entity_id = ANY($1)
             UNION SELECT tenant FROM view.different_from WHERE tenant <> '' AND (a = ANY($1) OR b = ANY($1))
             UNION SELECT tenant FROM view.retired WHERE tenant <> '' AND (entity_id = ANY($1) OR target = ANY($1))",
            &[&list],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let inferred = !cx
        .query(
            "SELECT 1 FROM view.link WHERE tenant = '' AND tier = 3
                AND (a = ANY($1) OR b = ANY($1)) LIMIT 1",
            &[&list],
        )
        .await
        .map_err(|e| sql(&e))?
        .is_empty();
    let decliners: BTreeSet<String> = if inferred || !resolve {
        declining(cx).await?.into_iter().collect()
    } else {
        BTreeSet::new()
    };
    tenants.extend(decliners.iter().cloned());
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
            let mut holders: BTreeSet<String> =
                crate::entities::EntityProjection::overlay_tenants(cx, &id)
                    .await?
                    .into_iter()
                    .collect();
            holders.extend(decliners.iter().cloned());
            for t in holders {
                let n = entities.resolve_subject(cx, &t, &id, None).await?;
                rows += usize::try_from(n).unwrap_or(0);
            }
        }
    }
    Ok(Applied::rows(rows))
}

/// A `reconcile:default` record landed. A tenant's may have turned inferred links off or
/// on for itself: its view of every shared cluster an inferred link joins is recomputed,
/// and the members resolved again in it. The instance's record applies to each entity's
/// next state, or to all at a rebuild (0004 A28).
async fn policy_changed(cx: &PgCx, tenant: &str) -> Result<Applied, String> {
    if tenant.is_empty() {
        return Ok(Applied::rows(0));
    }
    let rows = cx
        .query(
            "SELECT m.cluster_id, m.entity_id FROM view.cluster c
               JOIN view.cluster_member m ON m.tenant = '' AND m.cluster_id = c.cluster_id
              WHERE c.tenant = '' AND c.inferred ORDER BY m.cluster_id",
            &[],
        )
        .await
        .map_err(|e| sql(&e))?;
    let mut clusters: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for r in &rows {
        clusters.entry(r.get(0)).or_default().push(r.get(1));
    }
    let mut applied = Applied::rows(0);
    for members in clusters.into_values() {
        applied.rows += changed(cx, tenant, members).await?.rows;
    }
    Ok(applied)
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
/// leave them apart), and those the links and redirects reach from there, breadth first. An
/// alias and its target are connected: a link to one is a link to the other (04 §4.1).
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
                  WHERE tenant IN ($1, '') AND (a = ANY($2) OR b = ANY($2))
                 UNION
                 SELECT entity_id, target FROM view.retired
                  WHERE tenant IN ($1, '') AND target IS NOT NULL
                    AND (entity_id = ANY($2) OR target = ANY($2))",
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
    changed(cx, tenant, vec![id]).await
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

/// The retired IDs among `ids` in a view (a tenant's row before a shared one): each alias
/// with its target, and the IDs upstream deleted.
async fn retired_among<C: PgClient>(
    client: &C,
    tenant: &str,
    ids: &[String],
) -> Result<(BTreeMap<String, String>, BTreeSet<String>), String> {
    let rows = client
        .query(
            "SELECT DISTINCT ON (entity_id) entity_id, target FROM view.retired
              WHERE tenant IN ($1, '') AND entity_id = ANY($2)
              ORDER BY entity_id, tenant DESC",
            &[&tenant, &ids],
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut aliases = BTreeMap::new();
    let mut gone = BTreeSet::new();
    for r in &rows {
        let id: String = r.get(0);
        match r.get::<_, Option<String>>(1) {
            Some(target) => {
                aliases.insert(id, target);
            }
            None => {
                gone.insert(id);
            }
        }
    }
    Ok((aliases, gone))
}

/// The ID an alias stands for, through a chain of redirects; `id` itself when it is none.
fn target_of<'a>(aliases: &'a BTreeMap<String, String>, id: &'a str) -> &'a str {
    let mut at = id;
    for _ in 0..8 {
        match aliases.get(at) {
            Some(next) if next != at => at = next,
            _ => break,
        }
    }
    at
}

/// Which of the entities upstream deleted (`gone`) a tenant keeps (04 §4.4; 05 §2.1): one
/// its local graph sets `retain` on, or any without a policy of its own when the tenant's
/// `retention.default` is `retain`. The shared view keeps none.
async fn retained(
    cx: &PgCx,
    tenant: &str,
    gone: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    if tenant.is_empty() || gone.is_empty() {
        return Ok(out);
    }
    let default_retain = cx
        .query_opt(
            "SELECT COALESCE(
               (SELECT config->>'value' FROM view.registry
                 WHERE tenant = $1 AND kind = 'site' AND code = 'retention.default'),
               (SELECT coalesce(config->'record'->>'value', config->>'value') FROM view.registry
                 WHERE tenant = '' AND kind = 'template' AND code = 'site:retention.default'))",
            &[&tenant],
        )
        .await
        .map_err(|e| sql(&e))?
        .and_then(|r| r.get::<_, Option<String>>(0))
        .is_some_and(|v| v == "retain");
    for id in gone {
        let policy = crate::revision_state::graph_state(cx, tenant, id, "local")
            .await?
            .and_then(|(state, _, _)| state.retention);
        if policy == Some(Retention::Retain) || (policy.is_none() && default_retain) {
            out.insert(id.clone());
        }
    }
    Ok(out)
}

/// A tenant's correction of `statement` landed. Where a shared tier-2 link comes from the
/// statement, the tenant declines the link while its local graph deprecates or suppresses
/// the statement (04 §4.3): a `declined` row of the tenant's own with the link's key, which
/// leaves the link out of the tenant's clusters and makes them the tenant's to decide.
async fn correction(
    cx: &PgCx,
    tenant: &str,
    statement: &str,
    record: &Record,
) -> Result<Applied, String> {
    use scatter_wikibase_changeset::apply::Correction;
    use scatter_wikibase_model::statement::Rank;
    let links = cx
        .query(
            "SELECT a, b, source, subject FROM view.link
              WHERE tenant = '' AND tier = 2 AND lower(source) = lower($1) AND subject IS NOT NULL",
            &[&statement],
        )
        .await
        .map_err(|e| sql(&e))?;
    let mut seeds = Vec::new();
    for r in &links {
        let (a, b, source, subject): (String, String, String, String) =
            (r.get(0), r.get(1), r.get(2), r.get(3));
        let declined = crate::revision_state::graph_state(cx, tenant, &subject, "local")
            .await?
            .is_some_and(|(state, _, _)| {
                state.corrections.iter().any(|(id, c)| {
                    id.as_str().eq_ignore_ascii_case(&source)
                        && matches!(c, Correction::Suppress | Correction::Rank(Rank::Deprecated))
                })
            });
        if declined {
            let partition = scatter_log_postgres::ids::partition_to_db(record.header().partition);
            cx.execute(
                "INSERT INTO view.link (tenant, a, b, tier, source, status, \"partition\", \"offset\", subject)
                 VALUES ($1, $2, $3, 2, $4, 'declined', $5, $6, $7)
                 ON CONFLICT (tenant, a, b, tier, source) DO NOTHING",
                &[&tenant, &a, &b, &source, &partition, &offset_db(record)?, &subject],
            )
            .await
            .map_err(|e| sql(&e))?;
        } else {
            cx.execute(
                "DELETE FROM view.link WHERE tenant = $1 AND a = $2 AND b = $3 AND tier = 2
                    AND source = $4 AND status = 'declined'",
                &[&tenant, &a, &b, &source],
            )
            .await
            .map_err(|e| sql(&e))?;
        }
        seeds.extend([a, b]);
    }
    changed(cx, tenant, seeds).await
}

/// Writes each link's status: held in its conflict, `inactive` where it touches an entity
/// that has left (deleted, or deleted upstream and not retained), applied otherwise.
async fn mark_links(
    cx: &PgCx,
    tenant: &str,
    list: &[String],
    left: Vec<String>,
    conflicts: &[scatter_identity::Conflict<String>],
    link_rows: &[tokio_postgres::Row],
) -> Result<usize, String> {
    let mut n = 0usize;
    cx.execute(
        "UPDATE view.link SET status = 'applied', conflict_id = NULL
          WHERE tenant = $1 AND a = ANY($2) AND b = ANY($2)
            AND status NOT IN ('applied', 'declined')",
        &[&tenant, &list],
    )
    .await
    .map_err(|e| sql(&e))?;
    cx.execute(
        "UPDATE view.link SET status = 'inactive', conflict_id = NULL
          WHERE tenant = $1 AND (a = ANY($2) OR b = ANY($2)) AND status <> 'declined'",
        &[&tenant, &left],
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
/// to them, resolves aliases to their targets, leaves out the entities that have left and
/// the links the tenant does not take, works out the clusters, and writes them and each
/// link's status.
#[allow(clippy::too_many_lines)]
async fn recompute(cx: &PgCx, tenant: &str, seeds: Vec<String>) -> Result<Applied, String> {
    let nodes = component(cx, tenant, seeds).await?;
    if nodes.is_empty() {
        return Ok(Applied::rows(0));
    }
    let list: Vec<String> = nodes.iter().cloned().collect();
    let (aliases, gone) = retired_among(cx, tenant, &list).await?;
    // A deleted member leaves its cluster, and so does one upstream deleted that the
    // tenant does not retain: its links are kept, and not applied meanwhile (04 §4.4).
    let mut left = deleted_among(cx, tenant, &list).await?;
    let kept = retained(cx, tenant, &gone).await?;
    left.extend(gone.into_iter().filter(|id| !kept.contains(id)));
    // An alias has left with its target.
    let left_raw: Vec<String> = list
        .iter()
        .filter(|id| left.contains(target_of(&aliases, id)))
        .cloned()
        .collect();
    let no_inference = !TenantPolicy::load(cx, tenant).await?.infer;
    let all_rows = cx
        .query(
            "SELECT a, b, tier, source, tenant, status FROM view.link
              WHERE tenant IN ($1, '') AND a = ANY($2) AND b = ANY($2)
              ORDER BY a, b, tier, source, tenant",
            &[&tenant, &list],
        )
        .await
        .map_err(|e| sql(&e))?;
    // The shared links the tenant declines: its own `declined` row with the link's key.
    let key = |r: &tokio_postgres::Row| -> (String, String, i16, String) {
        (r.get(0), r.get(1), r.get(2), r.get(3))
    };
    let declined: BTreeSet<(String, String, i16, String)> = all_rows
        .iter()
        .filter(|r| r.get::<_, String>(5) == "declined")
        .map(key)
        .collect();
    let link_rows: Vec<tokio_postgres::Row> = all_rows
        .into_iter()
        .filter(|r| {
            let skipped = declined.contains(&key(r))
                || (no_inference && r.get::<_, i16>(2) >= 3)
                || left.contains(target_of(&aliases, &r.get::<_, String>(0)))
                || left.contains(target_of(&aliases, &r.get::<_, String>(1)));
            !skipped
        })
        .collect();
    let links: Vec<IdLink<String>> = link_rows
        .iter()
        .map(|r| IdLink {
            a: target_of(&aliases, &r.get::<_, String>(0)).to_string(),
            b: target_of(&aliases, &r.get::<_, String>(1)).to_string(),
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
        .map(|r| {
            (
                target_of(&aliases, &r.get::<_, String>(0)).to_string(),
                target_of(&aliases, &r.get::<_, String>(1)).to_string(),
            )
        })
        .collect();
    let canonical = CanonicalOrder::load(cx, tenant).await?;
    let out = compute(
        &links,
        &blocks,
        |id| namespace(id),
        |a, b| canonical.compare(a, b),
    );

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
        // Whether an inferred link, applied, joins members of the cluster.
        let inferred = links
            .iter()
            .enumerate()
            .any(|(i, l)| l.tier >= 3 && out.held(i).is_none() && members.contains(&l.a));
        cx.execute(
            "INSERT INTO view.cluster (tenant, cluster_id, canonical_id, version, inferred)
             VALUES ($1, $2, $3, COALESCE((SELECT max(version) FROM view.cluster WHERE tenant = $1), 0) + 1, $4)",
            &[&tenant, &id, canonical, &inferred],
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
    rows += mark_links(cx, tenant, &list, left_raw, &out.conflicts, &link_rows).await?;
    Ok(Applied::rows(rows))
}

/// Which of `ids` a tenant's own cluster rows decide (0004 A28): those in one of its
/// clusters, or touched by one of its links, blocks or retired IDs, and, for a tenant that
/// declines inferred links, those in a shared cluster an inferred link joins (A29). The
/// shared rows decide the rest. The shared view (`tenant = ''`) decides none of its own.
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
                             WHERE d.tenant = $1 AND (d.a = x.id OR d.b = x.id))
                 OR EXISTS (SELECT 1 FROM view.retired t
                             WHERE t.tenant = $1 AND (t.entity_id = x.id OR t.target = x.id))
                 OR (EXISTS (SELECT 1 FROM view.cluster_member m
                               JOIN view.cluster c ON c.tenant = '' AND c.cluster_id = m.cluster_id
                              WHERE m.tenant = '' AND m.entity_id = x.id AND c.inferred)
                     AND EXISTS (SELECT 1 FROM view.registry r
                                  WHERE r.tenant = $1 AND r.kind = 'reconcile' AND r.code = 'default'
                                    AND r.config->'infer' = 'false'::jsonb))",
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
    let canonical: String = first.get(1);
    let mut members: Vec<String> = rows.iter().map(|r| r.get(2)).collect();
    // The canonical member is the view's, by the policy's order; the rest by the default.
    members.sort_by(|a, b| {
        (*b == canonical)
            .cmp(&(*a == canonical))
            .then_with(|| order(a, b))
    });
    Ok(Some(Membership {
        cluster_id: first.get(0),
        canonical,
        members,
    }))
}

/// For each of `ids` that is in a cluster on `tenant`, the members of its cluster that are
/// its exact matches: what a response may name it by (04 §4.12). Every member is, in a
/// cluster of asserted and mirror-derived links; where an inferred link (tier 3) joins the
/// cluster, only the members the links of the higher tiers connect it to, since a shared
/// identifier value is no exact match.
///
/// # Errors
///
/// The database's.
pub async fn exact_matches<C: PgClient>(
    client: &C,
    tenant: &str,
    ids: &[String],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (view, ids) in by_view(client, tenant, ids).await? {
        if ids.is_empty() {
            continue;
        }
        let rows = client
            .query(
                "SELECT m1.entity_id, m2.entity_id, c.inferred FROM view.cluster_member m1
                   JOIN view.cluster c ON c.tenant = m1.tenant AND c.cluster_id = m1.cluster_id
                   JOIN view.cluster_member m2 ON m2.tenant = m1.tenant AND m2.cluster_id = m1.cluster_id
                  WHERE m1.tenant = $1 AND m1.entity_id = ANY($2) AND m2.entity_id <> m1.entity_id",
                &[&view, &ids],
            )
            .await
            .map_err(|e| e.to_string())?;
        let mut inferred: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in &rows {
            if r.get::<_, bool>(2) {
                inferred.entry(r.get(0)).or_default().push(r.get(1));
            } else {
                out.entry(r.get(0)).or_default().push(r.get(1));
            }
        }
        for (id, others) in inferred {
            let exact = exactly_linked(client, &view, &id, &others).await?;
            if !exact.is_empty() {
                out.insert(id, exact);
            }
        }
    }
    Ok(out)
}

/// Which of `others`, the other members of `id`'s cluster in a view, the links of tiers 1
/// and 2 connect `id` to.
async fn exactly_linked<C: PgClient>(
    client: &C,
    view: &str,
    id: &str,
    others: &[String],
) -> Result<Vec<String>, String> {
    let mut members: Vec<String> = others.to_vec();
    members.push(id.to_string());
    let rows = client
        .query(
            "SELECT l.a, l.b FROM view.link l
              WHERE l.tenant IN ($1, '') AND l.tier < 3 AND l.status NOT IN ('inactive', 'declined')
                AND NOT EXISTS (SELECT 1 FROM view.link d WHERE d.tenant = $1 AND d.status = 'declined'
                                 AND d.a = l.a AND d.b = l.b AND d.tier = l.tier AND d.source = l.source)
                AND (l.a = ANY($2) OR l.b = ANY($2)
                     OR EXISTS (SELECT 1 FROM view.retired t WHERE t.tenant IN ($1, '')
                                 AND t.target = ANY($2) AND t.entity_id IN (l.a, l.b)))",
            &[&view, &members],
        )
        .await
        .map_err(|e| e.to_string())?;
    let ends: Vec<String> = rows
        .iter()
        .flat_map(|r| [r.get::<_, String>(0), r.get(1)])
        .collect();
    let (aliases, _) = retired_among(client, view, &ends).await?;
    let mut reached: BTreeSet<&str> = BTreeSet::from([id]);
    loop {
        let before = reached.len();
        for pair in ends.chunks(2) {
            let (a, b) = (target_of(&aliases, &pair[0]), target_of(&aliases, &pair[1]));
            if reached.contains(a) || reached.contains(b) {
                reached.insert(a);
                reached.insert(b);
            }
        }
        if reached.len() == before {
            break;
        }
    }
    Ok(others
        .iter()
        .filter(|o| reached.contains(o.as_str()))
        .cloned()
        .collect())
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
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
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

    // 04 §4.4, §4.10 (0004 §4, A29): the instance's record orders the providers, by entity
    // type where it says so; a tenant's orders its own sources, after every provider.
    #[test]
    fn the_records_set_the_canonical_order() {
        let policy = Policy::parse(&serde_json::json!({
            "order": ["oa", "WD"],
            "order_by_type": {"property": ["WD", "OA"]},
            "infer": ["wdp356"],
        }));
        assert_eq!(policy.infer(), ["WDP356"]);
        assert_eq!(
            policy.slugs(Some("item")).unwrap(),
            ["openalex", "wikidata"]
        );
        let order = CanonicalOrder {
            policy,
            sources: TenantPolicy::parse(&serde_json::json!({"order": ["zeta", "mhc"]})).order,
        };
        let sorted = |ids: &[&str]| {
            let mut v: Vec<String> = ids.iter().map(|s| (*s).to_string()).collect();
            v.sort_by(|a, b| order.compare(a, b));
            v
        };
        assert_eq!(sorted(&["WDQ1", "OAW1", "Q1"]), ["Q1", "OAW1", "WDQ1"]);
        assert_eq!(sorted(&["OAP1", "WDP1"]), ["WDP1", "OAP1"]);
        assert_eq!(
            sorted(&["mhc:Q1", "zeta:Q1", "WDQ1"]),
            ["WDQ1", "zeta:Q1", "mhc:Q1"]
        );
        // Without a record, the default of 0004 §4.
        let order = CanonicalOrder {
            policy: Policy::default(),
            sources: Vec::new(),
        };
        assert_eq!(
            order.compare(&"WDQ1".to_string(), &"OAW1".to_string()),
            Ordering::Less
        );
    }

    // 04 §4.10 (0004 §9, A29): a tenant's record narrows the instance's and configures its
    // own sources; anything else is refused with `ts-instance-policy`.
    #[test]
    fn a_tenant_narrows_and_does_not_reorder() {
        let sources = ["mhc"];
        let ok = serde_json::json!({"kind": "reconcile", "infer": false, "order": ["mhc"],
            "links": {"mhc": [{"property": "P5", "target": "WD"}]}});
        assert!(TenantPolicy::refusals(&ok, &sources).is_empty());
        let policy = TenantPolicy::parse(&ok);
        assert!(!policy.infers());
        assert_eq!(policy.source_order(), ["mhc"]);
        assert_eq!(
            policy.links["mhc"],
            [("mhc:P5".to_string(), "WD".to_string())]
        );
        assert!(TenantPolicy::default().infers());

        for bad in [
            serde_json::json!({"order": ["WD"]}),
            serde_json::json!({"order_by_type": {"item": ["mhc"]}}),
            serde_json::json!({"links": {"WD": [{"property": "P10283", "target": "OA"}]}}),
            serde_json::json!({"infer": ["WDP356"]}),
        ] {
            assert_eq!(TenantPolicy::refusals(&bad, &sources).len(), 1, "{bad}");
        }
    }

    // 04 §4.3 (0004 §3, A28, A29): a link property's value names the target's entity: a
    // provider's by its code, a local one as it is, a source's by its name. A deprecated
    // statement, a value that is no such ID and a keyword make no link.
    #[test]
    fn link_properties_name_their_targets() {
        use scatter_wikibase_model::entity::Entity;
        let statement = |property: &str, value: &str, rank: &str, n: u8| {
            serde_json::json!({"mainsnak": {"snaktype": "value", "property": property,
                "datatype": "external-id", "datavalue": {"value": value, "type": "string"}},
                "type": "statement", "rank": rank,
                "id": format!("mhc:Q1$00000000-0000-0000-0000-00000000000{n}")})
        };
        let entity = Entity::from_value(serde_json::json!({"type": "item", "id": "mhc:Q1", "claims": {
            "mhc:P5": [statement("mhc:P5", "Q42", "normal", 1), statement("mhc:P5", "Q43", "deprecated", 2),
                       statement("mhc:P5", "not an id", "normal", 3)],
            "mhc:P6": [statement("mhc:P6", "Q6", "normal", 4)],
            "mhc:P7": [statement("mhc:P7", "Q9", "normal", 5)]}}))
        .unwrap()
        .entity;
        let subject = EntityId::parse("mhc:Q1").unwrap();
        let policy = TenantPolicy::parse(&serde_json::json!({"links": {"mhc": [
            {"property": "P5", "target": "WD"},
            {"property": "P6", "target": "local"},
            {"property": "P7", "target": "other"}]}}));
        let links: Vec<(String, String)> = links_of(&policy.links["mhc"], &subject, &entity)
            .into_iter()
            .map(|(a, b, _)| (a, b))
            .collect();
        assert_eq!(
            links,
            [
                ("Q6".to_string(), "mhc:Q1".to_string()),
                ("WDQ42".to_string(), "mhc:Q1".to_string()),
                ("mhc:Q1".to_string(), "other:Q9".to_string()),
            ]
        );
    }

    // 04 §4.1: an alias stands for its target, through a chain of redirects.
    #[test]
    fn an_alias_stands_for_its_target() {
        let aliases = BTreeMap::from([
            ("WDQ1".to_string(), "WDQ2".to_string()),
            ("WDQ2".to_string(), "WDQ3".to_string()),
        ]);
        assert_eq!(target_of(&aliases, "WDQ1"), "WDQ3");
        assert_eq!(target_of(&aliases, "WDQ9"), "WDQ9");
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
