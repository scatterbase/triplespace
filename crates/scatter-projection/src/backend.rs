//! What a pipeline needs from storage: a unit of work, positions, and the work queue.

use std::future::Future;

use crate::projection::ProjectionError;

/// Queued fan-out (0013 §7): a key to re-project for a record whose effects outran the
/// synchronous budget, applied in append order by the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    /// The projection.
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

    /// Queues fan-out for the worker.
    fn queue(
        &self,
        cx: &mut Self::Cx,
        work: &[Work],
    ) -> impl Future<Output = Result<(), ProjectionError>> + Send;

    /// Takes up to `limit` queued items for a projection, oldest first, removing them.
    fn take(
        &self,
        cx: &mut Self::Cx,
        projection: &str,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<Work>, ProjectionError>> + Send;
}
