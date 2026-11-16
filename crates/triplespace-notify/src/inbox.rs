//! An account's inbox (16 §5.3): its rows, newest first, with their read state, and the
//! account's seen time. Every function takes the recipient's actor key; nothing here
//! reads another account's rows. What a row is about is in `view.activity`, which the
//! caller reads with the row's `(partition, offset)` under the public role: there is no
//! join across schemas (03 §1.2).

use tokio_postgres::GenericClient;

/// One notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The inbox row's ID, which Echo calls the notification ID.
    pub id: i64,
    /// The activity row's partition, as stored.
    pub partition: i64,
    /// The activity row's offset.
    pub offset: i64,
    /// Why the account was addressed (16 §5.2).
    pub reason: String,
    /// What it is about.
    pub target_kind: Option<i16>,
    /// What it is about.
    pub target_id: Option<String>,
    /// When the activity happened, in microseconds.
    pub published: i64,
    /// When the account read it, in microseconds.
    pub read: Option<i64>,
}

/// Which rows to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// Read and unread.
    #[default]
    All,
    /// Unread only (`notfilter=!read`).
    Unread,
    /// Read only (`notfilter=read`).
    Read,
}

const MICROS: &str = "(extract(epoch FROM {c}) * 1e6)::bigint";

fn micros(column: &str) -> String {
    MICROS.replace("{c}", column)
}

/// Which reasons to list or mark. An empty list is every reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reasons<'a> {
    /// Every reason.
    #[default]
    All,
    /// Only these.
    Only(&'a [&'a str]),
    /// All but these.
    Except(&'a [&'a str]),
}

/// A reason filter as SQL parameters: the names, and whether a row's reason must be among
/// them (`true`) or not (`false`); no names is every reason.
fn reason_list(reasons: Reasons<'_>) -> (Vec<String>, bool) {
    let owned = |r: &[&str]| r.iter().map(|s| (*s).to_string()).collect();
    match reasons {
        Reasons::All => (Vec::new(), true),
        Reasons::Only(r) => (owned(r), true),
        Reasons::Except(r) => (owned(r), false),
    }
}

/// Up to `limit` rows older than the row `before` (by ID), newest first.
pub async fn list<C: GenericClient>(
    client: &C,
    actor_key: &str,
    filter: Filter,
    reasons: Reasons<'_>,
    limit: i64,
    before: Option<i64>,
) -> Result<Vec<Item>, tokio_postgres::Error> {
    let cond = match filter {
        Filter::All => "",
        Filter::Unread => " AND read_at IS NULL",
        Filter::Read => " AND read_at IS NOT NULL",
    };
    let (names, only) = reason_list(reasons);
    let q = format!(
        "SELECT id, partition, \"offset\", reason, target_kind, target_id, {}, {}
         FROM private.inbox
         WHERE actor_key = $1 AND ($2::bigint IS NULL OR id < $2){cond}
           AND (cardinality($4::text[]) = 0 OR (reason = ANY($4)) = $5)
         ORDER BY published DESC, id DESC LIMIT $3",
        micros("published"),
        micros("read_at"),
    );
    Ok(client
        .query(&q, &[&actor_key, &before, &limit, &names, &only])
        .await?
        .iter()
        .map(|r| Item {
            id: r.get(0),
            partition: r.get(1),
            offset: r.get(2),
            reason: r.get(3),
            target_kind: r.get(4),
            target_id: r.get(5),
            published: r.get(6),
            read: r.get(7),
        })
        .collect())
}

/// The number of unread rows.
pub async fn unread<C: GenericClient>(
    client: &C,
    actor_key: &str,
) -> Result<i64, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT count(*) FROM private.inbox WHERE actor_key = $1 AND read_at IS NULL",
            &[&actor_key],
        )
        .await?
        .get(0))
}

/// When the account last looked at its inbox, in microseconds.
pub async fn seen_at<C: GenericClient>(
    client: &C,
    actor_key: &str,
) -> Result<Option<i64>, tokio_postgres::Error> {
    let q = format!(
        "SELECT {} FROM private.inbox_state WHERE actor_key = $1",
        micros("seen_at")
    );
    Ok(client
        .query_opt(&q, &[&actor_key])
        .await?
        .and_then(|r| r.get(0)))
}

/// Marks rows read (`read`) or unread: the listed IDs, or every row of these reasons with
/// `None`. Returns how many changed.
pub async fn mark<C: GenericClient>(
    client: &C,
    actor_key: &str,
    ids: Option<&[i64]>,
    reasons: Reasons<'_>,
    read: bool,
) -> Result<u64, tokio_postgres::Error> {
    let set = if read { "now()" } else { "NULL" };
    let only = if read {
        "read_at IS NULL"
    } else {
        "read_at IS NOT NULL"
    };
    if let Some(ids) = ids {
        return client
            .execute(
                &format!(
                    "UPDATE private.inbox SET read_at = {set}
                     WHERE actor_key = $1 AND id = ANY($2) AND {only}"
                ),
                &[&actor_key, &ids],
            )
            .await;
    }
    let (names, include) = reason_list(reasons);
    client
        .execute(
            &format!(
                "UPDATE private.inbox SET read_at = {set} WHERE actor_key = $1 AND {only}
                   AND (cardinality($2::text[]) = 0 OR (reason = ANY($2)) = $3)"
            ),
            &[&actor_key, &names, &include],
        )
        .await
}

/// Sets the seen time to now and returns it, in microseconds.
pub async fn mark_seen<C: GenericClient>(
    client: &C,
    actor_key: &str,
) -> Result<i64, tokio_postgres::Error> {
    let q = format!(
        "INSERT INTO private.inbox_state (actor_key, seen_at) VALUES ($1, now())
         ON CONFLICT (actor_key) DO UPDATE SET seen_at = EXCLUDED.seen_at
         RETURNING {}",
        micros("seen_at")
    );
    Ok(client.query_one(&q, &[&actor_key]).await?.get(0))
}
