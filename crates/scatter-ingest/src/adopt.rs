//! The adoption job (0035 §2–5, A12–A14): a frozen source wiki's history becomes the
//! tenant's `local` graph under the source's own IDs. The job checks its preconditions,
//! sets every sequence past what the source consumed, writes the source's accounts as
//! actor records, and writes one `adopt` record per source revision, in source-revision
//! order within each entity: the source's revision ID in header field 7, its page ID in
//! field 9, its edit summary as the comment part and its author as the attestation's
//! actor, minting a surrogate `actor:{n}` for an IP or imported author (0007 A27). On a
//! re-run it skips a revision already present with the same content and rejects one
//! present with different content, and it records the floors and counts on its finish.
//!
//! With [`Adoption::history`] off (`--current-only`), it writes one `adopt` per entity,
//! of its newest revision, under a fresh revision ID and attributed to the job's
//! subsidiary, as before A12.

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

/// The actor payload type (payloads-actors.md), for the surrogates the job mints.
pub const PAYLOAD_ACTOR: &str = "scatter:v0/actor";

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

/// Who made an adopted revision (0035 §3, A12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Author {
    /// The job's subsidiary: a `--current-only` adoption's own revision.
    Job,
    /// An actor key: an adopted account, `{issuer}:{n}`.
    Actor(String),
    /// An author with no stable ID, by surrogate kind (`anonymous` for an IP, `imported`
    /// for `imported>Name`) and raw value; the job mints `actor:{n}` for it, once per raw
    /// value and tenant (0007 §5, A27).
    Surrogate {
        /// `anonymous` or `imported`.
        kind: String,
        /// The IP or the imported name.
        raw: String,
    },
    /// The source hid the user: no actor, and the `hidden` marker (0035 A14).
    Hidden,
}

/// One source revision of an entity to adopt, as the adapter reads it from the frozen
/// source.
#[derive(Debug, Clone, PartialEq)]
pub struct Adopted {
    /// The entity's ID, in local form.
    pub id: EntityId,
    /// The state, IDs in home form (0035 §3); absent for a redirect or a withheld text.
    pub entity: Option<Entity>,
    /// The redirect target, for a revision that made the page a redirect.
    pub redirect: Option<EntityId>,
    /// The source's revision ID.
    pub source_revid: u64,
    /// The source revision it followed, when that one is adopted too.
    pub source_parentid: Option<u64>,
    /// The source's timestamp, its own string.
    pub source_time: String,
    /// The entity's page ID on the source.
    pub source_pageid: u64,
    /// The source's minor-edit flag.
    pub minor: bool,
    /// The parts the source hid: `content`, `comment`, `attestation` (0035 A14).
    pub withheld: Vec<String>,
    /// The edit summary.
    pub comment: Option<String>,
    /// The author.
    pub author: Author,
}

impl Adopted {
    /// A `--current-only` adoption of a state: the job's own revision, with none of the
    /// history fields (0035 §3).
    #[must_use]
    pub fn current(
        entity: Entity,
        source_revid: u64,
        source_time: &str,
        source_pageid: u64,
    ) -> Self {
        Self {
            id: entity.id.clone(),
            entity: Some(entity),
            redirect: None,
            source_revid,
            source_parentid: None,
            source_time: source_time.to_string(),
            source_pageid,
            minor: false,
            withheld: Vec::new(),
            comment: None,
            author: Author::Job,
        }
    }
}

/// An adoption.
#[derive(Debug, Clone)]
pub struct Adoption {
    /// The source's base URL.
    pub source: String,
    /// The issuer code of the adopting tenant, which its accounts carry (0079 §6): the
    /// source's user 42 is `{issuer}:42`.
    pub issuer: String,
    /// The dump's identity, the source version.
    pub version: String,
    /// The adapter version.
    pub adapter_version: Option<String>,
    /// The operator has declared the source frozen (0035 §2, precondition 1).
    pub frozen: bool,
    /// Every revision is adopted under its own ID (0035 A12); off for `--current-only`.
    pub history: bool,
    /// The dump holds one revision per page while revisions name parents: a
    /// current-state export, which a history adoption refuses (0035 §2, precondition 6).
    pub current_state_dump: bool,
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
    if adoption.history && adoption.current_state_dump {
        return Err(IngestError::AdoptionRefused(
            "the dump is a current-state export and carries no history; take a full-history dump (dumpBackup.php --full) or adopt with --current-only".into(),
        ));
    }
    let prefix = format!("{}:", adoption.issuer);
    if let Some(a) = adoption
        .accounts
        .iter()
        .find(|d| !d.key.as_deref().is_some_and(|k| k.starts_with(&prefix)))
    {
        return Err(IngestError::AdoptionRefused(format!(
            "account `{}` is not under the tenant's issuer `{}` (`{tenant}`)",
            a.key.as_deref().unwrap_or("?"),
            adoption.issuer
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

/// Runs an adoption, `batch` revisions per unit of work. The revisions of an entity come
/// in source-revision order. Re-running it against the same dump skips what is present
/// and so completes a partial adoption (0035 §3).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_adoption<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    tenant: &str,
    attestation: Attestation,
    adoption: Adoption,
    entities_in: impl IntoIterator<Item = Adopted>,
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
    header
        .params
        .insert("history".into(), history_name(adoption.history).into());
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
    let job_attestation = job.record_attestation();
    let mut counts = Counts::default();
    let mut rejects = Vec::new();
    let mut cx = store.begin().await?;
    let mut in_unit = 0usize;
    let mut resume = Resume::default();
    let mut surrogates: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut entities = 0u64;
    for (i, adopted) in entities_in.into_iter().enumerate() {
        if in_unit >= batch.max(1) {
            store.commit(cx).await?;
            cx = store.begin().await?;
            in_unit = 0;
        }
        in_unit += 1;
        let line = i + 1;
        let id: EntityId = adopted.id.clone();
        let op = Operation::Adopt {
            id: id.clone(),
            entity: adopted.entity,
            redirect: adopted.redirect,
            source_revid: adopted.source_revid,
            source_parentid: adopted.source_parentid,
            source_time: adopted.source_time,
            source_pageid: adopted.source_pageid,
            minor: adopted.minor,
            withheld: adopted.withheld,
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
                format!(
                    "`{id}` revision {}: {}",
                    adopted.source_revid,
                    problems
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                &mut counts,
                &mut job,
                &mut rejects,
            );
            continue;
        }
        // Idempotent resume (0035 §3): present with the same content is skipped,
        // present with other content is rejected.
        let present = resume
            .newest(store, &mut cx, (tenant, local), &id, adoption.history)
            .await?;
        if let Some(present) = present {
            let same_revision = !adoption.history || adopted.source_revid == present.revid;
            if adoption.history && adopted.source_revid < present.revid {
                counts.unchanged += 1;
                continue;
            }
            if same_revision {
                let content = serde_json::to_value(&op).map_err(|e| e.to_string())?;
                let hash =
                    scatter_log::body::Part::of(&scatter_log::cbor::Value::from_json(&content))
                        .map_err(|e| e.to_string())?
                        .content_hash()
                        .ok_or_else(|| IngestError::Store("content present".into()))?;
                if hash == present.content_hash {
                    counts.unchanged += 1;
                } else {
                    reject(
                        format!(
                            "`{id}` revision {} is present with other content (ts-adopt-conflict)",
                            adopted.source_revid
                        ),
                        &mut counts,
                        &mut job,
                        &mut rejects,
                    );
                }
                continue;
            }
        } else {
            entities += 1;
        }
        let actor = match &adopted.author {
            Author::Job => Some(job_attestation.actor.clone()),
            Author::Actor(key) => Some(key.clone()),
            Author::Hidden => None,
            Author::Surrogate { kind, raw } => Some(
                surrogate(
                    store,
                    pipeline,
                    &mut cx,
                    tenant,
                    (kind, raw),
                    &mut surrogates,
                    &job_attestation,
                    now,
                )
                .await?,
            ),
        };
        let mut request = Request::new(tenant, job_attestation.as_actor(actor.as_deref()), now);
        request.budget = Budget::NONE;
        if adoption.history {
            request.revid = Some(adopted.source_revid);
            request.comment = adopted.comment.filter(|c| !c.is_empty());
        }
        let written = write_to(
            store,
            pipeline,
            &mut cx,
            &request,
            &op,
            &Graph::Local,
            registry,
        )
        .await?;
        resume.wrote(&id, adopted.source_revid, &written.record);
        counts.adopted += 1;
    }
    let mut extra = serde_json::Map::new();
    extra.insert(
        "floors".into(),
        serde_json::to_value(&adoption.floors).map_err(|e| e.to_string())?,
    );
    extra.insert("accounts".into(), accounts.into());
    extra.insert("history".into(), history_name(adoption.history).into());
    extra.insert("entities".into(), entities.into());
    extra.insert("surrogates".into(), (surrogates.len() as u64).into());
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

fn history_name(history: bool) -> &'static str {
    if history { "full" } else { "current" }
}

/// The newest record already present for an entity, for the resume check.
#[derive(Debug, Clone, Copy)]
struct Present {
    revid: u64,
    content_hash: [u8; 32],
}

/// What the job knows of the entity it is on: revisions come grouped by entity, so one
/// lookup per entity answers the resume check for all its revisions.
#[derive(Debug, Default)]
struct Resume {
    entity: Option<EntityId>,
    newest: Option<Present>,
}

impl Resume {
    async fn newest<S: IngestStore>(
        &mut self,
        store: &S,
        cx: &mut S::Cx,
        (tenant, local): (&str, u64),
        id: &EntityId,
        history: bool,
    ) -> Result<Option<Present>, IngestError> {
        if self.entity.as_ref() != Some(id) {
            self.entity = Some(id.clone());
            self.newest = if history {
                match store.latest_for_key(cx, local, id.as_str()).await? {
                    Some((offset, header)) => {
                        let record = store.read(cx, local, offset).await?.ok_or_else(|| {
                            IngestError::Store(format!("`{id}`'s newest record is compacted"))
                        })?;
                        // A `--current-only` record took a fresh revision ID: history can
                        // no longer go in front of it (0035 §7).
                        if let Some(Operation::Adopt { source_revid, .. }) = record
                            .body()
                            .content()
                            .value()
                            .ok()
                            .flatten()
                            .and_then(|v| v.to_json().ok())
                            .and_then(|v| serde_json::from_value::<Operation>(v).ok())
                            && header.revid != Some(source_revid)
                        {
                            return Err(IngestError::AdoptionRefused(format!(
                                "`{id}` was adopted with --current-only, so its history cannot be adopted in front of it; it is backfilled instead (0035 §7)"
                            )));
                        }
                        record
                            .body()
                            .content()
                            .content_hash()
                            .map(|content_hash| Present {
                                revid: header.revid.unwrap_or(0),
                                content_hash,
                            })
                    }
                    None => None,
                }
            } else {
                store
                    .cursor(cx, tenant, id, "local")
                    .await?
                    .map(|c| Present {
                        revid: 0,
                        content_hash: c.content_hash,
                    })
            };
        }
        Ok(self.newest)
    }

    fn wrote(&mut self, id: &EntityId, revid: u64, record: &scatter_log::record::Record) {
        self.entity = Some(id.clone());
        self.newest = record
            .body()
            .content()
            .content_hash()
            .map(|content_hash| Present {
                revid,
                content_hash,
            });
    }
}

/// The surrogate `actor:{n}` for a raw value, minted with its actor record in the
/// tenant's `actors` partition the first time the tenant sees it (0007 §5, A27).
#[allow(clippy::too_many_arguments)]
async fn surrogate<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    tenant: &str,
    (kind, raw): (&str, &str),
    known: &mut BTreeMap<(String, String), String>,
    attestation: &Attestation,
    now: u64,
) -> Result<String, IngestError> {
    let k = (kind.to_string(), raw.to_string());
    if let Some(key) = known.get(&k) {
        return Ok(key.clone());
    }
    if let Some(key) = store.surrogate_actor(cx, tenant, kind, raw).await? {
        known.insert(k, key.clone());
        return Ok(key);
    }
    let actors = store
        .partition(cx, tenant, "actors")
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: tenant.to_string(),
            graph: "actors".into(),
        })?;
    let n = store.next_id(cx, tenant, &Sequence::ActorSurrogate).await?;
    let key = format!("actor:{n}");
    let content = serde_json::json!({"kind": kind, "status": "active", "raw": raw});
    let draft = Draft {
        appended_at: now,
        payload_type: PAYLOAD_ACTOR.into(),
        key: Some(key.clone()),
        revid: None,
        logid: Some(store.next_id(cx, tenant, &Sequence::Log).await?),
        page_id: None,
        body: scatter_log::body::Body::core(
            &scatter_log::cbor::Value::from_json(&content),
            &scatter_log::cbor::Value::Null,
            &attestation.to_value(),
        )
        .map_err(|e| e.to_string())?,
    };
    append_projected(store, pipeline, cx, actors, draft, Budget::NONE).await?;
    known.insert(k, key.clone());
    Ok(key)
}
