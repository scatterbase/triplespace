//! Bundles of the log for publication and backup (0006 §9.1, §9.2; 0103 §1): what both
//! `triplespace export` and the dump's public bundle need. The partitions and their
//! origins, the instance's `key:` records, and [`Reader`], which decides what an export
//! leaves out of each record for a reader (0006 A30, A31).

use scatter_actors::{Effective, ReadDecision, Timestamp, read_decision};
use scatter_integrity::bundle::Withhold;
use scatter_integrity::checkpoint::{instance_origin, tenant_origin};
use scatter_integrity::keys::KeyChain;
use scatter_log::body::{ATTESTATION, COMMENT, CONTENT};
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::record::Record;
use scatter_log::store::Slot;
use scatter_log_postgres::{PgClient, checkpoints};
use tokio_postgres::GenericClient;
use tokio_postgres::types::ToSql;
use triplespace_projections::acl_read;

pub use scatter_integrity::checkpoint::host_of;
use triplespace_projections::common::target_kind;
use triplespace_projections::feed::{self, Viewer};

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
    /// The origin line recorded at creation (01 §4.2; 0006 A29); none for a partition
    /// created before the column existed that no migration could fill.
    pub origin: Option<String>,
}

/// The partitions, all or one tenant's, ascending by ID.
pub async fn partition_rows<C: GenericClient>(
    client: &C,
    tenant: Option<&str>,
) -> Result<Vec<PartitionRow>, String> {
    let rows = client
        .query(
            "SELECT partition, COALESCE(tenant, ''), name, integrity, export, origin FROM log.partition
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
            origin: r.get(5),
        })
        .collect())
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

/// A partition's origin line, which everything that signs, bundles or proves goes
/// through: the one its registry row recorded at creation (01 §4.2; 0006 A29), so that a
/// tenant aliased to a new host signs under the host it was created under. Only a
/// partition created before `log.partition.origin` existed, and that no migration could
/// fill, falls back to its first checkpoint's or manifest's origin, and before any to one
/// derived from the tenant's current host, or the farm's: the best that can be done when
/// the host at creation was not recorded (0006 A29).
pub async fn origin_of<C: PgClient + GenericClient>(
    client: &C,
    row: &PartitionRow,
    farm_host: &str,
) -> Result<String, String> {
    if let Some(origin) = &row.origin {
        return Ok(origin.clone());
    }
    let s = |e: scatter_integrity::checkpoints::CheckpointStoreError| e.to_string();
    if let Some(first) = checkpoints::list(client, row.partition)
        .await
        .map_err(s)?
        .first()
    {
        return Ok(first.checkpoint.origin.clone());
    }
    if let Some((n, first)) = checkpoints::manifests(client, row.partition)
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

/// The `key:` records of the instance `config` partition, in order, read through any
/// connection.
pub async fn key_records<C: PgClient>(client: &C) -> Result<Vec<Record>, String> {
    const PAGE: usize = 1000;
    let mut out = Vec::new();
    let mut from = 0;
    loop {
        let page = scatter_log_postgres::log::scan(client, CONFIG_PARTITION, from, PAGE)
            .await
            .map_err(|e| e.to_string())?;
        let Some((last, _)) = page.last() else {
            break;
        };
        from = last + 1;
        let done = page.len() < PAGE;
        out.extend(
            page.into_iter()
                .map(|(_, r)| r)
                .filter(KeyChain::is_key_record),
        );
        if done {
            break;
        }
    }
    Ok(out)
}

/// Which offsets [`Reader::decide`] looks at.
#[derive(Debug, Clone)]
pub enum Offsets<'a> {
    /// Every offset of the partitions.
    All,
    /// A range of them.
    Range(std::ops::Range<u64>),
    /// These offsets, in any of the partitions.
    At(&'a [u64]),
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
    pub async fn new<C: PgClient>(
        tx: &C,
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
                "tenant `{tenant}` is not readable by this reader, so it has no export"
            ));
        }
        let hidden = acl_read::hidden(tx, tenant, &effective, at).await?;
        Ok(Self {
            viewer: Viewer::of(&effective, hidden),
            effective,
        })
    }

    /// What to withhold of each record of the partitions `ids` that has an activity row,
    /// of those `offsets` names.
    ///
    /// # Errors
    ///
    /// A database failure.
    #[allow(clippy::too_many_lines)]
    pub async fn decide<C: PgClient>(
        &self,
        tx: &C,
        tenant: &str,
        ids: &[i64],
        offsets: Offsets<'_>,
    ) -> Result<std::collections::BTreeMap<(u64, u64), Withhold>, String> {
        let bound = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        let (from, to, at): (Option<i64>, Option<i64>, Option<Vec<i64>>) = match offsets {
            Offsets::All => (None, None, None),
            Offsets::Range(r) => (Some(bound(r.start)), Some(bound(r.end)), None),
            Offsets::At(list) => (None, None, Some(list.iter().map(|&o| bound(o)).collect())),
        };
        // The feeds' own row rules (0006 A31), selected per row rather than filtering:
        // whether the viewer may see the row's target at all, and whether a suppression
        // event is its group's (09 §6.5). A null answer (a suppression naming no group) is
        // "may not".
        let mut params: Vec<feed::Param> = vec![
            Box::new(tenant.to_string()),
            Box::new(ids.to_vec()),
            Box::new(from),
            Box::new(to),
            Box::new(at),
        ];
        let targets = feed::visible_targets(&self.viewer.hidden, &mut params)
            .unwrap_or_else(|| "true".into());
        let suppressions = feed::visible_suppressions(&self.viewer, &mut params);
        let sql = format!(
            "SELECT a.partition, a.\"offset\", a.target_kind, a.target_id,
                    a.params->'of'->>'kind', a.params->'of'->>'id', a.visibility,
                    a.actor_key, lp.name,
                    coalesce(({targets}), false), coalesce(({suppressions}), false)
             FROM view.activity a
             JOIN log.partition lp ON lp.partition = a.partition
             WHERE a.tenant = $1 AND a.partition = ANY($2)
               AND ($3::bigint IS NULL OR a.\"offset\" >= $3)
               AND ($4::bigint IS NULL OR a.\"offset\" < $4)
               AND ($5::bigint[] IS NULL OR a.\"offset\" = ANY($5))"
        );
        let refs: Vec<&(dyn ToSql + Sync)> = params
            .iter()
            .map(|p| p.as_ref() as &(dyn ToSql + Sync))
            .collect();
        let rows = tx
            .query(&sql, &refs)
            .await
            .map_err(|e| format!("reading view.activity: {e}"))?;
        let at = Timestamp(now_micros());
        let actors: Vec<String> = rows
            .iter()
            .filter_map(|r| r.get::<_, Option<String>>(7))
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
            let partition_name: String = r.get(8);
            // Absent: a target a confidential restriction hides.
            if !r.get::<_, bool>(9) {
                out.insert((partition, offset), Withhold::Record);
                continue;
            }
            // A suppression event is its group's (09 §6.5).
            if !r.get::<_, bool>(10) {
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
                        .target_readable(tx, tenant, tk, &tid, &partition_name, at)
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
            let actor: Option<String> = r.get(7);
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
    async fn target_readable<C: PgClient>(
        &self,
        tx: &C,
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

/// How a route names a partition: by ID, as record IRIs do, or by name, as origin lines
/// and `/.well-known/tlog/` do.
#[derive(Debug, Clone, Copy)]
pub enum PartitionRef<'a> {
    /// The partition ID.
    Id(u64),
    /// The partition's name on its tenant.
    Name(&'a str),
}

/// The partition `which` of `tenant` if it is published to `reader`: the tenant's own,
/// `public`, and of no graph a confidential restriction hides from the reader (17 §1.1).
///
/// # Errors
///
/// A database failure.
pub async fn published<C: GenericClient>(
    client: &C,
    tenant: &str,
    reader: &Reader,
    which: PartitionRef<'_>,
) -> Result<Option<PartitionRow>, String> {
    Ok(partition_rows(client, Some(tenant))
        .await?
        .into_iter()
        .find(|r| match which {
            PartitionRef::Id(id) => r.partition == id,
            PartitionRef::Name(name) => r.name == name,
        })
        .filter(|r| r.export == "public" && !reader.viewer.hidden.graphs.contains(&r.name)))
}

pub use scatter_integrity::records::Shown;

/// What an export shows of `slot` under `withhold`, as [`scatter_integrity::bundle`]
/// writes it.
///
/// # Errors
///
/// A part index the body does not have.
pub fn redact(slot: Slot, withhold: Option<&Withhold>) -> Result<Shown, String> {
    let mut record = match (slot, withhold) {
        (slot, Some(Withhold::Record)) => return Ok(Shown::Leaf(slot.leaf())),
        (Slot::Compacted { leaf }, _) => return Ok(Shown::Leaf(leaf)),
        (Slot::Record(r), _) => r,
    };
    let present: Vec<usize> = record
        .body()
        .parts()
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.is_erased())
        .map(|(i, _)| i)
        .collect();
    let parts: Vec<usize> = match withhold {
        Some(Withhold::Parts(want)) => present.into_iter().filter(|i| want.contains(i)).collect(),
        Some(Withhold::AllParts) => present,
        _ => Vec::new(),
    };
    for &i in &parts {
        record.body_mut().erase(i).map_err(|e| e.to_string())?;
    }
    Ok(Shown::Record {
        record,
        withheld: parts,
    })
}
