//! The SPARQL Update stream: the delta consumer that writes `view.rdf_delta`, and the
//! reads that serve it (02 §6; 03 §4.11; 08 §10.5).
//!
//! **The consumer** ([`DeltaConsumer`]) is a tier-3 consumer of composition
//! (`triplespace_projections::rdf_work` queues its work). For each queued entity it renders
//! the public form of the tenant's `resolved` and `local` graphs exactly as a dump does,
//! compares the canonical lines with what the stream last showed (`view.rdf_shown`), and
//! stores the difference under the next `seq`. It holds one lock for the instance while it
//! writes, so `seq` order is commit order.
//!
//! **Turning a stream on** ([`reconcile`]) makes the tenant's row in `ops.tenant_epoch`
//! and queues a fill: the consumer records what every entity shows, without deltas, and
//! starts a new epoch. A dump taken after that carries the cursor the stream continues
//! from.
//!
//! **Shared rows** are rendered once, under [`RESERVED_BASE`], and stored with the
//! empty-string tenant; a tenant's stream reads them with its own base in place of the
//! reserved one ([`for_tenant`]), except over the spans of `seq` in which the tenant
//! overlays the entity (`view.rdf_overlay`), where its own deltas stand instead.
//!
//! **Not yet:** the `metadata` graph and so `graphs=full`; mirror source graphs; the batch
//! files under `/dumps/updates/`; the erasure purge of stored rows (02 §6.3), which until
//! then is bounded by the retention sweep alone; a tenant's own provider list, every
//! tenant reading every shared provider as its dump does.
//!
//! About 0032 §1–§6 and 0083 §7.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::Triple;
use scatter_actors::{
    Effective, GrantRegistry, Group, GroupRegistry, Principal, ReadDecision, Timestamp, evaluate,
    read_decision,
};
use scatter_log_postgres::PgClient;
use scatter_normalize::KeyedRegistry;
use scatter_projection::{Backend, BoxFuture, Consumer};
use scatter_providers::Registry;
use scatter_wikibase_model::{EntityId, Hasher, PageInfo};
use scatter_wikibase_rdf::{
    Delta, GraphDelta, IriScheme, Namespaces, Produce, Renderer, canonical_lines, diff,
    update_request,
};
use triplespace_projections::acl_read;
use triplespace_projections::backend::PgCx;
use triplespace_projections::common::Partitions;
use triplespace_projections::entities::EntityProjection;
use triplespace_projections::entity_sources::registry_for;
use triplespace_projections::rdf_work::{CONSUMER, Change};
use triplespace_projections::read::{self, add_derived_terms, iso8601};

use crate::{LOCAL, now_micros, skolemize};

/// The base a shared row is rendered under (0032 A16). `.invalid` is reserved (RFC 2606),
/// so no IRI of any instance begins with it; a tenant's stream writes its own base in its
/// place.
pub const RESERVED_BASE: &str = "https://tenant.invalid/";

/// Incremented by any change to how a triple is written, since `DELETE DATA` matches only
/// a byte-identical triple (03 §4.11). It is part of every epoch.
pub const RENDERING_VERSION: i32 = 1;

/// The work key that asks for a tenant's fill.
pub const FILL_KEY: &str = "0:*";

/// The graphs the stream carries today.
pub const GRAPHS: [&str; 2] = ["resolved", "local"];

/// The advisory lock of the one sequencer ("rdfdelta").
const SEQUENCER: i64 = 0x7264_6664_656c_7461;

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn pack(lines: &[String]) -> Vec<u8> {
    lines.join("\n").into_bytes()
}

fn unpack(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Lines rendered under [`RESERVED_BASE`] as the tenant at `base` shows them, in canonical
/// order again, since the base is part of what they sort by.
#[must_use]
pub fn for_tenant(lines: &[String], base: &str) -> Vec<String> {
    let base = if base.ends_with('/') {
        base.to_string()
    } else {
        format!("{base}/")
    };
    let from = format!("<{RESERVED_BASE}");
    let to = format!("<{base}");
    let mut out: Vec<String> = lines.iter().map(|l| l.replace(&from, &to)).collect();
    out.sort_unstable();
    out
}

/// The reference or value node a line is about, if its subject is one: an IRI ending
/// `/reference/{hash}` or `/value/{hash}`, under the instance's base or a provider's.
fn shared_node(line: &str) -> Option<&str> {
    let subject = line.strip_prefix('<')?.split_once('>')?.0;
    let (rest, hash) = subject.rsplit_once('/')?;
    let hex = !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit());
    (hex && (rest.ends_with("/reference") || rest.ends_with("/value"))).then_some(subject)
}

/// What a tenant's graphs are rendered with.
struct Context {
    tenant: String,
    base: String,
    registry: Registry,
    order: EntityProjection,
    reader: Effective,
}

impl Context {
    async fn load(cx: &PgCx, tenant: &str) -> Result<Option<Self>, String> {
        let base = if tenant.is_empty() {
            RESERVED_BASE.to_string()
        } else {
            let row = cx
                .query_opt(
                    "SELECT config->>'base' FROM view.registry
                      WHERE tenant = '' AND kind = 'tenant' AND code = $1",
                    &[&tenant],
                )
                .await
                .map_err(e)?;
            match row.and_then(|r| r.get::<_, Option<String>>(0)) {
                Some(b) => b,
                None => return Ok(None),
            }
        };
        let registry = registry_for(cx, tenant).await?;
        let slugs: Vec<&str> = registry
            .providers()
            .iter()
            .map(|p| p.slug.as_str())
            .collect();
        let order = EntityProjection::new(&slugs, &registry);
        // The anonymous reader under the tenant's own groups, as a dump evaluates it.
        let mut groups = GroupRegistry::default_registry().clone();
        let rows = cx
            .query(
                "SELECT code, config FROM view.registry
                  WHERE tenant = $1 AND kind = 'group' ORDER BY code",
                &[&tenant],
            )
            .await
            .map_err(e)?;
        for r in rows {
            let name: String = r.get(0);
            let config: serde_json::Value = r.get(1);
            if let Ok(g) = Group::from_config(&name, &config, &groups) {
                let _ = groups.set_group(g);
            }
        }
        let reader = evaluate(
            &Principal::anonymous(),
            &groups,
            GrantRegistry::default_registry(),
        );
        Ok(Some(Self {
            tenant: tenant.to_string(),
            base,
            registry,
            order,
            reader,
        }))
    }

    fn readable(&self, chain: &[scatter_actors::Acl]) -> bool {
        read_decision(&self.reader, chain, Timestamp(now_micros())) == ReadDecision::Readable
    }

    /// Whether an anonymous reader may read the tenant at all: a tenant that is not
    /// public has no stream, as it has no dump (02 §6.4).
    async fn public(&self, cx: &PgCx) -> Result<bool, String> {
        if self.tenant.is_empty() {
            return Ok(true);
        }
        let chain = acl_read::current(
            cx,
            &self.tenant,
            &[scatter_actors::Target::Tenant(self.tenant.clone())],
        )
        .await?;
        Ok(self.readable(&chain))
    }
}

/// The public form of one entity on a tenant, per graph, as canonical skolemized lines.
#[derive(Default)]
struct Shown {
    /// Whether any graph holds the entity for the tenant at all.
    exists: bool,
    graphs: BTreeMap<&'static str, Vec<String>>,
    provider: Option<String>,
}

fn lines(triples: &[Triple]) -> Vec<String> {
    let skolem: Vec<Triple> = triples.iter().map(skolemize).collect();
    canonical_lines(&skolem)
}

/// Renders what a dump of the tenant would write for `id` now: the chain of
/// `Walk::entity`, one entity at a time.
async fn render<C: Partitions + PgClient>(
    cx: &C,
    ctx: &Context,
    id: &EntityId,
) -> Result<Shown, String> {
    let mut shown = Shown::default();
    let deleted = cx
        .query_opt(
            "SELECT bool_or(deleted) FROM view.entity
              WHERE tenant IN ($1, '') AND id = $2 HAVING count(*) > 0",
            &[&ctx.tenant, &id.as_str()],
        )
        .await
        .map_err(e)?
        .and_then(|r| r.get::<_, Option<bool>>(0));
    let Some(current) = read::current(cx, &ctx.order, &ctx.tenant, id).await? else {
        return Ok(shown);
    };
    shown.exists = true;
    let serving = current.contributions.first().map(|c| c.graph.clone());
    shown.provider = serving
        .as_deref()
        .and_then(|g| g.strip_prefix("mirror/"))
        .map(str::to_string);
    if deleted != Some(false) {
        return Ok(shown);
    }
    let chain = acl_read::entity_chain(cx, &ctx.tenant, id.as_str(), serving.as_deref()).await?;
    if !ctx.readable(&chain) {
        return Ok(shown);
    }
    let iris = IriScheme::new(
        Namespaces::triplespace(&ctx.base),
        &ctx.registry,
        KeyedRegistry::default_registry(),
    );
    let hasher = Hasher::local().with_iris(std::sync::Arc::new(
        scatter_wikibase_model::value_iri::ValueIris::new(
            &ctx.registry,
            Some(&iris.namespaces().entity()),
        ),
    ));

    let mut entity = current.resolved.entity.clone();
    add_derived_terms(&mut entity);
    let page = PageInfo {
        lastrevid: current.lastrevid,
        modified: Some(iso8601(current.modified)),
        ..PageInfo::default()
    };
    let upstream = current.upstream_statements(&ctx.registry);
    let triples = Renderer::new(&iris, &hasher, Produce::DUMP_FILE)
        .with_upstream(&upstream)
        .entity(&entity, Some(&page));
    shown.graphs.insert("resolved", lines(&triples));

    if let Some(local) = current.local() {
        let chain = acl_read::entity_chain(cx, &ctx.tenant, id.as_str(), Some("local")).await?;
        if ctx.readable(&chain) {
            let page = PageInfo {
                lastrevid: current.local_revid,
                ..PageInfo::default()
            };
            let triples = Renderer::new(&iris, &hasher, LOCAL).entity(local, Some(&page));
            shown.graphs.insert("local", lines(&triples));
        }
    }
    Ok(shown)
}

async fn shown_row(
    cx: &PgCx,
    tenant: &str,
    graph: &str,
    id: &str,
) -> Result<Option<(Vec<String>, Option<String>)>, String> {
    Ok(cx
        .query_opt(
            "SELECT triples, provider FROM view.rdf_shown
              WHERE tenant = $1 AND graph = $2 AND entity_id = $3",
            &[&tenant, &graph, &id],
        )
        .await
        .map_err(e)?
        .map(|r| (unpack(&r.get::<_, Vec<u8>>(0)), r.get(1))))
}

async fn store_shown(
    cx: &PgCx,
    tenant: &str,
    graph: &str,
    id: &str,
    provider: Option<&str>,
    new: Option<&[String]>,
) -> Result<(), String> {
    match new {
        Some(new) => cx
            .execute(
                "INSERT INTO view.rdf_shown (tenant, graph, entity_id, provider, triples)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (tenant, graph, entity_id)
                 DO UPDATE SET provider = EXCLUDED.provider, triples = EXCLUDED.triples",
                &[&tenant, &graph, &id, &provider, &pack(new)],
            )
            .await
            .map_err(e)?,
        None => cx
            .execute(
                "DELETE FROM view.rdf_shown
                  WHERE tenant = $1 AND graph = $2 AND entity_id = $3",
                &[&tenant, &graph, &id],
            )
            .await
            .map_err(e)?,
    };
    Ok(())
}

/// Counts the reference and value nodes an entity stopped and started showing, and takes
/// out of `delta.deleted` the triples of a node another entity still shows (0032 A17).
/// A node a tenant's rows drop is kept while a shared row shows it, and one the shared
/// rows drop while any tenant's rows show it: either may leave an orphan node behind,
/// never a missing one.
async fn count_nodes(
    cx: &PgCx,
    tenant: &str,
    graph: &str,
    old: &[String],
    new: &[String],
    delta: &mut Delta,
) -> Result<(), String> {
    let nodes = |ls: &[String]| -> BTreeSet<String> {
        ls.iter()
            .filter_map(|l| shared_node(l))
            .map(str::to_string)
            .collect()
    };
    let (was, is) = (nodes(old), nodes(new));
    let added: Vec<&String> = is.difference(&was).collect();
    let removed: Vec<&String> = was.difference(&is).collect();
    if !added.is_empty() {
        cx.execute(
            "INSERT INTO view.rdf_node (tenant, graph, node, uses)
             SELECT $1, $2, n, 1 FROM unnest($3::text[]) n
             ON CONFLICT (tenant, graph, node) DO UPDATE SET uses = view.rdf_node.uses + 1",
            &[&tenant, &graph, &added],
        )
        .await
        .map_err(e)?;
    }
    if removed.is_empty() {
        return Ok(());
    }
    cx.execute(
        "UPDATE view.rdf_node SET uses = uses - 1
          WHERE tenant = $1 AND graph = $2 AND node = ANY($3)",
        &[&tenant, &graph, &removed],
    )
    .await
    .map_err(e)?;
    cx.execute(
        "DELETE FROM view.rdf_node WHERE tenant = $1 AND graph = $2 AND uses <= 0",
        &[&tenant, &graph],
    )
    .await
    .map_err(e)?;
    let kept: BTreeSet<String> = cx
        .query(
            "SELECT DISTINCT node FROM view.rdf_node
              WHERE graph = $2 AND node = ANY($3)
                AND (tenant = $1 OR tenant = '' OR $1 = '')",
            &[&tenant, &graph, &removed],
        )
        .await
        .map_err(e)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    delta
        .deleted
        .retain(|l| shared_node(l).is_none_or(|n| !kept.contains(n)));
    Ok(())
}

async fn next_seq(cx: &PgCx) -> Result<i64, String> {
    Ok(cx
        .query_one("SELECT nextval('view.rdf_delta_seq')", &[])
        .await
        .map_err(e)?
        .get(0))
}

/// Whether the tenant holds a row of the entity, and whether the shared rows do. The
/// shared stream's own entities are its own.
async fn rows_of(cx: &PgCx, tenant: &str, id: &str) -> Result<(bool, bool), String> {
    if tenant.is_empty() {
        return Ok((true, false));
    }
    let r = cx
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM view.entity WHERE tenant = $1 AND id = $2),
                    EXISTS (SELECT 1 FROM view.entity WHERE tenant = '' AND id = $2)",
            &[&tenant, &id],
        )
        .await
        .map_err(e)?;
    Ok((r.get(0), r.get(1)))
}

/// Closes the tenant's overlay of a shared entity at `seq` where it `follows` the shared
/// row again, and opens one there where it has just stopped doing so.
async fn mark_overlay(
    cx: &PgCx,
    tenant: &str,
    id: &str,
    seq: i64,
    follows: bool,
    opens: bool,
) -> Result<(), String> {
    if follows {
        cx.execute(
            "UPDATE view.rdf_overlay SET to_seq = $3
              WHERE tenant = $1 AND entity_id = $2 AND to_seq IS NULL",
            &[&tenant, &id, &seq],
        )
        .await
        .map_err(e)?;
    } else if opens {
        cx.execute(
            "INSERT INTO view.rdf_overlay (tenant, entity_id, from_seq)
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            &[&tenant, &id, &seq],
        )
        .await
        .map_err(e)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // one per column
async fn store_delta(
    cx: &PgCx,
    tenant: &str,
    graph: &str,
    provider: Option<&str>,
    id: &str,
    version: Option<i64>,
    delta: &Delta,
    txn: i64,
    seq: i64,
) -> Result<(), String> {
    cx.execute(
        "INSERT INTO view.rdf_delta
           (seq, tenant, graph, provider, entity_id, resolved_version, deleted, inserted, txn)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        &[
            &seq,
            &tenant,
            &graph,
            &provider,
            &id,
            &version,
            &pack(&delta.deleted),
            &pack(&delta.inserted),
            &txn,
        ],
    )
    .await
    .map_err(e)?;
    Ok(())
}

/// Records what one entity shows now, and stores the delta from what it showed.
async fn apply_entity(cx: &PgCx, ctx: &Context, change: &Change) -> Result<usize, String> {
    let Ok(id) = EntityId::parse(&change.entity_id) else {
        return Ok(0);
    };
    let tenant = ctx.tenant.as_str();
    let now = render(cx, ctx, &id).await?;
    // On a tenant, an entity it holds no row of is the shared row's, unless the tenant
    // hides it: then, as when it overlays it, its own deltas stand in the shared ones'
    // place (08 §10.5).
    let (own, shared) = rows_of(cx, tenant, id.as_str()).await?;
    let hidden = now.exists && now.graphs.is_empty();
    let follows = shared && !own && !hidden;
    let version: Option<i64> = cx
        .query_opt(
            "SELECT resolved_version FROM view.entity
              WHERE tenant IN ($1, '') AND id = $2 ORDER BY tenant DESC LIMIT 1",
            &[&tenant, &id.as_str()],
        )
        .await
        .map_err(e)?
        .and_then(|r| r.get(0));

    let mut written = 0;
    for graph in GRAPHS {
        let was = shown_row(cx, tenant, graph, id.as_str()).await?;
        let shared_was = if shared && graph == "resolved" {
            shown_row(cx, "", graph, id.as_str())
                .await?
                .map(|(l, _)| for_tenant(&l, &ctx.base))
        } else {
            None
        };
        let (old, new): (Vec<String>, Option<Vec<String>>) = if follows {
            // The overlay is gone: back to what the shared stream shows, whose later
            // deltas carry it on from here.
            match &was {
                Some((l, _)) => (l.clone(), None),
                None => continue,
            }
        } else {
            let old = was
                .as_ref()
                .map(|(l, _)| l)
                .or(shared_was.as_ref())
                .cloned()
                .unwrap_or_default();
            (
                old,
                Some(now.graphs.get(graph).cloned().unwrap_or_default()),
            )
        };
        let target = match &new {
            Some(n) => n.clone(),
            None => shared_was.clone().unwrap_or_default(),
        };
        let mut delta = diff(&old, &target);
        // An overlay's own nodes are counted on the tenant; the shared row's stay the
        // shared stream's.
        let counted_old = if was.is_some() { old.as_slice() } else { &[] };
        let counted_new = new.as_deref().unwrap_or(&[]);
        count_nodes(cx, tenant, graph, counted_old, counted_new, &mut delta).await?;

        let seq = next_seq(cx).await?;
        if !tenant.is_empty() && shared && graph == "resolved" {
            mark_overlay(cx, tenant, id.as_str(), seq, follows, was.is_none()).await?;
        }
        let keep = match &new {
            // Nothing to hold for a graph the entity is not in, unless the row is what
            // marks a hidden shared entity as the tenant's own.
            Some(n) if n.is_empty() && !(shared && graph == "resolved") => None,
            Some(n) => Some(n.as_slice()),
            None => None,
        };
        store_shown(
            cx,
            tenant,
            graph,
            id.as_str(),
            now.provider.as_deref(),
            keep,
        )
        .await?;
        if delta.is_empty() {
            continue;
        }
        let provider = if tenant.is_empty() {
            now.provider
                .clone()
                .or_else(|| was.as_ref().and_then(|(_, p)| p.clone()))
        } else {
            None
        };
        store_delta(
            cx,
            tenant,
            graph,
            provider.as_deref(),
            id.as_str(),
            version,
            &delta,
            change.txn,
            seq,
        )
        .await?;
        written += 1;
    }
    Ok(written)
}

/// Records what every entity of the tenant shows, with no deltas, and starts its epoch
/// there (0032 A15).
async fn fill(cx: &PgCx, ctx: &Context) -> Result<usize, String> {
    let tenant = ctx.tenant.as_str();
    for table in [
        "view.rdf_delta",
        "view.rdf_shown",
        "view.rdf_node",
        "view.rdf_overlay",
    ] {
        cx.execute(
            &format!("DELETE FROM {table} WHERE tenant = $1"),
            &[&tenant],
        )
        .await
        .map_err(e)?;
    }
    let start = next_seq(cx).await?;
    let ids: Vec<String> = cx
        .query(
            "SELECT id FROM view.entity WHERE tenant = $1 ORDER BY id",
            &[&tenant],
        )
        .await
        .map_err(e)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let mut n = 0;
    for raw in &ids {
        let Ok(id) = EntityId::parse(raw) else {
            continue;
        };
        let now = render(cx, ctx, &id).await?;
        let overlays: bool = !tenant.is_empty()
            && cx
                .query_one(
                    "SELECT EXISTS (SELECT 1 FROM view.entity WHERE tenant = '' AND id = $1)",
                    &[&raw],
                )
                .await
                .map_err(e)?
                .get(0);
        if overlays {
            cx.execute(
                "INSERT INTO view.rdf_overlay (tenant, entity_id, from_seq) VALUES ($1, $2, $3)",
                &[&tenant, &raw, &start],
            )
            .await
            .map_err(e)?;
        }
        for graph in GRAPHS {
            let new = now.graphs.get(graph).cloned().unwrap_or_default();
            if new.is_empty() && !(overlays && graph == "resolved") {
                continue;
            }
            let mut none = Delta::default();
            count_nodes(cx, tenant, graph, &[], &new, &mut none).await?;
            store_shown(cx, tenant, graph, raw, now.provider.as_deref(), Some(&new)).await?;
        }
        n += 1;
    }
    cx.execute(
        "UPDATE ops.tenant_epoch
            SET epoch = epoch + 1, filled = true, rendering_version = $2, horizon = $3
          WHERE tenant = $1",
        &[&tenant, &RENDERING_VERSION, &start],
    )
    .await
    .map_err(e)?;
    Ok(n)
}

/// The delta consumer (0032 §2).
pub struct DeltaConsumer;

impl<B: Backend<Cx = PgCx>> Consumer<B> for DeltaConsumer {
    fn name(&self) -> &'static str {
        CONSUMER
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        tenant: &'a str,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let cx = &*cx;
            // One sequencer: whoever writes `seq` holds this until it commits, so no row
            // with a lower `seq` commits after one with a higher (03 §4.11).
            cx.execute("SELECT pg_advisory_xact_lock($1)", &[&SEQUENCER])
                .await
                .map_err(e)?;
            let state = cx
                .query_opt(
                    "SELECT filled, rendering_version FROM ops.tenant_epoch
                      WHERE tenant = $1 AND enabled FOR UPDATE",
                    &[&tenant],
                )
                .await
                .map_err(e)?;
            let Some(state) = state else {
                return Ok(0);
            };
            let Some(ctx) = Context::load(cx, tenant).await? else {
                return Ok(0);
            };
            if !ctx.public(cx).await? {
                return Ok(0);
            }
            let filled: bool = state.get(0);
            let version: i32 = state.get(1);
            if !filled || version != RENDERING_VERSION {
                // The fill reads every entity as it stands, which covers the work queued
                // with it.
                return fill(cx, &ctx).await;
            }
            // One transaction's changes together and in the order they were queued, each
            // entity once, so that an edit's deltas are contiguous in `seq`.
            let mut changes: Vec<Change> = Vec::new();
            for change in keys.iter().filter_map(|k| Change::parse(k)) {
                if change.entity_id == "*" {
                    continue;
                }
                changes.retain(|c| c.entity_id != change.entity_id);
                changes.push(change);
            }
            let mut order: Vec<i64> = Vec::new();
            for c in &changes {
                if !order.contains(&c.txn) {
                    order.push(c.txn);
                }
            }
            let mut n = 0;
            for txn in order {
                for change in changes.iter().filter(|c| c.txn == txn) {
                    n += apply_entity(cx, &ctx, change).await?;
                }
            }
            Ok(n)
        })
    }
}

/// Brings `ops.tenant_epoch` in line with the `updates.enabled` setting of every tenant
/// (02 §6.2): a tenant whose stream was turned on gets its row and a queued fill, with the
/// shared stream's beside it; one whose stream was turned off stops queueing work and
/// drops what it stored. Returns the tenants whose state changed.
///
/// # Errors
///
/// The database's.
pub async fn reconcile<C: PgClient>(client: &C) -> Result<Vec<String>, String> {
    let rows = client
        .query(
            "SELECT t.code,
                    coalesce(
                      (SELECT coalesce(s.config->'record'->'value', s.config->'value')
                         FROM view.registry s
                        WHERE s.tenant = t.code AND s.kind = 'site' AND s.code = 'updates.enabled'),
                      (SELECT coalesce(d.config->'record'->'value', d.config->'value')
                         FROM view.registry d
                        WHERE d.tenant = '' AND d.kind = 'template'
                          AND d.code = 'site:updates.enabled'),
                      'false'::jsonb) = 'true'::jsonb,
                    (SELECT enabled FROM ops.tenant_epoch x WHERE x.tenant = t.code)
               FROM view.registry t
              WHERE t.tenant = '' AND t.kind = 'tenant'",
            &[],
        )
        .await
        .map_err(e)?;
    let mut changed = Vec::new();
    let mut any_on = false;
    for r in &rows {
        let tenant: String = r.get(0);
        let want: bool = r.get(1);
        let have: Option<bool> = r.get(2);
        any_on |= want;
        if want != have.unwrap_or(false) {
            set_enabled(client, &tenant, want).await?;
            changed.push(tenant);
        }
    }
    let shared: Option<bool> = client
        .query_opt(
            "SELECT enabled FROM ops.tenant_epoch WHERE tenant = ''",
            &[],
        )
        .await
        .map_err(e)?
        .map(|r| r.get(0));
    if any_on != shared.unwrap_or(false) {
        set_enabled(client, "", any_on).await?;
        changed.push(String::new());
    }
    Ok(changed)
}

/// Turns one tenant's stream on or off, whatever its setting says; [`reconcile`] is what
/// follows the setting.
///
/// # Errors
///
/// The database's.
pub async fn set_enabled<C: PgClient>(client: &C, tenant: &str, on: bool) -> Result<(), String> {
    if on {
        client
            .execute(
                "INSERT INTO ops.tenant_epoch (tenant, rendering_version, enabled, filled)
                 VALUES ($1, $2, true, false)
                 ON CONFLICT (tenant) DO UPDATE SET enabled = true, filled = false",
                &[&tenant, &RENDERING_VERSION],
            )
            .await
            .map_err(e)?;
        client
            .execute(
                "INSERT INTO ops.projection_work (projection, tenant, partition, \"offset\", key)
                 VALUES ($1, $2, 0, 0, $3)",
                &[&CONSUMER, &tenant, &FILL_KEY],
            )
            .await
            .map_err(e)?;
    } else {
        client
            .execute(
                "UPDATE ops.tenant_epoch SET enabled = false, filled = false WHERE tenant = $1",
                &[&tenant],
            )
            .await
            .map_err(e)?;
        for table in [
            "view.rdf_delta",
            "view.rdf_shown",
            "view.rdf_node",
            "view.rdf_overlay",
        ] {
            client
                .execute(
                    &format!("DELETE FROM {table} WHERE tenant = $1"),
                    &[&tenant],
                )
                .await
                .map_err(e)?;
        }
    }
    Ok(())
}

/// The retention sweep (03 §4.11): deletes each stream's deltas older than its tenant's
/// `updates.retention` (the instance's default for the shared stream; thirty days where
/// neither says), and moves the stream's horizon past what it deleted. Returns the rows
/// deleted.
///
/// # Errors
///
/// The database's.
pub async fn sweep<C: PgClient>(client: &C) -> Result<u64, String> {
    let rows = client
        .query(
            "SELECT t.tenant,
                    coalesce(
                      (SELECT coalesce(s.config->'record'->>'value', s.config->>'value')
                         FROM view.registry s
                        WHERE s.tenant = t.tenant AND s.tenant <> '' AND s.kind = 'site'
                          AND s.code = 'updates.retention'),
                      (SELECT coalesce(d.config->'record'->>'value', d.config->>'value')
                         FROM view.registry d
                        WHERE d.tenant = '' AND d.kind = 'template'
                          AND d.code = 'site:updates.retention'),
                      'P30D')
               FROM ops.tenant_epoch t WHERE t.enabled",
            &[],
        )
        .await
        .map_err(e)?;
    let mut n = 0;
    for r in &rows {
        let (tenant, retention): (String, String) = (r.get(0), r.get(1));
        n += sweep_tenant(client, &tenant, &retention).await?;
    }
    Ok(n)
}

/// Deletes one stream's deltas older than `retention`, an ISO 8601 duration.
///
/// # Errors
///
/// The database's, a malformed duration among them.
pub async fn sweep_tenant<C: PgClient>(
    client: &C,
    tenant: &str,
    retention: &str,
) -> Result<u64, String> {
    let row = client
        .query_one(
            "WITH gone AS (
               DELETE FROM view.rdf_delta
                WHERE tenant = $1 AND time < now() - $2::text::interval RETURNING seq)
             SELECT count(*), max(seq) FROM gone",
            &[&tenant, &retention],
        )
        .await
        .map_err(e)?;
    let n: i64 = row.get(0);
    if let Some(last) = row.get::<_, Option<i64>>(1) {
        // A shared delta is part of every tenant's stream.
        client
            .execute(
                "UPDATE ops.tenant_epoch SET horizon = greatest(horizon, $2)
                  WHERE tenant = $1 OR $1 = ''",
                &[&tenant, &last],
            )
            .await
            .map_err(e)?;
    }
    Ok(u64::try_from(n).unwrap_or(0))
}

/// A position in a tenant's stream, `{epoch}:{seq}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// The tenant's epoch.
    pub epoch: i64,
    /// The last `seq` applied.
    pub seq: i64,
}

impl Cursor {
    /// Reads `{epoch}:{seq}`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (epoch, seq) = s.split_once(':')?;
        Some(Self {
            epoch: epoch.parse().ok()?,
            seq: seq.parse().ok()?,
        })
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.epoch, self.seq)
    }
}

/// Where a tenant's stream stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    /// The newest position: a dump taken now continues from it.
    pub cursor: Cursor,
    /// The `seq` before which the stream cannot be resumed.
    pub horizon: i64,
}

/// The head of a tenant's stream; `None` while it is off or has not recorded its graphs
/// yet. The head is the last delta stored, whoever's it was: `seq` order is commit order,
/// so within one snapshot no delta below it is missing, and a dump that reads it in its
/// own snapshot holds the effect of every delta up to it. A dump taken while work is
/// queued also holds changes whose deltas come later; applying those is harmless, since
/// both operations are set operations (02 §6.4).
///
/// # Errors
///
/// The database's.
pub async fn head<C: PgClient>(client: &C, tenant: &str) -> Result<Option<Head>, String> {
    let row = client
        .query_opt(
            "SELECT t.epoch, t.horizon,
                    greatest(t.horizon, coalesce((SELECT max(seq) FROM view.rdf_delta), 0))
               FROM ops.tenant_epoch t
              WHERE t.tenant = $1 AND t.enabled AND t.filled
                AND (t.tenant = '' OR EXISTS (
                      SELECT 1 FROM ops.tenant_epoch s
                       WHERE s.tenant = '' AND (s.filled OR NOT s.enabled)))",
            &[&tenant],
        )
        .await
        .map_err(e)?;
    Ok(row.map(|r| Head {
        cursor: Cursor {
            epoch: r.get(0),
            seq: r.get(2),
        },
        horizon: r.get(1),
    }))
}

/// One event of a stream: the deltas of one composing transaction (03 §4.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The `seq` of its last delta: the cursor after it.
    pub seq: i64,
    /// The composing transaction.
    pub txn: i64,
    /// When its last delta was stored, as Postgres writes a timestamp.
    pub time: String,
    /// The entities it changed, in order, each once.
    pub entities: Vec<String>,
    /// Its deltas, `(graph, delta)`, in `seq` order.
    pub parts: Vec<(String, Delta)>,
}

impl Event {
    /// The event as one SPARQL Update request (02 §6.4). With one graph subscribed its
    /// triples carry no `GRAPH` clause; with more, each is in `{base}graph/{name}`.
    #[must_use]
    pub fn request(&self, base: &str, graphs: &[String]) -> String {
        let base = if base.ends_with('/') {
            base.to_string()
        } else {
            format!("{base}/")
        };
        let iris: Vec<String> = self
            .parts
            .iter()
            .map(|(g, _)| format!("{base}graph/{g}"))
            .collect();
        let parts: Vec<GraphDelta<'_>> = self
            .parts
            .iter()
            .zip(&iris)
            .map(|((_, delta), iri)| GraphDelta {
                graph: (graphs.len() > 1).then_some(iri.as_str()),
                delta,
            })
            .collect();
        update_request(&parts)
    }
}

/// The events of a tenant's stream after `from`, oldest first: at most `limit` deltas,
/// and always whole events (0032 §4). A shared delta is given under the
/// tenant's `base`, and left out over the spans in which the tenant overlaid its entity.
///
/// # Errors
///
/// The database's.
pub async fn events<C: PgClient>(
    client: &C,
    tenant: &str,
    base: &str,
    graphs: &[String],
    from: i64,
    limit: i64,
) -> Result<Vec<Event>, String> {
    const SELECT: &str = "SELECT d.seq, d.tenant, d.graph, d.entity_id, d.txn, d.time::text,
                                 d.deleted, d.inserted
                            FROM view.rdf_delta d
                           WHERE d.graph = ANY($2)
                             AND (d.tenant = $1 OR (d.tenant = '' AND NOT EXISTS (
                                   SELECT 1 FROM view.rdf_overlay o
                                    WHERE o.tenant = $1 AND o.entity_id = d.entity_id
                                      AND o.from_seq <= d.seq
                                      AND (o.to_seq IS NULL OR d.seq < o.to_seq))))";
    let mut rows = client
        .query(
            &format!("{SELECT} AND d.seq > $3 ORDER BY d.seq LIMIT $4"),
            &[&tenant, &graphs, &from, &limit],
        )
        .await
        .map_err(e)?;
    // The rest of the last event, where the limit fell inside it.
    if i64::try_from(rows.len()).unwrap_or(i64::MAX) >= limit
        && let Some(last) = rows.last()
    {
        let (seq, txn): (i64, i64) = (last.get(0), last.get(4));
        let more = client
            .query(
                &format!(
                    "{SELECT} AND d.seq > $3 AND d.txn = $4
                       AND NOT EXISTS (SELECT 1 FROM view.rdf_delta x
                                        WHERE x.seq > $3 AND x.seq < d.seq AND x.txn <> $4
                                          AND (x.tenant = $1 OR x.tenant = ''))
                     ORDER BY d.seq"
                ),
                &[&tenant, &graphs, &seq, &txn],
            )
            .await
            .map_err(e)?;
        rows.extend(more);
    }
    let mut out: Vec<Event> = Vec::new();
    for r in &rows {
        let row_tenant: String = r.get(1);
        let entity: Option<String> = r.get(3);
        let txn: i64 = r.get(4);
        let side = |i: usize| {
            let l = unpack(&r.get::<_, Vec<u8>>(i));
            if row_tenant.is_empty() {
                for_tenant(&l, base)
            } else {
                l
            }
        };
        let delta = Delta {
            deleted: side(6),
            inserted: side(7),
        };
        if out.last().is_none_or(|ev| ev.txn != txn) {
            out.push(Event {
                seq: 0,
                txn,
                time: String::new(),
                entities: Vec::new(),
                parts: Vec::new(),
            });
        }
        let Some(ev) = out.last_mut() else { continue };
        ev.seq = r.get(0);
        ev.time = r.get(5);
        if let Some(id) = entity
            && !ev.entities.contains(&id)
        {
            ev.entities.push(id);
        }
        ev.parts.push((r.get(2), delta));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 08 §10.5: a shared delta is read under the reading tenant's base.
    #[test]
    fn a_shared_line_takes_the_tenants_base() {
        let shared = vec![
            "<http://www.wikidata.org/entity/Q42> <http://schema.org/name> \"https://tenant.invalid/\" ."
                .to_string(),
            "<https://tenant.invalid/wiki/Special:EntityData/WDQ42> <http://schema.org/about> <http://www.wikidata.org/entity/Q42> ."
                .to_string(),
        ];
        let got = for_tenant(&shared, "https://librarybase.org");
        assert_eq!(
            got[1],
            "<https://librarybase.org/wiki/Special:EntityData/WDQ42> <http://schema.org/about> <http://www.wikidata.org/entity/Q42> ."
        );
        // A literal that happens to hold the reserved base is not an IRI and stays.
        assert!(got[0].ends_with("\"https://tenant.invalid/\" ."));
    }

    // 0032 A17: reference and value nodes are told from every other subject.
    #[test]
    fn reference_and_value_nodes_are_recognized() {
        assert_eq!(
            shared_node("<https://x.org/reference/0a1b> <p> <o> ."),
            Some("https://x.org/reference/0a1b")
        );
        assert_eq!(
            shared_node("<http://www.wikidata.org/value/ff00> <p> \"1\" ."),
            Some("http://www.wikidata.org/value/ff00")
        );
        assert_eq!(shared_node("<https://x.org/entity/Q1> <p> <o> ."), None);
        assert_eq!(
            shared_node("<https://x.org/entity/statement/Q1-abc> <p> <o> ."),
            None
        );
    }

    // 02 §6.4: a cursor is `{epoch}:{seq}`.
    #[test]
    fn a_cursor_round_trips() {
        let c = Cursor { epoch: 3, seq: 41 };
        assert_eq!(Cursor::parse(&c.to_string()), Some(c));
        assert_eq!(Cursor::parse("41"), None);
    }
}
