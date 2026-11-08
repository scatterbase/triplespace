//! Batches (0104 §1, §2; 16 §2.2; 05 §3.3): a job the submitter keys. Any write may
//! carry a batch value, any string that identifies a batch of work to its submitter, as
//! Wikidata's edit groups do. The value never reaches the log: what the log holds is
//! [`hash`], the first 20 bytes of SHA-256 over `batch`, the tenant slug, the operator's
//! actor key and the value, each followed by a zero byte, in z-base-32. The operator is
//! the submitter's own account, or a subsidiary's operator ([`operator_of`]), so the same
//! string under two operators is two batches, and nobody can join or pass for another's.
//!
//! [`attach`] is what a write calls inside its own unit of work: the first write naming
//! a hash the tenant has not seen, or whose last batch under it was closed, opens a job
//! (mode `batch`, the hash and the operator in its parameters) in that same transaction;
//! a later one joins the open job; one naming a rejected batch is refused. The write
//! then carries the job's ID in its attestation, as a job's records do.
//!
//! implements 0104 §1 (A2)
//! implements 0104 §2

use scatter_ingest::IngestError;
use scatter_ingest::job::Job;
use scatter_ingest::write::Attestation;
use scatter_projection::Pipeline;
use scatter_wikibase_changeset::{JobHeader, Mode};
use sha2::Digest as _;
use tokio_postgres::GenericClient;
use triplespace_projections::PgCx;

use crate::PgIngest;

/// The hash a batch is known by in the log.
#[must_use]
pub fn hash(tenant: &str, operator: &str, value: &str) -> String {
    let mut h = sha2::Sha256::new();
    for part in ["batch", tenant, operator, value] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    let digest = h.finalize();
    scatter_log::registry::zbase32(&digest[..20])
}

/// The operator a batch is keyed by: a subsidiary's operator, else the actor itself.
///
/// # Errors
///
/// The store's.
pub async fn operator_of<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor: &str,
) -> Result<String, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT operator FROM view.actor WHERE tenant = $1 AND actor_key = $2",
            &[&tenant, &actor],
        )
        .await?;
    Ok(row
        .and_then(|r| r.get::<_, Option<String>>(0))
        .unwrap_or_else(|| actor.to_string()))
}

/// Why a write could not join its batch.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    /// The batch was rejected (0104 §2): writes naming it are refused.
    #[error("batch rejected as job {job}")]
    Rejected {
        /// The rejected job.
        job: u64,
    },
    /// The store refused.
    #[error(transparent)]
    Ingest(#[from] IngestError),
}

/// The batch a write joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attached {
    /// The job.
    pub job: u64,
    /// Whether this write opened it.
    pub opened: bool,
}

/// Joins, or opens, the batch `hash` names, inside the write's unit of work `cx`; the
/// job's actor is the write's.
///
/// # Errors
///
/// [`BatchError::Rejected`] for a rejected batch; the store's.
pub async fn attach(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor: &str,
    operator: &str,
    hash: &str,
    now: u64,
) -> Result<Attached, BatchError> {
    let row = cx
        .conn()
        .query_opt(
            "SELECT job_id, status FROM view.job WHERE tenant = $1 AND batch = $2
             ORDER BY job_id DESC LIMIT 1",
            &[&tenant, &hash],
        )
        .await
        .map_err(|e| IngestError::Store(e.to_string()))?;
    if let Some(r) = row {
        let job = u64::try_from(r.get::<_, i64>(0)).unwrap_or(0);
        match r.get::<_, String>(1).as_str() {
            "running" => return Ok(Attached { job, opened: false }),
            "rejected" => return Err(BatchError::Rejected { job }),
            _ => {}
        }
    }
    let mut header = JobHeader::local("batch");
    header.mode = Some(Mode::Batch);
    header.params.insert("batch".into(), hash.into());
    header.params.insert("operator".into(), operator.into());
    let job = Job::start(
        store,
        pipeline,
        cx,
        tenant,
        header,
        Attestation::by(actor),
        now,
    )
    .await?;
    Ok(Attached {
        job: job.id,
        opened: true,
    })
}

#[cfg(test)]
mod tests {
    use super::hash;

    /// The hash binds the value to the tenant and the operator (0104 §1): the same
    /// string under two operators, or on two wikis, is two batches.
    #[test]
    fn the_hash_is_keyed_by_tenant_and_operator() {
        let a = hash("wiki", "local:1", "run-2026-11-08");
        assert_eq!(a.len(), 32, "{a}");
        assert_eq!(a, hash("wiki", "local:1", "run-2026-11-08"));
        assert_ne!(a, hash("wiki", "local:2", "run-2026-11-08"));
        assert_ne!(a, hash("other", "local:1", "run-2026-11-08"));
        assert_ne!(a, hash("wiki", "local:1", "run-2026-11-09"));
        assert!(
            a.chars()
                .all(|c| "ybndrfg8ejkmcpqxot1uwisza345h769".contains(c))
        );
    }
}
