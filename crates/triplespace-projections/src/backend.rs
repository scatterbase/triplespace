//! `scatter-projection`'s [`Backend`] over Postgres: a unit of work is a pooled
//! connection with an open transaction, positions are `ops.projection_state`, and the
//! fan-out queue is `ops.projection_work`.
//!
//! The transaction is opened with `BEGIN` on the connection itself rather than through
//! `tokio_postgres::Transaction`, which borrows its client and so cannot be an owned
//! unit of work. Everything that runs inside it — the projections' SQL, and the log
//! append when the write path uses [`PgCx`] for both (0013 §7) — takes the connection
//! as a `GenericClient`, which `scatter-log-postgres`'s free functions do.

use deadpool_postgres::{Object, Pool};
use scatter_log_postgres::ids::{partition_to_db, to_db};
use scatter_projection::{Backend, ProjectionError, Work};
use triplespace_db::projection_state;

/// A unit of work: a connection inside `BEGIN … COMMIT`.
#[derive(Debug)]
pub struct PgCx {
    conn: Object,
}

impl PgCx {
    /// The connection, for SQL that runs inside this unit of work.
    #[must_use]
    pub fn conn(&self) -> &tokio_postgres::Client {
        &self.conn
    }
}

impl std::ops::Deref for PgCx {
    type Target = tokio_postgres::Client;
    fn deref(&self) -> &Self::Target {
        &self.conn
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
        Ok(PgCx { conn })
    }

    async fn commit(&self, cx: PgCx) -> Result<(), ProjectionError> {
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
        projection_state::applied(cx.conn(), projection, partition)
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
        projection_state::set_applied(cx.conn(), projection, partition, applied)
            .await
            .map_err(backend)
    }

    async fn queue(&self, cx: &mut PgCx, work: &[Work]) -> Result<(), ProjectionError> {
        for w in work {
            cx.conn()
                .execute(
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
            .conn()
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
