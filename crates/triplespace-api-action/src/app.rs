//! The shared state: the pool, the write store and its pipeline, the token secret, the
//! farm, and the deployment settings of 0056 §10.

use std::sync::Arc;

use deadpool_postgres::Pool;
use scatter_projection::Pipeline;
use scatter_providers::Registry;
use triplespace_accounts::Secret;
use triplespace_accounts::oauth::{HttpUpstream, Upstream};
use triplespace_api_ingest::PgIngest;
use triplespace_projections::{EntityProjection, Farm, PgBackend, milestone_pipeline};

use crate::forwarded::{KeyCache, TrustList};

/// `server.mode` (0056 §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Refuses insecure settings; serves only registered hosts.
    Production,
    /// Starts anyway, marks `siprop=triplespace` with `insecure: true`, and may serve an
    /// unregistered host as the fallback tenant.
    Development,
}

impl Mode {
    /// Parses `production` or `development`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "production" => Some(Self::Production),
            "development" => Some(Self::Development),
            _ => None,
        }
    }
}

/// What the server is configured with.
#[derive(Debug, Clone)]
pub struct Config {
    /// The mode.
    pub mode: Mode,
    /// Proxies whose `X-Forwarded-*` headers are trusted, as the operator wrote them:
    /// addresses and CIDR ranges (0057 §10).
    pub trusted_proxies: Vec<String>,
    /// In development mode, the tenant an unregistered host is served as.
    pub dev_tenant: Option<String>,
    /// The farm.
    pub farm: Farm,
    /// The auth host, `server.auth_base` (0089 §1): required once there is a second
    /// tenant, and never a tenant base or the farm base.
    pub auth_base: Option<String>,
    /// `MediaWiki`-compatible `generator` string.
    pub generator: String,
    /// The binary's `version.json` (0077 §15), which its build script writes; `None` in
    /// tests, which then report a stub build.
    pub version: Option<&'static str>,
}

/// The state behind every handler.
#[derive(Clone)]
pub struct App {
    inner: Arc<Inner>,
}

struct Inner {
    pool: Pool,
    store: PgIngest,
    pipeline: Pipeline<PgIngest>,
    order: EntityProjection,
    secret: Secret,
    config: Config,
    trust: TrustList,
    forwarder_keys: KeyCache,
    nothing_hidden: crate::pages::NothingHidden,
    build: crate::version::BuildInfo,
    probes: crate::version::Probes,
    upstream: Arc<dyn Upstream>,
    /// Each tenant's table of names (0078 §3), leaked for the adapters' `'static`
    /// borrows and rebuilt when the tenant's `entity-source` records move.
    registries: std::sync::Mutex<std::collections::HashMap<String, (i64, &'static Registry)>>,
}

impl App {
    /// Builds the state over a pool. An entry of `trusted_proxies` that is not an
    /// address or a CIDR range is an error.
    pub fn new(pool: Pool, secret: Secret, config: Config) -> Result<Self, String> {
        let agent = format!(
            "Triplespace/{} (+{})",
            env!("CARGO_PKG_VERSION"),
            config.farm.base.trim_end_matches('/')
        );
        let upstream = HttpUpstream::new(&agent).map_err(|e| e.to_string())?;
        Self::with_upstream(pool, secret, config, Arc::new(upstream))
    }

    /// Builds the state with what flows ask of the world outside (0088 §2, §8): the
    /// providers and source wikis. [`Self::new`] reaches them over HTTP; tests put a
    /// fake here.
    pub fn with_upstream(
        pool: Pool,
        secret: Secret,
        config: Config,
        upstream: Arc<dyn Upstream>,
    ) -> Result<Self, String> {
        let trust = TrustList::parse(&config.trusted_proxies)
            .map_err(|e| format!("trusted proxies: {e}"))?;
        let store = PgIngest::new(PgBackend::new(pool.clone()));
        let registry = Registry::default_registry();
        let pipeline = milestone_pipeline(
            scatter_actors::issuer::IssuerRegistry::default_registry().clone(),
            config.farm.clone(),
        )
        .and_then(triplespace_notify::with_addressing)
        .map_err(|e| e.to_string())?;
        // Mirror order: every registered provider, registry order (the tenant's opt-in
        // order is a later refinement, 0018 §6).
        let slugs: Vec<&str> = registry
            .providers()
            .iter()
            .map(|p| p.slug.as_str())
            .collect();
        let order = EntityProjection::new(&slugs, registry);
        let build = crate::version::BuildInfo::parse(config.version);
        Ok(Self {
            inner: Arc::new(Inner {
                pool,
                store,
                pipeline,
                order,
                secret,
                config,
                trust,
                forwarder_keys: KeyCache::default(),
                nothing_hidden: crate::pages::NothingHidden::default(),
                build,
                probes: crate::version::Probes::default(),
                upstream,
                registries: std::sync::Mutex::new(std::collections::HashMap::new()),
            }),
        })
    }

    /// The pool.
    #[must_use]
    pub fn pool(&self) -> &Pool {
        &self.inner.pool
    }

    /// The write store.
    #[must_use]
    pub fn store(&self) -> &PgIngest {
        &self.inner.store
    }

    /// The pipeline.
    #[must_use]
    pub fn pipeline(&self) -> &Pipeline<PgIngest> {
        &self.inner.pipeline
    }

    /// Brings the asynchronous projection steps up to date (0013 §7: what the write path
    /// does not apply inline), which today is addressing ([`triplespace_notify`]). Addressing
    /// reads membership and block records, in `actors` partitions; thread records, in
    /// `pages`; and the ACL records that hide a post or delete a thread, in a tenant's
    /// `log`. Only those partitions are replayed; every other partition's addressing
    /// position is moved to its head in one statement. Cheap when nothing is behind: one
    /// query. Returns the records read.
    ///
    /// # Errors
    ///
    /// The store's or a projection's error.
    pub async fn catch_up_async(&self) -> Result<u64, String> {
        let mut client = self.pool().get().await.map_err(|e| e.to_string())?;
        let behind: Vec<(i64, String)> = client
            .query(
                "SELECT p.partition, p.name FROM log.partition p
                 LEFT JOIN ops.projection_state s
                   ON s.projection = 'addressing' AND s.partition = p.partition
                 WHERE p.next_offset > coalesce(s.applied_offset, 0)",
                &[],
            )
            .await
            .map_err(|e| e.to_string())?
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect();
        if behind.is_empty() {
            return Ok(0);
        }
        let addressed = |name: &str| matches!(name, "actors" | "pages" | "log");
        let skipped: Vec<i64> = behind
            .iter()
            .filter(|(_, name)| !addressed(name))
            .map(|(p, _)| *p)
            .collect();
        if !skipped.is_empty() {
            client
                .execute(
                    "INSERT INTO ops.projection_state (projection, partition, applied_offset)
                     SELECT 'addressing', p.partition, p.next_offset FROM log.partition p
                     WHERE p.partition = ANY($1)
                     ON CONFLICT (projection, partition)
                     DO UPDATE SET applied_offset = EXCLUDED.applied_offset, updated_at = now()",
                    &[&skipped],
                )
                .await
                .map_err(|e| e.to_string())?;
        }
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let log = scatter_log_postgres::PgLog::new(&*tx);
        let mut read = 0;
        for (p, name) in &behind {
            if addressed(name) {
                read += self
                    .pipeline()
                    .catch_up(
                        self.store(),
                        &log,
                        scatter_log_postgres::ids::partition_from_db(*p),
                        500,
                    )
                    .await
                    .map_err(|e| e.to_string())?
                    .records;
            }
        }
        tx.rollback().await.map_err(|e| e.to_string())?;
        Ok(read)
    }

    /// Applies queued report work for every tenant (0047 §4.3): the `report` projection's
    /// worker, which keeps projection-backed reports in step with the page and ACL
    /// projections, in no write's transaction. Returns how many items it applied.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn drain_reports(&self, limit: usize) -> Result<usize, String> {
        triplespace_projections::reports::drain(self.pool(), None, limit).await
    }

    /// The mirror order for resolution.
    #[must_use]
    pub fn order(&self) -> &EntityProjection {
        &self.inner.order
    }

    /// The token secret.
    #[must_use]
    pub fn secret(&self) -> &Secret {
        &self.inner.secret
    }

    /// The settings.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// The provider registry.
    #[must_use]
    pub fn registry(&self) -> &'static Registry {
        Registry::default_registry()
    }

    /// A tenant's table of names (0078 §3): the registry with the tenant's entity
    /// sources. The registry itself for a tenant that declares none. One indexed query
    /// per call tells whether the cached table is current.
    ///
    /// # Errors
    ///
    /// If the store fails, or a declared source clashes with the registry.
    pub async fn registry_for<C: scatter_log_postgres::PgClient>(
        &self,
        client: &C,
        tenant: &str,
    ) -> Result<&'static Registry, String> {
        let frontier: Option<i64> = client
            .query_one(
                "SELECT max(\"offset\") FROM view.registry WHERE tenant = $1 AND kind = $2",
                &[&tenant, &triplespace_projections::entity_sources::KIND],
            )
            .await
            .map_err(|e| e.to_string())?
            .get(0);
        let Some(frontier) = frontier else {
            return Ok(Registry::default_registry());
        };
        if let Ok(cache) = self.inner.registries.lock()
            && let Some((at, r)) = cache.get(tenant)
            && *at == frontier
        {
            return Ok(r);
        }
        let built = triplespace_projections::entity_sources::registry_for(client, tenant).await?;
        let leaked: &'static Registry = Box::leak(Box::new(built));
        if let Ok(mut cache) = self.inner.registries.lock() {
            cache.insert(tenant.to_string(), (frontier, leaked));
        }
        Ok(leaked)
    }

    /// The proxies trusted by address (0057 §10).
    #[must_use]
    pub fn trust(&self) -> &TrustList {
        &self.inner.trust
    }

    /// The cache of forwarder key checks.
    #[must_use]
    pub fn forwarder_keys(&self) -> &KeyCache {
        &self.inner.forwarder_keys
    }

    /// The hidden-set L0 cache ([`crate::pages::hidden`]).
    #[must_use]
    pub fn nothing_hidden(&self) -> &crate::pages::NothingHidden {
        &self.inner.nothing_hidden
    }

    /// This binary's build and components (0077 §15).
    #[must_use]
    pub fn build(&self) -> &crate::version::BuildInfo {
        &self.inner.build
    }

    /// What login flows ask of providers and source wikis (0088 §2, §8).
    #[must_use]
    pub fn upstream(&self) -> &dyn Upstream {
        self.inner.upstream.as_ref()
    }

    /// The services' last known states (0077 §4).
    #[must_use]
    pub fn probes(&self) -> &crate::version::Probes {
        &self.inner.probes
    }
}
