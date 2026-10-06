//! The adoption job (0035 §2–5): a frozen source wiki's current state becomes the
//! tenant's `local` graph under the source's own IDs. The job checks its preconditions,
//! sets every sequence past what the source consumed, writes the source's accounts as
//! actor records, writes one `adopt` record per entity with the source's page ID in
//! header field 9, skips an entity already present with the same content and rejects
//! one present with different content, and records the floors and counts on its finish.

use std::collections::BTreeMap;

use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{
    Context, Graph, JobHeader, Mode, Operation, ProviderOrder, validate,
};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::DataType;
use serde::{Deserialize, Serialize};

use crate::IngestError;
use crate::job::{Counts, Job, Reject};
use crate::store::{IngestStore, Sequence};
use crate::write::{Attestation, Request, append_projected, write_to};

/// The source's consumed counters (0035 §4), from its `wb_id_counters` and highest IDs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Floors {
    /// Per minted entity type: `item` → the last item number.
    #[serde(default)]
    pub entity: BTreeMap<String, u64>,
    /// The highest page ID.
    #[serde(default)]
    pub page: u64,
    /// The highest revision ID.
    #[serde(default)]
    pub revision: u64,
    /// The highest log ID.
    #[serde(default)]
    pub log: u64,
    /// The highest user ID.
    #[serde(default)]
    pub user: u64,
}

/// An entity to adopt, as the adapter reads it from the frozen source.
#[derive(Debug, Clone, PartialEq)]
pub struct Adopted {
    /// The state, IDs in home form (0035 §3).
    pub entity: Entity,
    /// The source's revision ID of the state.
    pub source_revid: u64,
    /// The source's timestamp, its own string.
    pub source_time: String,
    /// The entity's page ID on the source.
    pub source_pageid: u64,
}

/// An adoption.
#[derive(Debug, Clone)]
pub struct Adoption {
    /// The source's base URL.
    pub source: String,
    /// The dump's identity, the source version.
    pub version: String,
    /// The adapter version.
    pub adapter_version: Option<String>,
    /// The operator has declared the source frozen (0035 §2, precondition 1).
    pub frozen: bool,
    /// The floors.
    pub floors: Floors,
    /// The source's accounts as actor-record drafts, keyed by actor key, for the tenant's
    /// `actors` partition; one already present is left alone (0035 §5).
    pub accounts: Vec<Draft>,
    /// The source's property types, for validation: the dump's own properties (the
    /// adapter's survey collects them). Empty means snaks are checked against their own
    /// `datatype` only.
    pub property_types: BTreeMap<EntityId, DataType>,
}

/// What an adoption produced.
#[derive(Debug, Clone, PartialEq)]
pub struct AdoptionOutcome {
    /// The job ID.
    pub job_id: u64,
    /// The counts.
    pub counts: Counts,
    /// Accounts written (those not already present).
    pub accounts: u64,
    /// The rejects.
    pub rejects: Vec<Reject>,
}

async fn preconditions<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    adoption: &Adoption,
    local: u64,
) -> Result<(), IngestError> {
    if !adoption.frozen {
        return Err(IngestError::AdoptionRefused(
            "the source is not declared frozen".into(),
        ));
    }
    let prefix = format!("{tenant}:");
    if let Some(a) = adoption
        .accounts
        .iter()
        .find(|d| !d.key.as_deref().is_some_and(|k| k.starts_with(&prefix)))
    {
        return Err(IngestError::AdoptionRefused(format!(
            "account `{}` is not under the tenant's issuer `{tenant}`",
            a.key.as_deref().unwrap_or("?")
        )));
    }
    if !store.only_adoptions(cx, local).await? {
        return Err(IngestError::AdoptionRefused(
            "the local partition holds entity records that are not adoptions".into(),
        ));
    }
    if let Some(other) = store
        .adoption_sources(cx, tenant)
        .await?
        .into_iter()
        .find(|s| *s != adoption.source)
    {
        return Err(IngestError::AdoptionRefused(format!(
            "the tenant was adopted from `{other}`, not `{}`",
            adoption.source
        )));
    }
    Ok(())
}

async fn set_floors<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    floors: &Floors,
) -> Result<(), IngestError> {
    for (t, n) in &floors.entity {
        store
            .floor(cx, tenant, &Sequence::Entity(t.clone()), *n)
            .await?;
    }
    store
        .floor(cx, tenant, &Sequence::Page, floors.page)
        .await?;
    store
        .floor(cx, tenant, &Sequence::Revision, floors.revision)
        .await?;
    store.floor(cx, tenant, &Sequence::Log, floors.log).await?;
    store
        .floor(cx, tenant, &Sequence::User, floors.user)
        .await?;
    Ok(())
}

/// Runs an adoption, `batch` entities per unit of work. Re-running it against the same
/// dump skips what is present and so completes a partial adoption (0035 §3).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_adoption<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    tenant: &str,
    attestation: Attestation,
    adoption: Adoption,
    entities: impl IntoIterator<Item = Adopted>,
    registry: &Registry,
    now: u64,
    batch: usize,
) -> Result<AdoptionOutcome, IngestError> {
    let mut cx = store.begin().await?;
    let local = store
        .partition(&mut cx, tenant, "local")
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: tenant.to_string(),
            graph: "local".into(),
        })?;
    preconditions(store, &mut cx, tenant, &adoption, local).await?;

    let mut header = JobHeader::local(&adoption.source);
    header.version = Some(adoption.version.clone());
    header.mode = Some(Mode::Adopt);
    header.adapter_version.clone_from(&adoption.adapter_version);
    header.params.insert(
        "floors".into(),
        serde_json::to_value(&adoption.floors).map_err(|e| e.to_string())?,
    );
    let mut job = Job::start(store, pipeline, &mut cx, tenant, header, attestation, now).await?;
    set_floors(store, &mut cx, tenant, &adoption.floors).await?;

    // Accounts (0035 §5): the source's users under the tenant's issuer, skipping any
    // already present, such as the owner `instance create --adopt` made.
    let mut accounts = 0u64;
    if !adoption.accounts.is_empty() {
        let actors = store
            .partition(&mut cx, tenant, "actors")
            .await?
            .ok_or_else(|| IngestError::NoPartition {
                tenant: tenant.to_string(),
                graph: "actors".into(),
            })?;
        for mut draft in adoption.accounts.clone() {
            let key = draft.key.clone().unwrap_or_default();
            if store.latest_for_key(&mut cx, actors, &key).await?.is_some() {
                continue;
            }
            if draft.logid.is_none() {
                draft.logid = Some(store.next_id(&mut cx, tenant, &Sequence::Log).await?);
            }
            append_projected(store, pipeline, &mut cx, actors, draft, Budget::NONE).await?;
            accounts += 1;
        }
    }
    store.commit(cx).await?;

    let order = ProviderOrder::default();
    let cx_v = Context {
        graph: Graph::Local,
        mode: Some(Mode::Adopt),
        order: &order,
        providers: registry,
        types: &adoption.property_types,
        strict_properties: false,
    };
    let mut request = Request::new(tenant, job.record_attestation(), now);
    request.budget = Budget::NONE;
    let mut counts = Counts::default();
    let mut rejects = Vec::new();
    let mut cx = store.begin().await?;
    let mut in_unit = 0usize;
    for (i, adopted) in entities.into_iter().enumerate() {
        if in_unit >= batch.max(1) {
            store.commit(cx).await?;
            cx = store.begin().await?;
            in_unit = 0;
        }
        in_unit += 1;
        let line = i + 1;
        let id: EntityId = adopted.entity.id.clone();
        let op = Operation::Adopt {
            id: id.clone(),
            entity: adopted.entity,
            source_revid: adopted.source_revid,
            source_time: adopted.source_time,
            source_pageid: adopted.source_pageid,
        };
        let reject =
            |reason: String, counts: &mut Counts, job: &mut Job, rejects: &mut Vec<Reject>| {
                counts.rejected += 1;
                let r = Reject {
                    line,
                    match_key: None,
                    reason,
                };
                job.reject(r.clone());
                rejects.push(r);
            };
        let problems = validate(&op, &cx_v);
        if !problems.is_empty() {
            reject(
                problems
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
                &mut counts,
                &mut job,
                &mut rejects,
            );
            continue;
        }
        // Idempotent resume (0035 §3): present with the same content is skipped,
        // present with other content is rejected.
        if let Some(cursor) = store.cursor(&mut cx, tenant, &id, "local").await? {
            let content = serde_json::to_value(&op).map_err(|e| e.to_string())?;
            let hash = scatter_log::body::Part::of(&scatter_log::cbor::Value::from_json(&content))
                .map_err(|e| e.to_string())?
                .content_hash()
                .ok_or_else(|| IngestError::Store("content present".into()))?;
            if hash == cursor.content_hash {
                counts.unchanged += 1;
            } else {
                reject(
                    format!("`{id}` is present with other content (ts-adopt-conflict)"),
                    &mut counts,
                    &mut job,
                    &mut rejects,
                );
            }
            continue;
        }
        write_to(
            store,
            pipeline,
            &mut cx,
            &request,
            &op,
            &Graph::Local,
            registry,
        )
        .await?;
        counts.adopted += 1;
    }
    let mut extra = serde_json::Map::new();
    extra.insert(
        "floors".into(),
        serde_json::to_value(&adoption.floors).map_err(|e| e.to_string())?,
    );
    extra.insert("accounts".into(), accounts.into());
    job.finish(store, pipeline, &mut cx, &counts, extra, now)
        .await?;
    store.commit(cx).await?;
    Ok(AdoptionOutcome {
        job_id: job.id,
        counts,
        accounts,
        rejects,
    })
}
