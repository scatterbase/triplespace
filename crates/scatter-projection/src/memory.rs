//! An in-memory [`Backend`] with transactional units of work, for tests of projections
//! and of the pipeline itself.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::backend::{Backend, QueueLag, Queued, Work};
use crate::projection::ProjectionError;

/// The committed state.
#[derive(Debug, Default)]
pub struct State {
    /// `(projection, partition)` → applied offset.
    pub positions: BTreeMap<(String, u64), u64>,
    /// Queued work, oldest first.
    pub work: WorkQueue,
    /// Rows the test projections write: `(table, key)` → value.
    pub rows: BTreeMap<(String, String), String>,
}

/// A unit of work: a copy of the state, swapped in on commit.
#[derive(Debug)]
pub struct MemoryCx {
    /// The state as this unit sees it.
    pub state: State,
}

impl MemoryCx {
    /// Writes a row, for a test projection.
    pub fn put(&mut self, table: &str, key: &str, value: &str) {
        self.state
            .rows
            .insert((table.into(), key.into()), value.into());
    }

    /// Removes every row of a table whose key starts with `prefix`.
    pub fn clear(&mut self, table: &str, prefix: &str) {
        self.state
            .rows
            .retain(|(t, k), _| !(t == table && k.starts_with(prefix)));
    }
}

/// The backend.
#[derive(Debug, Clone, Default)]
pub struct MemoryBackend {
    state: Arc<Mutex<State>>,
}

impl MemoryBackend {
    /// Empty.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The committed state.
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("not poisoned")
    }
}

fn clone_state(s: &State) -> State {
    State {
        positions: s.positions.clone(),
        work: s.work.clone(),
        rows: s.rows.clone(),
    }
}

impl Backend for MemoryBackend {
    type Cx = MemoryCx;

    async fn begin(&self) -> Result<MemoryCx, ProjectionError> {
        Ok(MemoryCx {
            state: clone_state(&self.state()),
        })
    }

    async fn commit(&self, cx: MemoryCx) -> Result<(), ProjectionError> {
        *self.state() = cx.state;
        Ok(())
    }

    async fn rollback(&self, cx: MemoryCx) -> Result<(), ProjectionError> {
        drop(cx);
        Ok(())
    }

    async fn applied(
        &self,
        cx: &mut MemoryCx,
        projection: &str,
        partition: u64,
    ) -> Result<u64, ProjectionError> {
        Ok(cx
            .state
            .positions
            .get(&(projection.into(), partition))
            .copied()
            .unwrap_or(0))
    }

    async fn set_applied(
        &self,
        cx: &mut MemoryCx,
        projection: &str,
        partition: u64,
        applied: u64,
    ) -> Result<(), ProjectionError> {
        cx.state
            .positions
            .insert((projection.into(), partition), applied);
        Ok(())
    }

    async fn queue(&self, cx: &mut MemoryCx, work: &[Work]) -> Result<(), ProjectionError> {
        cx.state.work.queue(work);
        Ok(())
    }

    async fn tenants_waiting(
        &self,
        cx: &mut MemoryCx,
        consumer: &str,
        max_attempts: u32,
    ) -> Result<Vec<String>, ProjectionError> {
        Ok(cx.state.work.tenants_waiting(consumer, max_attempts))
    }

    /// Single-threaded: always free.
    async fn lock_tenant(
        &self,
        _cx: &mut MemoryCx,
        _consumer: &str,
        _tenant: &str,
    ) -> Result<bool, ProjectionError> {
        Ok(true)
    }

    async fn claim(
        &self,
        cx: &mut MemoryCx,
        consumer: &str,
        tenant: &str,
        limit: usize,
        max_attempts: u32,
    ) -> Result<Vec<Queued>, ProjectionError> {
        Ok(cx.state.work.claim(consumer, tenant, limit, max_attempts))
    }

    async fn finish(&self, cx: &mut MemoryCx, ids: &[u64]) -> Result<(), ProjectionError> {
        cx.state.work.finish(ids);
        Ok(())
    }

    async fn fail(
        &self,
        cx: &mut MemoryCx,
        ids: &[u64],
        _error: &str,
    ) -> Result<(), ProjectionError> {
        cx.state.work.fail(ids);
        Ok(())
    }

    async fn queue_lag(
        &self,
        cx: &mut MemoryCx,
        max_attempts: u32,
    ) -> Result<Vec<QueueLag>, ProjectionError> {
        Ok(cx.state.work.lag(max_attempts))
    }
}

/// An in-memory work queue with the worker's operations, shared by the memory backends
/// of this crate and `scatter-ingest`. Single-threaded: a tenant's lock is always free.
#[derive(Debug, Clone, Default)]
pub struct WorkQueue {
    /// The items, oldest first.
    pub items: Vec<Queued>,
    next_id: u64,
}

impl WorkQueue {
    /// Queues items.
    pub fn queue(&mut self, work: &[Work]) {
        for w in work {
            self.next_id += 1;
            self.items.push(Queued {
                id: self.next_id,
                work: w.clone(),
                attempts: 0,
            });
        }
    }

    /// As [`Backend::tenants_waiting`].
    #[must_use]
    pub fn tenants_waiting(&self, consumer: &str, max_attempts: u32) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for q in &self.items {
            if q.work.projection == consumer
                && q.attempts < max_attempts
                && !out.contains(&q.work.tenant)
            {
                out.push(q.work.tenant.clone());
            }
        }
        out
    }

    /// As [`Backend::claim`].
    #[must_use]
    pub fn claim(
        &self,
        consumer: &str,
        tenant: &str,
        limit: usize,
        max_attempts: u32,
    ) -> Vec<Queued> {
        self.items
            .iter()
            .filter(|q| {
                q.work.projection == consumer
                    && q.work.tenant == tenant
                    && q.attempts < max_attempts
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// As [`Backend::finish`].
    pub fn finish(&mut self, ids: &[u64]) {
        self.items.retain(|q| !ids.contains(&q.id));
    }

    /// As [`Backend::fail`].
    pub fn fail(&mut self, ids: &[u64]) {
        for q in &mut self.items {
            if ids.contains(&q.id) {
                q.attempts += 1;
            }
        }
    }

    /// As [`Backend::queue_lag`].
    #[must_use]
    pub fn lag(&self, max_attempts: u32) -> Vec<QueueLag> {
        let mut out: Vec<QueueLag> = Vec::new();
        for q in &self.items {
            let found = out
                .iter()
                .position(|l| l.consumer == q.work.projection && l.tenant == q.work.tenant);
            let at = if let Some(i) = found {
                i
            } else {
                out.push(QueueLag {
                    consumer: q.work.projection.clone(),
                    tenant: q.work.tenant.clone(),
                    waiting: 0,
                    failed: 0,
                    oldest_seconds: 0,
                });
                out.len() - 1
            };
            if q.attempts < max_attempts {
                out[at].waiting += 1;
            } else {
                out[at].failed += 1;
            }
        }
        out
    }
}
