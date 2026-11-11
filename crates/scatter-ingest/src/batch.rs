//! A tenant's bulk job over an NDJSON batch (0002 §8.5, §8.7): temporary handles are
//! minted or matched, `create-or-add` is resolved against the identifier index, a bulk
//! `retain` fans out, every operation is validated against the job's graph and the
//! property type map, and each record-shaped operation is written. An `atomic` batch is
//! one unit of work and fails as a whole; a streamed one records the rejects on the job.
//!
//! The operations are written in blocks ([`write_many`]; 0013 A90), [`BLOCK`] at a time.
//! A streamed batch commits each block; a block that fails is rolled back and its
//! operations are written again each on its own, so that the one that fails is rejected
//! by itself and the rest land.

use std::collections::{BTreeMap, BTreeSet};

use scatter_projection::Pipeline;
use scatter_providers::Registry;
use scatter_wikibase_changeset::{
    Batch, Context, Graph, Mode, Operation, ProviderOrder, Resolution, validate,
};
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::DataType;

use crate::IngestError;
use crate::job::{Counts, Job, Reject};
use crate::store::IngestStore;
use crate::write::{
    Attestation, BlockEntry, BlockPayload, Request, mint_entity_id, write_many, write_to,
};

/// What a batch run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchOutcome {
    /// The job ID.
    pub job_id: u64,
    /// The counts on the finish record.
    pub counts: Counts,
    /// Temporary handles and the IDs they became (0002 §8.5: "returns the mapping").
    pub ids: BTreeMap<String, EntityId>,
    /// The rejects, in batch order (the first 100 are also on the finish record).
    pub rejects: Vec<Reject>,
}

/// Every property an operation's snaks name, for the type map.
fn properties_of(op: &Operation, out: &mut BTreeSet<EntityId>) {
    for s in op.statements() {
        for snak in s.snaks() {
            out.insert(snak.property.clone());
        }
    }
    if let Operation::Add {
        qualifiers,
        references,
        ..
    } = op
    {
        for groups in qualifiers.values() {
            out.extend(groups.keys().cloned());
        }
        for refs in references.values() {
            for r in refs {
                out.extend(r.snaks.keys().cloned());
            }
        }
    }
}

/// The entity type a raw `create` or `create-or-add` declares.
fn declared_type(op: &scatter_wikibase_changeset::WireOp) -> Option<String> {
    op.value
        .get("entity")?
        .get("type")?
        .as_str()
        .map(str::to_owned)
}

/// Mints or matches an ID for every operation that needs one — those with a `ref`, and
/// every `create-or-add` — and rewrites the handles. Returns the handle map and, per
/// line, how a `create-or-add` resolved.
async fn assign_ids<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    batch: &mut Batch,
) -> Result<(BTreeMap<String, EntityId>, BTreeMap<usize, Resolution>), IngestError> {
    batch.refs()?;
    let mut ids: BTreeMap<String, EntityId> = BTreeMap::new();
    let mut resolutions: BTreeMap<usize, Resolution> = BTreeMap::new();
    for op in &mut batch.ops {
        let kind = op.op().unwrap_or_default().to_string();
        if kind != "create" && kind != "create-or-add" {
            continue;
        }
        let entity_type = declared_type(op).ok_or_else(|| {
            IngestError::Adapter(format!("line {}: the entity has a `type`", op.line))
        })?;
        let resolution = if kind == "create-or-add" {
            let key: scatter_wikibase_changeset::MatchKey = op
                .value
                .get("match")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| IngestError::Adapter(format!("line {}: match key: {e}", op.line)))?
                .ok_or_else(|| {
                    IngestError::Adapter(format!("line {}: a create-or-add has `match`", op.line))
                })?;
            match store.find_match(cx, tenant, &key).await? {
                Some(found) => Resolution::Matched(found),
                None => Resolution::Minted(mint_entity_id(store, cx, tenant, &entity_type).await?),
            }
        } else {
            Resolution::Minted(mint_entity_id(store, cx, tenant, &entity_type).await?)
        };
        let id = match &resolution {
            Resolution::Matched(i) | Resolution::Minted(i) => i.clone(),
        };
        if let Some(handle) = op.declared_ref().map(str::to_owned) {
            ids.insert(handle, id.clone());
        } else {
            op.assign_id(&id);
        }
        resolutions.insert(op.line, resolution);
    }
    batch.resolve_refs(&ids)?;
    Ok((ids, resolutions))
}

/// The type map for the batch, from the store.
async fn type_map<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    ops: &[Operation],
) -> Result<BTreeMap<EntityId, DataType>, IngestError> {
    let mut properties = BTreeSet::new();
    for op in ops {
        properties_of(op, &mut properties);
    }
    let mut map = BTreeMap::new();
    for p in properties {
        if let Some(dt) = store.datatype(cx, tenant, &p).await? {
            map.insert(p, dt);
        }
    }
    Ok(map)
}

/// How many operations a batch writes at a time, and a streamed batch commits at a time
/// (0013 A90): a sync's unit (`--batch`).
pub const BLOCK: usize = 200;

/// Writes operations in order, as blocks ([`write_many`]): each run of consecutive
/// operations for one graph ([`crate::write::destination`]) is a block, so that the page
/// and revision IDs are taken in the order the operations come. What [`write_to`] writes
/// for each in turn.
async fn write_ops<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    request: &Request,
    ops: &[&Operation],
    registry: &Registry,
) -> Result<(), IngestError> {
    let mut graphs = Vec::with_capacity(ops.len());
    for op in ops {
        graphs.push(crate::write::destination(op, registry)?);
    }
    let mut from = 0;
    while from < ops.len() {
        let graph = &graphs[from];
        let to = from + graphs[from..].iter().take_while(|g| *g == graph).count();
        let owner = match graph {
            Graph::Mirror(_) => "",
            Graph::Local | Graph::Pages | Graph::Source(_) => request.tenant.as_str(),
        };
        let partition = store
            .partition(cx, owner, &graph.name())
            .await?
            .ok_or_else(|| IngestError::NoPartition {
                tenant: owner.to_string(),
                graph: graph.name(),
            })?;
        let entries: Vec<BlockEntry<'_>> = ops[from..to]
            .iter()
            .map(|op| BlockEntry {
                request,
                payload: BlockPayload::Operation(op),
            })
            .collect();
        write_many(
            store,
            pipeline,
            cx,
            partition,
            graph,
            &entries,
            request.budget,
            registry,
        )
        .await?;
        from = to;
    }
    Ok(())
}

fn count(counts: &mut Counts, op: &Operation) {
    match op {
        Operation::Create { .. } => counts.created += 1,
        Operation::Add { .. } | Operation::Remove { .. } => counts.merged += 1,
        Operation::Adopt { .. } => counts.adopted += 1,
        Operation::Tombstone { .. } => counts.tombstoned += 1,
        _ => counts.other += 1,
    }
}

/// Prepares a batch: assigns IDs, types, completes and validates the operations, and
/// resolves `create-or-add`. Returns, per line, the record-shaped operations or the
/// problems.
async fn prepare<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    batch: &mut Batch,
    order: &ProviderOrder,
    registry: &Registry,
) -> Result<
    (
        BTreeMap<String, EntityId>,
        Vec<(usize, Result<Vec<Operation>, String>)>,
    ),
    IngestError,
> {
    let (ids, resolutions) = assign_ids(store, cx, tenant, batch).await?;
    let mut typed: Vec<(usize, Result<Operation, String>)> = Vec::new();
    for op in &batch.ops {
        typed.push((op.line, op.typed().map_err(|e| e.to_string())));
    }
    let ok: Vec<Operation> = typed
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok().cloned())
        .collect();
    let datatypes = type_map(store, cx, tenant, &ok).await?;
    let graph = batch
        .job
        .graph()
        .ok_or_else(|| IngestError::Adapter(format!("`{}` is not a graph", batch.job.graph)))?;
    let cx_v = Context {
        graph,
        mode: batch.job.mode,
        order,
        providers: registry,
        types: &datatypes,
        strict_properties: false,
    };
    let mut out = Vec::new();
    for (line, t) in typed {
        let mut op = match t {
            Ok(op) => op,
            Err(e) => {
                out.push((line, Err(e)));
                continue;
            }
        };
        op.complete(order);
        let problems = validate(&op, &cx_v);
        if !problems.is_empty() {
            out.push((
                line,
                Err(problems
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")),
            ));
            continue;
        }
        if let Some(r) = resolutions.get(&line) {
            op = op.resolve_create_or_add(r.clone());
        }
        out.push((line, Ok(op.fan_out())));
    }
    Ok((ids, out))
}

/// Runs a tenant bulk job over an NDJSON batch. The job line's `graph` is `local` or
/// `pages`; a mirror sync runs through [`crate::sync::run_sync`] and an adoption through
/// [`crate::adopt::run_adoption`].
#[allow(clippy::too_many_arguments)]
pub async fn run_batch<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    tenant: &str,
    attestation: Attestation,
    text: &str,
    order: &ProviderOrder,
    registry: &Registry,
    now: u64,
) -> Result<BatchOutcome, IngestError> {
    run_batch_in(
        store,
        pipeline,
        tenant,
        attestation,
        text,
        order,
        registry,
        now,
        BLOCK,
    )
    .await
}

/// [`run_batch`], `block` operations at a time: [`BLOCK`] for a job, one for a batch
/// written an operation at a time.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_batch_in<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    tenant: &str,
    attestation: Attestation,
    text: &str,
    order: &ProviderOrder,
    registry: &Registry,
    now: u64,
    block: usize,
) -> Result<BatchOutcome, IngestError> {
    let block = block.max(1);
    let mut batch = Batch::parse(text)?;
    match batch.job.graph() {
        Some(Graph::Local | Graph::Pages) => {}
        _ => {
            return Err(IngestError::Adapter(format!(
                "a bulk job writes `local` or `pages`, not `{}`",
                batch.job.graph
            )));
        }
    }
    if batch.job.mode.is_some_and(|m| m != Mode::Upsert) {
        return Err(IngestError::Adapter(
            "a bulk job has no mode, or `upsert`".into(),
        ));
    }
    let header = batch.job.clone();
    let mut counts = Counts::default();
    let mut rejects = Vec::new();

    if header.atomic {
        // One unit of work; any failure rolls everything back and records the failure.
        let mut cx = store.begin().await?;
        let job = Job::start(
            store,
            pipeline,
            &mut cx,
            tenant,
            header.clone(),
            attestation.clone(),
            now,
        )
        .await?;
        let outcome: Result<BTreeMap<String, EntityId>, IngestError> = async {
            let (ids, prepared) =
                prepare(store, &mut cx, tenant, &mut batch, order, registry).await?;
            let mut request = Request::new(tenant, job.record_attestation(), now);
            request.budget = scatter_projection::Budget::NONE;
            // The operations before the first line that has problems are written, as
            // they were one at a time, so that a failure among them is the one reported.
            let mut ops: Vec<&Operation> = Vec::new();
            let mut problem = None;
            for (line, line_ops) in &prepared {
                match line_ops {
                    Ok(line_ops) => ops.extend(line_ops),
                    Err(reason) => {
                        problem = Some(IngestError::Adapter(format!("line {line}: {reason}")));
                        break;
                    }
                }
            }
            for ops in ops.chunks(block) {
                write_ops(store, pipeline, &mut cx, &request, ops, registry).await?;
                for op in ops {
                    count(&mut counts, op);
                }
            }
            match problem {
                Some(e) => Err(e),
                None => Ok(ids),
            }
        }
        .await;
        match outcome {
            Ok(ids) => {
                job.finish(
                    store,
                    pipeline,
                    &mut cx,
                    &counts,
                    serde_json::Map::new(),
                    now,
                )
                .await?;
                store.commit(cx).await?;
                Ok(BatchOutcome {
                    job_id: job.id,
                    counts,
                    ids,
                    rejects,
                })
            }
            Err(e) => {
                store.rollback(cx).await?;
                let mut cx = store.begin().await?;
                let job =
                    Job::start(store, pipeline, &mut cx, tenant, header, attestation, now).await?;
                job.fail(store, pipeline, &mut cx, &e.to_string(), now)
                    .await?;
                store.commit(cx).await?;
                Err(e)
            }
        }
    } else {
        // Streamed: the job starts, the operations are written and committed a block at
        // a time, rejects are recorded, the job finishes.
        let mut cx = store.begin().await?;
        let mut job =
            Job::start(store, pipeline, &mut cx, tenant, header, attestation, now).await?;
        let (ids, prepared) = prepare(store, &mut cx, tenant, &mut batch, order, registry).await?;
        store.commit(cx).await?;
        let mut request = Request::new(tenant, job.record_attestation(), now);
        request.budget = scatter_projection::Budget::NONE;
        // The operations waiting for their block, each with its line. A line with
        // problems is rejected after the operations before it are written, so that the
        // rejects come in batch order.
        let mut waiting: Vec<(usize, &Operation)> = Vec::new();
        for (line, ops) in &prepared {
            match ops {
                Ok(ops) => {
                    for op in ops {
                        waiting.push((*line, op));
                        if waiting.len() >= block {
                            let ops = std::mem::take(&mut waiting);
                            write_streamed(
                                store,
                                pipeline,
                                &request,
                                &ops,
                                registry,
                                (&mut counts, &mut job, &mut rejects),
                            )
                            .await?;
                        }
                    }
                }
                Err(reason) => {
                    let ops = std::mem::take(&mut waiting);
                    write_streamed(
                        store,
                        pipeline,
                        &request,
                        &ops,
                        registry,
                        (&mut counts, &mut job, &mut rejects),
                    )
                    .await?;
                    counts.rejected += 1;
                    let r = Reject {
                        line: *line,
                        match_key: None,
                        reason: reason.clone(),
                    };
                    job.reject(r.clone());
                    rejects.push(r);
                }
            }
        }
        write_streamed(
            store,
            pipeline,
            &request,
            &waiting,
            registry,
            (&mut counts, &mut job, &mut rejects),
        )
        .await?;
        let mut cx = store.begin().await?;
        job.finish(
            store,
            pipeline,
            &mut cx,
            &counts,
            serde_json::Map::new(),
            now,
        )
        .await?;
        store.commit(cx).await?;
        Ok(BatchOutcome {
            job_id: job.id,
            counts,
            ids,
            rejects,
        })
    }
}

/// Writes a streamed batch's block in a unit of work of its own (0013 A90). When the
/// block fails it is rolled back and its operations are written again, each in its own
/// unit of work, as a streamed batch wrote every operation before: the one that fails
/// is rejected with its line and the rest land.
async fn write_streamed<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    request: &Request,
    block: &[(usize, &Operation)],
    registry: &Registry,
    (counts, job, rejects): (&mut Counts, &mut Job, &mut Vec<Reject>),
) -> Result<(), IngestError> {
    if block.is_empty() {
        return Ok(());
    }
    let ops: Vec<&Operation> = block.iter().map(|(_, op)| *op).collect();
    let mut cx = store.begin().await?;
    if write_ops(store, pipeline, &mut cx, request, &ops, registry)
        .await
        .is_ok()
    {
        store.commit(cx).await?;
        for op in ops {
            count(counts, op);
        }
        return Ok(());
    }
    store.rollback(cx).await?;
    for (line, op) in block {
        let mut cx = store.begin().await?;
        let result = async {
            let graph = crate::write::destination(op, registry)?;
            write_to(store, pipeline, &mut cx, request, op, &graph, registry).await
        }
        .await;
        match result {
            Ok(_) => {
                store.commit(cx).await?;
                count(counts, op);
            }
            Err(e) => {
                store.rollback(cx).await?;
                counts.rejected += 1;
                let r = Reject {
                    line: *line,
                    match_key: match op {
                        Operation::Create { match_key, .. } | Operation::Add { match_key, .. } => {
                            match_key.clone()
                        }
                        _ => None,
                    },
                    reason: e.to_string(),
                };
                job.reject(r.clone());
                rejects.push(r);
            }
        }
    }
    Ok(())
}
