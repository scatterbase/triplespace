//! Compacting a `latest` partition (0013 §3, A96): for the keys of the records in a range
//! of offsets, every record but the newest and the one carrying the key's highest revision
//! ID goes. A follower compacts its day's range when the day's job closes (0092 A7), and
//! `triplespace compact` any range by hand.
//!
//! The range is taken a window of offsets at a time, each window its own unit of work, so
//! that the partition's append lock, which a compaction holds, is never held for long.

use crate::IngestError;
use crate::store::IngestStore;

/// Offsets per unit of work.
pub const WINDOW: u64 = 10_000;

/// Compacts by the keys of a partition's records at offsets `from..to`, `window` offsets
/// per unit of work, telling `progress` each window's end and the records removed so far.
/// Returns the records removed. Running it again over the same range removes nothing more.
pub async fn compact<S: IngestStore>(
    store: &S,
    partition: u64,
    from: u64,
    to: u64,
    window: u64,
    mut progress: impl FnMut(u64, u64),
) -> Result<u64, IngestError> {
    let mut removed = 0;
    let mut at = from;
    while at < to {
        let end = at.saturating_add(window.max(1)).min(to);
        let mut cx = store.begin().await?;
        removed += store.compact_range(&mut cx, partition, at, end).await?;
        store.commit(cx).await?;
        at = end;
        progress(at, removed);
    }
    Ok(removed)
}

/// A registry provider's mirror partition and its size, `None` while the mirror has none
/// (its first records create it).
pub async fn mirror_of<S: IngestStore>(
    store: &S,
    slug: &str,
) -> Result<Option<(u64, u64)>, IngestError> {
    let mut cx = store.begin().await?;
    let found = match store
        .partition(&mut cx, "", &format!("mirror/{slug}"))
        .await
    {
        Ok(Some(p)) => store.partition_size(&mut cx, p).await.map(|n| Some((p, n))),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    };
    store.rollback(cx).await?;
    found
}
