//! What a pipeline needs from storage: a unit of work, positions, and the work queue.
//!
//! The queue is shared by every consumer ([`crate::worker::Consumer`]): a projection's
//! fan-out past the synchronous budget, and the tier-3 consumers that follow the tables
//! other projections write (0083 §3). A row is queued in the transaction that found the
//! work, **leased** by a worker ([`Backend::claim`]) and removed when the work that
//! applied it commits ([`Backend::finish`]), so a worker that dies mid-batch loses
//! nothing; one that fails records the attempt ([`Backend::fail`]) and the row is
//! skipped once it has failed [`crate::worker::Worker::max_attempts`] times. One tenant's
//! work for one consumer is applied by one worker at a time ([`Backend::lock_tenant`]).

use std::future::Future;

use crate::projection::ProjectionError;

/// Queued work (0013 §7; 0083 §3): a key to re-project, for a consumer, in a tenant,
/// because of a record. Applied in queueing order by the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    /// The consumer: a projection's name, or a tier-3 consumer's.
    pub projection: String,
    /// The tenant, `""` for the instance.
    pub tenant: String,
    /// The partition of the record.
    pub partition: u64,
    /// The record.
    pub offset: u64,
    /// The key to re-project.
    pub key: String,
}

/// A queued item as a worker leases it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    /// The queue's own ID, by which the item is finished or failed.
    pub id: u64,
    /// The work.
    pub work: Work,
    /// How many times a worker has failed to apply it.
    pub attempts: u32,
}

/// What waits in one consumer's queue for one tenant: the lag `ops.projection_work`
/// is the unit of truth for (03 §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueLag {
    /// The consumer.
    pub consumer: String,
    /// The tenant, `""` for the instance.
    pub tenant: String,
    /// Items waiting to be applied.
    pub waiting: u64,
    /// Items that have failed too often to be retried, and wait for an operator.
    pub failed: u64,
    /// How long the oldest waiting item has waited, in seconds.
    pub oldest_seconds: u64,
}

/// How far a projection is behind a partition's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lag {
    /// The projection.
    pub projection: String,
    /// The partition.
    pub partition: u64,
    /// Offsets below this are applied.
    pub applied: u64,
    /// The partition's size.
    pub head: u64,
}

impl Lag {
    /// Records not yet applied.
    #[must_use]
    pub fn behind(&self) -> u64 {
        self.head.saturating_sub(self.applied)
    }
}

/// The storage a pipeline runs over.
pub trait Backend: Send + Sync {
    /// A unit of work: a transaction, committed as one.
    type Cx: Send;

    /// Opens a unit of work.
    fn begin(&self) -> impl Future<Output = Result<Self::Cx, ProjectionError>> + Send;

    /// Commits it.
    fn commit(&self, cx: Self::Cx) -> impl Future<Output = Result<(), ProjectionError>> + Send;

    /// Abandons it.
    fn rollback(&self, cx: Self::Cx) -> impl Future<Output = Result<(), ProjectionError>> + Send;

    /// The applied offset of a projection on a partition; 0 when none.
    fn applied(
        &self,
        cx: &mut Self::Cx,
        projection: &str,
        partition: u64,
    ) -> impl Future<Output = Result<u64, ProjectionError>> + Send;

    /// Records that offsets below `applied` are applied.
    fn set_applied(
        &self,
        cx: &mut Self::Cx,
        projection: &str,
        partition: u64,
        applied: u64,
    ) -> impl Future<Output = Result<(), ProjectionError>> + Send;

    /// Queues work for the worker.
    fn queue(
        &self,
        cx: &mut Self::Cx,
        work: &[Work],
    ) -> impl Future<Output = Result<(), ProjectionError>> + Send;

    /// The tenants with work waiting for a consumer that has failed fewer than
    /// `max_attempts` times, the tenant with the oldest item first.
    fn tenants_waiting(
        &self,
        cx: &mut Self::Cx,
        consumer: &str,
        max_attempts: u32,
    ) -> impl Future<Output = Result<Vec<String>, ProjectionError>> + Send;

    /// Takes the right to apply a tenant's work for a consumer, for the rest of this
    /// unit of work; `false` when another worker holds it, in which case the caller
    /// moves on rather than waits.
    fn lock_tenant(
        &self,
        cx: &mut Self::Cx,
        consumer: &str,
        tenant: &str,
    ) -> impl Future<Output = Result<bool, ProjectionError>> + Send;

    /// Leases up to `limit` of a tenant's items for a consumer, oldest first, leaving
    /// out those that have failed `max_attempts` times. The items stay queued, and
    /// invisible to other workers, until [`Backend::finish`] or the unit of work ends.
    fn claim(
        &self,
        cx: &mut Self::Cx,
        consumer: &str,
        tenant: &str,
        limit: usize,
        max_attempts: u32,
    ) -> impl Future<Output = Result<Vec<Queued>, ProjectionError>> + Send;

    /// Removes applied items, in the unit of work that applied them.
    fn finish(
        &self,
        cx: &mut Self::Cx,
        ids: &[u64],
    ) -> impl Future<Output = Result<(), ProjectionError>> + Send;

    /// Records a failed attempt on items, with the error, after the unit of work that
    /// failed has been rolled back.
    fn fail(
        &self,
        cx: &mut Self::Cx,
        ids: &[u64],
        error: &str,
    ) -> impl Future<Output = Result<(), ProjectionError>> + Send;

    /// What waits in every queue, by consumer and tenant.
    fn queue_lag(
        &self,
        cx: &mut Self::Cx,
        max_attempts: u32,
    ) -> impl Future<Output = Result<Vec<QueueLag>, ProjectionError>> + Send;
}
