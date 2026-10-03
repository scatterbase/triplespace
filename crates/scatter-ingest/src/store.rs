//! What a job needs of the store, beyond the projection [`Backend`] it extends: appending
//! inside the same unit of work the projections run in (0013 §7), the per-tenant
//! sequences and their floors (0013 §6; 0035 §4), the version cursor (0002 §8.4), match
//! keys (0002 §8.5), surrogates (0009 §7) and the property type map (0002 §8.6).
//!
//! The Postgres implementation lives with the surfaces; [`crate::memory::MemoryIngest`]
//! is the in-memory one the tests use.

use std::future::Future;

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::{Appended, Draft};
use scatter_projection::Backend;
use scatter_wikibase_changeset::MatchKey;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::DataType;

use crate::IngestError;

/// A per-tenant sequence (0013 §6). The instance's own sequences are under the tenant `""`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sequence {
    /// Header field 7 of a `local` or `pages` record.
    Revision,
    /// Header field 8 of a record that projects as a log event.
    Log,
    /// Header field 9, the first time a key is written.
    Page,
    /// A minted entity type's counter, by type name (`item`, `property`).
    Entity(String),
    /// A keyed type's surrogate counter (0009 §7), instance-wide.
    Surrogate(String),
    /// Job IDs (0011 §6.3), minted by the instance.
    Job,
    /// Local user IDs (0007 §3), per tenant.
    User,
}

/// A graph's current record for an entity: the version cursor of 0002 §8.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// The record's offset in the graph's partition.
    pub offset: u64,
    /// The upstream version the record carries, if any.
    pub version: Option<String>,
    /// `H(0x03 ‖ content)` of the record.
    pub content_hash: [u8; 32],
    /// The record's `appended_at`, µs since the epoch.
    pub synced_at: u64,
    /// The size the record declared, or its content length.
    pub size: u64,
}

/// What the store knows beyond the pipeline's backend.
pub trait IngestStore: Backend {
    /// Appends a draft to a partition inside the unit of work.
    fn append(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        draft: Draft,
    ) -> impl Future<Output = Result<Appended, IngestError>> + Send;

    /// The record at an offset; `None` when compacted away.
    fn read(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        offset: u64,
    ) -> impl Future<Output = Result<Option<Record>, IngestError>> + Send;

    /// The header of the newest record under a key in a partition, with its offset.
    fn latest_for_key(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        key: &str,
    ) -> impl Future<Output = Result<Option<(u64, Header)>, IngestError>> + Send;

    /// The page ID a key already has, in any partition the tenant reads (its own and the
    /// shared ones), since every record for a key repeats it (0015 §2).
    fn page_id_of(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<u64>, IngestError>> + Send;

    /// The partition of a graph: a tenant's `local`, `pages`, `log` or `actors`, or with
    /// the tenant `""` the instance's `log` or a `mirror/{slug}` (0018 §2).
    fn partition(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        graph: &str,
    ) -> impl Future<Output = Result<Option<u64>, IngestError>> + Send;

    /// The next value of a sequence.
    fn next_id(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        sequence: &Sequence,
    ) -> impl Future<Output = Result<u64, IngestError>> + Send;

    /// Raises a sequence past `consumed`, never lowering it (0035 §4).
    fn floor(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        sequence: &Sequence,
        consumed: u64,
    ) -> impl Future<Output = Result<(), IngestError>> + Send;

    /// A graph's cursor for an entity (`view.entity_source`).
    fn cursor(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        entity_id: &EntityId,
        graph: &str,
    ) -> impl Future<Output = Result<Option<Cursor>, IngestError>> + Send;

    /// Every entity a graph currently contributes to, for the `snapshot` sweep.
    fn entities_of_graph(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        graph: &str,
    ) -> impl Future<Output = Result<Vec<EntityId>, IngestError>> + Send;

    /// The entity a match key finds (`view.identifier` for an identifier key; the entity
    /// itself, if present, for a foreign ID).
    fn find_match(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        key: &MatchKey,
    ) -> impl Future<Output = Result<Option<EntityId>, IngestError>> + Send;

    /// The surrogate of a keyed entity, if one was minted (`view.keyed_surrogate`).
    fn surrogate(
        &self,
        cx: &mut Self::Cx,
        keyed_type: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<u64>, IngestError>> + Send;

    /// A property's data type, from the resolved view.
    fn datatype(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        property: &EntityId,
    ) -> impl Future<Output = Result<Option<DataType>, IngestError>> + Send;

    /// Whether every change-set record in a partition is an `adopt` (0035 §2,
    /// precondition 3): true for an empty partition.
    fn only_adoptions(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
    ) -> impl Future<Output = Result<bool, IngestError>> + Send;

    /// The sources of the tenant's earlier adoption jobs (`view.job` with mode `adopt`).
    fn adoption_sources(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
    ) -> impl Future<Output = Result<Vec<String>, IngestError>> + Send;
}
