//! `ops.projection_state` (0013 §7): how far each projection has replayed each
//! partition, and the lag to the partition's head.

use scatter_log_postgres::PgClient;
use scatter_log_postgres::ids::{partition_to_db, to_db};

/// The applied offset of a projection on a partition: offsets below it are applied.
/// Zero, and no row, mean nothing is.
pub async fn applied<C: PgClient>(
    client: &C,
    projection: &str,
    partition: u64,
) -> Result<u64, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT applied_offset FROM ops.projection_state WHERE projection = $1 AND partition = $2",
            &[&projection, &partition_to_db(partition)],
        )
        .await?;
    Ok(row.map_or(0, |r| u64::try_from(r.get::<_, i64>(0)).unwrap_or(0)))
}

/// Records that offsets below `applied_offset` are applied. Runs in the caller's
/// transaction with the projection's own writes (0013 §7).
pub async fn set_applied<C: PgClient>(
    client: &C,
    projection: &str,
    partition: u64,
    applied_offset: u64,
) -> Result<(), tokio_postgres::Error> {
    client
        .execute(
            "INSERT INTO ops.projection_state (projection, partition, applied_offset, updated_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (projection, partition) DO UPDATE
             SET applied_offset = EXCLUDED.applied_offset, updated_at = now()",
            &[
                &projection,
                &partition_to_db(partition),
                &to_db(applied_offset).map_err(|_| never())?,
            ],
        )
        .await?;
    Ok(())
}

/// Records several positions at once: one row per `(projection, partition, applied)`,
/// in one statement. The write path collects a unit of work's positions in memory and
/// writes them here at commit (0013 §7), rather than one row per record.
pub async fn set_applied_many<C: PgClient>(
    client: &C,
    positions: &[(&str, u64, u64)],
) -> Result<(), tokio_postgres::Error> {
    if positions.is_empty() {
        return Ok(());
    }
    let mut projections: Vec<&str> = Vec::with_capacity(positions.len());
    let mut partitions: Vec<i64> = Vec::with_capacity(positions.len());
    let mut offsets: Vec<i64> = Vec::with_capacity(positions.len());
    for (projection, partition, applied) in positions {
        projections.push(projection);
        partitions.push(partition_to_db(*partition));
        offsets.push(to_db(*applied).map_err(|_| never())?);
    }
    client
        .execute(
            "INSERT INTO ops.projection_state (projection, partition, applied_offset, updated_at)
             SELECT t.projection, t.partition, t.applied_offset, now()
             FROM unnest($1::text[], $2::bigint[], $3::bigint[]) AS t(projection, partition, applied_offset)
             ON CONFLICT (projection, partition) DO UPDATE
             SET applied_offset = EXCLUDED.applied_offset, updated_at = now()",
            &[&projections, &partitions, &offsets],
        )
        .await?;
    Ok(())
}

/// Every projection's applied offset on a partition, with the partition's head, so lag
/// is `head - applied`.
pub async fn lags<C: PgClient>(
    client: &C,
    partition: u64,
) -> Result<Vec<(String, u64, u64)>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT s.projection, s.applied_offset, p.next_offset
             FROM ops.projection_state s JOIN log.partition p ON p.partition = s.partition
             WHERE s.partition = $1 ORDER BY s.projection",
            &[&partition_to_db(partition)],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<_, String>(0),
                u64::try_from(r.get::<_, i64>(1)).unwrap_or(0),
                u64::try_from(r.get::<_, i64>(2)).unwrap_or(0),
            )
        })
        .collect())
}

/// An offset past `i64::MAX` cannot happen; `to_db` is total on real offsets.
fn never() -> tokio_postgres::Error {
    unreachable!("an offset fits a bigint")
}
