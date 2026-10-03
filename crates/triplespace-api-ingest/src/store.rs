//! [`PgIngest`]: the ingest store over Postgres.

use scatter_ingest::{Cursor, IngestError, IngestStore, Sequence};
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::{Appended, Draft, Slot};
use scatter_log_postgres::ids::{from_db, partition_from_db, partition_to_db};
use scatter_log_postgres::{log, sequences};
use scatter_projection::{Backend, ProjectionError, Work};
use scatter_wikibase_changeset::{MatchKey, Operation};
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::key::{Identity, value_key};
use scatter_wikibase_model::value::{DataType, DataValue};
use triplespace_projections::{PgBackend, PgCx};

/// The store: the projections' backend plus the ingest reads.
#[derive(Debug, Clone)]
pub struct PgIngest {
    backend: PgBackend,
}

fn store(e: impl std::fmt::Display) -> IngestError {
    IngestError::Store(e.to_string())
}

fn sequence(s: &Sequence) -> sequences::Sequence {
    match s {
        Sequence::Revision => sequences::Sequence::Revision,
        Sequence::Log => sequences::Sequence::Log,
        Sequence::Page => sequences::Sequence::Page,
        Sequence::Entity(t) => sequences::Sequence::Entity(t.clone()),
        Sequence::Surrogate(t) => sequences::Sequence::Surrogate(t.clone()),
        Sequence::Job => sequences::Sequence::Job,
        Sequence::User => sequences::Sequence::User,
    }
}

impl PgIngest {
    /// Over the projections' backend.
    #[must_use]
    pub fn new(backend: PgBackend) -> Self {
        Self { backend }
    }

    /// The backend.
    #[must_use]
    pub fn backend(&self) -> &PgBackend {
        &self.backend
    }

    /// Creates the instance's sequences and a tenant's, if missing (0013 §6).
    pub async fn create_sequences(
        client: &tokio_postgres::Client,
        tenant: &str,
        entity_types: &[&str],
        keyed_types: &[&str],
    ) -> Result<(), IngestError> {
        sequences::create_instance(client, keyed_types).await?;
        sequences::create(client, tenant, entity_types).await?;
        Ok(())
    }

    /// A property's data type from the resolved view: the materialized JSON, or the
    /// whole-state record its cursor names.
    async fn property_datatype(
        cx: &PgCx,
        tenant: &str,
        property: &EntityId,
    ) -> Result<Option<DataType>, IngestError> {
        let row = cx
            .conn()
            .query_opt(
                "SELECT tenant, resolved FROM view.entity WHERE id = $1 AND (tenant = $2 OR tenant = '') AND type = 'property'
                 ORDER BY tenant DESC LIMIT 1",
                &[&property.as_str(), &tenant],
            )
            .await
            .map_err(store)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if let Some(bytes) = row.get::<_, Option<Vec<u8>>>(1) {
            let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(store)?;
            return Ok(v
                .get("datatype")
                .and_then(|d| d.as_str())
                .map(DataType::parse));
        }
        let owner: String = row.get(0);
        let source = cx
            .conn()
            .query_opt(
                "SELECT graph, \"offset\" FROM view.entity_source WHERE entity_id = $1 AND (tenant = $2 OR tenant = '')
                 ORDER BY tenant DESC LIMIT 1",
                &[&property.as_str(), &owner],
            )
            .await
            .map_err(store)?;
        let Some(source) = source else {
            return Ok(None);
        };
        let graph: String = source.get(0);
        let offset = from_db(source.get(1))?;
        let partition_owner = if graph.starts_with("mirror/") {
            ""
        } else {
            owner.as_str()
        };
        let Some(partition) = partition_lookup(cx, partition_owner, &graph).await? else {
            return Ok(None);
        };
        let Slot::Record(record) = log::read(cx.conn(), partition, offset).await? else {
            return Ok(None);
        };
        let Some(content) = record.body().content().value().map_err(store)? else {
            return Ok(None);
        };
        let op: Operation = scatter_log::cbor::from_value(content).map_err(store)?;
        Ok(op.entity().and_then(|e| e.datatype.clone()))
    }
}

async fn partition_lookup(
    cx: &PgCx,
    tenant: &str,
    graph: &str,
) -> Result<Option<u64>, IngestError> {
    let tenant = (!tenant.is_empty()).then_some(tenant);
    let row = cx
        .conn()
        .query_opt(
            "SELECT partition FROM log.partition WHERE tenant IS NOT DISTINCT FROM $1 AND name = $2",
            &[&tenant, &graph],
        )
        .await
        .map_err(store)?;
    Ok(row.map(|r| partition_from_db(r.get(0))))
}

impl Backend for PgIngest {
    type Cx = PgCx;

    async fn begin(&self) -> Result<PgCx, ProjectionError> {
        self.backend.begin().await
    }

    async fn commit(&self, cx: PgCx) -> Result<(), ProjectionError> {
        self.backend.commit(cx).await
    }

    async fn rollback(&self, cx: PgCx) -> Result<(), ProjectionError> {
        self.backend.rollback(cx).await
    }

    async fn applied(
        &self,
        cx: &mut PgCx,
        projection: &str,
        partition: u64,
    ) -> Result<u64, ProjectionError> {
        self.backend.applied(cx, projection, partition).await
    }

    async fn set_applied(
        &self,
        cx: &mut PgCx,
        projection: &str,
        partition: u64,
        applied: u64,
    ) -> Result<(), ProjectionError> {
        self.backend
            .set_applied(cx, projection, partition, applied)
            .await
    }

    async fn queue(&self, cx: &mut PgCx, work: &[Work]) -> Result<(), ProjectionError> {
        self.backend.queue(cx, work).await
    }

    async fn take(
        &self,
        cx: &mut PgCx,
        projection: &str,
        limit: usize,
    ) -> Result<Vec<Work>, ProjectionError> {
        self.backend.take(cx, projection, limit).await
    }
}

impl IngestStore for PgIngest {
    async fn append(
        &self,
        cx: &mut PgCx,
        partition: u64,
        draft: Draft,
    ) -> Result<Appended, IngestError> {
        Ok(log::append(cx.conn(), partition, draft).await?)
    }

    async fn read(
        &self,
        cx: &mut PgCx,
        partition: u64,
        offset: u64,
    ) -> Result<Option<Record>, IngestError> {
        match log::read(cx.conn(), partition, offset).await? {
            Slot::Record(r) => Ok(Some(r)),
            Slot::Compacted { .. } => Ok(None),
        }
    }

    async fn latest_for_key(
        &self,
        cx: &mut PgCx,
        partition: u64,
        key: &str,
    ) -> Result<Option<(u64, Header)>, IngestError> {
        let row = cx
            .conn()
            .query_opt(
                "SELECT \"offset\", header FROM log.record WHERE partition = $1 AND key = $2 ORDER BY \"offset\" DESC LIMIT 1",
                &[&partition_to_db(partition), &key],
            )
            .await
            .map_err(store)?;
        match row {
            Some(r) => Ok(Some((
                from_db(r.get(0))?,
                Header::decode(r.get(1)).map_err(store)?,
            ))),
            None => Ok(None),
        }
    }

    async fn page_id_of(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        key: &str,
    ) -> Result<Option<u64>, IngestError> {
        let tenant = (!tenant.is_empty()).then_some(tenant);
        let row = cx
            .conn()
            .query_opt(
                "SELECT r.page_id FROM log.record r JOIN log.partition p ON p.partition = r.partition
                 WHERE r.key = $1 AND r.page_id IS NOT NULL AND (p.tenant IS NOT DISTINCT FROM $2 OR p.tenant IS NULL)
                 LIMIT 1",
                &[&key, &tenant],
            )
            .await
            .map_err(store)?;
        row.map(|r| from_db(r.get(0)).map_err(IngestError::from))
            .transpose()
    }

    async fn partition(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        graph: &str,
    ) -> Result<Option<u64>, IngestError> {
        partition_lookup(cx, tenant, graph).await
    }

    async fn next_id(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        seq: &Sequence,
    ) -> Result<u64, IngestError> {
        Ok(sequences::next(cx.conn(), tenant, &sequence(seq)).await?)
    }

    async fn floor(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        seq: &Sequence,
        consumed: u64,
    ) -> Result<(), IngestError> {
        Ok(sequences::floor(cx.conn(), tenant, &sequence(seq), consumed).await?)
    }

    async fn cursor(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        entity_id: &EntityId,
        graph: &str,
    ) -> Result<Option<Cursor>, IngestError> {
        let owner = if graph.starts_with("mirror/") {
            ""
        } else {
            tenant
        };
        let row = cx
            .conn()
            .query_opt(
                "SELECT \"offset\", upstream_version, content_hash, synced_at, size FROM view.entity_source
                 WHERE tenant = $1 AND entity_id = $2 AND graph = $3",
                &[&owner, &entity_id.as_str(), &graph],
            )
            .await
            .map_err(store)?;
        let Some(r) = row else {
            return Ok(None);
        };
        let hash: Vec<u8> = r.get(2);
        let synced: std::time::SystemTime = r.get(3);
        let synced_at = synced
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(store)?
            .as_micros();
        Ok(Some(Cursor {
            offset: from_db(r.get(0))?,
            version: r.get(1),
            content_hash: hash.try_into().map_err(|_| store("a 32-byte hash"))?,
            synced_at: u64::try_from(synced_at).map_err(store)?,
            size: u64::try_from(r.get::<_, i32>(4)).map_err(store)?,
        }))
    }

    async fn entities_of_graph(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        graph: &str,
    ) -> Result<Vec<EntityId>, IngestError> {
        let owner = if graph.starts_with("mirror/") {
            ""
        } else {
            tenant
        };
        let rows = cx
            .conn()
            .query(
                "SELECT entity_id FROM view.entity_source WHERE tenant = $1 AND graph = $2",
                &[&owner, &graph],
            )
            .await
            .map_err(store)?;
        rows.iter()
            .map(|r| EntityId::parse(r.get(0)).map_err(store))
            .collect()
    }

    async fn find_match(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        key: &MatchKey,
    ) -> Result<Option<EntityId>, IngestError> {
        match key {
            MatchKey::Identifier(m) => {
                for (property, value) in m {
                    let k = value_key(
                        &DataValue::String(value.clone()),
                        Some(&DataType::ExternalId),
                        property,
                        &Identity,
                    );
                    let row = cx
                        .conn()
                        .query_opt(
                            "SELECT entity_id FROM view.identifier
                             WHERE (tenant = $1 OR tenant = '') AND property = $2 AND value_key = $3
                             ORDER BY tenant DESC LIMIT 1",
                            &[&tenant, &property.as_str(), &k.as_str()],
                        )
                        .await
                        .map_err(store)?;
                    if let Some(r) = row {
                        return Ok(Some(EntityId::parse(r.get(0)).map_err(store)?));
                    }
                }
                Ok(None)
            }
            MatchKey::Entity(id) => {
                let row = cx
                    .conn()
                    .query_opt(
                        "SELECT 1 FROM view.entity WHERE id = $1 AND (tenant = $2 OR tenant = '') LIMIT 1",
                        &[&id.as_str(), &tenant],
                    )
                    .await
                    .map_err(store)?;
                Ok(row.map(|_| id.clone()))
            }
        }
    }

    async fn surrogate(
        &self,
        cx: &mut PgCx,
        keyed_type: &str,
        key: &str,
    ) -> Result<Option<u64>, IngestError> {
        let row = cx
            .conn()
            .query_opt(
                "SELECT surrogate FROM view.keyed_surrogate WHERE tenant = '' AND keyed_type = $1 AND key = $2",
                &[&keyed_type, &key],
            )
            .await
            .map_err(store)?;
        row.map(|r| u64::try_from(r.get::<_, i64>(0)).map_err(store))
            .transpose()
    }

    async fn datatype(
        &self,
        cx: &mut PgCx,
        tenant: &str,
        property: &EntityId,
    ) -> Result<Option<DataType>, IngestError> {
        Self::property_datatype(cx, tenant, property).await
    }

    async fn only_adoptions(&self, cx: &mut PgCx, partition: u64) -> Result<bool, IngestError> {
        // The activity projection names every change set's operation; the write path
        // projects inline, so it is current for anything this instance wrote.
        let row = cx
            .conn()
            .query_one(
                "SELECT NOT EXISTS (SELECT 1 FROM view.activity WHERE partition = $1 AND kind IN ('edit', 'erased')
                                    AND (summary_op IS DISTINCT FROM 'adopt'))",
                &[&partition_to_db(partition)],
            )
            .await
            .map_err(store)?;
        Ok(row.get(0))
    }

    async fn adoption_sources(
        &self,
        cx: &mut PgCx,
        tenant: &str,
    ) -> Result<Vec<String>, IngestError> {
        let rows = cx
            .conn()
            .query(
                "SELECT DISTINCT source FROM view.job WHERE tenant = $1 AND mode = 'adopt'",
                &[&tenant],
            )
            .await
            .map_err(store)?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }
}
