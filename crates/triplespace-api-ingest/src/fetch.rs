//! The fetch queue, `ops.entity_fetch` (0070 §2.2, §3; 0078 §4; 03 §4.12): entities
//! waiting to be read from their provider or source and written to its mirror.
//!
//! A row names the entity as the upstream wiki knows it (`Q42`), under the provider's slug
//! or the source's name, with `tenant` `''` for a registry provider and the tenant for its
//! source. A busy entity is queued once: queuing it again keeps the row it has. The
//! follower of the provider or source drains the queue with its polls (0070 A9): each poll
//! takes the oldest rows, fetches them with the entities the wiki's recent changes named,
//! and deletes them in the transaction that commits the batch, so that a crash leaves
//! them queued. An entity upstream reports missing that the mirror does not hold writes no
//! record (0070 A5), and its row goes all the same.

use std::time::SystemTime;

use tokio_postgres::GenericClient;

/// Why an entity was queued (03 §4.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Read on a tenant that lists the provider (`on-demand`).
    Read,
    /// Named in a graph of a listing tenant (`linked`).
    Linked,
    /// Reached along a closure property.
    Closure,
    /// Named by an event stream.
    Event,
    /// Found by the daily sweep.
    Sweep,
    /// Queued by an operator.
    Manual,
    /// Listed by a crawl (0092 §4).
    Crawl,
}

impl Reason {
    /// The word the table holds.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Linked => "linked",
            Self::Closure => "closure",
            Self::Event => "event",
            Self::Sweep => "sweep",
            Self::Manual => "manual",
            Self::Crawl => "crawl",
        }
    }
}

/// Queues `ids`, upstream IDs of the provider or source `provider`, for `tenant` (`""` for
/// a registry provider), at closure depth 0. An ID already queued keeps its row. Returns
/// how many were queued now.
///
/// # Errors
///
/// The database's.
pub async fn enqueue<C: GenericClient>(
    client: &C,
    tenant: &str,
    provider: &str,
    ids: &[String],
    reason: Reason,
) -> Result<u64, tokio_postgres::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    client
        .execute(
            "INSERT INTO ops.entity_fetch (tenant, provider, entity_id, reason)
             SELECT $1, $2, id, $4 FROM unnest($3::text[]) AS id
             ON CONFLICT (tenant, provider, entity_id) DO NOTHING",
            &[&tenant, &provider, &ids, &reason.as_str()],
        )
        .await
}

/// Queues `ids`, the closure values of an entity at depth `depth - 1` (0070 §2.3), with the
/// reason `closure`, leaving out those `graph` holds already: closure stops at an entity
/// already mirrored. Returns how many were queued now.
///
/// # Errors
///
/// The database's.
pub async fn enqueue_closure<C: GenericClient>(
    client: &C,
    tenant: &str,
    provider: &str,
    graph: &str,
    ids: &[(String, String)],
    depth: i16,
) -> Result<u64, tokio_postgres::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let upstream: Vec<&str> = ids.iter().map(|(u, _)| u.as_str()).collect();
    let stored: Vec<&str> = ids.iter().map(|(_, s)| s.as_str()).collect();
    client
        .execute(
            "INSERT INTO ops.entity_fetch (tenant, provider, entity_id, reason, depth)
             SELECT $1, $2, c.upstream, 'closure', $6
             FROM unnest($3::text[], $4::text[]) AS c(upstream, stored)
             WHERE NOT EXISTS (SELECT 1 FROM view.entity_source s
                               WHERE s.tenant = $1 AND s.graph = $5
                                 AND s.entity_id = c.stored)
             ON CONFLICT (tenant, provider, entity_id) DO NOTHING",
            &[&tenant, &provider, &upstream, &stored, &graph, &depth],
        )
        .await
}

/// How many entities wait for `provider` under `tenant`, counted to at most `cap`, so that
/// a long queue costs no more than `cap` index entries to count.
///
/// # Errors
///
/// The database's.
pub async fn queued<C: GenericClient>(
    client: &C,
    tenant: &str,
    provider: &str,
    cap: u64,
) -> Result<u64, tokio_postgres::Error> {
    let cap = i64::try_from(cap).unwrap_or(i64::MAX);
    let row = client
        .query_one(
            "SELECT count(*) FROM (SELECT 1 FROM ops.entity_fetch
                                   WHERE tenant = $1 AND provider = $2 LIMIT $3) q",
            &[&tenant, &provider, &cap],
        )
        .await?;
    Ok(u64::try_from(row.get::<_, i64>(0)).unwrap_or(0))
}

/// An entity taken from the queue, with when its row was queued, so that deleting it
/// leaves a row queued again since.
pub type Taken = (String, SystemTime);

/// The oldest `limit` entities waiting for `provider` under `tenant`, leaving out
/// `except` (those a batch still being written took), each with its closure depth, and
/// whether more wait.
///
/// # Errors
///
/// The database's.
pub async fn take<C: GenericClient>(
    client: &C,
    tenant: &str,
    provider: &str,
    limit: usize,
    except: &[String],
) -> Result<(Vec<(Taken, i16)>, bool), tokio_postgres::Error> {
    let want = i64::try_from(limit).unwrap_or(i64::MAX - 1) + 1;
    let rows = client
        .query(
            "SELECT entity_id, enqueued, depth FROM ops.entity_fetch
             WHERE tenant = $1 AND provider = $2 AND NOT (entity_id = ANY($3))
             ORDER BY enqueued, entity_id LIMIT $4",
            &[&tenant, &provider, &except, &want],
        )
        .await?;
    let more = rows.len() > limit;
    Ok((
        rows.iter()
            .take(limit)
            .map(|r| ((r.get(0), r.get(1)), r.get(2)))
            .collect(),
        more,
    ))
}

/// Deletes the rows `taken` took, each unless it was queued again after it was taken.
///
/// # Errors
///
/// The database's.
pub async fn done<C: GenericClient>(
    client: &C,
    tenant: &str,
    provider: &str,
    taken: &[Taken],
) -> Result<(), tokio_postgres::Error> {
    if taken.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = taken.iter().map(|(id, _)| id.as_str()).collect();
    let at: Vec<SystemTime> = taken.iter().map(|(_, at)| *at).collect();
    client
        .execute(
            "DELETE FROM ops.entity_fetch f
             USING unnest($3::text[], $4::timestamptz[]) AS d(id, at)
             WHERE f.tenant = $1 AND f.provider = $2 AND f.entity_id = d.id
               AND f.enqueued <= d.at",
            &[&tenant, &provider, &ids, &at],
        )
        .await?;
    Ok(())
}

/// Counts a failed attempt on each of `ids`, and drops the rows that have now failed
/// `max` times. Returns the IDs dropped.
///
/// # Errors
///
/// The database's.
pub async fn failed<C: GenericClient>(
    client: &C,
    tenant: &str,
    provider: &str,
    ids: &[String],
    max: i32,
) -> Result<Vec<String>, tokio_postgres::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    client
        .execute(
            "UPDATE ops.entity_fetch SET attempts = attempts + 1
             WHERE tenant = $1 AND provider = $2 AND entity_id = ANY($3)",
            &[&tenant, &provider, &ids],
        )
        .await?;
    let rows = client
        .query(
            "DELETE FROM ops.entity_fetch
             WHERE tenant = $1 AND provider = $2 AND entity_id = ANY($3) AND attempts >= $4
             RETURNING entity_id",
            &[&tenant, &provider, &ids, &max],
        )
        .await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}
