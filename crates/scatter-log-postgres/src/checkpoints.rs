//! `CheckpointStore` over `log.checkpoint` and `log.segment_manifest` (0013 §2): rows
//! beside the partition, the C2SP note stored verbatim.

use std::time::SystemTime;

use scatter_integrity::checkpoint::SignedCheckpoint;
use scatter_integrity::checkpoints::{CheckpointStore, CheckpointStoreError};
use scatter_log::hash::hex;
use tokio_postgres::{Client, Transaction};

use crate::client::PgClient;

use crate::ids::{partition_to_db, to_db};

/// Checkpoints over a client or a transaction.
#[derive(Debug)]
pub struct PgCheckpoints<C> {
    client: C,
}

impl<C> PgCheckpoints<C> {
    /// Wraps a client or a borrowed transaction.
    pub fn new(client: C) -> Self {
        Self { client }
    }

    /// The client or transaction.
    pub fn client(&self) -> &C {
        &self.client
    }
}

fn storage(e: &tokio_postgres::Error) -> CheckpointStoreError {
    CheckpointStoreError::Storage(e.to_string())
}

fn db(n: u64) -> Result<i64, CheckpointStoreError> {
    to_db(n).map_err(|e| CheckpointStoreError::Storage(e.to_string()))
}

/// The signing key's note key hash, eight hex digits: what `key_id` holds.
fn key_id_of(checkpoint: &SignedCheckpoint) -> String {
    checkpoint
        .note
        .signatures
        .first()
        .map(|s| hex(&s.bytes[..4.min(s.bytes.len())]))
        .unwrap_or_default()
}

/// Stores a checkpoint, replacing one of the same size.
pub async fn put<C: PgClient>(
    client: &C,
    partition: u64,
    checkpoint: &SignedCheckpoint,
) -> Result<(), CheckpointStoreError> {
    client
        .execute(
            "INSERT INTO log.checkpoint (partition, tree_size, root, key_id, signed_note, written_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (partition, tree_size) DO UPDATE
             SET root = EXCLUDED.root, key_id = EXCLUDED.key_id, signed_note = EXCLUDED.signed_note,
                 written_at = EXCLUDED.written_at",
            &[
                &partition_to_db(partition),
                &db(checkpoint.checkpoint.size)?,
                &checkpoint.checkpoint.root.as_slice(),
                &key_id_of(checkpoint),
                &checkpoint.to_text(),
                &SystemTime::now(),
            ],
        )
        .await
        .map_err(|e| storage(&e))?;
    Ok(())
}

/// Every checkpoint of a partition, ascending by size.
pub async fn list<C: PgClient>(
    client: &C,
    partition: u64,
) -> Result<Vec<SignedCheckpoint>, CheckpointStoreError> {
    let rows = client
        .query(
            "SELECT signed_note FROM log.checkpoint WHERE partition = $1 ORDER BY tree_size",
            &[&partition_to_db(partition)],
        )
        .await
        .map_err(|e| storage(&e))?;
    rows.iter()
        .map(|r| Ok(SignedCheckpoint::parse(r.get::<_, String>(0).as_bytes())?))
        .collect()
}

/// The checkpoint a record is first covered by: the smallest size past its offset
/// (0012 §5, `GET /record/{partition}/{offset}/checkpoint`).
pub async fn covering<C: PgClient>(
    client: &C,
    partition: u64,
    offset: u64,
) -> Result<Option<SignedCheckpoint>, CheckpointStoreError> {
    let row = client
        .query_opt(
            "SELECT signed_note FROM log.checkpoint WHERE partition = $1 AND tree_size > $2
             ORDER BY tree_size LIMIT 1",
            &[&partition_to_db(partition), &db(offset)?],
        )
        .await
        .map_err(|e| storage(&e))?;
    row.map(|r| Ok(SignedCheckpoint::parse(r.get::<_, String>(0).as_bytes())?))
        .transpose()
}

/// Stores a segment manifest. A later one for the same segment is a new row.
pub async fn put_manifest<C: PgClient>(
    client: &C,
    partition: u64,
    n: u64,
    manifest: &SignedCheckpoint,
) -> Result<(), CheckpointStoreError> {
    client
        .execute(
            "INSERT INTO log.segment_manifest (partition, segment, root, signed_note, sealed_at)
             VALUES ($1, $2, $3, $4, $5)",
            &[
                &partition_to_db(partition),
                &db(n)?,
                &manifest.checkpoint.root.as_slice(),
                &manifest.to_text(),
                &SystemTime::now(),
            ],
        )
        .await
        .map_err(|e| storage(&e))?;
    Ok(())
}

/// The latest manifest of every sealed segment, by segment.
pub async fn manifests<C: PgClient>(
    client: &C,
    partition: u64,
) -> Result<Vec<(u64, SignedCheckpoint)>, CheckpointStoreError> {
    let rows = client
        .query(
            "SELECT DISTINCT ON (segment) segment, signed_note FROM log.segment_manifest
             WHERE partition = $1 ORDER BY segment, sealed_at DESC",
            &[&partition_to_db(partition)],
        )
        .await
        .map_err(|e| storage(&e))?;
    rows.iter()
        .map(|r| {
            let n = u64::try_from(r.get::<_, i64>(0))
                .map_err(|_| CheckpointStoreError::Storage("a negative segment".into()))?;
            Ok((
                n,
                SignedCheckpoint::parse(r.get::<_, String>(1).as_bytes())?,
            ))
        })
        .collect()
}

impl CheckpointStore for PgCheckpoints<Client> {
    async fn put(
        &mut self,
        partition: u64,
        checkpoint: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        put(&self.client, partition, checkpoint).await
    }

    async fn list(&self, partition: u64) -> Result<Vec<SignedCheckpoint>, CheckpointStoreError> {
        list(&self.client, partition).await
    }

    async fn put_manifest(
        &mut self,
        partition: u64,
        n: u64,
        manifest: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        put_manifest(&self.client, partition, n, manifest).await
    }

    async fn manifests(
        &self,
        partition: u64,
    ) -> Result<Vec<(u64, SignedCheckpoint)>, CheckpointStoreError> {
        manifests(&self.client, partition).await
    }
}

impl CheckpointStore for PgCheckpoints<&Transaction<'_>> {
    async fn put(
        &mut self,
        partition: u64,
        checkpoint: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        put(self.client, partition, checkpoint).await
    }

    async fn list(&self, partition: u64) -> Result<Vec<SignedCheckpoint>, CheckpointStoreError> {
        list(self.client, partition).await
    }

    async fn put_manifest(
        &mut self,
        partition: u64,
        n: u64,
        manifest: &SignedCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        put_manifest(self.client, partition, n, manifest).await
    }

    async fn manifests(
        &self,
        partition: u64,
    ) -> Result<Vec<(u64, SignedCheckpoint)>, CheckpointStoreError> {
        manifests(self.client, partition).await
    }
}
