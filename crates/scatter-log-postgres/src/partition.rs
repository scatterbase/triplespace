//! The `log.partition` row: the registry entry a partition was created with (0013 §2).

use scatter_log::registry::{Export, History, Integrity};

/// What `log.partition` records beside the ID and the segment layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionInfo {
    /// The tenant slug, or none for an instance partition.
    pub tenant: Option<String>,
    /// The graph name, `local` or `mirror/wikidata`.
    pub name: String,
    /// `{base}/graph/{name}` (0015 §5).
    pub graph_iri: String,
    /// The history policy.
    pub history: History,
    /// The integrity policy.
    pub integrity: Integrity,
    /// The export policy.
    pub export: Export,
    /// The hash function, `sha-256`.
    pub hash: String,
}

impl PartitionInfo {
    /// The row a partition gets when only its ID is known, as the `LogStore` contract's
    /// `create_partition` creates it: an instance partition named after its ID, logged
    /// and full, exported internally. The write path registers partitions with their
    /// registry rows instead.
    #[must_use]
    pub fn unnamed(partition: u64) -> Self {
        Self {
            tenant: None,
            name: format!("partition/{partition:016x}"),
            graph_iri: String::new(),
            history: History::Full,
            integrity: Integrity::Logged,
            export: Export::Internal,
            hash: scatter_log::registry::HASH_SHA256.into(),
        }
    }
}

pub(crate) fn history_str(h: History) -> &'static str {
    match h {
        History::Full => "full",
        History::Latest => "latest",
    }
}

pub(crate) fn integrity_str(i: Integrity) -> &'static str {
    match i {
        Integrity::Logged => "logged",
        Integrity::Hashed => "hashed",
    }
}

pub(crate) fn export_str(e: Export) -> &'static str {
    match e {
        Export::Public => "public",
        Export::Internal => "internal",
        Export::Private => "private",
    }
}
