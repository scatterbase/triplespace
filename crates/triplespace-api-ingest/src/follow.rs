//! A registry provider's follower (0092 §1–§3, §5): the [`ChangeSource`] the ingester's
//! follow loop reads, over a Wikibase's `list=recentchanges` and `wbgetentities`.
//!
//! Each `next` is one poll: the head request (and, once caught up, the tail pass), the
//! changed titles turned into upstream IDs, their current states read in one request per
//! fifty (five hundred with the higher limits), and each state through the provider's
//! key-mapped adapter into the sync items the mirror writes. Its [`Saver`] stores the
//! position, and the follower's jobs (0092 A6, A8), in `ops.repo_cursor`, in the units of
//! work that commit the batch, so that they commit together (A10).
//!
//! The follower continues a load (0092 §5): with no position of its own it starts at
//! `loaded_to`, the end of the last dump sync, and it refuses to start from a time the
//! wiki no longer keeps recent changes for (a gap), since a load is the only way across one.

use std::collections::BTreeMap;

use scatter_adapter_mediawiki::{Change, Poller, Windows};
use scatter_adapter_wikibase::follow::{EntityNamespaces, Fetched, entity_id_of, read_entities};
use scatter_adapter_wikibase::{Index, KeyMappedAdapter, KeyRules, Outcome};
use scatter_ingest::IngestError;
use scatter_ingest::follow::{ChangeBatch, ChangeSource, FollowJob, FollowState, SavePosition};
use scatter_ingest::sync::SyncItem;
use scatter_providers::Provider;
use scatter_wikibase_changeset::{Adapter, Graph, Upstream, WikibaseAdapter};
use scatter_wikibase_model::id::EntityId;
use tokio_postgres::GenericClient;
use triplespace_projections::PgCx;
use triplespace_upstream::Wiki;

use crate::PgIngest;

/// The position row's kind for a registry provider.
pub const KIND_PROVIDER: &str = "provider";
/// The position row's kind for a tenant's entity source.
pub const KIND_SOURCE: &str = "source";

/// Where a follower is, as `ops.repo_cursor` holds it.
#[derive(Debug, Clone, PartialEq, Default)]
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
    /// The follower's jobs: the day's open job (0092 A6), and a closed day's still
    /// compacting (A8).
    pub state: FollowState,
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
                    EXTRACT(EPOCH FROM loaded_to)::bigint, handed_over, late, stale, rc_retry,
                    job
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
    let job: Option<serde_json::Value> = row.get(8);
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
        state: job.map(state_of).unwrap_or_default(),
    })
}

/// The follower's jobs as `ops.repo_cursor.job` holds them: a [`FollowState`], or, as a
/// position saved before A8 left it, the open job alone.
fn state_of(v: serde_json::Value) -> FollowState {
    serde_json::from_value(v.clone()).unwrap_or_else(|_| FollowState {
        open: serde_json::from_value::<FollowJob>(v).ok(),
        closing: None,
    })
}

/// Records where a load ended (0092 §5), for the follower to continue from. A load
/// after a follower has run resets the follower's position to the load's end. A load
/// that ends no later than the end already recorded changes nothing (0092 A4): a sync of
/// a few entities, or of an older dump, must not move the follower back. Returns whether
/// it moved the end.
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
) -> Result<bool, IngestError> {
    let t = i64::try_from(loaded_to).unwrap_or(i64::MAX);
    let moved = client
        .execute(
            "INSERT INTO ops.repo_cursor (tenant, kind, name, loaded_to, handed_over)
             VALUES ($1, $2, $3, to_timestamp($4::bigint), false)
             ON CONFLICT (tenant, kind, name) DO UPDATE
             SET loaded_to = EXCLUDED.loaded_to, handed_over = false, rc_time = NULL,
                 rc_continue = NULL, rc_seen = NULL, rc_retry = NULL, updated = now()
             WHERE ops.repo_cursor.loaded_to IS NULL
                OR EXCLUDED.loaded_to > ops.repo_cursor.loaded_to",
            &[&tenant, &kind, &name, &t],
        )
        .await
        .map_err(|e| IngestError::Store(e.to_string()))?;
    Ok(moved > 0)
}

/// How long a follower waits before polling again once caught up, and where it says so.
type Wait = (std::time::Duration, fn(&str));

/// A registry provider's follower over its wiki, or a tenant's entity source's (0078 §4):
/// the same poll, the source's position and records under the tenant.
pub struct ProviderFollower<'a> {
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
    /// The jobs, as the row held them when the follower started (0092 A6, A8).
    state: FollowState,
    /// IDs whose state lagged the row that named them, read again next time.
    retry: Vec<(EntityId, Option<u64>)>,
    /// Whether the last head request had no continuation.
    at_head: bool,
    /// How long to wait before polling again once caught up, and where to say so; `None`
    /// polls again at once, for a run that stops when caught up (0092 A9).
    wait: Option<Wait>,
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

impl<'a> ProviderFollower<'a> {
    /// Starts a follower for `provider`: from its stored position, else from the end of
    /// its last load, else from the newest revision its graph holds (a lost position,
    /// 0092 A11), else from `from` (epoch seconds).
    ///
    /// # Errors
    ///
    /// [`StartError`].
    #[allow(clippy::too_many_arguments)]
    pub async fn start<C: GenericClient>(
        client: &C,
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
        // A position and a load's end both lost, with the graph written: the position is
        // rebuilt from the newest revision the graph holds (0092 A11).
        let recovered = if stored.rc_time.is_none() && stored.loaded_to.is_none() {
            recovered(client, wiki, tenant, provider).await?
        } else {
            None
        };
        let (time, cont, seen) = match (stored.rc_time, stored.loaded_to.or(recovered), from) {
            (Some(t), _, _) => (t, stored.rc_continue.clone(), stored.rc_seen.clone()),
            // From a load's end, or the newest revision the graph holds, less the tail
            // window (0092 §5, A11): the version cursor skips what was written.
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
            state: stored.state.clone(),
            retry: stored
                .rc_retry
                .iter()
                .filter_map(|(id, revid)| EntityId::parse(id).ok().map(|id| (id, *revid)))
                .collect(),
            at_head: false,
            wait: None,
        })
    }

    /// The position the poller holds.
    #[must_use]
    pub fn poller(&self) -> &Poller {
        &self.poller
    }

    /// The entity IDs a batch of changes names, with the revision and page ID each row
    /// carried.
    fn ids_of(&self, changes: &[Change]) -> Vec<Named> {
        changes
            .iter()
            .filter_map(|c| {
                entity_id_of(&c.title, c.ns, &self.namespaces).map(|id| (id, c.revid, c.pageid))
            })
            .collect()
    }

    /// Reads the current states of `ids` and turns them into sync items.
    async fn states(&mut self, ids: Vec<Named>) -> Result<(Vec<SyncItem>, u64), IngestError> {
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
                            .map(|(id, _, _)| id.as_str())
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
                Fetched::Missing(_) | Fetched::Unreadable { .. } => {}
            }
        }
        self.adapter.index_mut().finish();
        for (id, revid, pageid) in ids {
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
            self.items_of(f, revid, pageid, &mut items);
        }
        Ok((items, stale))
    }

    /// The sync items one fetched entity becomes.
    /// A deletion or a redirect carries the page ID its change row named, so that its
    /// record's page ID comes from its content, as a state's does (0015 A65).
    fn items_of(
        &self,
        fetched: Fetched,
        revid: Option<u64>,
        pageid: Option<u64>,
        items: &mut Vec<SyncItem>,
    ) {
        let version = || {
            revid
                .map_or_else(Upstream::default, |r| Upstream::revision(r, None))
                .with_pageid(pageid)
        };
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
            Fetched::Unreadable { id, reason } => items.push(SyncItem::Rejected {
                upstream_id: id.as_str().to_string(),
                reason,
            }),
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

/// An entity a change row named, the revision it named, and its page ID.
type Named = (EntityId, Option<u64>, Option<u64>);

/// Where a follower stands after a batch, as its [`Saver`] stores it.
#[derive(Debug, Clone)]
pub struct Saved {
    time: u64,
    cont: Option<serde_json::Value>,
    seen: serde_json::Value,
    retry: serde_json::Value,
    late: u64,
    stale: u64,
}

impl ProviderFollower<'_> {
    /// Waits `interval` before each poll that follows one that caught up, telling `report`
    /// where the follower stands as it starts to wait: for a follower that runs until
    /// stopped, so that a closed day's compaction runs beside the wait (0092 A9).
    #[must_use]
    pub fn waiting(mut self, interval: std::time::Duration, report: fn(&str)) -> Self {
        self.wait = Some((interval, report));
        self
    }

    /// The position as it stands now.
    fn saved(&self) -> Saved {
        Saved {
            time: self.poller.time,
            cont: self.poller.cont.as_ref().map(|c| serde_json::json!(c)),
            seen: serde_json::json!(self.poller.seen()),
            retry: serde_json::json!(
                self.retry
                    .iter()
                    .map(|(id, revid)| (id.as_str(), *revid))
                    .collect::<Vec<_>>()
            ),
            late: self.late,
            stale: self.stale,
        }
    }
}

impl ChangeSource<PgIngest> for ProviderFollower<'_> {
    type Position = Saved;
    type Saver = Saver;

    async fn next(&mut self) -> Result<(ChangeBatch, Saved), IngestError> {
        if self.at_head
            && let Some((interval, report)) = self.wait
        {
            report(&format!(
                "caught up at {}; polling again in {}s",
                scatter_adapter_mediawiki::recentchanges::iso(self.poller.time),
                interval.as_secs()
            ));
            tokio::time::sleep(interval).await;
        }
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
        // A retried ID is read again without the page ID its row named: a deletion found
        // then takes the key's page ID, as before 0015 A65.
        let mut ids: Vec<Named> = std::mem::take(&mut self.retry)
            .into_iter()
            .map(|(id, revid)| (id, revid, None))
            .collect();
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
        let batch = ChangeBatch {
            items,
            rows,
            late,
            stale,
            caught_up: self.at_head,
        };
        Ok((batch, self.saved()))
    }

    async fn state(&mut self) -> Result<FollowState, IngestError> {
        Ok(self.state.clone())
    }

    fn saver(&self) -> Saver {
        Saver {
            tenant: self.tenant.clone(),
            kind: self.kind,
            name: self.provider.slug.clone(),
            now: self.now,
        }
    }
}

/// Saves a follower's row in `ops.repo_cursor` in the ingest's units of work, so that it
/// commits with the records and job records it describes (0092 A10).
#[derive(Debug, Clone)]
pub struct Saver {
    tenant: String,
    kind: &'static str,
    name: String,
    now: fn() -> u64,
}

impl Saver {
    /// The saver of the follower `kind` (`provider` or `source`) of `name` for `tenant`
    /// (`""` for a registry provider), with `now` for the lag it records.
    #[must_use]
    pub fn new(tenant: &str, kind: &'static str, name: &str, now: fn() -> u64) -> Self {
        Self {
            tenant: tenant.to_string(),
            kind,
            name: name.to_string(),
            now,
        }
    }
}

impl Saved {
    /// A position at `time` (epoch seconds), with nothing seen, nothing to retry and no
    /// continuation.
    #[must_use]
    pub fn at(time: u64) -> Self {
        Self {
            time,
            cont: None,
            seen: serde_json::json!([]),
            retry: serde_json::json!([]),
            late: 0,
            stale: 0,
        }
    }
}

impl SavePosition<PgIngest> for Saver {
    type Position = Saved;

    async fn save(
        &self,
        cx: &mut PgCx,
        at: Option<&Saved>,
        jobs: &FollowState,
    ) -> Result<(), IngestError> {
        let job_value = Some(jobs)
            .filter(|s| !s.is_empty())
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| IngestError::Store(e.to_string()))?;
        let store = |e: tokio_postgres::Error| IngestError::Store(e.to_string());
        let Some(at) = at else {
            // The jobs alone: the row is there, since a follower starts from one.
            cx.conn()
                .execute(
                    "UPDATE ops.repo_cursor SET job = $4, updated = now()
                     WHERE tenant = $1 AND kind = $2 AND name = $3",
                    &[&self.tenant, &self.kind, &self.name, &job_value],
                )
                .await
                .map_err(store)?;
            return Ok(());
        };
        let now = (self.now)();
        let time = i64::try_from(at.time).unwrap_or(i64::MAX);
        let (cont, seen, retry) = (&at.cont, &at.seen, &at.retry);
        let lag = i32::try_from(now.saturating_sub(at.time)).unwrap_or(i32::MAX);
        let late = i64::try_from(at.late).unwrap_or(i64::MAX);
        let stale = i64::try_from(at.stale).unwrap_or(i64::MAX);
        cx.conn()
            .execute(
                "INSERT INTO ops.repo_cursor (tenant, kind, name, rc_time, rc_continue, rc_seen,
                                              rc_retry, handed_over, late, stale, lag_seconds,
                                              job)
                 VALUES ($10, $1, $2, to_timestamp($3::bigint), $4, $5, $6, true, $7, $8, $9,
                         $11)
                 ON CONFLICT (tenant, kind, name) DO UPDATE
                 SET rc_time = EXCLUDED.rc_time, rc_continue = EXCLUDED.rc_continue,
                     rc_seen = EXCLUDED.rc_seen, rc_retry = EXCLUDED.rc_retry, handed_over = true,
                     late = EXCLUDED.late, stale = EXCLUDED.stale,
                     lag_seconds = EXCLUDED.lag_seconds, job = EXCLUDED.job, updated = now()",
                &[
                    &self.kind,
                    &self.name,
                    &time,
                    cont,
                    seen,
                    retry,
                    &late,
                    &stale,
                    &lag,
                    &self.tenant,
                    &job_value,
                ],
            )
            .await
            .map_err(store)?;
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
/// Where a follower whose position and load's end are both lost starts, before the tail
/// window: when the wiki saved the newest revision its graph holds (0092 A11); `None`
/// when the graph holds none.
async fn recovered<C: GenericClient>(
    client: &C,
    wiki: &Wiki,
    tenant: &str,
    provider: &Provider,
) -> Result<Option<u64>, StartError> {
    let graph = if tenant.is_empty() {
        Graph::Mirror(provider.slug.clone())
    } else {
        Graph::Source(provider.slug.clone())
    };
    let Some(revid) = newest_revision(client, tenant, &graph.name())
        .await
        .map_err(|e| StartError::Failed(e.to_string()))?
    else {
        return Ok(None);
    };
    revision_time(wiki, revid).await?.map(Some).ok_or_else(|| {
        StartError::Failed(format!(
            "the position of `{}` is lost, and the wiki has no time for revision {revid}, the newest it holds: start from a time with `--from`",
            provider.slug
        ))
    })
}

/// The newest upstream revision `graph` holds for `tenant`, by `view.entity_source`'s
/// version cursor (0092 A11); `None` when it holds none. It reads every row of the graph:
/// it runs only when a follower's position is lost.
///
/// # Errors
///
/// The database's.
pub async fn newest_revision<C: GenericClient>(
    client: &C,
    tenant: &str,
    graph: &str,
) -> Result<Option<u64>, tokio_postgres::Error> {
    let row = client
        .query_one(
            "SELECT max(upstream_version::bigint) FROM view.entity_source
             WHERE tenant = $1 AND graph = $2 AND upstream_version ~ '^[0-9]{1,18}$'",
            &[&tenant, &graph],
        )
        .await?;
    Ok(row
        .get::<_, Option<i64>>(0)
        .and_then(|r| u64::try_from(r).ok()))
}

/// When the wiki saved revision `revid` (epoch seconds); `None` when it has no such
/// revision, as when it was deleted.
async fn revision_time(wiki: &Wiki, revid: u64) -> Result<Option<u64>, StartError> {
    let body = wiki
        .get(&[
            ("action", "query".into()),
            ("prop", "revisions".into()),
            ("revids", revid.to_string()),
            ("rvprop", "timestamp".into()),
        ])
        .await
        .map_err(|e| StartError::Failed(e.to_string()))?;
    Ok(revision_time_of(&body))
}

/// The timestamp of the one revision a `prop=revisions` answer names, in either format.
fn revision_time_of(body: &str) -> Option<u64> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let pages = v.pointer("/query/pages")?;
    let pages: Vec<&serde_json::Value> = match pages {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Object(o) => o.values().collect(),
        _ => return None,
    };
    pages.iter().find_map(|p| {
        p.pointer("/revisions/0/timestamp")
            .and_then(serde_json::Value::as_str)
            .and_then(scatter_adapter_mediawiki::recentchanges::parse_iso)
    })
}

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

#[cfg(test)]
mod tests {
    use super::revision_time_of;

    #[test]
    fn a_revisions_answer_gives_its_time_in_either_format() {
        let v2 = r#"{"batchcomplete":true,"query":{"pages":[{"pageid":7,"ns":0,"title":"Q1","revisions":[{"revid":2572371073,"parentid":1,"timestamp":"2026-11-20T00:00:14Z"}]}]}}"#;
        assert_eq!(revision_time_of(v2), Some(1_795_132_814));
        let v1 = r#"{"query":{"pages":{"7":{"pageid":7,"revisions":[{"timestamp":"2026-11-20T00:00:14Z"}]}}}}"#;
        assert_eq!(revision_time_of(v1), Some(1_795_132_814));
        let gone =
            r#"{"batchcomplete":true,"query":{"badrevids":{"9":{"revid":9,"missing":true}}}}"#;
        assert_eq!(revision_time_of(gone), None);
    }
}
