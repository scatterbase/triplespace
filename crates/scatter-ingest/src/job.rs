//! A job's records (0002 §8.3; 0011 §6.3; payloads.md §5): a `job/start` log event when
//! it starts, keyed by the job ID the instance mints, and a `job/finish` or `job/fail`
//! under the same key. A tenant's bulk job writes them to the tenant's `log`; a mirror
//! sync, an instance action, to the instance `log`.

use std::collections::BTreeMap;

use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_wikibase_changeset::{JobHeader, MatchKey};
use serde::{Deserialize, Serialize};

use crate::IngestError;
use crate::store::{IngestStore, Sequence};
use crate::write::Attestation;

/// The log-event payload type.
pub const PAYLOAD_LOGEVENT: &str = "scatter:v0/logevent";

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
    /// Entities adopted (0035).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub adopted: u64,
    /// Entities tombstoned, by a `tombstone` or the `snapshot` sweep.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tombstoned: u64,
    /// Other records written: redirects, retains, overrides, links.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub other: u64,
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
        cx: &mut S::Cx,
        tenant: &str,
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
        let id = store.next_id(cx, "", &Sequence::Job).await?;
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
        let appended = store.append(cx, log_partition, draft).await?;
        job.start_offset = appended.offset;
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
        let logid = store.next_id(cx, &self.tenant, &Sequence::Log).await?;
        let content = serde_json::json!({
            "type": "job", "action": action, "time": now,
            "target": {"kind": "job", "id": self.id},
            "params": params,
        });
        Ok(Draft {
            appended_at: now,
            payload_type: PAYLOAD_LOGEVENT.into(),
            key: Some(self.id.to_string()),
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
        Ok(store.append(cx, self.log_partition, draft).await?.offset)
    }

    /// Appends the failure record.
    pub async fn fail<S: IngestStore>(
        &self,
        store: &S,
        cx: &mut S::Cx,
        error: &str,
        now: u64,
    ) -> Result<u64, IngestError> {
        let draft = self
            .event(store, cx, "fail", serde_json::json!({"error": error}), now)
            .await?;
        Ok(store.append(cx, self.log_partition, draft).await?.offset)
    }

    /// The attestation a record this job writes carries: the actor, the job ID and tags.
    #[must_use]
    pub fn record_attestation(&self) -> Attestation {
        self.attestation.with_job(Some(self.id))
    }
}
