//! The worker (0013 §7; 0083 §3; 03 §6.1): one loop that applies every queue.
//!
//! A [`Consumer`] is anything that applies queued work for a tenant inside a unit of
//! work: a projection's fan-out ([`crate::Pipeline::consumers`]), or a tier-3 consumer
//! that follows the tables other projections write, such as the report projection. The
//! [`Worker`] drives them all the same way, one tenant of one consumer per unit of work:
//!
//! 1. **Lease.** The worker takes the tenant's lock for the consumer
//!    ([`crate::Backend::lock_tenant`]) and leases up to `batch` of its oldest items
//!    ([`crate::Backend::claim`]). Another worker on the same tenant and consumer finds
//!    the lock taken and moves to the next tenant, so one tenant's work is applied by one
//!    worker at a time, in order, and several workers share the tenants.
//! 2. **Apply.** The consumer gets the distinct keys, in the order first queued, and
//!    applies them in the unit of work. A key queued twice is computed once.
//! 3. **Finish.** The items are removed in the same unit of work, so they go with the
//!    rows they produced: a worker that dies mid-batch leaves them queued, and the next
//!    worker applies them again, which every consumer must tolerate (its work is a
//!    recompute of the key, never an increment).
//! 4. **Fail.** When the consumer fails, the unit of work is rolled back, the attempt is
//!    recorded on the items ([`crate::Backend::fail`]), and the worker goes on to the
//!    next tenant. An item that has failed [`Worker::max_attempts`] times is left out of
//!    every lease, so one poisoned key stops neither its tenant nor the queue; it waits
//!    in the lag report for an operator.
//!
//! A read never does this work: a page shows the lag ([`crate::Backend::queue_lag`]) and
//! the worker catches up behind it.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::backend::Backend;
use crate::projection::{BoxFuture, ProjectionError};

/// Something that applies queued work for a tenant.
pub trait Consumer<B: Backend>: Send + Sync {
    /// The queue's name: the `projection` of its [`crate::Work`].
    fn name(&self) -> &'static str;

    /// Applies the keys queued for a tenant inside the unit of work. The keys are
    /// distinct, in the order first queued. Returns the rows written. An error rolls the
    /// unit of work back and counts as one failed attempt on every item of the batch.
    fn apply<'a>(
        &'a self,
        cx: &'a mut B::Cx,
        tenant: &'a str,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<usize, String>>;
}

/// One batch that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The consumer.
    pub consumer: String,
    /// The tenant.
    pub tenant: String,
    /// The items, by queue ID.
    pub ids: Vec<u64>,
    /// What the consumer said.
    pub error: String,
}

/// What one run of the worker did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ran {
    /// Items applied and removed.
    pub applied: usize,
    /// Rows the consumers wrote.
    pub rows: usize,
    /// Batches that failed, each rolled back and recorded.
    pub failures: Vec<Failure>,
}

impl Ran {
    /// Whether the run found anything to do.
    #[must_use]
    pub fn idle(&self) -> bool {
        self.applied == 0 && self.failures.is_empty()
    }
}

/// The worker: the consumers it drives and how.
pub struct Worker<B: Backend> {
    consumers: Vec<Arc<dyn Consumer<B>>>,
    /// Items leased per unit of work.
    pub batch: usize,
    /// Failed attempts after which an item is left to an operator.
    pub max_attempts: u32,
}

impl<B: Backend> Default for Worker<B> {
    fn default() -> Self {
        Self {
            consumers: Vec::new(),
            batch: 500,
            max_attempts: 5,
        }
    }
}

impl<B: Backend> Worker<B> {
    /// No consumers yet; a batch of 500, five attempts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a consumer.
    ///
    /// # Errors
    ///
    /// Two consumers of one name.
    pub fn with(mut self, consumer: Arc<dyn Consumer<B>>) -> Result<Self, ProjectionError> {
        if self.consumers.iter().any(|c| c.name() == consumer.name()) {
            return Err(ProjectionError::DuplicateName(consumer.name().into()));
        }
        self.consumers.push(consumer);
        Ok(self)
    }

    /// Adds several.
    ///
    /// # Errors
    ///
    /// Two consumers of one name.
    pub fn with_all(
        mut self,
        consumers: impl IntoIterator<Item = Arc<dyn Consumer<B>>>,
    ) -> Result<Self, ProjectionError> {
        for c in consumers {
            self = self.with(c)?;
        }
        Ok(self)
    }

    /// The consumers, in order.
    #[must_use]
    pub fn consumers(&self) -> &[Arc<dyn Consumer<B>>] {
        &self.consumers
    }

    /// One pass over every consumer and every tenant with work for it: at most one
    /// batch per tenant per consumer. Call it again while [`Ran::idle`] is false.
    ///
    /// # Errors
    ///
    /// The backend's; a consumer's failure is recorded in the result instead.
    pub async fn run_once(&self, backend: &B) -> Result<Ran, ProjectionError> {
        let mut ran = Ran::default();
        for consumer in &self.consumers {
            let tenants = {
                let mut cx = backend.begin().await?;
                let t = backend
                    .tenants_waiting(&mut cx, consumer.name(), self.max_attempts)
                    .await?;
                backend.rollback(cx).await?;
                t
            };
            for tenant in tenants {
                self.run_tenant(backend, consumer.as_ref(), &tenant, &mut ran)
                    .await?;
            }
        }
        Ok(ran)
    }

    /// Runs until a pass finds nothing, or `limit` items have been applied.
    ///
    /// # Errors
    ///
    /// The backend's.
    pub async fn run_until_idle(&self, backend: &B, limit: usize) -> Result<Ran, ProjectionError> {
        let mut total = Ran::default();
        while total.applied < limit {
            let ran = self.run_once(backend).await?;
            let idle = ran.idle();
            total.applied += ran.applied;
            total.rows += ran.rows;
            total.failures.extend(ran.failures);
            if idle {
                break;
            }
        }
        Ok(total)
    }

    async fn run_tenant(
        &self,
        backend: &B,
        consumer: &dyn Consumer<B>,
        tenant: &str,
        ran: &mut Ran,
    ) -> Result<(), ProjectionError> {
        let mut cx = backend.begin().await?;
        if !backend
            .lock_tenant(&mut cx, consumer.name(), tenant)
            .await?
        {
            backend.rollback(cx).await?;
            return Ok(());
        }
        let leased = backend
            .claim(
                &mut cx,
                consumer.name(),
                tenant,
                self.batch,
                self.max_attempts,
            )
            .await?;
        if leased.is_empty() {
            backend.rollback(cx).await?;
            return Ok(());
        }
        let ids: Vec<u64> = leased.iter().map(|q| q.id).collect();
        let mut seen = BTreeSet::new();
        let keys: Vec<String> = leased
            .iter()
            .filter(|q| seen.insert(q.work.key.as_str()))
            .map(|q| q.work.key.clone())
            .collect();
        match consumer.apply(&mut cx, tenant, &keys).await {
            Ok(rows) => {
                backend.finish(&mut cx, &ids).await?;
                backend.commit(cx).await?;
                ran.applied += ids.len();
                ran.rows += rows;
            }
            Err(error) => {
                backend.rollback(cx).await?;
                let mut cx = backend.begin().await?;
                backend.fail(&mut cx, &ids, &error).await?;
                backend.commit(cx).await?;
                ran.failures.push(Failure {
                    consumer: consumer.name().into(),
                    tenant: tenant.into(),
                    ids,
                    error,
                });
            }
        }
        Ok(())
    }
}
