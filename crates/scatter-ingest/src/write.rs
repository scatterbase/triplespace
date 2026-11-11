//! The write path (0013 §7; 0015 §1–2): one record-shaped operation becomes one record.
//!
//! The partition is the operation's: a mirror operation goes to the provider's
//! `mirror/{slug}` (an instance partition), a page's statements to the tenant's `pages`,
//! everything else to the tenant's `local`. The key is the subject, or for a keyed entity
//! its surrogate, minted and recorded in the instance `log` the first time (0009 §7). The
//! page ID is the key's, or a fresh one; the revision ID is the tenant's next, or for a
//! mirror record the provider-ranged upstream revision (0015 §2). After the append, the
//! synchronous projections run on the record inside the same unit of work.

use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::record::{Record, global_id};
use scatter_log::store::Draft;
use scatter_projection::{Budget, Inline, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{Graph, Operation, deterministic_statement_id_for};
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, IdForm, Subject};
use scatter_wikibase_model::statement::Statement;

use crate::IngestError;
use crate::store::{IngestStore, Sequence};

/// The change-set payload type.
pub const PAYLOAD_CHANGESET: &str = "scatter:v0/changeset";
/// The surrogate mapping payload type (0009 §7; payloads.md §3.4).
pub const PAYLOAD_KEYED_SURROGATE: &str = "scatter:v0/keyed-surrogate";

/// Who is responsible for a record (payloads.md §2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attestation {
    /// The actor key, `local:42`; ignored when `hidden`.
    pub actor: String,
    /// The job whose run writes the record.
    pub job: Option<u64>,
    /// Change tags.
    pub tags: Vec<String>,
    /// The actor is unknown because the source hid it: the part names no actor and
    /// carries the `hidden` marker instead (0007 §5; 0035 A14).
    pub hidden: bool,
    /// Values the appending transaction minted, which the client neither submitted nor
    /// signed over: a thread's title and talk page IDs (14 §1.5). Emitted after the
    /// common fields, in order.
    pub minted: Vec<(String, MintedValue)>,
}

/// A value the appending transaction minted into an attestation map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintedValue {
    /// A text, such as a title.
    Text(String),
    /// An ID, such as a page ID.
    Id(u64),
    /// A list of IDs, in order.
    Ids(Vec<u64>),
}

impl MintedValue {
    fn to_value(&self) -> Value {
        match self {
            MintedValue::Text(t) => Value::text(t),
            MintedValue::Id(n) => Value::Int(i128::from(*n)),
            MintedValue::Ids(ns) => {
                Value::Array(ns.iter().map(|n| Value::Int(i128::from(*n))).collect())
            }
        }
    }
}

impl Attestation {
    /// An interactive edit by an actor.
    #[must_use]
    pub fn by(actor: &str) -> Self {
        Self {
            actor: actor.to_string(),
            job: None,
            tags: Vec::new(),
            hidden: false,
            minted: Vec::new(),
        }
    }

    /// The same attestation with another actor, or none and the `hidden` marker.
    #[must_use]
    pub fn as_actor(&self, actor: Option<&str>) -> Self {
        Self {
            actor: actor.unwrap_or_default().to_string(),
            hidden: actor.is_none(),
            ..self.clone()
        }
    }

    /// The same attestation under a job.
    #[must_use]
    pub fn with_job(&self, job: Option<u64>) -> Self {
        Self {
            job,
            ..self.clone()
        }
    }

    /// The attestation part's value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut pairs = if self.hidden {
            vec![(Value::text("hidden"), Value::Bool(true))]
        } else {
            vec![(Value::text("actor"), Value::text(&self.actor))]
        };
        if let Some(j) = self.job {
            pairs.push((Value::text("job"), Value::Int(i128::from(j))));
        }
        if !self.tags.is_empty() {
            pairs.push((
                Value::text("tags"),
                Value::Array(self.tags.iter().map(|t| Value::text(t)).collect()),
            ));
        }
        for (k, v) in &self.minted {
            pairs.push((Value::text(k), v.to_value()));
        }
        Value::map(pairs)
    }
}

/// What a write asks for beyond the operation.
#[derive(Debug, Clone)]
pub struct Request {
    /// The tenant whose write it is.
    pub tenant: String,
    /// Who is responsible.
    pub attestation: Attestation,
    /// The edit summary.
    pub comment: Option<String>,
    /// The offset the editor last saw for the key (0006 §8); a newer record is a conflict.
    pub base_offset: Option<u64>,
    /// `appended_at`, µs since the epoch.
    pub now: u64,
    /// The inline fan-out budget.
    pub budget: Budget,
    /// A revision ID the caller supplies in place of the tenant's sequence: an adopted
    /// revision's own, below the floor (0035 §3, A12).
    pub revid: Option<u64>,
    /// A log ID for a record that is also a log event, as a thread `create` or `move` is
    /// (16 §6.1); [`write_raw`] writes it into the header beside the revision ID.
    pub logid: Option<u64>,
}

impl Request {
    /// A request at `now` by an actor, no base offset, the default budget.
    #[must_use]
    pub fn new(tenant: &str, attestation: Attestation, now: u64) -> Self {
        Self {
            tenant: tenant.to_string(),
            attestation,
            comment: None,
            base_offset: None,
            now,
            budget: Budget::default(),
            revid: None,
            logid: None,
        }
    }
}

/// What a write produced.
#[derive(Debug, Clone)]
pub struct Written {
    /// The partition written.
    pub partition: u64,
    /// The record.
    pub record: Record,
    /// What the inline projections did.
    pub inline: Inline,
}

impl Written {
    /// The record's offset.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.record.header().offset
    }
}

/// Where an operation is written: a mirror operation to its provider's mirror, a page's
/// statements to `pages`, everything else to `local`.
pub fn destination(op: &Operation, registry: &Registry) -> Result<Graph, IngestError> {
    if op.is_mirror() {
        let id = op.subject().and_then(|s| s.entity_id().cloned());
        let slug = match id {
            Some(id) if id.form() == IdForm::Foreign => registry
                .by_code(&id.as_str()[..2])
                .map(|p| p.slug.clone())
                .ok_or_else(|| IngestError::UnknownProvider(id.to_string()))?,
            Some(id) if id.form() == IdForm::Keyed => {
                // A source's entity (0078 §2) names its source by its prefix, in the
                // tenant's table of names (`registry`).
                if let Ok(f) = registry.parse_foreign_id(id.as_str())
                    && f.provider().source
                {
                    return Ok(Graph::Source(f.provider().slug.clone()));
                }
                // A key-mapped put names its provider only through `upstream_id`'s
                // owner, which the caller knows; the mirror graph is given by the job.
                return Err(IngestError::Adapter(format!(
                    "a mirror operation on `{id}` needs its graph named by the job (write_to)"
                )));
            }
            other => {
                return Err(IngestError::Adapter(format!(
                    "a mirror operation without a foreign subject: {other:?}"
                )));
            }
        };
        return Ok(Graph::Mirror(slug));
    }
    match op.subject() {
        Some(Subject::Page(_)) => Ok(Graph::Pages),
        _ => Ok(Graph::Local),
    }
}

/// Appends a draft and runs the synchronous projections on the record it became, inside
/// the unit of work: every record the write side produces goes through this, so the
/// `view` tables are current when the unit commits (0013 §7).
pub async fn append_projected<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    partition: u64,
    draft: Draft,
    budget: Budget,
) -> Result<(Record, Inline), IngestError> {
    let (_, record) = store.append_sealed(cx, partition, draft).await?;
    let inline = pipeline.apply_inline(store, cx, &record, budget).await?;
    Ok((record, inline))
}

/// The header key of a subject: the entity ID, or a keyed entity's surrogate, minted and
/// recorded in the instance `log` when it has none yet. The mapping record is projected
/// inline, so the resolution projection finds the surrogate in the same unit of work. A
/// source's entity is in the colon shape but needs no surrogate (0078 §2): its key is
/// its ID, as `WDQ42`'s is; `registry` is the tenant's table of names that says so.
pub async fn key_for<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    id: &EntityId,
    attestation: &Attestation,
    now: u64,
    registry: &Registry,
) -> Result<String, IngestError> {
    if id.form() != IdForm::Keyed || registry.parse_foreign_id(id.as_str()).is_ok() {
        return Ok(id.as_str().to_string());
    }
    let (keyed_type, key) = id
        .keyed_parts()
        .ok_or_else(|| IngestError::Store("a keyed ID has a type".into()))?;
    if let Some(n) = store.surrogate(cx, keyed_type, key).await? {
        return Ok(format!("{keyed_type}#{n}"));
    }
    let n = store
        .next_id(cx, "", &Sequence::Surrogate(keyed_type.to_string()))
        .await?;
    let surrogate = format!("{keyed_type}#{n}");
    let instance_log =
        store
            .partition(cx, "", "log")
            .await?
            .ok_or_else(|| IngestError::NoPartition {
                tenant: String::new(),
                graph: "log".into(),
            })?;
    let content = serde_json::json!({"keyed_type": keyed_type, "surrogate": n, "key": key});
    let logid = store.next_id(cx, "", &Sequence::Log).await?;
    let draft = Draft {
        appended_at: now,
        payload_type: PAYLOAD_KEYED_SURROGATE.into(),
        key: Some(surrogate.clone()),
        revid: None,
        logid: Some(logid),
        page_id: None,
        body: Body::core(
            &Value::from_json(&content),
            &Value::Null,
            &attestation.to_value(),
        )
        .map_err(|e| e.to_string())?,
    };
    append_projected(store, pipeline, cx, instance_log, draft, Budget::NONE).await?;
    Ok(surrogate)
}

/// Writes one record-shaped operation to the partition [`destination`] names, then runs
/// the synchronous projections on it.
pub async fn write_operation<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    request: &Request,
    op: &Operation,
    registry: &Registry,
) -> Result<Written, IngestError> {
    let graph = destination(op, registry)?;
    write_to(store, pipeline, cx, request, op, &graph, registry).await
}

/// Writes one record-shaped operation to a named graph: the tenant's for `local` and
/// `pages`, the instance's for a mirror.
#[allow(clippy::too_many_lines)]
pub async fn write_to<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    request: &Request,
    op: &Operation,
    graph: &Graph,
    registry: &Registry,
) -> Result<Written, IngestError> {
    if !op.is_record_shape() {
        return Err(IngestError::Adapter(format!(
            "`{}` is not a record shape; resolve it first",
            op.name()
        )));
    }
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
    let subject = op
        .subject()
        .ok_or_else(|| IngestError::Adapter(format!("`{}` has no subject", op.name())))?;
    let key = match &subject {
        Subject::Entity(id) => {
            key_for(
                store,
                pipeline,
                cx,
                id,
                &request.attestation,
                request.now,
                registry,
            )
            .await?
        }
        Subject::Page(p) => p.to_string(),
    };
    // The base-offset check (0006 §8).
    let latest = store.latest_for_key(cx, partition, &key).await?;
    if let (Some(base), Some((latest_offset, _))) = (request.base_offset, &latest)
        && *latest_offset > base
    {
        return Err(IngestError::Conflict {
            key,
            base,
            latest: *latest_offset,
        });
    }
    // The page ID: the key's, the adoption's, or a fresh one (0015 §2; 0035 §4).
    let page_id = match store.page_id_of(cx, &request.tenant, &key).await? {
        Some(p) => p,
        None => match op {
            Operation::Adopt { source_pageid, .. } => *source_pageid,
            _ => store.next_id(cx, owner, &Sequence::Page).await?,
        },
    };
    // The revision ID (0013 §6; 0015 §2).
    let revid = match graph {
        Graph::Mirror(_) | Graph::Source(_) => upstream_revid(graph, op, registry)?,
        Graph::Local | Graph::Pages => match request.revid {
            Some(n) => Some(n),
            None => Some(store.next_id(cx, owner, &Sequence::Revision).await?),
        },
    };
    let draft = draft_of(store, request, op, &subject, key, page_id, revid)?;
    let (record, inline) =
        append_projected(store, pipeline, cx, partition, draft, request.budget).await?;
    Ok(Written {
        partition,
        record,
        inline,
    })
}

/// The revision ID of a mirror or source record: the provider's range, at the upstream
/// revision the operation carries (0013 §6; 0015 §2).
fn upstream_revid(
    graph: &Graph,
    op: &Operation,
    registry: &Registry,
) -> Result<Option<u64>, IngestError> {
    let (Graph::Mirror(slug) | Graph::Source(slug)) = graph else {
        return Ok(None);
    };
    let provider = registry
        .by_slug(slug)
        .ok_or_else(|| IngestError::UnknownProvider(slug.clone()))?;
    let upstream = match op {
        Operation::Put { upstream, .. } | Operation::Tombstone { upstream, .. } => upstream.revid,
        Operation::Redirect {
            upstream: Some(u), ..
        } => u.revid,
        _ => None,
    };
    upstream
        .map(|n| {
            global_id(u64::from(provider.number), n).ok_or_else(|| {
                IngestError::Adapter(format!("upstream revision {n} is out of range"))
            })
        })
        .transpose()
}

/// The draft of a change-set record, its key, page ID and revision ID settled, the
/// operation handed to the store's projections once encoded ([`IngestStore::encoded`]).
fn draft_of<S: IngestStore>(
    store: &S,
    request: &Request,
    op: &Operation,
    subject: &Subject,
    key: String,
    page_id: u64,
    revid: Option<u64>,
) -> Result<Draft, IngestError> {
    // Local statements without a GUID get one here, as Wikibase assigns them on save;
    // deterministic, so the same statement re-added gets the same ID (0002 §8.4).
    let op = with_statement_ids(op, subject);
    let content = serde_json::to_value(&op).map_err(|e| e.to_string())?;
    let comment = request.comment.as_deref().map_or(Value::Null, Value::text);
    let draft = Draft {
        appended_at: request.now,
        payload_type: PAYLOAD_CHANGESET.into(),
        key: Some(key),
        revid,
        logid: None,
        page_id: Some(page_id),
        body: Body::core(
            &Value::from_json_owned(content),
            &comment,
            &request.attestation.to_value(),
        )
        .map_err(|e| e.to_string())?,
    };
    store.encoded(&draft.body, op);
    Ok(draft)
}

/// Whether an entity is its own header key, as a block's subjects are ([`write_block`]):
/// any entity but a keyed one, whose key is its surrogate (0009 §7); a source's entity
/// is in the colon shape but is a foreign entity (0078 §2), and its own key.
fn is_own_key(id: &EntityId, registry: &Registry) -> bool {
    id.form() != IdForm::Keyed || registry.parse_foreign_id(id.as_str()).is_ok()
}

/// Whether an operation can be one of a block's ([`write_block`]): a record shape whose
/// subject is an entity that is its own key.
#[must_use]
pub fn is_block_operation(op: &Operation, registry: &Registry) -> bool {
    op.is_record_shape()
        && matches!(op.subject(), Some(Subject::Entity(id)) if is_own_key(&id, registry))
}

/// Writes a sync unit's operations to a mirror or source graph as one block (0013 A76):
/// what [`write_to`] writes for each, in order, with the lookups made once for the block,
/// the records appended together ([`IngestStore::append_many`]) and the synchronous
/// projections run over the block ([`Pipeline::apply_inline_many`]). Each operation's
/// subject is an entity that is its own key, no two the same, and the request carries no
/// base offset: a unit that is not so is written operation by operation. It is
/// [`write_many`] with one request for the block and those conditions asked of it.
///
/// # Errors
///
/// When an operation is not of that kind, a lookup or the append fails, or a projection
/// fails.
#[allow(clippy::too_many_arguments)]
pub async fn write_block<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    request: &Request,
    partition: u64,
    ops: &[Operation],
    graph: &Graph,
    registry: &Registry,
) -> Result<Vec<Record>, IngestError> {
    if !matches!(graph, Graph::Mirror(_) | Graph::Source(_)) {
        return Err(IngestError::Adapter(
            "a sync's block is written to a mirror or source graph".into(),
        ));
    }
    let mut keys = std::collections::BTreeSet::new();
    for op in ops {
        match op.subject() {
            Some(Subject::Entity(id)) if op.is_record_shape() && is_own_key(&id, registry) => {
                if !keys.insert(id) {
                    return Err(IngestError::Adapter("a block names a subject twice".into()));
                }
            }
            _ => {
                return Err(IngestError::Adapter(format!(
                    "`{}` in a block needs an entity that is its own key",
                    op.name()
                )));
            }
        }
    }
    let entries: Vec<BlockEntry<'_>> = ops
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
    .await
}

/// What one record of a block ([`write_many`]) is made from.
#[derive(Debug, Clone, Copy)]
pub enum BlockPayload<'a> {
    /// A record-shaped operation, as [`write_to`] takes.
    Operation(&'a Operation),
    /// A record whose payload the caller encoded, as [`write_raw`] takes.
    Raw(&'a RawRecord),
}

/// One record of a block ([`write_many`]): its payload, and the request that says who is
/// responsible for it, its comment and the revision ID it brings, if any. Each record has
/// its own, since an adoption or an import attributes every revision to its author.
#[derive(Debug, Clone, Copy)]
pub struct BlockEntry<'a> {
    /// The record's request. Its tenant is the block's; it carries no base offset.
    pub request: &'a Request,
    /// The record's payload.
    pub payload: BlockPayload<'a>,
}

/// One entry of a block with its key and IDs settled.
struct Settled<'a> {
    entry: BlockEntry<'a>,
    subject: Option<Subject>,
    key: String,
    page_id: u64,
    revid: Option<u64>,
}

/// Writes a block of records to one partition (0013 A87): what [`write_to`] writes for
/// each operation and [`write_raw`] for each caller-encoded record, in order, at
/// consecutive offsets, with the same keys, page IDs and revision IDs, the lookups and
/// the sequences taken once for the block, the records appended together
/// ([`IngestStore::append_many`]) and the synchronous projections run over blocks
/// ([`Pipeline::apply_inline_many`]).
///
/// `graph` is the partition's: a mirror or source graph, whose records carry their
/// upstream revision, or the tenant's `local` or `pages`, whose records carry the
/// revision ID their request brings or the tenant's next. A key gets the page ID it
/// has; one with none gets what its first record brings (an adoption's source page ID,
/// a raw record's) or the next of the sequence, in record order, and keeps it for the
/// rest of the block. A keyed entity's surrogate is looked up, or minted, before the
/// block is written ([`key_for`]).
///
/// A key may come more than once. The records are appended together all the same, but
/// the projections run over each stretch of records in which no key repeats, one
/// stretch after another: a projection applied over a block sees what the earlier
/// projections wrote for the whole block, which for two records of one key is not what
/// it sees one record at a time.
///
/// A block makes no base-offset check (0006 §8): a write that has one is an
/// interactive one, written by itself.
///
/// # Errors
///
/// When the entries are not of one tenant, one carries a base offset or does not belong
/// to the graph, a lookup or the append fails, or a projection fails.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn write_many<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    partition: u64,
    graph: &Graph,
    entries: &[BlockEntry<'_>],
    budget: Budget,
    registry: &Registry,
) -> Result<Vec<Record>, IngestError> {
    let Some(first) = entries.first() else {
        return Ok(Vec::new());
    };
    let tenant = first.request.tenant.as_str();
    let upstream = matches!(graph, Graph::Mirror(_) | Graph::Source(_));
    let owner = match graph {
        Graph::Mirror(_) => "",
        Graph::Local | Graph::Pages | Graph::Source(_) => tenant,
    };
    let graph_name = graph.name();

    // Each entry's subject and key, and the page ID it brings for a key that has none.
    let mut keyed: Vec<(Option<Subject>, Option<String>, Option<u64>)> =
        Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.request.tenant != tenant {
            return Err(IngestError::Adapter(
                "a block's records are one tenant's".into(),
            ));
        }
        if entry.request.base_offset.is_some() {
            return Err(IngestError::Adapter(
                "a block's records carry no base offset".into(),
            ));
        }
        match entry.payload {
            BlockPayload::Operation(op) => {
                if !op.is_record_shape() {
                    return Err(IngestError::Adapter(format!(
                        "`{}` is not a record shape; resolve it first",
                        op.name()
                    )));
                }
                let subject = op.subject().ok_or_else(|| {
                    IngestError::Adapter(format!("`{}` has no subject", op.name()))
                })?;
                let key = match &subject {
                    Subject::Entity(id) => {
                        key_for(
                            store,
                            pipeline,
                            cx,
                            id,
                            &entry.request.attestation,
                            entry.request.now,
                            registry,
                        )
                        .await?
                    }
                    Subject::Page(p) => p.to_string(),
                };
                let brings = match op {
                    Operation::Adopt { source_pageid, .. } => Some(*source_pageid),
                    _ => None,
                };
                keyed.push((Some(subject), Some(key), brings));
            }
            BlockPayload::Raw(raw) => {
                if upstream || raw.graph != graph_name {
                    return Err(IngestError::Adapter(format!(
                        "a `{}` record of `{}` in a block of `{graph_name}`",
                        raw.payload_type, raw.graph
                    )));
                }
                keyed.push((None, raw.key.clone(), raw.page_id));
            }
        }
    }

    // Page IDs (0015 §2; 0035 §4): the key's; else what its first record in the block
    // brings; else the next ones, in record order, as one write at a time takes them.
    let mut distinct: Vec<String> = keyed.iter().filter_map(|(_, k, _)| k.clone()).collect();
    distinct.sort();
    distinct.dedup();
    let mut page_ids = store.page_ids_of(cx, tenant, &distinct).await?;
    let mut fresh = 0usize;
    {
        let mut first_seen = std::collections::BTreeSet::new();
        for (_, key, brings) in &keyed {
            let new = match key {
                Some(k) => !page_ids.contains_key(k) && first_seen.insert(k.as_str()),
                None => true,
            };
            if new && brings.is_none() {
                fresh += 1;
            }
        }
    }
    let mut fresh = store
        .next_ids(cx, owner, &Sequence::Page, fresh)
        .await?
        .into_iter();
    // Revision IDs (0013 §6; 0015 §2): a tenant record's own, or the tenant's next.
    let minted = if upstream {
        0
    } else {
        entries.iter().filter(|e| e.request.revid.is_none()).count()
    };
    // A mirror has no revision sequence to ask, even for none.
    let mut revids = if minted == 0 {
        Vec::new()
    } else {
        store
            .next_ids(cx, owner, &Sequence::Revision, minted)
            .await?
    }
    .into_iter();
    let mut settled = Vec::with_capacity(entries.len());
    for (entry, (subject, key, brings)) in entries.iter().zip(keyed) {
        let mut next_page = || {
            brings.map_or_else(
                || {
                    fresh
                        .next()
                        .ok_or_else(|| IngestError::Store("a page ID for each new key".into()))
                },
                Ok,
            )
        };
        let (key, page_id) = if let Some(key) = key {
            let page_id = if let Some(&p) = page_ids.get(&key) {
                p
            } else {
                let p = next_page()?;
                page_ids.insert(key.clone(), p);
                p
            };
            (key, page_id)
        } else {
            // A record with no key is keyed by the page ID it mints (10 §3.1).
            let p = next_page()?;
            (p.to_string(), p)
        };
        let revid = match (entry.payload, upstream) {
            (BlockPayload::Operation(op), true) => upstream_revid(graph, op, registry)?,
            _ => Some(match entry.request.revid {
                Some(n) => n,
                None => revids
                    .next()
                    .ok_or_else(|| IngestError::Store("a revision ID for each record".into()))?,
            }),
        };
        settled.push(Settled {
            entry: *entry,
            subject,
            key,
            page_id,
            revid,
        });
    }

    // The stretches the projections take: a stretch ends before a key it already holds.
    let mut stretches: Vec<usize> = Vec::new();
    {
        let mut held = std::collections::BTreeSet::new();
        for (i, s) in settled.iter().enumerate() {
            if !held.insert(s.key.as_str()) {
                stretches.push(i);
                held.clear();
                held.insert(s.key.as_str());
            }
        }
        stretches.push(settled.len());
    }

    // Encoding and hashing each record is its own work: on every core.
    let drafts = scatter_projection::par_map(settled, |s| {
        let draft = match (s.entry.payload, &s.subject) {
            (BlockPayload::Operation(op), Some(subject)) => draft_of(
                store,
                s.entry.request,
                op,
                subject,
                s.key,
                s.page_id,
                s.revid,
            )?,
            (BlockPayload::Raw(raw), _) => raw_draft(
                s.entry.request,
                raw,
                s.key,
                s.page_id,
                s.revid
                    .ok_or_else(|| IngestError::Store("a raw record has a revision ID".into()))?,
            )?,
            (BlockPayload::Operation(op), None) => {
                return Err(IngestError::Adapter(format!(
                    "`{}` has no subject",
                    op.name()
                )));
            }
        };
        let commitment = draft.body.commitment();
        Ok::<_, IngestError>((draft, commitment))
    })
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let records = store.append_many(cx, partition, drafts).await?;
    let mut from = 0;
    for to in stretches {
        pipeline
            .apply_inline_many(store, cx, &records[from..to], budget)
            .await?;
        from = to;
    }
    Ok(records)
}

/// [`append_projected`] for a block: drafts appended at consecutive offsets, each with its
/// body's commitment ([`IngestStore::append_many`]), and the synchronous projections run
/// over the block at once ([`Pipeline::apply_inline_many`]). What both a bootstrap load
/// (0013 §9) and a sync's unit ([`write_block`]; A76) write a block with.
///
/// # Errors
///
/// When the append or a projection fails.
pub async fn append_projected_many<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    partition: u64,
    drafts: Vec<(Draft, scatter_log::hash::Hash)>,
    budget: Budget,
) -> Result<Vec<Record>, IngestError> {
    let records = store.append_many(cx, partition, drafts).await?;
    pipeline
        .apply_inline_many(store, cx, &records, budget)
        .await?;
    Ok(records)
}

/// A record whose payload the caller encodes: one this crate does not model, such as a
/// document page's (`scatter:v0/page`, 10 §3.2), whose crate the write side does not
/// depend on (22 §2.1).
#[derive(Debug, Clone)]
pub struct RawRecord {
    /// The tenant graph it goes to: `pages`.
    pub graph: String,
    /// The payload type.
    pub payload_type: String,
    /// The header key. `None` mints a page ID and keys the record by it, as a page's
    /// `create` does (10 §3.1).
    pub key: Option<String>,
    /// The page ID a key gets when it has none yet: an adopted page's source page ID
    /// (0035 A13). `None` takes the tenant's next.
    pub page_id: Option<u64>,
    /// The content part.
    pub content: Value,
    /// The parts after the three standard ones, in order.
    pub extra: Vec<Value>,
}

/// The draft of a caller-encoded record, its key, page ID and revision ID settled.
fn raw_draft(
    request: &Request,
    raw: &RawRecord,
    key: String,
    page_id: u64,
    revid: u64,
) -> Result<Draft, IngestError> {
    let comment = request.comment.as_deref().map_or(Value::Null, Value::text);
    let mut body = Body::core(&raw.content, &comment, &request.attestation.to_value())
        .map_err(|e| e.to_string())?;
    for part in &raw.extra {
        body.push(scatter_log::body::Part::of(part).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    Ok(Draft {
        appended_at: request.now,
        payload_type: raw.payload_type.clone(),
        key: Some(key),
        revid: Some(revid),
        logid: request.logid,
        page_id: Some(page_id),
        body,
    })
}

/// Writes a caller-encoded record to a tenant graph with the base-offset check, the
/// page ID and the revision ID of [`write_to`], then runs the synchronous projections.
pub async fn write_raw<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    request: &Request,
    raw: RawRecord,
) -> Result<Written, IngestError> {
    let owner = request.tenant.as_str();
    let partition = store
        .partition(cx, owner, &raw.graph)
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: owner.to_string(),
            graph: raw.graph.clone(),
        })?;
    let (key, page_id) = if let Some(key) = raw.key.clone() {
        let latest = store.latest_for_key(cx, partition, &key).await?;
        if let (Some(base), Some((latest_offset, _))) = (request.base_offset, &latest)
            && *latest_offset > base
        {
            return Err(IngestError::Conflict {
                key,
                base,
                latest: *latest_offset,
            });
        }
        let page_id = match store.page_id_of(cx, owner, &key).await? {
            Some(p) => p,
            None => match raw.page_id {
                Some(p) => p,
                None => store.next_id(cx, owner, &Sequence::Page).await?,
            },
        };
        (key, page_id)
    } else {
        let page_id = match raw.page_id {
            Some(p) => p,
            None => store.next_id(cx, owner, &Sequence::Page).await?,
        };
        (page_id.to_string(), page_id)
    };
    let revid = match request.revid {
        Some(n) => n,
        None => store.next_id(cx, owner, &Sequence::Revision).await?,
    };
    let draft = raw_draft(request, &raw, key, page_id, revid)?;
    let (record, inline) =
        append_projected(store, pipeline, cx, partition, draft, request.budget).await?;
    Ok(Written {
        partition,
        record,
        inline,
    })
}

/// The operation with every statement it carries given an ID under `subject`, where it
/// had none. Mirror operations are returned as they are: their adapter owns the IDs.
#[must_use]
pub fn with_statement_ids(op: &Operation, subject: &Subject) -> Operation {
    let hasher = Hasher::local();
    let mut op = op.clone();
    let fill = |statements: &mut Vec<Statement>, subject: &Subject| {
        for s in statements {
            if s.id.is_none() {
                s.id = Some(deterministic_statement_id_for(subject, s, &hasher));
            }
        }
    };
    match &mut op {
        Operation::Create { entity, .. }
        | Operation::Adopt {
            entity: Some(entity),
            ..
        } => {
            for group in entity.statements.values_mut() {
                fill(group, subject);
            }
        }
        Operation::Add { claims, entity, .. } => {
            for group in claims.values_mut() {
                fill(group, subject);
            }
            if let Some(e) = entity {
                for group in e.statements.values_mut() {
                    fill(group, subject);
                }
            }
        }
        _ => {}
    }
    op
}

/// The ID a freshly minted local entity of a type gets: the type letter and the next
/// number of the tenant's sequence for the type (0013 §6).
pub async fn mint_entity_id<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    entity_type: &str,
) -> Result<EntityId, IngestError> {
    let letter = match entity_type {
        "item" => 'Q',
        "property" => 'P',
        "lexeme" => 'L',
        other => return Err(IngestError::NoIdFor(other.to_string())),
    };
    let n = store
        .next_id(cx, tenant, &Sequence::Entity(entity_type.to_string()))
        .await?;
    EntityId::parse(&format!("{letter}{n}")).map_err(|e| IngestError::Store(e.to_string()))
}
