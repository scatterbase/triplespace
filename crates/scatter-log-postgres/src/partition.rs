//! The `log.partition` row: the registry entry a partition was created with (0013 §2),
//! including the origin line its checkpoints are signed under, fixed at creation
//! (01 §4.2; 0006 A29).

use scatter_integrity::checkpoint::{host_of, instance_origin, tenant_origin};
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
    /// The origin line of its checkpoints and segment manifests, fixed when the partition
    /// is created from the host it is created under, so that an `alias` of the tenant
    /// afterwards does not change it (01 §4.2; 0006 A29): [`origin_under`]. None for a
    /// partition registered without one, which is signed under the origin of its first
    /// checkpoint or, before any, the host current when it is first signed.
    pub origin: Option<String>,
}

/// The origin line of a partition created under `base` (01 §4.2; 0006 A29): a tenant's
/// partition under the tenant's base at creation, `{host}/log/{name}`, and an instance
/// partition (no `tenant`) under the farm's, `{farm host}/instance/log/{name}` (0046 §7).
/// The one derivation of an origin from a base, so that what the registry row records and
/// what a partition without a recorded origin is signed under agree.
#[must_use]
pub fn origin_under(base: &str, tenant: Option<&str>, name: &str) -> String {
    let host = host_of(base);
    match tenant {
        Some(_) => tenant_origin(&host, name),
        None => instance_origin(&host, name),
    }
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
            origin: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    // 01 §4.2; 0006 A29: the origin a partition is registered with.
    #[test]
    fn origin_from_base() {
        assert_eq!(
            origin_under("https://librarybase.org", Some("lb"), "local"),
            "librarybase.org/log/local"
        );
        assert_eq!(
            origin_under("https://librarybase.org/", Some("lb"), "source/wd"),
            "librarybase.org/log/source/wd"
        );
        assert_eq!(
            origin_under("https://farm.example:8443/", None, "mirror/wikidata"),
            "farm.example:8443/instance/log/mirror/wikidata"
        );
        assert_eq!(PartitionInfo::unnamed(7).origin, None);
    }
}
