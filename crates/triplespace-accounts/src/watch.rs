//! The watch set (0020 §3; 16 §4.3): `private.watch`, an account's watched targets.
//!
//! A target is `(kind, id)` in `view.activity`'s numbering ([`kind`]); the caller has
//! already made a talk page its subject, so the star on either toggles one row. A row whose
//! `expires` has passed is treated as gone by every read, and [`sweep`] deletes it.
//! `seen` is the newest change the holder has seen of the target, the inverse of
//! MediaWiki's `wl_notificationtimestamp`. `notify` delivers the target's changes as `watch`
//! notifications (0021 §2).
//!
//! Every function takes the holder's actor key but [`notified`], which the notifier asks:
//! who watches these targets with `notify`. Nothing here reads `view`; the caller expands a
//! watch set to the activity rows it covers (03 §1.2: no join across schemas).

use tokio_postgres::GenericClient;

/// Target kinds, as `view.activity` numbers them.
pub mod kind {
    /// An entity, by ID.
    pub const ENTITY: i16 = 1;
    /// A page, by page ID: a document page, a board or a thread.
    pub const PAGE: i16 = 2;
    /// An actor, by key: their user page and user talk page.
    pub const ACTOR: i16 = 3;
    /// A title no page holds yet, `{ns}:{title}`.
    pub const TITLE: i16 = 6;
}

/// One watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    /// The target kind.
    pub kind: i16,
    /// The target.
    pub id: String,
    /// When it was added, in microseconds.
    pub added: i64,
    /// When it lapses, in microseconds; `None` for a permanent watch.
    pub expires: Option<i64>,
    /// The newest change the holder has seen, in microseconds.
    pub seen: Option<i64>,
    /// Whether its changes are notifications too.
    pub notify: bool,
}

const LIVE: &str = "(expires IS NULL OR expires > now())";

fn us(column: &str) -> String {
    format!("(extract(epoch FROM {column}) * 1e6)::bigint")
}

fn columns() -> String {
    format!(
        "target_kind, target_id, {}, {}, {}, notify",
        us("added"),
        us("expires"),
        us("seen")
    )
}

fn row(r: &tokio_postgres::Row) -> Watch {
    Watch {
        kind: r.get(0),
        id: r.get(1),
        added: r.get(2),
        expires: r.get(3),
        seen: r.get(4),
        notify: r.get(5),
    }
}

fn ts(us: Option<i64>) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)]
    us.map(|u| u as f64 / 1e6)
}

/// Watches a target, or changes a watch: `expires` replaces the expiry (`None` makes it
/// permanent), as MediaWiki's `action=watch` does; `notify` is set when given and kept
/// otherwise. Returns the watch as it now stands.
///
/// # Errors
///
/// A database failure.
pub async fn watch<C: GenericClient>(
    client: &C,
    actor_key: &str,
    target_kind: i16,
    target_id: &str,
    expires: Option<i64>,
    notify: Option<bool>,
) -> Result<Watch, tokio_postgres::Error> {
    // An expired row is a new watch: its seen time and its notify go with it.
    client
        .execute(
            &format!(
                "DELETE FROM private.watch WHERE actor_key = $1 AND target_kind = $2
                   AND target_id = $3 AND NOT {LIVE}"
            ),
            &[&actor_key, &target_kind, &target_id],
        )
        .await?;
    let q = format!(
        "INSERT INTO private.watch (actor_key, target_kind, target_id, expires, notify)
         VALUES ($1, $2, $3, to_timestamp($4), coalesce($5, false))
         ON CONFLICT (actor_key, target_kind, target_id) DO UPDATE
           SET expires = EXCLUDED.expires,
               notify = coalesce($5, private.watch.notify)
         RETURNING {}",
        columns()
    );
    let r = client
        .query_one(
            &q,
            &[&actor_key, &target_kind, &target_id, &ts(expires), &notify],
        )
        .await?;
    Ok(row(&r))
}

/// Stops watching a target. Returns whether a watch was removed.
///
/// # Errors
///
/// A database failure.
pub async fn unwatch<C: GenericClient>(
    client: &C,
    actor_key: &str,
    target_kind: i16,
    target_id: &str,
) -> Result<bool, tokio_postgres::Error> {
    Ok(client
        .execute(
            "DELETE FROM private.watch WHERE actor_key = $1 AND target_kind = $2 AND target_id = $3",
            &[&actor_key, &target_kind, &target_id],
        )
        .await?
        > 0)
}

/// The holder's watch of one target, if it stands.
///
/// # Errors
///
/// A database failure.
pub async fn get<C: GenericClient>(
    client: &C,
    actor_key: &str,
    target_kind: i16,
    target_id: &str,
) -> Result<Option<Watch>, tokio_postgres::Error> {
    let q = format!(
        "SELECT {} FROM private.watch
         WHERE actor_key = $1 AND target_kind = $2 AND target_id = $3 AND {LIVE}",
        columns()
    );
    Ok(client
        .query_opt(&q, &[&actor_key, &target_kind, &target_id])
        .await?
        .as_ref()
        .map(row))
}

/// The holder's watch set, in target order, up to `limit` after `after`.
///
/// # Errors
///
/// A database failure.
pub async fn list<C: GenericClient>(
    client: &C,
    actor_key: &str,
    limit: i64,
    after: Option<(i16, &str)>,
) -> Result<Vec<Watch>, tokio_postgres::Error> {
    let (k, i) = after.map_or((None, None), |(k, i)| (Some(k), Some(i.to_string())));
    let q = format!(
        "SELECT {} FROM private.watch
         WHERE actor_key = $1 AND {LIVE}
           AND ($2::smallint IS NULL OR (target_kind, target_id) > ($2, $3::text))
         ORDER BY target_kind, target_id LIMIT $4",
        columns()
    );
    Ok(client
        .query(&q, &[&actor_key, &k, &i, &limit])
        .await?
        .iter()
        .map(row)
        .collect())
}

/// The newest change the holder has seen of one target, or of every target with `None`,
/// becomes `at` (microseconds), where it is newer. Returns how many changed.
///
/// # Errors
///
/// A database failure.
pub async fn mark_seen<C: GenericClient>(
    client: &C,
    actor_key: &str,
    target: Option<(i16, &str)>,
    at: i64,
) -> Result<u64, tokio_postgres::Error> {
    let (k, i) = target.map_or((None, None), |(k, i)| (Some(k), Some(i.to_string())));
    client
        .execute(
            "UPDATE private.watch SET seen = to_timestamp($4)
             WHERE actor_key = $1 AND ($2::smallint IS NULL OR (target_kind = $2 AND target_id = $3::text))
               AND (seen IS NULL OR seen < to_timestamp($4))",
            &[&actor_key, &k, &i, &ts(Some(at))],
        )
        .await
}

/// Who watches any of these targets with `notify`, and which: `(holder, kind, id)`.
///
/// # Errors
///
/// A database failure.
pub async fn notified<C: GenericClient>(
    client: &C,
    targets: &[(i16, String)],
) -> Result<Vec<(String, i16, String)>, tokio_postgres::Error> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let kinds: Vec<i16> = targets.iter().map(|(k, _)| *k).collect();
    let ids: Vec<String> = targets.iter().map(|(_, i)| i.clone()).collect();
    Ok(client
        .query(
            &format!(
                "SELECT w.actor_key, w.target_kind, w.target_id FROM private.watch w
                 JOIN unnest($1::smallint[], $2::text[]) AS t(k, i)
                   ON w.target_kind = t.k AND w.target_id = t.i
                 WHERE w.notify AND {}",
                LIVE.replace("expires", "w.expires")
            ),
            &[&kinds, &ids],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}

/// Deletes lapsed watches (16 §4.3: the daily sweep). Returns how many.
///
/// # Errors
///
/// A database failure.
pub async fn sweep<C: GenericClient>(client: &C) -> Result<u64, tokio_postgres::Error> {
    client
        .execute(
            "DELETE FROM private.watch WHERE expires IS NOT NULL AND expires <= now()",
            &[],
        )
        .await
}
