//! The worker's consumers (03 §6.1; 0083 §3, A4): the one list every process that runs the
//! work queue builds its worker from, the server's worker tasks and `triplespace worker`
//! alike, so that neither leaves a queue unserved.

use std::sync::Arc;

use scatter_projection::{Pipeline, ProjectionError, Worker};

use crate::PgIngest;

/// The worker over the work queue: every projection's queued fan-out, then the tier-3
/// consumers (reports, reference counts, the update stream's deltas).
///
/// # Errors
///
/// Two consumers of one name.
pub fn worker(pipeline: &Pipeline<PgIngest>) -> Result<Worker<PgIngest>, ProjectionError> {
    Worker::new()
        .with_all(pipeline.consumers())?
        .with(Arc::new(triplespace_projections::reports::ReportConsumer))?
        .with(Arc::new(triplespace_projections::refs::RefCountConsumer))?
        .with(Arc::new(triplespace_rdf::updates::DeltaConsumer))
}

/// A parked item: one that failed as often as the worker retries, and waits for an
/// operator (03 §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parked {
    /// The queue ID.
    pub id: i64,
    /// The consumer.
    pub consumer: String,
    /// The tenant, `""` for the instance.
    pub tenant: String,
    /// The key to compute again.
    pub key: String,
    /// Failed attempts.
    pub attempts: i32,
    /// What the consumer said last.
    pub last_error: Option<String>,
}

/// Which parked items a command names: all, or those of a consumer, a tenant or a key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Which {
    /// The consumer's name.
    pub consumer: Option<String>,
    /// The tenant's slug; `""` is the instance.
    pub tenant: Option<String>,
    /// The key.
    pub key: Option<String>,
}

const FILTER: &str = "attempts >= $1
    AND ($2::text IS NULL OR projection = $2)
    AND ($3::text IS NULL OR tenant = $3)
    AND ($4::text IS NULL OR key = $4)";

fn attempts(max_attempts: u32) -> i32 {
    i32::try_from(max_attempts).unwrap_or(i32::MAX)
}

/// The parked items `which` names, oldest first, at most `limit`.
///
/// # Errors
///
/// The database's.
pub async fn parked(
    client: &tokio_postgres::Client,
    max_attempts: u32,
    which: &Which,
    limit: i64,
) -> Result<Vec<Parked>, tokio_postgres::Error> {
    let rows = client
        .query(
            &format!(
                "SELECT id, projection, tenant, key, attempts, last_error
                   FROM ops.projection_work WHERE {FILTER} ORDER BY id LIMIT $5"
            ),
            &[
                &attempts(max_attempts),
                &which.consumer,
                &which.tenant,
                &which.key,
                &limit,
            ],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| Parked {
            id: r.get(0),
            consumer: r.get(1),
            tenant: r.get(2),
            key: r.get(3),
            attempts: r.get(4),
            last_error: r.get(5),
        })
        .collect())
}

/// Puts the parked items `which` names back in the queue, their attempts forgotten, for
/// the worker to try again; the last error is kept until the next attempt. Returns how
/// many.
///
/// # Errors
///
/// The database's.
pub async fn retry(
    client: &tokio_postgres::Client,
    max_attempts: u32,
    which: &Which,
) -> Result<u64, tokio_postgres::Error> {
    client
        .execute(
            &format!("UPDATE ops.projection_work SET attempts = 0 WHERE {FILTER}"),
            &[
                &attempts(max_attempts),
                &which.consumer,
                &which.tenant,
                &which.key,
            ],
        )
        .await
}

/// Removes the parked items `which` names from the queue: their keys are not computed
/// until something queues them again, or the projection is rebuilt. Returns how many.
///
/// # Errors
///
/// The database's.
pub async fn discard(
    client: &tokio_postgres::Client,
    max_attempts: u32,
    which: &Which,
) -> Result<u64, tokio_postgres::Error> {
    client
        .execute(
            &format!("DELETE FROM ops.projection_work WHERE {FILTER}"),
            &[
                &attempts(max_attempts),
                &which.consumer,
                &which.tenant,
                &which.key,
            ],
        )
        .await
}
