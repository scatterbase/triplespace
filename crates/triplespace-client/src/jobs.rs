//! `GET /jobs` and `GET /jobs/{id}` (0104 §6; 18 §3.2): the tenant's jobs, running and
//! recent, and one job, as `Special:Jobs` and `Special:Jobs/{id}` read them (0010 §9).

use serde::Deserialize;
use serde_json::Value;

use crate::api::{CacheInfo, Fetched};
use crate::settings::{BASE, read};
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// A job's actor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct JobActor {
    /// The key.
    #[serde(default)]
    pub key: String,
    /// The name, where the actor has one.
    #[serde(default)]
    pub name: Option<String>,
}

/// One reject.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct JobReject {
    /// The line in the batch.
    #[serde(default)]
    pub line: u64,
    /// The match key, where the operation had one.
    #[serde(default)]
    pub r#match: Option<Value>,
    /// Why.
    #[serde(default)]
    pub reason: String,
}

/// A job, as the API shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct JobInfo {
    /// The job ID.
    pub id: u64,
    /// `queued`, `running`, `finished`, `failed`, `interrupted`, `reverted`, …
    pub status: String,
    /// The mode.
    #[serde(default)]
    pub mode: Option<String>,
    /// A queued job's kind.
    #[serde(default)]
    pub kind: Option<String>,
    /// The source.
    #[serde(default)]
    pub source: Option<String>,
    /// The source version.
    #[serde(default)]
    pub version: Option<String>,
    /// The adapter version.
    #[serde(default)]
    pub adapter: Option<String>,
    /// The graph written.
    #[serde(default)]
    pub graph: Option<String>,
    /// The parameters.
    #[serde(default)]
    pub params: Value,
    /// The actor.
    #[serde(default)]
    pub actor: JobActor,
    /// The account it ran for.
    #[serde(default)]
    pub requested_by: Option<String>,
    /// When it was submitted, ISO 8601 (a queued job).
    #[serde(default)]
    pub submitted: Option<String>,
    /// When it started, ISO 8601.
    #[serde(default)]
    pub started: Option<String>,
    /// When it ended, ISO 8601.
    #[serde(default)]
    pub finished: Option<String>,
    /// A finished job's counts.
    #[serde(default)]
    pub counts: Option<Value>,
    /// The checkpoint's tree size.
    #[serde(default)]
    pub checkpoint_size: Option<u64>,
    /// A failed job's error.
    #[serde(default)]
    pub error: Option<String>,
    /// The job that found an interrupted one dead.
    #[serde(default)]
    pub interrupted_by: Option<u64>,
    /// A queued job's place in the queue.
    #[serde(default)]
    pub position: Option<u64>,
    /// A running job's progress.
    #[serde(default)]
    pub progress: Option<Value>,
    /// How many times a runner has claimed it.
    #[serde(default)]
    pub attempts: Option<u32>,
    /// The first rejects (`GET /jobs/{id}`).
    #[serde(default)]
    pub rejects: Vec<JobReject>,
}

#[derive(Debug, Deserialize)]
struct Listing {
    jobs: Vec<JobInfo>,
}

impl Client {
    /// `GET /jobs`, narrowed to a status when one is given.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn jobs(
        &self,
        incoming: &Incoming,
        status: Option<&str>,
    ) -> Result<Fetched<Vec<JobInfo>>, ClientError> {
        let mut path = format!("{BASE}/jobs");
        if let Some(s) = status {
            path.push_str("?status=");
            path.extend(url::form_urlencoded::byte_serialize(s.as_bytes()));
        }
        let r = self.get(incoming, &path).await?;
        let listing: Listing = read(&r, "jobs")?;
        Ok(Fetched {
            value: listing.jobs,
            cache: CacheInfo::of(&r),
        })
    }

    /// `GET /jobs/{id}`.
    ///
    /// # Errors
    ///
    /// `job-not-found`, or the API away or unreadable.
    pub async fn job(&self, incoming: &Incoming, id: u64) -> Result<Fetched<JobInfo>, ClientError> {
        let r = self.get(incoming, &format!("{BASE}/jobs/{id}")).await?;
        Ok(Fetched {
            value: read(&r, "job")?,
            cache: CacheInfo::of(&r),
        })
    }
}
