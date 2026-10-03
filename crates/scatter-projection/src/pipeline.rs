//! The pipeline: projections in step order, applied inline or catching up.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use scatter_log::record::Record;
use scatter_log::store::{LogStore, Slot};

use crate::backend::{Backend, Lag, Work};
use crate::projection::{Applied, Projection, ProjectionError, Step, Target};

/// The synchronous budget (0013 §7): fan-out is applied inline until either is spent,
/// then queued. `projections.sync_budget` and `projections.sync_time` in `site`
/// configuration; the defaults are 1,000 rows and 250 ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Rows the inline fan-out may write.
    pub rows: usize,
    /// Time it may take.
    pub time: Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            rows: 1_000,
            time: Duration::from_millis(250),
        }
    }
}

impl Budget {
    /// No fan-out inline at all: everything is queued.
    pub const NONE: Self = Self {
        rows: 0,
        time: Duration::ZERO,
    };
    /// No limit: everything inline, as a bulk job's batch would.
    pub const UNLIMITED: Self = Self {
        rows: usize::MAX,
        time: Duration::MAX,
    };
}

/// What an inline application did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    /// Rows written for the record's own subjects, across projections.
    pub rows: usize,
    /// Fan-out targets re-projected inline.
    pub fanned_out: usize,
    /// Fan-out targets queued for the worker.
    pub queued: usize,
}

/// What a catch-up did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Progress {
    /// Records read.
    pub records: u64,
    /// Rows written.
    pub rows: usize,
    /// Fan-out targets queued.
    pub queued: usize,
    /// The head the partition was caught up to.
    pub head: u64,
}

/// Projections in step order.
pub struct Pipeline<B: Backend> {
    projections: Vec<Box<dyn Projection<B>>>,
}

impl<B: Backend> Default for Pipeline<B> {
    fn default() -> Self {
        Self {
            projections: Vec::new(),
        }
    }
}

impl<B: Backend> Pipeline<B> {
    /// No projections yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a projection, keeping step order; within a step, the order of addition.
    pub fn with(
        mut self,
        projection: impl Projection<B> + 'static,
    ) -> Result<Self, ProjectionError> {
        if self
            .projections
            .iter()
            .any(|p| p.name() == projection.name())
        {
            return Err(ProjectionError::DuplicateName(projection.name().into()));
        }
        let step = projection.step();
        let at = self
            .projections
            .iter()
            .position(|p| p.step() > step)
            .unwrap_or(self.projections.len());
        self.projections.insert(at, Box::new(projection));
        Ok(self)
    }

    /// The projections, in order.
    #[must_use]
    pub fn projections(&self) -> &[Box<dyn Projection<B>>] {
        &self.projections
    }

    /// The projections of the steps the write path applies synchronously.
    fn synchronous(&self) -> impl Iterator<Item = &dyn Projection<B>> {
        self.projections
            .iter()
            .filter(|p| p.step().is_synchronous())
            .map(AsRef::as_ref)
    }

    fn failed(p: &dyn Projection<B>, record: &Record, message: String) -> ProjectionError {
        ProjectionError::Failed {
            projection: p.name().into(),
            partition: record.header().partition,
            offset: record.header().offset,
            message,
        }
    }

    /// Applies one just-appended record inside the appending transaction (0013 §7):
    /// every synchronous projection, then the fan-out within `budget`, the rest queued.
    /// Advances each synchronous projection's position past the record.
    pub async fn apply_inline(
        &self,
        backend: &B,
        cx: &mut B::Cx,
        record: &Record,
        budget: Budget,
    ) -> Result<Inline, ProjectionError> {
        let started = Instant::now();
        let mut out = Inline::default();
        let mut pending: Vec<(&dyn Projection<B>, Target)> = Vec::new();
        let h = record.header();
        for p in self.synchronous() {
            if p.accepts(h) {
                let applied = p
                    .apply(cx, record)
                    .await
                    .map_err(|m| Self::failed(p, record, m))?;
                out.rows += applied.rows;
                pending.extend(applied.fanout.into_iter().map(|t| (p, t)));
            }
            backend
                .set_applied(cx, p.name(), h.partition, h.offset + 1)
                .await?;
        }
        let mut spent = 0usize;
        let mut queue = Vec::new();
        for (p, target) in pending {
            if spent < budget.rows && started.elapsed() < budget.time {
                let rows = p
                    .fanout(cx, &target.tenant, &target.key)
                    .await
                    .map_err(|m| Self::failed(p, record, m))?;
                spent += rows;
                out.fanned_out += 1;
            } else {
                queue.push(Work {
                    projection: p.name().into(),
                    tenant: target.tenant,
                    partition: h.partition,
                    offset: h.offset,
                    key: target.key,
                });
            }
        }
        if !queue.is_empty() {
            out.queued = queue.len();
            backend.queue(cx, &queue).await?;
        }
        Ok(out)
    }

    /// Applies one record for every projection whose position is at or below it.
    /// Fan-out is queued, never applied inline: a catch-up is bulk work.
    async fn apply_behind(
        &self,
        backend: &B,
        cx: &mut B::Cx,
        record: &Record,
        positions: &mut BTreeMap<&'static str, u64>,
        progress: &mut Progress,
    ) -> Result<(), ProjectionError> {
        let h = record.header();
        let mut queue = Vec::new();
        for p in &self.projections {
            let pos = positions.entry(p.name()).or_insert(0);
            if *pos > h.offset {
                continue;
            }
            if p.accepts(h) {
                let Applied { rows, fanout } = p
                    .apply(cx, record)
                    .await
                    .map_err(|m| Self::failed(p.as_ref(), record, m))?;
                progress.rows += rows;
                queue.extend(fanout.into_iter().map(|t| Work {
                    projection: p.name().into(),
                    tenant: t.tenant,
                    partition: h.partition,
                    offset: h.offset,
                    key: t.key,
                }));
            }
            *pos = h.offset + 1;
        }
        if !queue.is_empty() {
            progress.queued += queue.len();
            backend.queue(cx, &queue).await?;
        }
        Ok(())
    }

    /// Catches every projection up to the partition's head, `batch` records per unit of
    /// work. A failing record rolls its batch back and leaves positions where they were.
    pub async fn catch_up<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        partition: u64,
        batch: usize,
    ) -> Result<Progress, ProjectionError> {
        let mut progress = Progress::default();
        let head = store.head(partition).await?.size;
        progress.head = head;
        let batch = batch.max(1);
        loop {
            let mut cx = backend.begin().await?;
            let mut positions: BTreeMap<&'static str, u64> = BTreeMap::new();
            for p in &self.projections {
                positions.insert(
                    p.name(),
                    backend.applied(&mut cx, p.name(), partition).await?,
                );
            }
            let from = positions.values().copied().min().unwrap_or(head);
            if from >= head {
                backend.rollback(cx).await?;
                break;
            }
            let to = from.saturating_add(batch as u64).min(head);
            let outcome: Result<(), ProjectionError> = async {
                for offset in from..to {
                    match store.read(partition, offset).await? {
                        Slot::Record(record) => {
                            self.apply_behind(
                                backend,
                                &mut cx,
                                &record,
                                &mut positions,
                                &mut progress,
                            )
                            .await?;
                        }
                        Slot::Compacted { .. } => {
                            for pos in positions.values_mut() {
                                if *pos <= offset {
                                    *pos = offset + 1;
                                }
                            }
                        }
                    }
                    progress.records += 1;
                }
                for (name, pos) in &positions {
                    backend.set_applied(&mut cx, name, partition, *pos).await?;
                }
                Ok(())
            }
            .await;
            match outcome {
                Ok(()) => backend.commit(cx).await?,
                Err(e) => {
                    backend.rollback(cx).await?;
                    return Err(e);
                }
            }
        }
        Ok(progress)
    }

    /// Applies up to `limit` queued fan-out items of each projection, one unit of work
    /// per projection. Returns the items applied.
    pub async fn drain(&self, backend: &B, limit: usize) -> Result<usize, ProjectionError> {
        let mut done = 0;
        for p in &self.projections {
            let mut cx = backend.begin().await?;
            let work = backend.take(&mut cx, p.name(), limit).await?;
            for w in &work {
                p.fanout(&mut cx, &w.tenant, &w.key)
                    .await
                    .map_err(|message| ProjectionError::Failed {
                        projection: p.name().into(),
                        partition: w.partition,
                        offset: w.offset,
                        message,
                    })?;
            }
            done += work.len();
            backend.commit(cx).await?;
        }
        Ok(done)
    }

    /// Resets every projection on `partitions` and replays each from offset 0.
    pub async fn rebuild<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        partitions: &[u64],
        batch: usize,
    ) -> Result<Vec<Progress>, ProjectionError> {
        let mut cx = backend.begin().await?;
        for &partition in partitions {
            for p in &self.projections {
                p.reset(&mut cx, partition)
                    .await
                    .map_err(|message| ProjectionError::Failed {
                        projection: p.name().into(),
                        partition,
                        offset: 0,
                        message,
                    })?;
                backend.set_applied(&mut cx, p.name(), partition, 0).await?;
            }
        }
        backend.commit(cx).await?;
        let mut out = Vec::new();
        for &partition in partitions {
            out.push(self.catch_up(backend, store, partition, batch).await?);
        }
        Ok(out)
    }

    /// Each projection's lag on a partition.
    pub async fn lags<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        partition: u64,
    ) -> Result<Vec<Lag>, ProjectionError> {
        let head = store.head(partition).await?.size;
        let mut cx = backend.begin().await?;
        let mut out = Vec::new();
        for p in &self.projections {
            out.push(Lag {
                projection: p.name().into(),
                partition,
                applied: backend.applied(&mut cx, p.name(), partition).await?,
                head,
            });
        }
        backend.rollback(cx).await?;
        Ok(out)
    }

    /// The steps present, in order.
    #[must_use]
    pub fn steps(&self) -> Vec<Step> {
        let mut steps: Vec<Step> = self.projections.iter().map(|p| p.step()).collect();
        steps.dedup();
        steps
    }
}
