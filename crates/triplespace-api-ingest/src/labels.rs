//! The label cache (0070 §4, A13; 05 §5.6): labels of foreign and source entities the
//! instance does not hold, read from the provider's or source's wiki with
//! `wbgetentities&props=labels` for display, and kept for `entities.label_ttl` (default
//! seven days). A label from it is never a term: it is not written to `view.term`, to
//! search or to RDF.
//!
//! The cache is in the process, keyed by the entity as stored and the language chain, and
//! by tenant for a source's entities, whose names are the tenant's (05 §6.4). An entity
//! the wiki reports missing is kept as having no labels, so that it is not asked for again
//! until the entry expires. A request is bounded: at most [`MAX_FETCH`] entities are asked
//! upstream per call, each request within [`TIMEOUT`], and what a failed or slow request
//! leaves out is shown as its bare ID and asked again next time. The requests carry no
//! `maxlag`: a reader is waiting for them (0070 A13).
//!
//! implements 0070 §4 (A13)

use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use scatter_providers::Registry;
use scatter_wikibase_model::id::EntityId;
use tokio_postgres::GenericClient;
use triplespace_upstream::Wiki;

/// The most entities one call asks upstream for.
pub const MAX_FETCH: usize = 100;

/// How long one upstream request may take before its entities are left bare.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// `entities.label_ttl`'s default (0070 §4).
pub const LABEL_TTL: Duration = Duration::from_hours(7 * 24);

/// Entries kept before the expired ones are swept out.
const SWEEP_AT: usize = 50_000;

/// (tenant for a source's entity, else `""`; the stored ID; the chain, `|`-joined).
type Key = (String, String, String);

/// Labels by language, and when they were read.
type Entry = (Instant, BTreeMap<String, String>);

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    /// One client per API endpoint, so that requests share connections.
    wikis: HashMap<String, Wiki>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

fn cache() -> std::sync::MutexGuard<'static, Cache> {
    CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `entities.label_ttl` for `tenant`, else the instance's, else seven days.
async fn ttl<C: GenericClient>(client: &C, tenant: &str) -> Duration {
    crate::shallow::site_value(client, tenant, "entities.label_ttl")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_string))
        .and_then(|s| triplespace_accounts::settings::value::duration_seconds(&s))
        .map_or(LABEL_TTL, Duration::from_secs_f64)
}

/// The labels in `chain` of `ids` (stored IDs, `WDQ42` or `lb:Q42`) that belong to a
/// provider or source with an `api`, from the cache or, for at most [`MAX_FETCH`] of
/// them, from upstream; `agent` is the User-Agent upstream sees. An ID left out has no
/// label to show, for now.
pub async fn labels<C: GenericClient>(
    client: &C,
    registry: &Registry,
    tenant: &str,
    ids: &[EntityId],
    chain: &[String],
    agent: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let joined = chain.join("|");
    let ttl = ttl(client, tenant).await;
    let mut out = BTreeMap::new();
    // Per API endpoint: (stored ID, upstream ID, key) to ask for.
    let mut wanted: BTreeMap<String, Vec<(String, String, Key)>> = BTreeMap::new();
    let mut asking = 0;
    {
        let c = cache();
        for id in ids {
            let Ok(f) = registry.parse_foreign_id(id.as_str()) else {
                continue;
            };
            let p = f.provider();
            let Some(api) = p.api.as_ref().filter(|_| !p.retired) else {
                continue;
            };
            let scope = if p.source { tenant } else { "" };
            let key: Key = (scope.to_string(), id.as_str().to_string(), joined.clone());
            match c.entries.get(&key) {
                Some((at, labels)) if at.elapsed() < ttl => {
                    out.insert(id.as_str().to_string(), labels.clone());
                }
                _ if asking < MAX_FETCH => {
                    asking += 1;
                    wanted.entry(api.clone()).or_default().push((
                        id.as_str().to_string(),
                        f.upstream_id(),
                        key,
                    ));
                }
                _ => {}
            }
        }
    }
    for (api, entities) in wanted {
        let wiki = {
            let mut c = cache();
            match c.wikis.get(&api) {
                Some(w) => w.clone(),
                None => match Wiki::new(&api, agent) {
                    Ok(w) => {
                        c.wikis.insert(api.clone(), w.clone());
                        w
                    }
                    Err(_) => continue,
                },
            }
        };
        for chunk in entities.chunks(wiki.ids_limit()) {
            let params = [
                ("action", "wbgetentities".into()),
                ("props", "labels".into()),
                ("languages", joined.clone()),
                (
                    "ids",
                    chunk
                        .iter()
                        .map(|(_, u, _)| u.as_str())
                        .collect::<Vec<_>>()
                        .join("|"),
                ),
            ];
            let Ok(Ok(body)) = tokio::time::timeout(TIMEOUT, wiki.get_interactive(&params)).await
            else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
                continue;
            };
            let mut c = cache();
            if c.entries.len() >= SWEEP_AT {
                c.entries.retain(|_, (at, _)| at.elapsed() < ttl);
                if c.entries.len() >= SWEEP_AT {
                    c.entries.clear();
                }
            }
            for (stored, upstream, key) in chunk {
                let Some(e) = v.pointer(&format!("/entities/{upstream}")) else {
                    continue;
                };
                let labels: BTreeMap<String, String> = e
                    .get("labels")
                    .and_then(serde_json::Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter_map(|(lang, l)| {
                        l.get("value")
                            .and_then(serde_json::Value::as_str)
                            .map(|s| (lang.clone(), s.to_string()))
                    })
                    .collect();
                c.entries
                    .insert(key.clone(), (Instant::now(), labels.clone()));
                out.insert(stored.clone(), labels);
            }
        }
    }
    out
}
