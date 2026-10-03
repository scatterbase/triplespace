//! `log.merkle_node` (0013 §2): every complete node of a partition's RFC 6962 tree, by
//! level and index, written as leaves are appended. Level 0 is the leaves themselves,
//! which is where a compacted offset's leaf lives once its record row is deleted
//! (0006 A14).
//!
//! The root over `n` leaves folds the perfect subtrees of `n` (one row each); an
//! inclusion proof reads one sibling per level, computing a partial right sibling from
//! its children when the tree's right edge cuts through it.

use scatter_log::hash::{Hash, merkle_node};
use scatter_log::store::StoreError;
use scatter_log::tree::Frontier;
use tokio_postgres::GenericClient;

use crate::ids::{partition_to_db, to_db};
use crate::storage_error;

fn hash_from(row: &tokio_postgres::Row) -> Result<Hash, StoreError> {
    let bytes: Vec<u8> = row.get(0);
    bytes
        .try_into()
        .map_err(|_| StoreError::Corrupt("a merkle node is not 32 bytes".into()))
}

/// The stored node at `(level, index)`, if the subtree is complete.
pub async fn node<C: GenericClient + Sync>(
    client: &C,
    partition: u64,
    level: u32,
    index: u64,
) -> Result<Option<Hash>, StoreError> {
    let level = i16::try_from(level).map_err(|_| StoreError::Corrupt("level".into()))?;
    let row = client
        .query_opt(
            "SELECT hash FROM log.merkle_node WHERE partition = $1 AND level = $2 AND index = $3",
            &[&partition_to_db(partition), &level, &to_db(index)?],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    row.as_ref().map(hash_from).transpose()
}

async fn required<C: GenericClient + Sync>(
    client: &C,
    partition: u64,
    level: u32,
    index: u64,
) -> Result<Hash, StoreError> {
    node(client, partition, level, index)
        .await?
        .ok_or_else(|| StoreError::Corrupt(format!("merkle node ({level}, {index}) is missing")))
}

/// Stores the leaf at `index` and every ancestor it completes.
pub async fn put_leaf<C: GenericClient + Sync>(
    client: &C,
    partition: u64,
    index: u64,
    leaf: Hash,
) -> Result<(), StoreError> {
    let p = partition_to_db(partition);
    let mut level: u32 = 0;
    let mut idx = index;
    let mut hash = leaf;
    loop {
        client
            .execute(
                "INSERT INTO log.merkle_node (partition, level, index, hash) VALUES ($1, $2, $3, $4)",
                &[
                    &p,
                    &i16::try_from(level).map_err(|_| StoreError::Corrupt("level".into()))?,
                    &to_db(idx)?,
                    &hash.as_slice(),
                ],
            )
            .await
            .map_err(|e| storage_error(&e))?;
        if idx & 1 == 0 {
            return Ok(());
        }
        let left = required(client, partition, level, idx - 1).await?;
        hash = merkle_node(&left, &hash);
        level += 1;
        idx >>= 1;
    }
}

/// The right edge of the tree over the first `size` leaves, from stored nodes.
pub async fn frontier<C: GenericClient + Sync>(
    client: &C,
    partition: u64,
    size: u64,
) -> Result<Frontier, StoreError> {
    let mut f = Frontier::new();
    let mut before: u64 = 0;
    for level in (0..u64::BITS).rev() {
        if size & (1u64 << level) == 0 {
            continue;
        }
        let index = before >> level;
        let hash = required(client, partition, level, index).await?;
        f.push_subtree(level, hash)
            .map_err(|e| StoreError::Corrupt(e.to_string()))?;
        before += 1u64 << level;
    }
    Ok(f)
}

/// The tree head over the first `size` leaves.
pub async fn root<C: GenericClient + Sync>(
    client: &C,
    partition: u64,
    size: u64,
) -> Result<Hash, StoreError> {
    Ok(frontier(client, partition, size).await?.root())
}

/// The RFC 6962 head of the leaves the subtree `(level, index)` covers, cut at `size`:
/// the stored node when the subtree is complete, otherwise folded from its children.
pub async fn subtree_root<C: GenericClient + Sync>(
    client: &C,
    partition: u64,
    level: u32,
    index: u64,
    size: u64,
) -> Result<Hash, StoreError> {
    let start = index << level;
    if start >= size {
        return Err(StoreError::Corrupt("an empty subtree has no root".into()));
    }
    if level == 0 || (index + 1) << level <= size {
        return required(client, partition, level, index).await;
    }
    let left = Box::pin(subtree_root(client, partition, level - 1, index << 1, size)).await?;
    let right_start = ((index << 1) | 1) << (level - 1);
    if right_start < size {
        let right = Box::pin(subtree_root(
            client,
            partition,
            level - 1,
            (index << 1) | 1,
            size,
        ))
        .await?;
        Ok(merkle_node(&left, &right))
    } else {
        Ok(left)
    }
}

/// The inclusion proof of leaf `index` against the tree of `size` leaves (RFC 9162
/// §2.1.3.1), leaf level first, from stored nodes.
pub async fn inclusion_proof<C: GenericClient + Sync>(
    client: &C,
    partition: u64,
    index: u64,
    size: u64,
) -> Result<Vec<Hash>, StoreError> {
    if index >= size {
        return Err(StoreError::NoOffset {
            partition,
            offset: index,
        });
    }
    let mut path = Vec::new();
    let mut level = 0;
    while (1u64 << level) < size {
        let idx = index >> level;
        let sibling = idx ^ 1;
        if sibling < idx || (sibling << level) < size {
            path.push(subtree_root(client, partition, level, sibling, size).await?);
        }
        level += 1;
    }
    Ok(path)
}
