//! `scatter-projection`'s [`Backend`] over Postgres: a unit of work is a pooled
//! connection with an open transaction, positions are `ops.projection_state`, and the
//! fan-out queue is `ops.projection_work`.
//!
//! The transaction is opened with `BEGIN` on the connection itself rather than through
//! `tokio_postgres::Transaction`, which borrows its client and so cannot be an owned
//! unit of work. Everything that runs inside it — the projections' SQL, and the log
//! append when the write path uses [`PgCx`] for both (0013 §7) — takes the connection
//! as a [`PgClient`], which prepares each statement once per pooled connection.
//!
//! A unit of work carries three things besides the connection, all scoped to the
//! transaction and dropped with it:
//!
//! - **positions**: `set_applied` records a projection's position in memory, and
//!   `commit` writes every position in one statement before `COMMIT`. The write path
//!   sets a position per projection per record, which on a bulk load is the single
//!   largest source of statements; the positions still land in the same transaction as
//!   the projections' rows, which is what 0013 §7 requires.
//! - **partition metadata**: `log.partition`'s `(tenant, name)` for a partition and the
//!   partition for a `(tenant, name)` are fixed at creation, and every projection asks
//!   for them on every record. The first answer in a unit of work serves the rest.
//! - **frontiers**: the Merkle frontier of each partition appended to
//!   ([`scatter_log_postgres::Frontiers`]), so an append writes the nodes it completes
//!   without reading siblings or the root back.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use deadpool_postgres::{ClientWrapper, Object, Pool};
use scatter_log_postgres::ids::{partition_to_db, to_db};
use scatter_log_postgres::{Frontiers, PgClient};
use scatter_projection::{Backend, ProjectionError, Work};
use triplespace_db::projection_state;

use crate::common::{PartitionInfo, Partitions};

/// A unit of work: a connection inside `BEGIN … COMMIT`, with the state the transaction
/// accumulates (see the module docs).
#[derive(Debug)]
pub struct PgCx {
    conn: Object,
    /// `(projection, partition)` → applied offset, written at commit.
    positions: BTreeMap<(String, u64), u64>,
    /// `log.partition` rows seen in this unit of work. Behind a mutex because the
    /// projections hold `&PgCx` across awaits; it is never contended.
    partitions: Mutex<PartitionCache>,
    /// The Merkle frontiers of the partitions appended to.
    frontiers: Frontiers,
}

#[derive(Debug, Default)]
struct PartitionCache {
    info: HashMap<u64, PartitionInfo>,
    by_name: HashMap<(String, String), u64>,
}

impl PgCx {
    /// The driver's own client, for SQL outside the store's functions. Statements sent
    /// through it are not prepared; the write path sends its SQL through `PgCx` itself
    /// as a [`PgClient`], which caches them.
    #[must_use]
    pub fn conn(&self) -> &tokio_postgres::Client {
        &self.conn
    }

    /// The connection and the frontiers, for an append (the two borrows are disjoint).
    pub fn for_append(&mut self) -> (&ClientWrapper, &mut Frontiers) {
        (&self.conn, &mut self.frontiers)
    }
}

impl PgClient for PgCx {
    fn raw(&self) -> &tokio_postgres::Client {
        &self.conn
    }

    fn statement(
        &self,
        sql: &str,
    ) -> impl std::future::Future<Output = Result<tokio_postgres::Statement, tokio_postgres::Error>> + Send
    {
        self.conn.prepare_cached(sql)
    }
}

impl Partitions for PgCx {
    /// The tenant and name of a partition, from the cache or `log.partition`.
    async fn partition_info(&self, partition: u64) -> Result<PartitionInfo, String> {
        if let Some(info) = self.partitions.lock().expect("cache").info.get(&partition) {
            return Ok(info.clone());
        }
        let row = self
            .conn
            .query_opt(
                "SELECT tenant, name FROM log.partition WHERE partition = $1",
                &[&partition_to_db(partition)],
            )
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("partition {partition} is not registered"))?;
        let info = PartitionInfo {
            tenant: row.get::<_, Option<String>>(0).unwrap_or_default(),
            name: row.get(1),
        };
        let mut cache = self.partitions.lock().expect("cache");
        cache
            .by_name
            .insert((info.tenant.clone(), info.name.clone()), partition);
        cache.info.insert(partition, info.clone());
        Ok(info)
    }

    /// The partition of a graph. Only found partitions are cached, so one created later
    /// in the same unit of work is still seen.
    async fn partition_of(&self, tenant: &str, name: &str) -> Result<Option<u64>, String> {
        let key = (tenant.to_string(), name.to_string());
        if let Some(p) = self.partitions.lock().expect("cache").by_name.get(&key) {
            return Ok(Some(*p));
        }
        let owner = (!tenant.is_empty()).then_some(tenant);
        let row = self
            .conn
            .query_opt(
                "SELECT partition FROM log.partition WHERE tenant IS NOT DISTINCT FROM $1 AND name = $2",
                &[&owner, &name],
            )
            .await
            .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            return Ok(None);
        };
        let partition = scatter_log_postgres::ids::partition_from_db(row.get(0));
        let mut cache = self.partitions.lock().expect("cache");
        cache.info.insert(
            partition,
            PartitionInfo {
                tenant: tenant.to_string(),
                name: name.to_string(),
            },
        );
        cache.by_name.insert(key, partition);
        Ok(Some(partition))
    }
}

/// The backend.
#[derive(Debug, Clone)]
pub struct PgBackend {
    pool: Pool,
}

fn backend(e: impl std::fmt::Display) -> ProjectionError {
    ProjectionError::Backend(e.to_string())
}

impl PgBackend {
    /// Over a pool.
    #[must_use]
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// The pool.
    #[must_use]
    pub fn pool(&self) -> &Pool {
        &self.pool
    }
}

impl Backend for PgBackend {
    type Cx = PgCx;

    async fn begin(&self) -> Result<PgCx, ProjectionError> {
        let conn = self.pool.get().await.map_err(backend)?;
        conn.batch_execute("BEGIN").await.map_err(backend)?;
        Ok(PgCx {
            conn,
            positions: BTreeMap::new(),
            partitions: Mutex::new(PartitionCache::default()),
            frontiers: Frontiers::new(),
        })
    }

    async fn commit(&self, cx: PgCx) -> Result<(), ProjectionError> {
        if !cx.positions.is_empty() {
            let rows: Vec<(&str, u64, u64)> = cx
                .positions
                .iter()
                .map(|((projection, partition), applied)| {
                    (projection.as_str(), *partition, *applied)
                })
                .collect();
            projection_state::set_applied_many(&cx, &rows)
                .await
                .map_err(backend)?;
        }
        cx.conn.batch_execute("COMMIT").await.map_err(backend)
    }

    async fn rollback(&self, cx: PgCx) -> Result<(), ProjectionError> {
        cx.conn.batch_execute("ROLLBACK").await.map_err(backend)
    }

    async fn applied(
        &self,
        cx: &mut PgCx,
        projection: &str,
        partition: u64,
    ) -> Result<u64, ProjectionError> {
        if let Some(applied) = cx
            .positions
            .get(&(projection.to_string(), partition))
            .copied()
        {
            return Ok(applied);
        }
        projection_state::applied(&*cx, projection, partition)
            .await
            .map_err(backend)
    }

    async fn set_applied(
        &self,
        cx: &mut PgCx,
        projection: &str,
        partition: u64,
        applied: u64,
    ) -> Result<(), ProjectionError> {
        cx.positions
            .insert((projection.to_string(), partition), applied);
        Ok(())
    }

    async fn queue(&self, cx: &mut PgCx, work: &[Work]) -> Result<(), ProjectionError> {
        for w in work {
            cx.execute(
                "INSERT INTO ops.projection_work (projection, tenant, partition, \"offset\", key)
                     VALUES ($1, $2, $3, $4, $5)",
                &[
                    &w.projection,
                    &w.tenant,
                    &partition_to_db(w.partition),
                    &to_db(w.offset).map_err(backend)?,
                    &w.key,
                ],
            )
            .await
            .map_err(backend)?;
        }
        Ok(())
    }

    async fn take(
        &self,
        cx: &mut PgCx,
        projection: &str,
        limit: usize,
    ) -> Result<Vec<Work>, ProjectionError> {
        let rows = cx
            .query(
                "DELETE FROM ops.projection_work
                 WHERE id IN (SELECT id FROM ops.projection_work WHERE projection = $1
                              ORDER BY partition, \"offset\", id LIMIT $2 FOR UPDATE SKIP LOCKED)
                 RETURNING projection, tenant, partition, \"offset\", key",
                &[&projection, &i64::try_from(limit).unwrap_or(i64::MAX)],
            )
            .await
            .map_err(backend)?;
        rows.iter()
            .map(|r| {
                Ok(Work {
                    projection: r.get(0),
                    tenant: r.get(1),
                    partition: scatter_log_postgres::ids::partition_from_db(r.get(2)),
                    offset: u64::try_from(r.get::<_, i64>(3)).map_err(backend)?,
                    key: r.get(4),
                })
            })
            .collect()
    }
}
