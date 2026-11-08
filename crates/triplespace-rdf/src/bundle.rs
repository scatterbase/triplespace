//! Bundles of the log for publication and backup (0006 §9; 0103 §1): what both
//! `triplespace export` and the dump's public bundle need. The partitions and their
//! origins, the instance's `key:` records, and [`Reader`], which decides what an export
//! leaves out of each record for a reader (0006 A30, A31).

use scatter_actors::{Effective, ReadDecision, Timestamp, read_decision};
use scatter_integrity::bundle::Withhold;
use scatter_integrity::checkpoint::{instance_origin, tenant_origin};
use scatter_integrity::checkpoints::CheckpointStore;
use scatter_integrity::keys::KeyChain;
use scatter_log::body::{ATTESTATION, COMMENT, CONTENT};
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::record::Record;
use scatter_log::store::LogStore;
use tokio_postgres::GenericClient;
use triplespace_projections::acl_read::{self, ActivityTarget};
use triplespace_projections::common::target_kind;
use triplespace_projections::feed::Viewer;

fn now_micros() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}

/// One partition as `log.partition` registers it.
#[derive(Debug, Clone)]
pub struct PartitionRow {
    /// The partition ID.
    pub partition: u64,
    /// The tenant, empty for an instance partition.
    pub tenant: String,
    /// The graph name.
    pub name: String,
    /// `logged` or `hashed`.
    pub integrity: String,
    /// `public`, `internal` or `private`.
    pub export: String,
}

/// The partitions, all or one tenant's, ascending by ID.
pub async fn partition_rows<C: GenericClient>(
    client: &C,
    tenant: Option<&str>,
) -> Result<Vec<PartitionRow>, String> {
    let rows = client
        .query(
            "SELECT partition, COALESCE(tenant, ''), name, integrity, export FROM log.partition
             WHERE $1::text IS NULL OR tenant = $1 ORDER BY partition",
            &[&tenant],
        )
        .await
        .map_err(|e| format!("reading log.partition: {e}"))?;
    Ok(rows
        .iter()
        .map(|r| PartitionRow {
            partition: scatter_log_postgres::ids::partition_from_db(r.get(0)),
            tenant: r.get(1),
            name: r.get(2),
            integrity: r.get(3),
            export: r.get(4),
        })
        .collect())
}

/// The host of a base URI, without scheme or path.
pub fn host_of(base: &str) -> String {
    let rest = base.split_once("://").map_or(base, |(_, r)| r);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_string()
}

/// The tenant's current base, from the instance registry.
pub async fn tenant_base<C: GenericClient>(client: &C, tenant: &str) -> Result<String, String> {
    client
        .query_opt(
            "SELECT config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&tenant],
        )
        .await
        .map_err(|e| e.to_string())?
        .and_then(|r| r.get::<_, Option<String>>(0))
        .ok_or_else(|| format!("no base for tenant `{tenant}`"))
}

/// A partition's origin line: its first checkpoint's or manifest's where it has one, so
/// that it never changes; otherwise derived from the current host.
pub async fn origin_of<C: GenericClient, S: CheckpointStore>(
    client: &C,
    checkpoints: &S,
    row: &PartitionRow,
    farm_host: &str,
) -> Result<String, String> {
    let s = |e: scatter_integrity::checkpoints::CheckpointStoreError| e.to_string();
    if let Some(first) = checkpoints.list(row.partition).await.map_err(s)?.first() {
        return Ok(first.checkpoint.origin.clone());
    }
    if let Some((n, first)) = checkpoints
        .manifests(row.partition)
        .await
        .map_err(s)?
        .first()
    {
        let suffix = format!("/segment/{n}");
        if let Some(origin) = first.checkpoint.origin.strip_suffix(&suffix) {
            return Ok(origin.to_string());
        }
    }
    Ok(if row.tenant.is_empty() {
        instance_origin(farm_host, &row.name)
    } else {
        tenant_origin(
            &host_of(&tenant_base(client, &row.tenant).await?),
            &row.name,
        )
    })
}

/// The `key:` records of the instance `config` partition, in order.
pub async fn key_records<S: LogStore>(store: &S) -> Result<Vec<Record>, String> {
    let head = store
        .head(CONFIG_PARTITION)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for offset in 0..head.size {
        if let Some(r) = store
            .read(CONFIG_PARTITION, offset)
            .await
            .map_err(|e| e.to_string())?
            .record()
            && KeyChain::is_key_record(r)
        {
            out.push(r.clone());
        }
    }
    Ok(out)
}

/// What a reader may read on one tenant: what an export scope keeps of each record.
pub struct Reader {
    effective: Effective,
    /// What the reader may see.
    pub viewer: Viewer,
}

/// The visibility bits of an activity row (0023 §5), as MediaWiki numbers them.
mod bits {
    pub const CONTENT: i16 = 1;
    pub const COMMENT: i16 = 2;
    pub const USER: i16 = 4;
}

impl Reader {
    /// The reader `effective` (an anonymous reader's, for an export) of `tenant`, refusing
    /// a tenant it may not read.
    ///
    /// # Errors
    ///
    /// A tenant the reader may not read, or a database failure.
    pub async fn new(
        tx: &tokio_postgres::Transaction<'_>,
        tenant: &str,
        effective: Effective,
    ) -> Result<Self, String> {
        let at = Timestamp(now_micros());
        let chain = acl_read::current(
            tx,
            tenant,
            &[scatter_actors::Target::Tenant(tenant.to_string())],
        )
        .await?;
        if read_decision(&effective, &chain, at) != ReadDecision::Readable {
            return Err(format!(
                "tenant `{tenant}` is not readable without logging in, so it has no export"
            ));
        }
        let hidden = acl_read::hidden(tx, tenant, &effective, at).await?;
        Ok(Self {
            viewer: Viewer::of(&effective, hidden),
            effective,
        })
    }

    /// What to withhold of each record of the partitions `ids` that has an activity row.
    ///
    /// # Errors
    ///
    /// A database failure.
    pub async fn decide(
        &self,
        tx: &tokio_postgres::Transaction<'_>,
        tenant: &str,
        ids: &[i64],
    ) -> Result<std::collections::BTreeMap<(u64, u64), Withhold>, String> {
        let rows = tx
            .query(
                "SELECT a.partition, a.\"offset\", a.target_kind, a.target_id,
                        a.params->'of'->>'kind', a.params->'of'->>'id', a.visibility, a.log_type,
                        coalesce(a.params->'restrictions'->'read'->>'group',
                                 a.params->'previous'->'read'->>'group'),
                        a.actor_key, lp.name, p.ns, op.ns
                 FROM view.activity a
                 JOIN log.partition lp ON lp.partition = a.partition
                 LEFT JOIN view.page p ON p.tenant = a.tenant AND a.target_kind = 2
                      AND p.page_id = CASE WHEN a.target_id ~ '^[0-9]+$' THEN a.target_id::bigint END
                 LEFT JOIN view.page op ON op.tenant = a.tenant AND a.params->'of'->>'kind' = '2'
                      AND op.page_id = CASE WHEN a.params->'of'->>'id' ~ '^[0-9]+$'
                                            THEN (a.params->'of'->>'id')::bigint END
                 WHERE a.tenant = $1 AND a.partition = ANY($2)",
                &[&tenant, &ids],
            )
            .await
            .map_err(|e| format!("reading view.activity: {e}"))?;
        let at = Timestamp(now_micros());
        let actors: Vec<String> = rows
            .iter()
            .filter_map(|r| r.get::<_, Option<String>>(9))
            .collect();
        let hidden_actors =
            acl_read::hidden_actors(tx, tenant, &actors, |g| self.viewer.reads_in(g), at).await?;
        let mut readable: std::collections::HashMap<(i16, String), bool> =
            std::collections::HashMap::new();
        let mut out = std::collections::BTreeMap::new();
        for r in &rows {
            let partition = scatter_log_postgres::ids::partition_from_db(r.get(0));
            let offset = u64::try_from(r.get::<_, i64>(1)).unwrap_or_default();
            let kind: Option<i16> = r.get(2);
            let id: Option<String> = r.get(3);
            let of_kind: Option<i16> = r.get::<_, Option<String>>(4).and_then(|k| k.parse().ok());
            let of_id: Option<String> = r.get(5);
            let target = ActivityTarget {
                kind,
                id: id.clone(),
                of_kind,
                of_id: of_id.clone(),
                page_ns: r.get(11),
                of_page_ns: r.get(12),
                partition_name: r.get(10),
            };
            // Absent: a target a confidential restriction hides.
            if self.viewer.hidden.hides_activity(&target) {
                out.insert((partition, offset), Withhold::Record);
                continue;
            }
            // A suppression event is its group's (09 §6.5).
            let log_type: Option<String> = r.get(7);
            let named: Option<String> = r.get(8);
            if log_type.as_deref() == Some("suppress")
                && !named.as_deref().is_some_and(|g| self.viewer.reads_in(g))
            {
                out.insert((partition, offset), Withhold::AllParts);
                continue;
            }
            // A target the reader may not read, deleted or restricted: its records' parts.
            let (tk, tid) = if kind == Some(target_kind::RECORD) {
                (of_kind, of_id)
            } else {
                (kind, id)
            };
            if let (Some(tk), Some(tid)) = (tk, tid) {
                let key = (tk, tid.clone());
                let ok = if let Some(ok) = readable.get(&key) {
                    *ok
                } else {
                    let ok = self
                        .target_readable(tx, tenant, tk, &tid, &target.partition_name, at)
                        .await?;
                    readable.insert(key, ok);
                    ok
                };
                if !ok {
                    out.insert((partition, offset), Withhold::AllParts);
                    continue;
                }
            }
            // The parts a record ACL hides, and a hidden actor's attestation.
            let visibility: i16 = r.get(6);
            let mut parts = Vec::new();
            if visibility & bits::CONTENT != 0 {
                parts.push(CONTENT);
            }
            if visibility & bits::COMMENT != 0 {
                parts.push(COMMENT);
            }
            let actor: Option<String> = r.get(9);
            if visibility & bits::USER != 0 || actor.is_some_and(|a| hidden_actors.contains(&a)) {
                parts.push(ATTESTATION);
            }
            if !parts.is_empty() {
                out.insert((partition, offset), Withhold::Parts(parts));
            }
        }
        Ok(out)
    }

    /// Whether the reader may read an entity or page now, by its chain (09 §5).
    async fn target_readable(
        &self,
        tx: &tokio_postgres::Transaction<'_>,
        tenant: &str,
        kind: i16,
        id: &str,
        graph: &str,
        at: Timestamp,
    ) -> Result<bool, String> {
        Ok(match kind {
            target_kind::ENTITY => {
                let chain = acl_read::entity_chain(tx, tenant, id, Some(graph)).await?;
                read_decision(&self.effective, &chain, at) == ReadDecision::Readable
            }
            target_kind::PAGE => match id.parse::<u64>() {
                Ok(page) => acl_read::page_readable(tx, tenant, &self.effective, page).await?,
                Err(_) => true,
            },
            _ => true,
        })
    }
}
