//! A job's records (0002 §8.3; 0011 §6.3; payloads.md §5): a `job/start` log event when
//! it starts, keyed by the job ID the instance mints, and a `job/finish` or `job/fail`
//! under the same key; a `job/interrupt` under that key when a later load of the same
//! source and graph finds it still running (0011 A36). A tenant's bulk job writes them to the tenant's `log`; a mirror
//! sync, an instance action, to the instance `log`. Every record is projected inline, so
//! `view.job` is current when the unit of work commits.

use std::collections::BTreeMap;

use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_wikibase_changeset::{JobHeader, MatchKey};
use serde::{Deserialize, Serialize};

use crate::IngestError;
use crate::store::{IngestStore, Sequence};
use crate::write::{Attestation, append_projected};
use scatter_projection::{Budget, Pipeline};

/// The log-event payload type.
pub const PAYLOAD_LOGEVENT: &str = "scatter:v0/logevent";

/// How long a dump load waits for another load of the same source and graph, such as a
/// follower's batch, before it gives up (0011 A37). A follower's batch waits as long as
/// it takes (A38).
pub const LOAD_LOCK_WAIT: std::time::Duration = std::time::Duration::from_mins(1);

/// How many rejects a finish record carries for the job page (0010 §9).
pub const REJECTS_KEPT: usize = 100;

/// A rejected line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reject {
    /// The line in the batch, 1-based; the job line is 1.
    pub line: usize,
    /// The match key, where the operation had one.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "match")]
    pub match_key: Option<MatchKey>,
    /// Why.
    pub reason: String,
}

/// A job's counts by outcome (0011 §6.3).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Counts {
    /// Entities created.
    pub created: u64,
    /// Operations merged onto existing subjects.
    pub merged: u64,
    /// Operations that changed nothing and were skipped.
    pub unchanged: u64,
    /// Operations rejected.
    pub rejected: u64,
    /// Revisions adopted (0035 A12): one per entity under `--current-only`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub adopted: u64,
    /// Entities tombstoned, by a `tombstone` or the `snapshot` sweep.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tombstoned: u64,
    /// Mirror contributions cleared with a `put` of the empty state: a keyed entity whose
    /// upstream item went away, or an entity of a `deletion = clear` provider (0009 §9).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cleared: u64,
    /// Other records written: redirects, retains, overrides, links.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub other: u64,
}

impl Counts {
    /// Adds `other`'s counts to these.
    pub fn add(&mut self, other: &Counts) {
        self.created += other.created;
        self.merged += other.merged;
        self.unchanged += other.unchanged;
        self.rejected += other.rejected;
        self.adopted += other.adopted;
        self.tombstoned += other.tombstoned;
        self.cleared += other.cleared;
        self.other += other.other;
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// A running job.
#[derive(Debug, Clone)]
pub struct Job {
    /// The job ID.
    pub id: u64,
    /// The tenant whose `log` holds the records; `""` for an instance job.
    pub tenant: String,
    /// The partition of that log.
    pub log_partition: u64,
    /// The job line.
    pub header: JobHeader,
    /// Who runs it: the subsidiary, and the tags.
    pub attestation: Attestation,
    /// The offset of the start record.
    pub start_offset: u64,
    /// Upstream snak and reference hashes kept because the recomputation differed, by
    /// value type (0006 §2).
    pub hash_mismatches: BTreeMap<String, u64>,
    /// The first [`REJECTS_KEPT`] rejects.
    pub rejects: Vec<Reject>,
    /// How many were rejected in all.
    pub rejected_total: u64,
}

impl Job {
    /// Mints a job ID and appends the start record to `tenant`'s `log` (the instance's
    /// when `tenant` is `""`), with the job line's fields as parameters.
    pub async fn start<S: IngestStore>(
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        tenant: &str,
        header: JobHeader,
        attestation: Attestation,
        now: u64,
    ) -> Result<Self, IngestError> {
        let id = store.next_id(cx, "", &Sequence::Job).await?;
        Self::start_as(store, pipeline, cx, tenant, id, header, attestation, now).await
    }

    /// Appends the start record under a job ID minted earlier: the runner's, minted at
    /// submission so that the job has an ID while it waits (0104 §4).
    #[allow(clippy::too_many_arguments)]
    pub async fn start_as<S: IngestStore>(
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        tenant: &str,
        id: u64,
        header: JobHeader,
        attestation: Attestation,
        now: u64,
    ) -> Result<Self, IngestError> {
        let log_partition =
            store
                .partition(cx, tenant, "log")
                .await?
                .ok_or_else(|| IngestError::NoPartition {
                    tenant: tenant.to_string(),
                    graph: "log".to_string(),
                })?;
        let mut params = serde_json::Map::new();
        params.insert("source".into(), header.source.clone().into());
        if let Some(v) = header.source_version() {
            params.insert("version".into(), v.into());
        }
        if let Some(m) = header.mode {
            params.insert(
                "mode".into(),
                serde_json::to_value(m).map_err(|e| e.to_string())?,
            );
        }
        params.insert("graph".into(), header.graph.clone().into());
        if let Some(a) = &header.adapter_version {
            params.insert("adapter".into(), a.clone().into());
        }
        if !header.params.is_empty() {
            params.insert(
                "args".into(),
                serde_json::Value::Object(header.params.clone().into_iter().collect()),
            );
        }
        let mut job = Self {
            id,
            tenant: tenant.to_string(),
            log_partition,
            header,
            attestation,
            start_offset: 0,
            hash_mismatches: BTreeMap::new(),
            rejects: Vec::new(),
            rejected_total: 0,
        };
        let draft = job
            .event(store, cx, "start", serde_json::Value::Object(params), now)
            .await?;
        let (record, _) =
            append_projected(store, pipeline, cx, log_partition, draft, Budget::NONE).await?;
        job.start_offset = record.header().offset;
        Ok(job)
    }

    /// A `job/{action}` event draft keyed by the job ID, with a fresh log ID.
    async fn event<S: IngestStore>(
        &self,
        store: &S,
        cx: &mut S::Cx,
        action: &str,
        params: serde_json::Value,
        now: u64,
    ) -> Result<Draft, IngestError> {
        self.event_for(store, cx, self.id, action, params, now)
            .await
    }

    /// A `job/{action}` event draft about job `target`, keyed by its ID, under this
    /// job's attestation.
    async fn event_for<S: IngestStore>(
        &self,
        store: &S,
        cx: &mut S::Cx,
        target: u64,
        action: &str,
        params: serde_json::Value,
        now: u64,
    ) -> Result<Draft, IngestError> {
        let logid = store.next_id(cx, &self.tenant, &Sequence::Log).await?;
        let content = serde_json::json!({
            "type": "job", "action": action, "time": now,
            "target": {"kind": "job", "id": target},
            "params": params,
        });
        Ok(Draft {
            appended_at: now,
            payload_type: PAYLOAD_LOGEVENT.into(),
            key: Some(target.to_string()),
            revid: None,
            logid: Some(logid),
            page_id: None,
            body: Body::core(
                &Value::from_json(&content),
                &Value::Null,
                &self.attestation.with_job(None).to_value(),
            )
            .map_err(|e| e.to_string())?,
        })
    }

    /// Records a reject, keeping the first [`REJECTS_KEPT`] for the finish record.
    pub fn reject(&mut self, reject: Reject) {
        self.rejected_total += 1;
        if self.rejects.len() < REJECTS_KEPT {
            self.rejects.push(reject);
        }
    }

    /// Counts a hash mismatch by value type.
    pub fn mismatch(&mut self, value_type: &str, n: u64) {
        if n > 0 {
            *self
                .hash_mismatches
                .entry(value_type.to_string())
                .or_default() += n;
        }
    }

    /// Appends the finish record with the counts, the hash mismatches, the rejects, and
    /// whatever else the job has to say (`checkpoint`, `sweep`, `floors`).
    pub async fn finish<S: IngestStore>(
        &self,
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        counts: &Counts,
        extra: serde_json::Map<String, serde_json::Value>,
        now: u64,
    ) -> Result<u64, IngestError> {
        let mut params = extra;
        params.insert(
            "counts".into(),
            serde_json::to_value(counts).map_err(|e| e.to_string())?,
        );
        if !self.hash_mismatches.is_empty() {
            params.insert(
                "hash_mismatches".into(),
                serde_json::to_value(&self.hash_mismatches).map_err(|e| e.to_string())?,
            );
        }
        if !self.rejects.is_empty() {
            params.insert(
                "rejects".into(),
                serde_json::to_value(&self.rejects).map_err(|e| e.to_string())?,
            );
        }
        let draft = self
            .event(store, cx, "finish", serde_json::Value::Object(params), now)
            .await?;
        let (record, _) =
            append_projected(store, pipeline, cx, self.log_partition, draft, Budget::NONE).await?;
        Ok(record.header().offset)
    }

    /// Appends the failure record.
    pub async fn fail<S: IngestStore>(
        &self,
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        error: &str,
        now: u64,
    ) -> Result<u64, IngestError> {
        let draft = self
            .event(store, cx, "fail", serde_json::json!({"error": error}), now)
            .await?;
        let (record, _) =
            append_projected(store, pipeline, cx, self.log_partition, draft, Budget::NONE).await?;
        Ok(record.header().offset)
    }

    /// Marks interrupted every earlier job of the same tenant, source and graph that is
    /// still `running`: a load whose process died without a finish or fail record. Each
    /// gets a `job/interrupt` record under its own key, naming this job as `by` (0011 A36).
    /// A follower's job is open between its batches, so only a follower's new job marks
    /// an earlier follower's (0092 A6). Returns the IDs marked.
    pub async fn interrupt_earlier<S: IngestStore>(
        &self,
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        now: u64,
    ) -> Result<Vec<u64>, IngestError> {
        let earlier: Vec<u64> = store
            .running_jobs(
                cx,
                &self.tenant,
                &self.header.source,
                &self.header.graph,
                self.is_follow(),
            )
            .await?
            .into_iter()
            .filter(|&id| id != self.id)
            .collect();
        for &id in &earlier {
            let draft = self
                .event_for(
                    store,
                    cx,
                    id,
                    "interrupt",
                    serde_json::json!({"by": self.id}),
                    now,
                )
                .await?;
            append_projected(store, pipeline, cx, self.log_partition, draft, Budget::NONE).await?;
        }
        Ok(earlier)
    }

    /// Whether this is a follower's job (`follow` among its arguments).
    #[must_use]
    pub fn is_follow(&self) -> bool {
        self.header
            .params
            .get("follow")
            .is_some_and(|f| f.as_bool() == Some(true))
    }

    /// A follower's job started earlier, possibly by another process, to write more
    /// batches under and finish (0092 A6): its start record is already in the log, and
    /// the rejects and hash mismatches so far are the follower's to restore.
    pub async fn reopen<S: IngestStore>(
        store: &S,
        cx: &mut S::Cx,
        tenant: &str,
        id: u64,
        header: JobHeader,
        attestation: Attestation,
    ) -> Result<Self, IngestError> {
        let mut job = Self::handle(store, cx, tenant, id, attestation).await?;
        job.header = header;
        Ok(job)
    }

    /// The attestation a record this job writes carries: the actor, the job ID and tags.
    #[must_use]
    pub fn record_attestation(&self) -> Attestation {
        self.attestation.with_job(Some(self.id))
    }

    /// A handle on a job that is not this process's, for writing an event under its key:
    /// the runner's `job/interrupt` and `job/fail` about a job another runner started
    /// (0104 §4). Nothing is appended by this alone.
    pub async fn handle<S: IngestStore>(
        store: &S,
        cx: &mut S::Cx,
        tenant: &str,
        id: u64,
        attestation: Attestation,
    ) -> Result<Self, IngestError> {
        let log_partition =
            store
                .partition(cx, tenant, "log")
                .await?
                .ok_or_else(|| IngestError::NoPartition {
                    tenant: tenant.to_string(),
                    graph: "log".to_string(),
                })?;
        Ok(Self {
            id,
            tenant: tenant.to_string(),
            log_partition,
            header: JobHeader::local(""),
            attestation,
            start_offset: 0,
            hash_mismatches: BTreeMap::new(),
            rejects: Vec::new(),
            rejected_total: 0,
        })
    }

    /// Appends a `job/reject` record under this job's key (0104 §2): a holder of
    /// `ts-revertjob` rejected the batch, or the running job, for `reason`, and started
    /// the revert `revert` where they asked for one. Writes naming a rejected batch are
    /// refused from then on.
    pub async fn reject_job<S: IngestStore>(
        &self,
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        reason: &str,
        revert: Option<u64>,
        now: u64,
    ) -> Result<u64, IngestError> {
        let mut params = serde_json::json!({"reason": reason});
        if let Some(r) = revert {
            params["revert"] = serde_json::json!(r);
        }
        let draft = self.event(store, cx, "reject", params, now).await?;
        let (record, _) =
            append_projected(store, pipeline, cx, self.log_partition, draft, Budget::NONE).await?;
        Ok(record.header().offset)
    }

    /// Appends a `job/close` record under this job's key (0104 §1): the batch's submitter
    /// ended it; a later write naming its value opens a new one.
    pub async fn close<S: IngestStore>(
        &self,
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        now: u64,
    ) -> Result<u64, IngestError> {
        let draft = self
            .event(store, cx, "close", serde_json::json!({}), now)
            .await?;
        let (record, _) =
            append_projected(store, pipeline, cx, self.log_partition, draft, Budget::NONE).await?;
        Ok(record.header().offset)
    }

    /// Appends a `job/revert` record under this job's key (0104 §3): the revert job
    /// `reverted_by` wrote the inverse of its records, `skipped` of them left as they
    /// were because something changed them since.
    pub async fn reverted<S: IngestStore>(
        &self,
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        reverted_by: u64,
        skipped: u64,
        now: u64,
    ) -> Result<u64, IngestError> {
        let draft = self
            .event(
                store,
                cx,
                "revert",
                serde_json::json!({"reverted_by": reverted_by, "skipped": skipped}),
                now,
            )
            .await?;
        let (record, _) =
            append_projected(store, pipeline, cx, self.log_partition, draft, Budget::NONE).await?;
        Ok(record.header().offset)
    }

    /// Appends a `job/interrupt` record under this job's key, naming `by`, the job that
    /// noticed it had died (0011 A36; 0104 §4).
    pub async fn interrupt<S: IngestStore>(
        &self,
        store: &S,
        pipeline: &Pipeline<S>,
        cx: &mut S::Cx,
        by: u64,
        now: u64,
    ) -> Result<u64, IngestError> {
        let draft = self
            .event(store, cx, "interrupt", serde_json::json!({"by": by}), now)
            .await?;
        let (record, _) =
            append_projected(store, pipeline, cx, self.log_partition, draft, Budget::NONE).await?;
        Ok(record.header().offset)
    }
}
