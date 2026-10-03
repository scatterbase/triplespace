//! A tenant's bulk job over an NDJSON batch (0002 §8.5, §8.7): temporary handles are
//! minted or matched, `create-or-add` is resolved against the identifier index, a bulk
//! `retain` fans out, every operation is validated against the job's graph and the
//! property type map, and each record-shaped operation is written. An `atomic` batch is
//! one unit of work and fails as a whole; a streamed one applies each operation on its
//! own and records the rejects on the job.

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
use crate::write::{Attestation, Request, mint_entity_id, write_to};

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
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
            for (line, ops) in prepared {
                let ops =
                    ops.map_err(|reason| IngestError::Adapter(format!("line {line}: {reason}")))?;
                for op in &ops {
                    let graph = crate::write::destination(op, registry)?;
                    write_to(store, pipeline, &mut cx, &request, op, &graph, registry).await?;
                    count(&mut counts, op);
                }
            }
            Ok(ids)
        }
        .await;
        match outcome {
            Ok(ids) => {
                job.finish(store, &mut cx, &counts, serde_json::Map::new(), now)
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
                let job = Job::start(store, &mut cx, tenant, header, attestation, now).await?;
                job.fail(store, &mut cx, &e.to_string(), now).await?;
                store.commit(cx).await?;
                Err(e)
            }
        }
    } else {
        // Streamed: the job starts, each operation is its own unit of work, rejects are
        // recorded, the job finishes.
        let mut cx = store.begin().await?;
        let mut job = Job::start(store, &mut cx, tenant, header, attestation, now).await?;
        let (ids, prepared) = prepare(store, &mut cx, tenant, &mut batch, order, registry).await?;
        store.commit(cx).await?;
        let mut request = Request::new(tenant, job.record_attestation(), now);
        request.budget = scatter_projection::Budget::NONE;
        for (line, ops) in prepared {
            let ops = match ops {
                Ok(ops) => ops,
                Err(reason) => {
                    counts.rejected += 1;
                    let r = Reject {
                        line,
                        match_key: None,
                        reason,
                    };
                    job.reject(r.clone());
                    rejects.push(r);
                    continue;
                }
            };
            for op in &ops {
                let mut cx = store.begin().await?;
                let result = async {
                    let graph = crate::write::destination(op, registry)?;
                    write_to(store, pipeline, &mut cx, &request, op, &graph, registry).await
                }
                .await;
                match result {
                    Ok(_) => {
                        store.commit(cx).await?;
                        count(&mut counts, op);
                    }
                    Err(e) => {
                        store.rollback(cx).await?;
                        counts.rejected += 1;
                        let r = Reject {
                            line,
                            match_key: match op {
                                Operation::Create { match_key, .. }
                                | Operation::Add { match_key, .. } => match_key.clone(),
                                _ => None,
                            },
                            reason: e.to_string(),
                        };
                        job.reject(r.clone());
                        rejects.push(r);
                    }
                }
            }
        }
        let mut cx = store.begin().await?;
        job.finish(store, &mut cx, &counts, serde_json::Map::new(), now)
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
