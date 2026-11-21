//! A crawl (0092 §4; 05 §5.9): a load without a dump. It lists the wiki's entity
//! namespaces with `list=allpages` and queues every entity listed in `ops.entity_fetch`,
//! reason `crawl`, for the follower of the provider or source to fetch fifty at a time
//! (0070 A9). The time it began is the load's end, from which the follower takes over.
//!
//! A crawl is a snapshot. Its sweep queues the entities the mirror holds that were not
//! listed, rather than tombstoning them at once (0092 A16): the fetch tombstones one the
//! wiki reports missing and leaves one created after its namespace was listed. More
//! unlisted entities than the threshold, as an interrupted listing would leave, fail the
//! crawl with nothing of them queued and no end recorded, as a snapshot sync's threshold
//! fails its job (05 §3.4).
//!
//! The listed IDs are kept in a temporary table on the crawl's connection, not in memory,
//! so that a large wiki costs the crawl no more than its requests.
//!
//! implements 0092 §4 (A16)

use scatter_adapter_wikibase::follow::{EntityNamespaces, entity_id_of};
use scatter_providers::{Provider, Registry};
use scatter_wikibase_changeset::{Adapter as _, WikibaseAdapter};
use tokio_postgres::Client;
use triplespace_upstream::Wiki;

use crate::fetch::{self, Reason};
use crate::follow::{KIND_PROVIDER, KIND_SOURCE, record_load};

/// What a crawl did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Crawled {
    /// Entities listed.
    pub listed: u64,
    /// Entities the mirror holds that were not listed, queued to be checked.
    pub unlisted: u64,
    /// Rows added to the queue: an entity queued already keeps its row.
    pub queued: u64,
    /// Whether the crawl moved the load's end (0092 A4).
    pub moved: bool,
}

/// Why a crawl failed.
#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    /// The provider or source cannot be crawled.
    #[error("`{0}` cannot be crawled: {1}")]
    NotCrawlable(String, String),
    /// The wiki could not be read.
    #[error("the wiki: {0}")]
    Upstream(String),
    /// The store failed.
    #[error("the store: {0}")]
    Store(String),
    /// More entities the mirror holds went unlisted than the threshold allows.
    #[error(
        "{count} entities the mirror holds were not listed, more than the threshold of \
         {threshold}: none of them was queued and no end was recorded"
    )]
    Threshold {
        /// How many, counted to one past the threshold.
        count: usize,
        /// The threshold.
        threshold: usize,
    },
}

fn store(e: &tokio_postgres::Error) -> CrawlError {
    CrawlError::Store(e.to_string())
}

fn upstream(e: &dyn std::fmt::Display) -> CrawlError {
    CrawlError::Upstream(e.to_string())
}

/// Crawls `provider` (a registry provider when `tenant` is `""`, else the tenant's source)
/// through `wiki`, which began at `began` (epoch seconds), sweeping at most `threshold`
/// unlisted entities. `listed` is told the running count after each page of the listing.
///
/// # Errors
///
/// [`CrawlError`]; a failure leaves what was queued queued, and records no end.
#[allow(clippy::too_many_arguments)]
pub async fn crawl(
    client: &Client,
    wiki: &Wiki,
    registry: &'static Registry,
    provider: &Provider,
    tenant: &str,
    threshold: usize,
    began: u64,
    mut listed: impl FnMut(u64),
) -> Result<Crawled, CrawlError> {
    let slug = provider.slug.as_str();
    if provider.source == tenant.is_empty() {
        return Err(CrawlError::NotCrawlable(
            slug.into(),
            "a source is crawled under its tenant, a provider under the instance".into(),
        ));
    }
    if provider.api.is_none() {
        return Err(CrawlError::NotCrawlable(
            slug.into(),
            "it has no `api`".into(),
        ));
    }
    let adapter = WikibaseAdapter::with_registry(slug, registry)
        .ok_or_else(|| CrawlError::NotCrawlable(slug.into(), "not registered".into()))?;
    let body = wiki
        .get(&[
            ("action", "query".into()),
            ("meta", "siteinfo".into()),
            ("siprop", "namespaces".into()),
        ])
        .await
        .map_err(|e| upstream(&e))?;
    let namespaces = EntityNamespaces::from_siteinfo(&body).map_err(|e| upstream(&e))?;
    if namespaces.by_ns.is_empty() {
        return Err(CrawlError::NotCrawlable(
            slug.into(),
            "the wiki reports no entity namespaces".into(),
        ));
    }
    client
        .batch_execute(
            "DROP TABLE IF EXISTS pg_temp.crawl_listed;
             CREATE TEMP TABLE crawl_listed (entity_id text PRIMARY KEY)",
        )
        .await
        .map_err(|e| store(&e))?;

    let mut out = Crawled::default();
    for &ns in namespaces.by_ns.keys() {
        list_namespace(
            client,
            wiki,
            &adapter,
            &namespaces,
            ns,
            (tenant, slug),
            &mut out,
            &mut listed,
        )
        .await?;
    }
    let graph = if provider.source {
        format!("source/{slug}")
    } else {
        format!("mirror/{slug}")
    };
    let unlisted = unlisted(client, registry, tenant, &graph, threshold).await?;
    out.unlisted = unlisted.len() as u64;
    out.queued += fetch::enqueue(client, tenant, slug, &unlisted, Reason::Crawl)
        .await
        .map_err(|e| store(&e))?;
    let _ = client
        .batch_execute("DROP TABLE IF EXISTS pg_temp.crawl_listed")
        .await;

    let kind = if provider.source {
        KIND_SOURCE
    } else {
        KIND_PROVIDER
    };
    out.moved = record_load(client, tenant, kind, slug, began)
        .await
        .map_err(|e| CrawlError::Store(e.to_string()))?;
    Ok(out)
}

/// Lists namespace `ns` page by page, queuing each page's entities for `(tenant, slug)` and
/// noting their stored IDs in `crawl_listed`.
#[allow(clippy::too_many_arguments)]
async fn list_namespace(
    client: &Client,
    wiki: &Wiki,
    adapter: &WikibaseAdapter,
    namespaces: &EntityNamespaces,
    ns: i64,
    (tenant, slug): (&str, &str),
    out: &mut Crawled,
    listed: &mut impl FnMut(u64),
) -> Result<(), CrawlError> {
    let mut cont: Vec<(String, String)> = Vec::new();
    loop {
        let mut params: Vec<(&str, String)> = vec![
            ("action", "query".into()),
            ("list", "allpages".into()),
            ("apnamespace", ns.to_string()),
            ("aplimit", wiki.list_limit().to_string()),
        ];
        for (k, v) in &cont {
            params.push((k.as_str(), v.clone()));
        }
        let body = wiki.get(&params).await.map_err(|e| upstream(&e))?;
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| upstream(&e))?;
        let ids: Vec<String> = v
            .pointer("/query/allpages")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let title = p.get("title")?.as_str()?;
                entity_id_of(title, ns, namespaces).map(|id| id.as_str().to_string())
            })
            .collect();
        let stored: Vec<String> = ids
            .iter()
            .filter_map(|id| adapter.rewrite_id(id).ok())
            .map(|id| id.as_str().to_string())
            .collect();
        client
            .execute(
                "INSERT INTO pg_temp.crawl_listed SELECT unnest($1::text[])
                 ON CONFLICT DO NOTHING",
                &[&stored],
            )
            .await
            .map_err(|e| store(&e))?;
        out.queued += fetch::enqueue(client, tenant, slug, &ids, Reason::Crawl)
            .await
            .map_err(|e| store(&e))?;
        out.listed += ids.len() as u64;
        listed(out.listed);
        match v.get("continue").and_then(serde_json::Value::as_object) {
            Some(c) => {
                cont = c
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                    .collect();
            }
            None => return Ok(()),
        }
    }
}

/// The sweep (0092 A16): the upstream IDs of what `graph` holds and the listing did not
/// name, failing when there are more than `threshold`. A keyed entity has no upstream ID
/// to ask for, and is left to the sync of a dump.
async fn unlisted(
    client: &Client,
    registry: &Registry,
    tenant: &str,
    graph: &str,
    threshold: usize,
) -> Result<Vec<String>, CrawlError> {
    let cap = i64::try_from(threshold).unwrap_or(i64::MAX - 1) + 1;
    let rows = client
        .query(
            "SELECT s.entity_id FROM view.entity_source s
             WHERE s.tenant = $1 AND s.graph = $2
               AND NOT EXISTS (SELECT 1 FROM pg_temp.crawl_listed l
                               WHERE l.entity_id = s.entity_id)
             ORDER BY s.entity_id LIMIT $3",
            &[&tenant, &graph, &cap],
        )
        .await
        .map_err(|e| store(&e))?;
    if rows.len() > threshold {
        return Err(CrawlError::Threshold {
            count: rows.len(),
            threshold,
        });
    }
    Ok(rows
        .iter()
        .filter_map(|r| {
            registry
                .parse_foreign_id(r.get::<_, &str>(0))
                .ok()
                .map(|f| f.upstream_id())
        })
        .collect())
}
