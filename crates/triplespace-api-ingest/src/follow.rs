//! A registry provider's follower (0092 §1–§3, §5): the [`ChangeSource`] the ingester's
//! follow loop reads, over a Wikibase's `list=recentchanges` and `wbgetentities`.
//!
//! Each `next` is one poll: the head request (and, once caught up, the tail pass), the
//! changed titles turned into upstream IDs, their current states read in one request per
//! fifty (five hundred with the higher limits), and each state through the provider's
//! key-mapped adapter into the sync items the mirror writes. `save` stores the position in
//! `ops.repo_cursor` after the batch's job has finished.
//!
//! The follower continues a load (0092 §5): with no position of its own it starts at
//! `loaded_to`, the end of the last dump sync, and it refuses to start from a time the
//! wiki no longer keeps recent changes for (a gap), since a load is the only way across one.

use std::collections::BTreeMap;

use scatter_adapter_mediawiki::{Change, Poller, Windows};
use scatter_adapter_wikibase::follow::{EntityNamespaces, Fetched, entity_id_of, read_entities};
use scatter_adapter_wikibase::{Index, KeyMappedAdapter, KeyRules, Outcome};
use scatter_ingest::IngestError;
use scatter_ingest::follow::{ChangeBatch, ChangeSource};
use scatter_ingest::sync::SyncItem;
use scatter_providers::Provider;
use scatter_wikibase_changeset::{Adapter, Upstream, WikibaseAdapter};
use scatter_wikibase_model::id::EntityId;
use tokio_postgres::GenericClient;
use triplespace_upstream::Wiki;

/// The position row's kind for a registry provider.
pub const KIND_PROVIDER: &str = "provider";
/// The position row's kind for a tenant's entity source.
pub const KIND_SOURCE: &str = "source";

/// Where a follower is, as `ops.repo_cursor` holds it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Position {
    /// The newest change time seen, epoch seconds.
    pub rc_time: Option<u64>,
    /// The continuation.
    pub rc_continue: Option<BTreeMap<String, String>>,
    /// The change IDs seen, with their times.
    pub rc_seen: Vec<(u64, u64)>,
    /// Entities whose state was read stale, with the revision the row named, to read
    /// again (0092 §2).
    pub rc_retry: Vec<(String, Option<u64>)>,
    /// Where the last load ended, epoch seconds.
    pub loaded_to: Option<u64>,
    /// Whether the follower has taken over from the load.
    pub handed_over: bool,
    /// Late rows, cumulative.
    pub late: u64,
    /// Stale states, cumulative.
    pub stale: u64,
}

/// Reads a follower's position.
///
/// # Errors
///
/// If the store fails.
pub async fn position<C: GenericClient>(
    client: &C,
    tenant: &str,
    kind: &str,
    name: &str,
) -> Result<Position, IngestError> {
    let row = client
        .query_opt(
            "SELECT EXTRACT(EPOCH FROM rc_time)::bigint, rc_continue, rc_seen,
                    EXTRACT(EPOCH FROM loaded_to)::bigint, handed_over, late, stale, rc_retry
             FROM ops.repo_cursor WHERE tenant = $1 AND kind = $2 AND name = $3",
            &[&tenant, &kind, &name],
        )
        .await
        .map_err(|e| IngestError::Store(e.to_string()))?;
    let Some(row) = row else {
        return Ok(Position::default());
    };
    let rc_time: Option<i64> = row.get(0);
    let rc_continue: Option<serde_json::Value> = row.get(1);
    let rc_seen: Option<serde_json::Value> = row.get(2);
    let loaded_to: Option<i64> = row.get(3);
    let late: i64 = row.get(5);
    let stale: i64 = row.get(6);
    let rc_retry: Option<serde_json::Value> = row.get(7);
    Ok(Position {
        rc_time: rc_time.and_then(|t| u64::try_from(t).ok()),
        rc_continue: rc_continue.and_then(|v| serde_json::from_value(v).ok()),
        rc_seen: rc_seen
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        rc_retry: rc_retry
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        loaded_to: loaded_to.and_then(|t| u64::try_from(t).ok()),
        handed_over: row.get(4),
        late: u64::try_from(late).unwrap_or(0),
        stale: u64::try_from(stale).unwrap_or(0),
    })
}

/// Records where a load ended (0092 §5), for the follower to continue from. A load
/// after a follower has run resets the follower's position to the load's end.
///
/// # Errors
///
/// If the store fails.
pub async fn record_load<C: GenericClient>(
    client: &C,
    tenant: &str,
    kind: &str,
    name: &str,
    loaded_to: u64,
) -> Result<(), IngestError> {
    let t = i64::try_from(loaded_to).unwrap_or(i64::MAX);
    client
        .execute(
            "INSERT INTO ops.repo_cursor (tenant, kind, name, loaded_to, handed_over)
             VALUES ($1, $2, $3, to_timestamp($4::bigint), false)
             ON CONFLICT (tenant, kind, name) DO UPDATE
             SET loaded_to = EXCLUDED.loaded_to, handed_over = false, rc_time = NULL,
                 rc_continue = NULL, rc_seen = NULL, rc_retry = NULL, updated = now()",
            &[&tenant, &kind, &name, &t],
        )
        .await
        .map_err(|e| IngestError::Store(e.to_string()))?;
    Ok(())
}

/// A registry provider's follower over its wiki, or a tenant's entity source's (0078 §4):
/// the same poll, the source's position and records under the tenant.
pub struct ProviderFollower<'a, C: GenericClient> {
    client: &'a C,
    wiki: &'a Wiki,
    provider: &'a Provider,
    /// `""` for a registry provider; the tenant for its source.
    tenant: String,
    /// `provider` or `source`.
    kind: &'static str,
    adapter: KeyMappedAdapter,
    namespaces: EntityNamespaces,
    poller: Poller,
    now: fn() -> u64,
    late: u64,
    stale: u64,
    /// IDs whose state lagged the row that named them, read again next time.
    retry: Vec<(EntityId, Option<u64>)>,
    /// Whether the last head request had no continuation.
    at_head: bool,
}

/// Why a follower could not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    /// Neither a position nor a load to continue from.
    #[error("no position for `{0}`: sync a dump first, or start from a time with `--from`")]
    NoPosition(String),
    /// The position is older than the wiki's oldest recent change (0092 §3).
    #[error(
        "`{name}` is behind the wiki's recent changes (position {position}, oldest change {oldest}): sync a fresh dump to catch up"
    )]
    Gap {
        /// The provider.
        name: String,
        /// The position, ISO.
        position: String,
        /// The wiki's oldest recent change, ISO.
        oldest: String,
    },
    /// The provider is not a Wikibase, or its API is not registered.
    #[error("`{0}` cannot be followed: {1}")]
    NotFollowable(String, String),
    /// A request or the store failed.
    #[error("{0}")]
    Failed(String),
}

impl<'a, C: GenericClient> ProviderFollower<'a, C> {
    /// Starts a follower for `provider`: from its stored position, else from the end of
    /// its last load, else from `from` (epoch seconds).
    ///
    /// # Errors
    ///
    /// [`StartError`].
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        client: &'a C,
        wiki: &'a Wiki,
        provider: &'a Provider,
        registry: &'static scatter_providers::Registry,
        tenant: &str,
        windows: Windows,
        from: Option<u64>,
        now: fn() -> u64,
    ) -> Result<Self, StartError> {
        let fail = |e: &dyn std::fmt::Display| StartError::Failed(e.to_string());
        let kind = if provider.source {
            KIND_SOURCE
        } else {
            KIND_PROVIDER
        };
        if provider.source == tenant.is_empty() {
            return Err(StartError::NotFollowable(
                provider.slug.clone(),
                "a source is followed under its tenant, a provider under the instance".into(),
            ));
        }
        let inner = WikibaseAdapter::with_registry(&provider.slug, registry).ok_or_else(|| {
            StartError::NotFollowable(provider.slug.clone(), "not registered".into())
        })?;
        let rules = KeyRules::from_provider(provider)
            .map_err(|e| StartError::NotFollowable(provider.slug.clone(), e.to_string()))?;
        let index = restore_index(client, tenant, &provider.slug, &rules)
            .await
            .map_err(|e| fail(&e))?;
        let adapter = KeyMappedAdapter::new(inner, rules, index);

        let body = wiki
            .get(&[
                ("action", "query".into()),
                ("meta", "siteinfo".into()),
                ("siprop", "namespaces".into()),
            ])
            .await
            .map_err(|e| fail(&e))?;
        let namespaces = EntityNamespaces::from_siteinfo(&body).map_err(|e| fail(&e))?;
        if namespaces.by_ns.is_empty() {
            return Err(StartError::NotFollowable(
                provider.slug.clone(),
                "the wiki reports no entity namespaces".into(),
            ));
        }

        let stored = position(client, tenant, kind, &provider.slug)
            .await
            .map_err(|e| fail(&e))?;
        let (time, cont, seen) = match (stored.rc_time, stored.loaded_to, from) {
            (Some(t), _, _) => (t, stored.rc_continue.clone(), stored.rc_seen.clone()),
            // From a load's end, less the tail window (0092 §5): the version cursor skips
            // what the load wrote.
            (None, Some(t), _) => (t.saturating_sub(windows.tail), None, Vec::new()),
            (None, None, Some(t)) => (t, None, Vec::new()),
            (None, None, None) => return Err(StartError::NoPosition(provider.slug.clone())),
        };
        let poller = Poller::restore(
            time,
            cont,
            seen,
            namespaces.numbers(),
            wiki.list_limit(),
            windows,
        );

        // The gap check (0092 §3): the wiki's oldest recent change must be no newer than
        // the position, or changes between have been lost.
        let oldest = oldest_change(wiki, &namespaces)
            .await
            .map_err(|e| fail(&e))?;
        if let Some(oldest) = oldest
            && oldest > time
        {
            return Err(StartError::Gap {
                name: provider.slug.clone(),
                position: scatter_adapter_mediawiki::recentchanges::iso(time),
                oldest: scatter_adapter_mediawiki::recentchanges::iso(oldest),
            });
        }

        Ok(Self {
            client,
            wiki,
            provider,
            tenant: tenant.to_string(),
            kind,
            adapter,
            namespaces,
            poller,
            now,
            late: stored.late,
            stale: stored.stale,
            retry: stored
                .rc_retry
                .iter()
                .filter_map(|(id, revid)| EntityId::parse(id).ok().map(|id| (id, *revid)))
                .collect(),
            at_head: false,
        })
    }

    /// The position the poller holds.
    #[must_use]
    pub fn poller(&self) -> &Poller {
        &self.poller
    }

    /// The entity IDs a batch of changes names, with the revision each row carried.
    fn ids_of(&self, changes: &[Change]) -> Vec<(EntityId, Option<u64>)> {
        changes
            .iter()
            .filter_map(|c| entity_id_of(&c.title, c.ns, &self.namespaces).map(|id| (id, c.revid)))
            .collect()
    }

    /// Reads the current states of `ids` and turns them into sync items.
    async fn states(
        &mut self,
        ids: Vec<(EntityId, Option<u64>)>,
    ) -> Result<(Vec<SyncItem>, u64), IngestError> {
        let mut items = Vec::new();
        let mut stale = 0;
        let mut fetched = BTreeMap::new();
        for chunk in ids.chunks(self.wiki.ids_limit()) {
            let body = self
                .wiki
                .get(&[
                    ("action", "wbgetentities".into()),
                    (
                        "ids",
                        chunk
                            .iter()
                            .map(|(id, _)| id.as_str())
                            .collect::<Vec<_>>()
                            .join("|"),
                    ),
                ])
                .await
                .map_err(|e| IngestError::Store(format!("upstream: {e}")))?;
            fetched.extend(read_entities(&body).map_err(|e| IngestError::Store(e.to_string()))?);
        }
        // Observe every state first, so that a key moved between two items in one batch
        // settles before either is written.
        for f in fetched.values() {
            match f {
                Fetched::State(p) => {
                    let rules = self.adapter.rules().clone();
                    let _ = self.adapter.index_mut().observe(&p.entity, &rules);
                }
                Fetched::Redirect { from, to } => self
                    .adapter
                    .index_mut()
                    .observe_redirect(from.as_str(), to.as_str()),
                Fetched::Missing(_) => {}
            }
        }
        self.adapter.index_mut().finish();
        for (id, revid) in ids {
            let Some(f) = fetched.remove(id.as_str()) else {
                continue;
            };
            if let Fetched::State(p) = &f
                && let (Some(want), Some(got)) =
                    (revid, p.page.as_ref().and_then(|pg| pg.lastrevid))
                && got < want
            {
                // A lagging replica: the row named a newer revision than the state read
                // (0092 §2). Read it again next time.
                stale += 1;
                self.retry.push((id, revid));
                continue;
            }
            self.items_of(f, revid, &mut items);
        }
        Ok((items, stale))
    }

    /// The sync items one fetched entity becomes.
    fn items_of(&self, fetched: Fetched, revid: Option<u64>, items: &mut Vec<SyncItem>) {
        let version = || revid.map_or_else(Upstream::default, |r| Upstream::revision(r, None));
        match fetched {
            Fetched::State(p) => {
                let upstream_id = p.entity.id.as_str().to_string();
                match self.adapter.sync_item(*p) {
                    Ok(Outcome::State {
                        entity,
                        upstream,
                        upstream_id,
                        upstream_aliases,
                    }) => items.push(SyncItem::State {
                        entity,
                        upstream,
                        upstream_id,
                        upstream_aliases,
                    }),
                    Ok(Outcome::Rejected {
                        entity,
                        upstream,
                        upstream_id,
                        reason,
                    }) => {
                        items.push(SyncItem::Rejected {
                            upstream_id,
                            reason,
                        });
                        items.push(SyncItem::State {
                            entity,
                            upstream,
                            upstream_id: None,
                            upstream_aliases: Vec::new(),
                        });
                    }
                    Err(e) => items.push(SyncItem::Rejected {
                        upstream_id,
                        reason: e.to_string(),
                    }),
                }
            }
            Fetched::Redirect { from, to } => {
                match self
                    .adapter
                    .sync_redirect(from.as_str(), to.as_str(), version())
                {
                    Ok(Some((from, to, upstream))) => {
                        items.push(SyncItem::Redirected { from, to, upstream });
                    }
                    Ok(None) => {}
                    Err(e) => items.push(SyncItem::Rejected {
                        upstream_id: from.as_str().to_string(),
                        reason: e.to_string(),
                    }),
                }
            }
            Fetched::Missing(id) => match self.adapter.rewrite_id(id.as_str()) {
                Ok(local) => items.push(SyncItem::Deleted {
                    id: local,
                    upstream: version(),
                }),
                Err(e) => items.push(SyncItem::Rejected {
                    upstream_id: id.as_str().to_string(),
                    reason: e.to_string(),
                }),
            },
        }
    }
}

impl<C: GenericClient + Sync> ChangeSource for ProviderFollower<'_, C> {
    async fn next(&mut self) -> Result<ChangeBatch, IngestError> {
        let now = (self.now)();
        let upstream = |e: &dyn std::fmt::Display| IngestError::Store(format!("upstream: {e}"));
        let query = self.poller.query(now, true);
        let body = self
            .wiki
            .get(&query.params)
            .await
            .map_err(|e| upstream(&e))?;
        let head = self
            .poller
            .read(&body, false)
            .map_err(|e| IngestError::Store(e.to_string()))?;
        let mut rows = u64::try_from(head.rows).unwrap_or(u64::MAX);
        let mut ids = std::mem::take(&mut self.retry);
        ids.extend(self.ids_of(&head.changes));
        let mut late = 0;
        self.at_head = !head.more;
        if self.at_head
            && let Some(tail) = self.poller.tail_query(now, true)
        {
            let body = self
                .wiki
                .get(&tail.params)
                .await
                .map_err(|e| upstream(&e))?;
            let tail = self
                .poller
                .read(&body, true)
                .map_err(|e| IngestError::Store(e.to_string()))?;
            rows += u64::try_from(tail.rows).unwrap_or(u64::MAX);
            late = u64::try_from(tail.changes.len()).unwrap_or(u64::MAX);
            ids.extend(self.ids_of(&tail.changes));
        }
        let (items, stale) = self.states(ids).await?;
        self.late += late;
        self.stale += stale;
        Ok(ChangeBatch {
            items,
            rows,
            late,
            stale,
            caught_up: self.at_head,
        })
    }

    async fn save(&mut self) -> Result<(), IngestError> {
        let now = (self.now)();
        let time = i64::try_from(self.poller.time).unwrap_or(i64::MAX);
        let cont = self.poller.cont.as_ref().map(|c| serde_json::json!(c));
        let seen = serde_json::json!(self.poller.seen());
        let retry = serde_json::json!(
            self.retry
                .iter()
                .map(|(id, revid)| (id.as_str(), *revid))
                .collect::<Vec<_>>()
        );
        let lag = i32::try_from(now.saturating_sub(self.poller.time)).unwrap_or(i32::MAX);
        let late = i64::try_from(self.late).unwrap_or(i64::MAX);
        let stale = i64::try_from(self.stale).unwrap_or(i64::MAX);
        self.client
            .execute(
                "INSERT INTO ops.repo_cursor (tenant, kind, name, rc_time, rc_continue, rc_seen,
                                              rc_retry, handed_over, late, stale, lag_seconds)
                 VALUES ($10, $1, $2, to_timestamp($3::bigint), $4, $5, $6, true, $7, $8, $9)
                 ON CONFLICT (tenant, kind, name) DO UPDATE
                 SET rc_time = EXCLUDED.rc_time, rc_continue = EXCLUDED.rc_continue,
                     rc_seen = EXCLUDED.rc_seen, rc_retry = EXCLUDED.rc_retry, handed_over = true,
                     late = EXCLUDED.late, stale = EXCLUDED.stale,
                     lag_seconds = EXCLUDED.lag_seconds, updated = now()",
                &[
                    &self.kind,
                    &self.provider.slug,
                    &time,
                    &cont,
                    &seen,
                    &retry,
                    &late,
                    &stale,
                    &lag,
                    &self.tenant,
                ],
            )
            .await
            .map_err(|e| IngestError::Store(e.to_string()))?;
        Ok(())
    }
}

/// The key index as the mirror projected it (`view.keyed_map`), for the follower to
/// continue (0092 §2).
async fn restore_index<C: GenericClient>(
    client: &C,
    tenant: &str,
    slug: &str,
    rules: &KeyRules,
) -> Result<Index, IngestError> {
    let rows = client
        .query(
            "SELECT upstream_id, keyed_type, key, alias FROM view.keyed_map
             WHERE tenant = $1 AND provider = $2",
            &[&tenant, &slug],
        )
        .await
        .map_err(|e| IngestError::Store(e.to_string()))?;
    let mut mapped = Vec::new();
    let mut aliases = Vec::new();
    let mut key_holder: BTreeMap<String, String> = BTreeMap::new();
    for r in &rows {
        let upstream: String = r.get(0);
        let keyed_type: String = r.get(1);
        let key: String = r.get(2);
        let alias: bool = r.get(3);
        let keyed = format!("{keyed_type}:{key}");
        if alias {
            aliases.push((upstream, keyed));
        } else if let Some(rule) = rules.rule_for_type(&keyed_type) {
            key_holder.insert(keyed.clone(), upstream.clone());
            mapped.push((upstream, keyed, rule));
        }
    }
    // An alias row names the key; the index wants the item that holds it.
    let aliases = aliases
        .into_iter()
        .filter_map(|(from, keyed)| key_holder.get(&keyed).map(|to| (from, to.clone())))
        .collect::<Vec<_>>();
    Ok(Index::restore(mapped, aliases))
}

/// The wiki's oldest recent change in the entity namespaces, epoch seconds; `None` when
/// it has none.
async fn oldest_change(
    wiki: &Wiki,
    namespaces: &EntityNamespaces,
) -> Result<Option<u64>, triplespace_upstream::UpstreamError> {
    let body = wiki
        .get(&[
            ("action", "query".into()),
            ("list", "recentchanges".into()),
            ("rcdir", "newer".into()),
            ("rcprop", "timestamp".into()),
            ("rclimit", "1".into()),
            (
                "rcnamespace",
                namespaces
                    .numbers()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
        ])
        .await?;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    Ok(v.pointer("/query/recentchanges/0/timestamp")
        .and_then(|t| t.as_str())
        .and_then(scatter_adapter_mediawiki::recentchanges::parse_iso))
}
