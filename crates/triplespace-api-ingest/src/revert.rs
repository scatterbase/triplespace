//! Reverting a job, record by record (0104 §3; 05 §3.3; 19 §3.10): a job of its own,
//! mode `revert`, naming the job it reverts, attributed to the holder of `ts-revertjob`
//! who asked for it. For each record the job wrote, newest first, the inverse: the state
//! before an entity record restored by the operations that take the current state back
//! to it ([`crate::diff`]); a deletion for an entity or page the job created; the
//! previous text as an `edit` for a page revision. A record someone has changed since,
//! whose key has a newer record that neither the reverted job nor this revert wrote, is
//! skipped and listed, as is a record that has no inverse yet (a move, a deletion, a
//! post in a thread). The revert's finish record carries what was skipped and why, and a
//! `job/revert` record under the reverted job's key names the revert.
//!
//! Reverting a job again writes nothing: every key's newest record is then the revert's
//! own, so each record is skipped as changed since.
//!
//! implements 0104 §3 (A2)

use scatter_actors::acl::{Acl, ReadKind, Target};
use scatter_ingest::IngestError;
use scatter_ingest::IngestStore as _;
use scatter_ingest::job::{Counts, Job};
use scatter_ingest::write::{Attestation, RawRecord, Request, write_operation, write_raw};
use scatter_log::cbor::Value;
use scatter_log_postgres::PgClient;
use scatter_pages::PageOp;
use scatter_projection::{Backend as _, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::apply::LocalState;
use scatter_wikibase_changeset::{JobHeader, Mode};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use tokio_postgres::GenericClient;
use triplespace_projections::common::{attested, content, target_kind, to_jsonb};
use triplespace_projections::pages::PAYLOAD as PAYLOAD_PAGE;
use triplespace_projections::revision_state;

use crate::PgIngest;
use crate::acl::{AclWrite, write as write_acl};
use crate::diff::operations_from;

/// A record the revert left as it was, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Skipped {
    /// The record's partition.
    pub partition: u64,
    /// Its offset.
    pub offset: u64,
    /// What it was about: an entity ID or a page ID.
    pub target: String,
    /// Why.
    pub reason: String,
}

/// What a revert came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertReport {
    /// The revert job's ID.
    pub job_id: u64,
    /// The job reverted.
    pub reverts: u64,
    /// Records of the reverted job whose inverse was written.
    pub reverted: u64,
    /// Records written by the revert.
    pub records: u64,
    /// What was left as it was.
    pub skipped: Vec<Skipped>,
}

fn store_err(e: impl std::fmt::Display) -> IngestError {
    IngestError::Store(e.to_string())
}

/// One record of the reverted job, as the activity feed lists it.
struct JobRecord {
    partition: u64,
    offset: u64,
    kind: String,
    graph: String,
    target_kind: Option<i16>,
    target_id: Option<String>,
    log: Option<String>,
}

/// The job's records, newest first.
async fn records_of<C: GenericClient>(
    client: &C,
    tenant: &str,
    job: u64,
) -> Result<Vec<JobRecord>, IngestError> {
    let rows = GenericClient::query(
        client,
        "SELECT a.partition, a.\"offset\", a.kind, p.name, a.target_kind, a.target_id,
                    a.log_type || '/' || a.log_action
             FROM view.activity a JOIN log.partition p ON p.partition = a.partition
             WHERE a.tenant = $1 AND a.job_id = $2 AND a.kind <> 'job'
             ORDER BY a.time DESC, a.\"offset\" DESC",
        &[&tenant, &i64::try_from(job).map_err(store_err)?],
    )
    .await
    .map_err(store_err)?;
    Ok(rows
        .iter()
        .map(|r| JobRecord {
            partition: scatter_log_postgres::ids::partition_from_db(r.get(0)),
            offset: u64::try_from(r.get::<_, i64>(1)).unwrap_or(0),
            kind: r.get(2),
            graph: r.get(3),
            target_kind: r.get(4),
            target_id: r.get(5),
            log: r.get(6),
        })
        .collect())
}

/// The state a key held before a record: the newest `view.entity_revision` row of the
/// key below the record's offset.
async fn state_before<C: GenericClient>(
    client: &C,
    tenant: &str,
    key: &str,
    offset: u64,
) -> Result<Option<Vec<u8>>, IngestError> {
    let row = GenericClient::query_opt(
        client,
        "SELECT state FROM view.entity_revision
             WHERE tenant = $1 AND entity_id = $2 AND \"offset\" < $3
             ORDER BY \"offset\" DESC LIMIT 1",
        &[&tenant, &key, &i64::try_from(offset).map_err(store_err)?],
    )
    .await
    .map_err(store_err)?;
    Ok(row.map(|r| r.get(0)))
}

/// The tenant's deletion and suppression groups (09 §6.2).
async fn groups<C: GenericClient>(
    client: &C,
    tenant: &str,
) -> Result<(String, String), IngestError> {
    let (s, _) = triplespace_accounts::settings::load(client, Some(tenant))
        .await
        .map_err(store_err)?;
    let get = |k: &str, d: &str| {
        s.value(k)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| d.to_string())
    };
    Ok((
        get("moderation.delete_group", "sysop"),
        get("moderation.suppress_group", "suppress"),
    ))
}

/// What the revert does with one record.
enum Inverse {
    /// Operations on an entity, against the base offset given.
    Operations(Vec<scatter_wikibase_changeset::Operation>, u64),
    /// A page's text restored, against the base offset given.
    PageText(u64, String, u64),
    /// Delete what the record created.
    Delete(Target),
    /// Nothing: left as it is, for the reason given.
    Skip(String),
}

/// Runs a revert of job `reverts` as `actor`, under `job_id` where the runner minted one.
///
/// # Errors
///
/// The store's; a job that does not exist on the tenant.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_revert<C: GenericClient + PgClient>(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    client: &C,
    tenant: &str,
    actor: &str,
    reverts: u64,
    job_id: Option<u64>,
    reason: Option<&str>,
    registry: &Registry,
    now: fn() -> u64,
) -> Result<RevertReport, IngestError> {
    let reverted_row = GenericClient::query_opt(
        client,
        "SELECT status FROM view.job WHERE tenant = $1 AND job_id = $2",
        &[&tenant, &i64::try_from(reverts).map_err(store_err)?],
    )
    .await
    .map_err(store_err)?;
    if reverted_row.is_none() {
        return Err(store_err(format!(
            "job {reverts} is not a job of `{tenant}`"
        )));
    }
    let records = records_of(client, tenant, reverts).await?;
    let (delete_group, suppress_group) = groups(client, tenant).await?;
    let who = triplespace_accounts::identity::effective_of(client, tenant, actor)
        .await
        .map_err(store_err)?
        .ok_or_else(|| store_err(format!("`{actor}` has no account on `{tenant}`")))?;

    // The revert job's own record first, in its own unit of work.
    let mut cx = store.begin().await?;
    let mut header = JobHeader::local("revert");
    header.mode = Some(Mode::Revert);
    header
        .params
        .insert("reverts".into(), serde_json::json!(reverts));
    if let Some(r) = reason.filter(|r| !r.is_empty()) {
        header.params.insert("reason".into(), r.into());
    }
    let attestation = Attestation::by(actor);
    let job = match job_id {
        Some(id) => {
            Job::start_as(
                store,
                pipeline,
                &mut cx,
                tenant,
                id,
                header,
                attestation,
                now(),
            )
            .await?
        }
        None => Job::start(store, pipeline, &mut cx, tenant, header, attestation, now()).await?,
    };
    store.commit(cx).await?;
    let comment = format!("Reverting job {reverts}");

    let mut skipped = Vec::new();
    let mut reverted = 0u64;
    let mut written = 0u64;
    for r in &records {
        let target = r.target_id.clone().unwrap_or_default();
        let skip = |reason: String| Skipped {
            partition: r.partition,
            offset: r.offset,
            target: target.clone(),
            reason,
        };
        if r.kind != "edit" {
            skipped.push(skip(format!(
                "no inverse for a {} record yet",
                r.log.as_deref().unwrap_or(&r.kind)
            )));
            continue;
        }
        let mut cx = store.begin().await?;
        let inverse = match inverse_of(store, client, &mut cx, tenant, r, reverts, job.id).await {
            Ok(i) => i,
            Err(e) => {
                let _ = store.rollback(cx).await;
                return Err(e);
            }
        };
        let mut request = Request::new(tenant, job.record_attestation(), now());
        request.comment = Some(comment.clone());
        let outcome: Result<u64, IngestError> = match inverse {
            Inverse::Skip(reason) => {
                let _ = store.rollback(cx).await;
                skipped.push(skip(reason));
                continue;
            }
            Inverse::Operations(ops, base) => {
                request.base_offset = Some(base);
                let mut n = 0;
                let mut out = Ok(0);
                for op in &ops {
                    match write_operation(store, pipeline, &mut cx, &request, op, registry).await {
                        Ok(w) => {
                            request.base_offset = Some(w.offset());
                            n += 1;
                        }
                        Err(e) => {
                            out = Err(e);
                            break;
                        }
                    }
                }
                out.map(|_| n)
            }
            Inverse::PageText(page_id, text, base) => {
                request.base_offset = Some(base);
                let op = PageOp::Edit {
                    model: None,
                    minor: false,
                    source: None,
                };
                let raw = RawRecord {
                    graph: "pages".into(),
                    payload_type: PAYLOAD_PAGE.into(),
                    key: Some(page_id.to_string()),
                    page_id: Some(page_id),
                    content: Value::from_json(&op.to_json()),
                    extra: vec![Value::text(&text)],
                };
                write_raw(store, pipeline, &mut cx, &request, raw)
                    .await
                    .map(|_| 1)
            }
            Inverse::Delete(target) => {
                let acl = Acl::read(target.clone(), ReadKind::Moderation, &delete_group, None);
                write_acl(
                    store,
                    pipeline,
                    &mut cx,
                    tenant,
                    actor,
                    &who,
                    AclWrite {
                        target,
                        acl: Some(acl),
                        reason: comment.clone(),
                        suppress_group: suppress_group.clone(),
                        reserve: None,
                        enclosing: Vec::new(),
                        job: Some(job.id),
                    },
                    now(),
                )
                .await
                .map(|_| 1)
                .map_err(|e| store_err(e.to_string()))
            }
        };
        match outcome {
            Ok(n) => {
                store.commit(cx).await?;
                reverted += 1;
                written += n;
            }
            Err(IngestError::Conflict { .. }) => {
                let _ = store.rollback(cx).await;
                skipped.push(skip("changed while the revert ran".into()));
            }
            Err(e) => {
                let _ = store.rollback(cx).await;
                skipped.push(skip(format!("the inverse could not be written: {e}")));
            }
        }
    }

    // The finish record, and the reverted job's own `job/revert`.
    let mut cx = store.begin().await?;
    let counts = Counts {
        other: written,
        ..Counts::default()
    };
    let mut extra = serde_json::Map::new();
    extra.insert("reverts".into(), serde_json::json!(reverts));
    extra.insert("reverted".into(), serde_json::json!(reverted));
    extra.insert(
        "skipped".into(),
        serde_json::to_value(&skipped).map_err(store_err)?,
    );
    job.finish(store, pipeline, &mut cx, &counts, extra, now())
        .await?;
    let handle = Job::handle(store, &mut cx, tenant, reverts, Attestation::by(actor)).await?;
    handle
        .reverted(
            store,
            pipeline,
            &mut cx,
            job.id,
            u64::try_from(skipped.len()).unwrap_or(u64::MAX),
            now(),
        )
        .await?;
    store.commit(cx).await?;
    Ok(RevertReport {
        job_id: job.id,
        reverts,
        reverted,
        records: written,
        skipped,
    })
}

/// The inverse of one record, or why there is none.
async fn inverse_of<C: GenericClient + PgClient>(
    store: &PgIngest,
    client: &C,
    cx: &mut triplespace_projections::PgCx,
    tenant: &str,
    r: &JobRecord,
    reverts: u64,
    revert_job: u64,
) -> Result<Inverse, IngestError> {
    let Some(key) = r.target_id.clone() else {
        return Ok(Inverse::Skip("the record names no target".into()));
    };
    // Changed since: a newer record under the key that neither job wrote.
    let Some((latest_offset, _)) = store.latest_for_key(cx, r.partition, &key).await? else {
        return Ok(Inverse::Skip("the key has no record".into()));
    };
    if latest_offset > r.offset {
        let newer = store.read(cx, r.partition, latest_offset).await?;
        let by = newer.as_ref().and_then(|n| attested(n).job);
        if by != Some(reverts) && by != Some(revert_job) {
            return Ok(Inverse::Skip("changed since the job".into()));
        }
    }
    let Some(record) = store.read(cx, r.partition, r.offset).await? else {
        return Ok(Inverse::Skip("the record was compacted away".into()));
    };
    match (r.graph.as_str(), r.target_kind) {
        ("local", Some(target_kind::ENTITY)) => {
            let before = state_before(client, tenant, &key, r.offset).await?;
            let Some(before) = before else {
                // The job created the entity: the inverse is its deletion.
                return Ok(Inverse::Delete(Target::Entity(key)));
            };
            let before = revision_state::decode(&before).map_err(store_err)?;
            let current = revision_state::graph_state(client, tenant, &key, "local")
                .await
                .map_err(store_err)?
                .map_or_else(LocalState::new, |(s, _, _)| s);
            let id = EntityId::parse(&key).map_err(|e| store_err(e.to_string()))?;
            let as_entity = |s: &LocalState, other: Option<&Entity>| -> Option<Entity> {
                s.entity
                    .clone()
                    .or_else(|| other.map(|o| Entity::new(id.clone(), o.entity_type.clone())))
            };
            let was = as_entity(&current, before.entity.as_ref());
            let after = as_entity(&before, current.entity.as_ref());
            let (Some(was), Some(after)) = (was, after) else {
                return Ok(Inverse::Skip(
                    "the entity's states could not be read".into(),
                ));
            };
            let local: Vec<_> = was.all_statements().filter_map(|s| s.id.clone()).collect();
            let ops = operations_from(&was, &local, &after);
            if ops.is_empty() {
                return Ok(Inverse::Skip("nothing to take back".into()));
            }
            Ok(Inverse::Operations(ops, latest_offset))
        }
        ("pages", Some(target_kind::PAGE)) => {
            let Ok(page_id) = key.parse::<u64>() else {
                return Ok(Inverse::Skip("the page ID could not be read".into()));
            };
            let Some(c) = content(&record)? else {
                return Ok(Inverse::Skip("the record's content is erased".into()));
            };
            let op = PageOp::from_json(to_jsonb(&c)).map_err(store_err)?;
            match op {
                PageOp::Create { .. } => Ok(Inverse::Delete(Target::Page(page_id))),
                PageOp::Edit { .. } => {
                    let Some(text) = state_before(client, tenant, &key, r.offset).await? else {
                        return Ok(Inverse::Skip("the previous text could not be read".into()));
                    };
                    Ok(Inverse::PageText(
                        page_id,
                        String::from_utf8_lossy(&text).into_owned(),
                        latest_offset,
                    ))
                }
                PageOp::Move { .. } => Ok(Inverse::Skip("no inverse for a move yet".into())),
            }
        }
        _ => Ok(Inverse::Skip(format!(
            "no inverse for a record in `{}` yet",
            r.graph
        ))),
    }
}
