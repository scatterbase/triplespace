//! Starting a `config` partition (0015 §3; payloads.md §4): the `key:` record at offset
//! 0 and `graph:config` at offset 1, before anything else. The instance's is partition
//! 0; a tenant's gets a random ID and begins with a copy of the current `key:` record
//! (0018 §2).

use crate::body::{Body, BodyError};
use crate::cbor::Value;
use crate::header::CONFIG_PARTITION;
use crate::registry::{GraphEntry, GraphRegistry, KeyEntry, PAYLOAD_CONFIG, Scope};
use crate::store::{Appended, Draft, LogStore, StoreError};
use crate::tree::Segments;

/// What a new `config` partition starts with.
#[derive(Debug, Clone, PartialEq)]
pub struct Genesis {
    /// The partition ID: [`CONFIG_PARTITION`] for the instance, random for a tenant.
    pub partition: u64,
    /// Whose `config` it is.
    pub scope: Scope,
    /// The instance key the partition's checkpoints will be signed with.
    pub key: KeyEntry,
    /// The segment exponent `k` of the partition.
    pub segment_exponent: u8,
    /// Header field 3 of both records.
    pub appended_at: u64,
    /// The attestation part of both records: who started the partition.
    pub attestation: Value,
    /// The comment part of both records.
    pub comment: Option<String>,
}

impl Genesis {
    /// The two records, in order: `key:{id}`, then `graph:config`.
    pub fn drafts(&self, registry: &GraphRegistry) -> Result<[Draft; 2], GenesisError> {
        let (config, _) = registry
            .lookup("config")
            .ok_or(GenesisError::NoConfigGraph)?;
        let mut entry = GraphEntry::source(
            config,
            "config".into(),
            self.partition,
            self.segment_exponent,
        );
        entry.scope = self.scope;
        let comment = self
            .comment
            .as_ref()
            .map_or(Value::Null, |c| Value::Text(c.clone()));
        let draft = |key: String, content: Value| -> Result<Draft, GenesisError> {
            Ok(Draft {
                appended_at: self.appended_at,
                payload_type: PAYLOAD_CONFIG.into(),
                key: Some(key),
                revid: None,
                logid: None,
                page_id: None,
                body: Body::core(&content, &comment, &self.attestation)?,
            })
        };
        Ok([
            draft(self.key.key(), self.key.to_value())?,
            draft(
                entry.key(),
                entry
                    .to_value()
                    .map_err(|e| GenesisError::Entry(e.to_string()))?,
            )?,
        ])
    }

    /// Creates the partition in `store` and appends the two records.
    pub async fn append<S: LogStore>(
        &self,
        store: &mut S,
        registry: &GraphRegistry,
    ) -> Result<[Appended; 2], GenesisError> {
        let [key, graph] = self.drafts(registry)?;
        let segments = Segments::new(self.segment_exponent).ok_or(GenesisError::Exponent)?;
        store.create_partition(self.partition, segments).await?;
        let a = store.append(self.partition, key).await?;
        let b = store.append(self.partition, graph).await?;
        Ok([a, b])
    }
}

/// Why a partition could not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GenesisError {
    /// The registry has no `config` row.
    #[error("the registry has no `config` graph")]
    NoConfigGraph,
    /// A segment exponent of 64 or more.
    #[error("the segment exponent must be below 64")]
    Exponent,
    /// A part did not encode.
    #[error(transparent)]
    Body(#[from] BodyError),
    /// The `graph:` entry did not serialize.
    #[error("graph entry: {0}")]
    Entry(String),
    /// The store refused.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A fresh partition ID: 64 random bits, never [`CONFIG_PARTITION`] (0018 §2).
///
/// # Panics
///
/// If the operating system's random source fails.
#[must_use]
pub fn fresh_partition_id() -> u64 {
    loop {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("the OS random source works");
        let id = u64::from_be_bytes(b);
        if id != CONFIG_PARTITION {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryStore;
    use crate::registry::{History, Integrity};
    use crate::store::Slot;

    fn genesis(partition: u64, scope: Scope) -> Genesis {
        Genesis {
            partition,
            scope,
            key: KeyEntry {
                key_id: "ybndrfg8ejkmcpqxot1uwisza345h769".into(),
                alg: "ed25519".into(),
                public_key: vec![3; 32],
                final_checkpoint: None,
            },
            segment_exponent: 16,
            appended_at: 1_790_000_000_000_000,
            attestation: Value::map(vec![(
                Value::text("actor"),
                Value::text("instance:scatter"),
            )]),
            comment: Some("instance create".into()),
        }
    }

    #[test]
    fn the_instance_config_starts_with_key_then_graph() {
        pollster::block_on(async {
            let registry = GraphRegistry::embedded();
            let mut store = MemoryStore::new();
            let [a, b] = genesis(CONFIG_PARTITION, Scope::Instance)
                .append(&mut store, &registry)
                .await
                .unwrap();
            assert_eq!((a.offset, b.offset), (0, 1));
            let head = store.head(CONFIG_PARTITION).await.unwrap();
            assert_eq!(head.size, 2);
            assert_eq!(head.root, b.root);
            assert_eq!(head.segments, Segments::new(16).unwrap());

            let Slot::Record(key) = store.read(0, 0).await.unwrap() else {
                panic!("a record")
            };
            assert_eq!(
                key.header().key.as_deref(),
                Some("key:ybndrfg8ejkmcpqxot1uwisza345h769")
            );
            assert_eq!(key.header().payload_type, PAYLOAD_CONFIG);
            assert_eq!(key.header().revid, None);
            let k = KeyEntry::from_value(&key.body().content().value().unwrap().unwrap()).unwrap();
            assert_eq!(k.public_key, vec![3; 32]);

            let Slot::Record(graph) = store.read(0, 1).await.unwrap() else {
                panic!("a record")
            };
            assert_eq!(graph.header().key.as_deref(), Some("graph:config"));
            let g =
                GraphEntry::from_value(graph.body().content().value().unwrap().unwrap()).unwrap();
            assert_eq!(g.name, "config");
            assert_eq!(g.partition, Some(0));
            assert_eq!(g.scope, Scope::Instance);
            assert_eq!(g.segment_exponent, Some(16));
            assert_eq!(g.history, Some(History::Full));
            assert_eq!(g.integrity, Some(Integrity::Logged));
            assert!(g.payload_types.iter().any(|t| t == PAYLOAD_CONFIG));
            assert_eq!(
                graph.body().comment().value().unwrap(),
                Some(Value::text("instance create"))
            );

            // A second genesis of the same partition is refused by the store.
            assert!(matches!(
                genesis(CONFIG_PARTITION, Scope::Instance)
                    .append(&mut store, &registry)
                    .await,
                Err(GenesisError::Store(StoreError::PartitionExists(0)))
            ));
            // A tenant's config has its own ID and scope.
            let id = fresh_partition_id();
            assert_ne!(id, CONFIG_PARTITION);
            let [a, _] = genesis(id, Scope::Tenant)
                .append(&mut store, &registry)
                .await
                .unwrap();
            assert_eq!(a.offset, 0);
            assert_eq!(store.partitions().await.unwrap().len(), 2);
        });
    }

    #[test]
    fn partition_ids_are_random() {
        assert_ne!(fresh_partition_id(), fresh_partition_id());
    }
}
