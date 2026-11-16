//! Read-your-writes across replicas (0013 §7): after a write, the session records the
//! commit's WAL position; a read goes to a replica only when that replica's replay
//! position has passed it, and to the primary otherwise. MediaWiki's
//! ChronologyProtector, with the LSN in place of the binlog position.

use std::cmp::Ordering;

use tokio_postgres::GenericClient;

/// A WAL position, `X/Y` in Postgres's text form, as one number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lsn(pub u64);

impl Lsn {
    /// Parses `X/Y`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (hi, lo) = s.split_once('/')?;
        let hi = u32::from_str_radix(hi, 16).ok()?;
        let lo = u32::from_str_radix(lo, 16).ok()?;
        Some(Self((u64::from(hi) << 32) | u64::from(lo)))
    }

    /// `X/Y`.
    #[must_use]
    pub fn to_text(self) -> String {
        format!("{:X}/{:X}", self.0 >> 32, self.0 & 0xffff_ffff)
    }
}

impl std::fmt::Display for Lsn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_text())
    }
}

/// The primary's current WAL position: what a session records after a commit.
pub async fn current_lsn<C: GenericClient>(client: &C) -> Result<Lsn, tokio_postgres::Error> {
    let row = client
        .query_one("SELECT pg_current_wal_lsn()::text", &[])
        .await?;
    Ok(Lsn::parse(&row.get::<_, String>(0)).unwrap_or(Lsn(0)))
}

/// A replica's replay position, or `None` on a primary. A server that is not in
/// recovery is a primary even if it once replayed WAL (after a crash recovery or a
/// promotion), so the recovery flag decides, not the replay position alone.
pub async fn replay_lsn<C: GenericClient>(
    client: &C,
) -> Result<Option<Lsn>, tokio_postgres::Error> {
    let row = client
        .query_one(
            "SELECT pg_is_in_recovery(), pg_last_wal_replay_lsn()::text",
            &[],
        )
        .await?;
    if !row.get::<_, bool>(0) {
        return Ok(None);
    }
    Ok(row.get::<_, Option<String>>(1).and_then(|s| Lsn::parse(&s)))
}

/// Where a read may go, given the position the session must see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The replica has replayed past the session's position.
    Replica,
    /// It has not, or the session's position is unknown.
    Primary,
}

/// Routes a read: to the replica when its replay position is at or past `needed`.
#[must_use]
pub fn route(needed: Option<Lsn>, replica_replayed: Option<Lsn>) -> Route {
    match (needed, replica_replayed) {
        (None, Some(_)) => Route::Replica,
        (Some(n), Some(r)) if r.cmp(&n) != Ordering::Less => Route::Replica,
        _ => Route::Primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsns_parse_and_order() {
        let a = Lsn::parse("0/16B3748").unwrap();
        let b = Lsn::parse("0/16B3750").unwrap();
        let c = Lsn::parse("1/0").unwrap();
        assert!(a < b && b < c);
        assert_eq!(a.to_text(), "0/16B3748");
        assert_eq!(c.0, 1 << 32);
        assert!(Lsn::parse("nonsense").is_none());
        assert!(Lsn::parse("100000000/0").is_none());
        assert_eq!(route(Some(b), Some(a)), Route::Primary);
        assert_eq!(route(Some(a), Some(b)), Route::Replica);
        assert_eq!(route(Some(a), Some(a)), Route::Replica);
        assert_eq!(route(None, Some(a)), Route::Replica);
        assert_eq!(route(Some(a), None), Route::Primary);
    }
}
