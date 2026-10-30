//! The worker over the memory backend: leases per tenant, keys computed once, failures
//! recorded and bounded, the queue never blocked by one poisoned key.

use std::sync::Arc;

use scatter_projection::memory::MemoryCx;
use scatter_projection::{Backend, BoxFuture, Consumer, MemoryBackend, Work, Worker};

/// Writes `applied:{tenant}:{key}` rows; fails on any key containing `BOOM`.
struct Recorder;

impl Consumer<MemoryBackend> for Recorder {
    fn name(&self) -> &'static str {
        "recorder"
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut MemoryCx,
        tenant: &'a str,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            if let Some(k) = keys.iter().find(|k| k.contains("BOOM")) {
                return Err(format!("cannot apply {k}"));
            }
            for k in keys {
                let n = cx
                    .state
                    .rows
                    .get(&("applied".into(), format!("{tenant}:{k}")))
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                cx.put("applied", &format!("{tenant}:{k}"), &(n + 1).to_string());
            }
            Ok(keys.len())
        })
    }
}

fn work(tenant: &str, key: &str) -> Work {
    Work {
        projection: "recorder".into(),
        tenant: tenant.into(),
        partition: 1,
        offset: 0,
        key: key.into(),
    }
}

async fn queue(backend: &MemoryBackend, items: &[Work]) {
    let mut cx = backend.begin().await.unwrap();
    backend.queue(&mut cx, items).await.unwrap();
    backend.commit(cx).await.unwrap();
}

fn applied(backend: &MemoryBackend, tenant: &str, key: &str) -> Option<String> {
    backend
        .state()
        .rows
        .get(&("applied".into(), format!("{tenant}:{key}")))
        .cloned()
}

#[test]
fn a_key_queued_twice_is_computed_once_and_tenants_are_kept_apart() {
    pollster::block_on(async {
        let backend = MemoryBackend::new();
        queue(
            &backend,
            &[
                work("a", "Q1"),
                work("b", "Q1"),
                work("a", "Q2"),
                work("a", "Q1"),
            ],
        )
        .await;
        let worker = Worker::new().with(Arc::new(Recorder)).unwrap();
        let ran = worker.run_once(&backend).await.unwrap();
        assert_eq!((ran.applied, ran.rows), (4, 3), "{ran:?}");
        assert!(ran.failures.is_empty());
        assert_eq!(applied(&backend, "a", "Q1").as_deref(), Some("1"));
        assert_eq!(applied(&backend, "a", "Q2").as_deref(), Some("1"));
        assert_eq!(applied(&backend, "b", "Q1").as_deref(), Some("1"));
        assert!(backend.state().work.items.is_empty());
        assert!(worker.run_once(&backend).await.unwrap().idle());
    });
}

#[test]
fn a_failure_is_recorded_and_bounded_and_blocks_no_other_tenant() {
    pollster::block_on(async {
        let backend = MemoryBackend::new();
        queue(
            &backend,
            &[work("a", "Q1"), work("a", "QBOOM"), work("b", "Q1")],
        )
        .await;
        let mut worker = Worker::new().with(Arc::new(Recorder)).unwrap();
        worker.max_attempts = 2;
        // First pass: tenant a's batch fails as a whole and is rolled back; b is applied.
        let ran = worker.run_once(&backend).await.unwrap();
        assert_eq!(ran.applied, 1);
        assert_eq!(ran.failures.len(), 1);
        assert_eq!(ran.failures[0].tenant, "a");
        assert_eq!(ran.failures[0].ids.len(), 2);
        assert!(ran.failures[0].error.contains("QBOOM"));
        assert!(applied(&backend, "a", "Q1").is_none(), "rolled back");
        assert_eq!(applied(&backend, "b", "Q1").as_deref(), Some("1"));
        {
            let s = backend.state();
            assert_eq!(s.work.items.len(), 2);
            assert!(s.work.items.iter().all(|q| q.attempts == 1));
        }
        // Second pass: fails again; both items reach the attempt limit.
        let ran = worker.run_once(&backend).await.unwrap();
        assert_eq!(ran.failures.len(), 1);
        // Third pass: nothing is leased; the items wait for an operator.
        let ran = worker.run_once(&backend).await.unwrap();
        assert!(ran.idle(), "{ran:?}");
        assert_eq!(backend.state().work.items.len(), 2);
        let mut cx = backend.begin().await.unwrap();
        let lag = backend
            .queue_lag(&mut cx, worker.max_attempts)
            .await
            .unwrap();
        assert_eq!(lag.len(), 1);
        assert_eq!((lag[0].waiting, lag[0].failed), (0, 2));
        // New work for the tenant is still applied: the poisoned items hold nothing up.
        drop(cx);
        queue(&backend, &[work("a", "Q9")]).await;
        let ran = worker.run_once(&backend).await.unwrap();
        assert_eq!(ran.applied, 1);
        assert_eq!(applied(&backend, "a", "Q9").as_deref(), Some("1"));
    });
}

#[test]
fn run_until_idle_stops_at_the_limit() {
    pollster::block_on(async {
        let backend = MemoryBackend::new();
        let items: Vec<Work> = (0..7).map(|i| work("a", &format!("Q{i}"))).collect();
        queue(&backend, &items).await;
        let mut worker = Worker::new().with(Arc::new(Recorder)).unwrap();
        worker.batch = 2;
        let ran = worker.run_until_idle(&backend, 5).await.unwrap();
        assert_eq!(ran.applied, 6, "three batches of two");
        assert_eq!(backend.state().work.items.len(), 1);
        let ran = worker.run_until_idle(&backend, 100).await.unwrap();
        assert_eq!(ran.applied, 1);
    });
}
