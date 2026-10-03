//! The resolution projection (step 4; 0013 §5.1–5.2, §7): `view.entity`, `view.term` and
//! `view.identifier` for one subject, recomputed whenever a record about it lands in any
//! source graph.
//!
//! For a subject the projection gathers its contributions from `view.entity_source`: the
//! tenant's local graph, folded from the subject's local records
//! ([`LocalState`]), and each mirror's current
//! state, read from the record the cursor names; resolves them in graph order
//! ([`scatter_wikibase_resolve::resolve`]); and writes the rows. The resolved JSON is
//! stored only where it differs from a single whole-state source (0013 §5.1).
//!
//! Tenant overlays (0018 §6; 0013 §7): a record in a tenant partition recomputes that
//! tenant's `(tenant, id)` row; a record in a shared mirror partition recomputes the shared
//! `('', id)` row and the row of every tenant that holds one for the key.
//!
//! `term` and `identifier` are tables of this projection rather than projections of their
//! own: they are derived from the same resolution pass, and splitting them would resolve
//! each subject three times. Clusters are not handled yet: every ID is its own canonical
//! ID, and there is no fan-out to referrers.

use std::collections::BTreeMap;

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::Slot;
use scatter_log_postgres::log;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_providers::Registry;
use scatter_wikibase_changeset::apply::{Correction as LocalCorrection, LocalState};
use scatter_wikibase_changeset::{Operation, Retention};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::{EntityId, IdForm, StatementId};
use scatter_wikibase_model::key::{Identity, value_key};
use scatter_wikibase_model::statement::SnakKind;
use scatter_wikibase_model::value::{DataType, DataValue};
use scatter_wikibase_resolve::{Contribution, Correction, Resolved, resolve};

use crate::backend::PgCx;
use crate::common::{
    PAYLOAD_CHANGESET, header_key_for, operation, parse_surrogate_key, partition_info,
    partition_of, time_of,
};
use crate::sources::entity_subject;

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// `view.entity`, `view.term`, `view.identifier`.
#[derive(Debug, Clone)]
pub struct EntityProjection {
    /// Provider slugs in the instance's order (0004 §4), for the order of mirror graphs.
    order: Vec<String>,
}

impl Default for EntityProjection {
    /// The default order of 0004 §4: Wikidata, OpenAlex, then the registry's order.
    fn default() -> Self {
        Self::new(&["wikidata", "openalex"], Registry::default_registry())
    }
}

/// One source graph's cursor row.
struct Source {
    tenant: String,
    graph: String,
    offset: u64,
    synced_at: std::time::SystemTime,
}

/// What the local graph contributed, beyond its state.
#[derive(Default)]
struct Local {
    state: LocalState,
    latest: Option<Header>,
    /// Whether the local records are exactly one whole-state record.
    single_whole: bool,
}

impl EntityProjection {
    /// With the given provider slugs first, then the registry's other providers.
    #[must_use]
    pub fn new(slugs: &[&str], registry: &Registry) -> Self {
        let mut order: Vec<String> = slugs.iter().map(|s| (*s).to_string()).collect();
        for p in registry.providers() {
            if !order.contains(&p.slug) {
                order.push(p.slug.clone());
            }
        }
        Self { order }
    }

    fn mirror_rank(&self, graph: &str) -> usize {
        graph
            .strip_prefix("mirror/")
            .and_then(|slug| self.order.iter().position(|s| s == slug))
            .unwrap_or(usize::MAX)
    }

    async fn sources(cx: &PgCx, tenant: &str, id: &EntityId) -> Result<Vec<Source>, String> {
        let rows = cx
            .conn()
            .query(
                "SELECT tenant, graph, \"offset\", synced_at FROM view.entity_source
                 WHERE entity_id = $1 AND (tenant = $2 OR tenant = '')",
                &[&id.as_str(), &tenant],
            )
            .await
            .map_err(|e| sql(&e))?;
        rows.iter()
            .map(|r| {
                Ok(Source {
                    tenant: r.get(0),
                    graph: r.get(1),
                    offset: scatter_log_postgres::ids::from_db(r.get(2))
                        .map_err(|e| e.to_string())?,
                    synced_at: r.get(3),
                })
            })
            .collect()
    }

    /// Folds the tenant's local records for the subject.
    async fn local(cx: &PgCx, tenant: &str, id: &EntityId) -> Result<Local, String> {
        let Some(partition) = partition_of(cx.conn(), tenant, "local").await? else {
            return Ok(Local::default());
        };
        // A keyed subject with local records has a surrogate, written before them; its
        // absence means the instance `log` has not been projected yet, which a rebuild
        // that orders partitions wrongly would do. Fail loudly rather than drop the local
        // contribution.
        let Some(key) = header_key_for(cx.conn(), id).await? else {
            return Err(format!(
                "`{id}` has local records but no surrogate in view.keyed_surrogate; project the instance log first"
            ));
        };
        let records = log::read_by_key(cx.conn(), partition, &key)
            .await
            .map_err(|e| e.to_string())?;
        let mut local = Local::default();
        let mut whole = 0usize;
        let mut applied = 0usize;
        for (offset, record) in &records {
            if record.header().payload_type != PAYLOAD_CHANGESET {
                continue;
            }
            let Some(op) = operation(record)? else {
                continue;
            };
            if entity_subject(&op).as_ref() != Some(id) {
                // A surrogate key is shared by nothing else; an entity key names only
                // itself. This guards a stale index, not a design case.
                continue;
            }
            if matches!(op, Operation::Create { .. } | Operation::Adopt { .. }) {
                whole += 1;
            }
            applied += 1;
            local.state.apply(&op, Some(*offset));
            local.latest = Some(record.header().clone());
        }
        local.single_whole = applied == 1 && whole == 1;
        Ok(local)
    }

    /// The state a mirror's cursor names.
    async fn mirror(cx: &PgCx, source: &Source) -> Result<Option<(Entity, Header)>, String> {
        let Some(partition) = partition_of(cx.conn(), &source.tenant, &source.graph).await? else {
            return Err(format!("graph `{}` has no partition", source.graph));
        };
        let slot = log::read(cx.conn(), partition, source.offset)
            .await
            .map_err(|e| e.to_string())?;
        let Slot::Record(record) = slot else {
            return Ok(None);
        };
        match operation(&record)? {
            Some(Operation::Put { entity, .. }) => Ok(Some((entity, record.header().clone()))),
            _ => Ok(None),
        }
    }

    async fn delete_rows(cx: &PgCx, tenant: &str, id: &EntityId) -> Result<u64, String> {
        let mut n = 0;
        for table in ["view.term", "view.identifier"] {
            n += cx
                .conn()
                .execute(
                    &format!("DELETE FROM {table} WHERE tenant = $1 AND entity_id = $2"),
                    &[&tenant, &id.as_str()],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        n += cx
            .conn()
            .execute(
                "DELETE FROM view.entity WHERE tenant = $1 AND id = $2",
                &[&tenant, &id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        Ok(n)
    }

    /// Recomputes one `(tenant, id)` row and its terms and identifiers.
    #[allow(clippy::too_many_lines)]
    async fn resolve_subject(&self, cx: &PgCx, tenant: &str, id: &EntityId) -> Result<u64, String> {
        let mut sources = Self::sources(cx, tenant, id).await?;
        sources.sort_by_key(|s| {
            (
                s.graph != "local",
                self.mirror_rank(&s.graph),
                s.graph.clone(),
            )
        });
        let has_local = !tenant.is_empty()
            && sources
                .iter()
                .any(|s| s.graph == "local" && s.tenant == tenant);
        let local = if has_local {
            Self::local(cx, tenant, id).await?
        } else {
            Local::default()
        };
        let mut contributions = Vec::new();
        let mut headers: Vec<Header> = local.latest.iter().cloned().collect();
        let mut first_seen = sources.iter().map(|s| s.synced_at).min();
        if let Some(e) = &local.state.entity {
            contributions.push(Contribution {
                graph: "local".to_string(),
                entity: e.clone(),
            });
        }
        let mut mirror_states = 0usize;
        for s in sources.iter().filter(|s| s.graph.starts_with("mirror/")) {
            if let Some((entity, header)) = Self::mirror(cx, s).await? {
                headers.push(header);
                mirror_states += 1;
                contributions.push(Contribution {
                    graph: s.graph.clone(),
                    entity,
                });
            }
        }
        let corrections: BTreeMap<StatementId, Correction> = local
            .state
            .corrections
            .iter()
            .map(|(k, c)| {
                (
                    k.clone(),
                    match c {
                        LocalCorrection::Rank(r) => Correction::Rank(*r),
                        LocalCorrection::Suppress => Correction::Suppress,
                    },
                )
            })
            .collect();
        let Some(resolved) = resolve(&contributions, &corrections) else {
            return Self::delete_rows(cx, tenant, id).await;
        };
        let page_id = headers
            .iter()
            .filter_map(|h| h.page_id)
            .max()
            .ok_or_else(|| format!("no record for `{id}` carries a page ID (0013 §6)"))?;
        let page_id = i64::try_from(page_id).map_err(|e| e.to_string())?;
        let single_source = contributions.len() == 1
            && resolved.equals(&contributions[0].entity)
            && (mirror_states == 1 || local.single_whole);
        let json = resolved.canonical_json();
        let (kind, blob): (&str, Option<Vec<u8>>) = if single_source {
            ("source", None)
        } else {
            ("materialized", Some(json.as_bytes().to_vec()))
        };
        let size = i32::try_from(json.len()).unwrap_or(i32::MAX);
        let surrogate = if id.form() == IdForm::Keyed {
            header_key_for(cx.conn(), id)
                .await?
                .and_then(|k| parse_surrogate_key(&k).map(|(_, n)| n))
        } else {
            None
        };
        let local_offset = local
            .latest
            .as_ref()
            .map(|h| i64::try_from(h.offset))
            .transpose()
            .map_err(|e| e.to_string())?;
        let local_revid = local
            .latest
            .as_ref()
            .and_then(|h| h.revid)
            .map(i64::try_from)
            .transpose()
            .map_err(|e| e.to_string())?;
        let retention = local.state.retention.map(|r| match r {
            Retention::Cascade => "cascade",
            Retention::Orphan => "orphan",
            Retention::Retain => "retain",
        });
        let first_seen = first_seen
            .get_or_insert_with(|| {
                time_of(headers.iter().map(|h| h.appended_at).min().unwrap_or(0))
            })
            .to_owned();
        let mut n = cx
            .conn()
            .execute(
                "INSERT INTO view.entity (tenant, id, type, page_id, surrogate, cluster_id, canonical_id, retention,
                                          first_seen, local_offset, local_revid, resolved_version, generation,
                                          resolved_kind, resolved, resolved_size, deleted)
                 VALUES ($1, $2, $3, $4, $5, NULL, $2, $6, $7, $8, $9, 1, 0, $10, $11, $12, false)
                 ON CONFLICT (tenant, id) DO UPDATE SET
                   type = EXCLUDED.type, page_id = EXCLUDED.page_id, surrogate = EXCLUDED.surrogate,
                   retention = EXCLUDED.retention, first_seen = LEAST(view.entity.first_seen, EXCLUDED.first_seen),
                   local_offset = EXCLUDED.local_offset, local_revid = EXCLUDED.local_revid,
                   resolved_version = view.entity.resolved_version + 1,
                   resolved_kind = EXCLUDED.resolved_kind, resolved = EXCLUDED.resolved, resolved_size = EXCLUDED.resolved_size",
                &[
                    &tenant,
                    &id.as_str(),
                    &resolved.entity.entity_type.name(),
                    &page_id,
                    &surrogate,
                    &retention,
                    &first_seen,
                    &local_offset,
                    &local_revid,
                    &kind,
                    &blob,
                    &size,
                ],
            )
            .await
            .map_err(|e| sql(&e))?;
        n += Self::write_terms(cx, tenant, id, &resolved).await?;
        n += Self::write_identifiers(cx, tenant, id, &resolved).await?;
        Ok(n)
    }

    async fn write_terms(
        cx: &PgCx,
        tenant: &str,
        id: &EntityId,
        resolved: &Resolved,
    ) -> Result<u64, String> {
        let mut n = cx
            .conn()
            .execute(
                "DELETE FROM view.term WHERE tenant = $1 AND entity_id = $2",
                &[&tenant, &id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        let e = &resolved.entity;
        let mut rows: Vec<(i16, &str, i16, &str)> = Vec::new();
        for (lang, text) in &e.labels {
            rows.push((1, lang, 0, text));
        }
        for (lang, text) in &e.descriptions {
            rows.push((2, lang, 0, text));
        }
        for (lang, values) in &e.aliases {
            for (i, v) in values.iter().enumerate() {
                rows.push((3, lang, i16::try_from(i).unwrap_or(i16::MAX), v));
            }
        }
        for (kind, lang, ordinal, text) in rows {
            n += cx
                .conn()
                .execute(
                    "INSERT INTO view.term (tenant, entity_id, kind, lang, ordinal, text) VALUES ($1, $2, $3, $4, $5, $6)",
                    &[&tenant, &id.as_str(), &kind, &lang, &ordinal, &text],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        Ok(n)
    }

    async fn write_identifiers(
        cx: &PgCx,
        tenant: &str,
        id: &EntityId,
        resolved: &Resolved,
    ) -> Result<u64, String> {
        let mut n = cx
            .conn()
            .execute(
                "DELETE FROM view.identifier WHERE tenant = $1 AND entity_id = $2",
                &[&tenant, &id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        for (s, prov) in resolved.entity.all_statements().zip(&resolved.statements) {
            let (Some(DataType::ExternalId), SnakKind::Value(v @ DataValue::String(_)), Some(sid)) =
                (s.mainsnak.datatype.as_ref(), &s.mainsnak.kind, &s.id)
            else {
                continue;
            };
            let key = value_key(
                v,
                Some(&DataType::ExternalId),
                &s.mainsnak.property,
                &Identity,
            );
            for graph in &prov.graphs {
                n += cx
                    .conn()
                    .execute(
                        "INSERT INTO view.identifier (tenant, property, value_key, entity_id, graph, statement_id)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT (tenant, property, value_key, entity_id, graph) DO UPDATE SET statement_id = EXCLUDED.statement_id",
                        &[&tenant, &s.mainsnak.property.as_str(), &key.as_str(), &id.as_str(), &graph, &sid.as_str()],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }
        }
        Ok(n)
    }

    /// The tenants whose rows a shared-partition record about `id` touches: every tenant
    /// with an overlay row or a local cursor for the key.
    async fn overlay_tenants(cx: &PgCx, id: &EntityId) -> Result<Vec<String>, String> {
        let rows = cx
            .conn()
            .query(
                "SELECT tenant FROM view.entity WHERE id = $1 AND tenant <> ''
                 UNION SELECT tenant FROM view.entity_source WHERE entity_id = $1 AND tenant <> ''",
                &[&id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }
}

impl<B: Backend<Cx = PgCx>> Projection<B> for EntityProjection {
    fn name(&self) -> &'static str {
        "entity"
    }

    fn step(&self) -> Step {
        Step::Resolution
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CHANGESET
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let info = partition_info(cx.conn(), record.header().partition).await?;
            let Some(op) = operation(record)? else {
                return Ok(Applied::default());
            };
            let mut ids: Vec<EntityId> = entity_subject(&op).into_iter().collect();
            if let Operation::Convert { local, .. } = &op {
                ids.push(local.clone());
            }
            let mut n = 0;
            for id in &ids {
                if info.tenant.is_empty() {
                    n += self.resolve_subject(cx, "", id).await?;
                    for tenant in Self::overlay_tenants(cx, id).await? {
                        n += self.resolve_subject(cx, &tenant, id).await?;
                    }
                } else {
                    n += self.resolve_subject(cx, &info.tenant, id).await?;
                }
            }
            Ok(Applied::rows(usize::try_from(n).unwrap_or(0)))
        })
    }

    fn fanout<'a>(
        &'a self,
        cx: &'a mut PgCx,
        tenant: &'a str,
        key: &'a str,
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let id = EntityId::parse(key).map_err(|e| e.to_string())?;
            let n = self.resolve_subject(cx, tenant, &id).await?;
            Ok(usize::try_from(n).unwrap_or(0))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx.conn(), partition).await?;
            // A tenant partition resets the tenant's rows; a shared one the shared rows.
            for table in ["view.term", "view.identifier"] {
                cx.conn()
                    .execute(
                        &format!("DELETE FROM {table} WHERE tenant = $1"),
                        &[&info.tenant],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }
            cx.conn()
                .execute("DELETE FROM view.entity WHERE tenant = $1", &[&info.tenant])
                .await
                .map(|_| ())
                .map_err(|e| sql(&e))
        })
    }
}
