//! An in-memory [`Backend`] with transactional units of work, for tests of projections
//! and of the pipeline itself.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::backend::{Backend, Work};
use crate::projection::ProjectionError;

/// The committed state.
#[derive(Debug, Default)]
pub struct State {
    /// `(projection, partition)` → applied offset.
    pub positions: BTreeMap<(String, u64), u64>,
    /// Queued fan-out, oldest first.
    pub work: Vec<Work>,
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
        cx.state.work.extend_from_slice(work);
        Ok(())
    }

    async fn take(
        &self,
        cx: &mut MemoryCx,
        projection: &str,
        limit: usize,
    ) -> Result<Vec<Work>, ProjectionError> {
        let mut taken = Vec::new();
        let mut kept = Vec::new();
        for w in std::mem::take(&mut cx.state.work) {
            if w.projection == projection && taken.len() < limit {
                taken.push(w);
            } else {
                kept.push(w);
            }
        }
        cx.state.work = kept;
        Ok(taken)
    }
}
