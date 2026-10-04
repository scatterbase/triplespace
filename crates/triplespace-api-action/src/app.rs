//! The shared state: the pool, the write store and its pipeline, the token secret, the
//! farm, and the deployment settings of 0056 §10.

use std::collections::BTreeSet;
use std::sync::Arc;

use deadpool_postgres::Pool;
use scatter_projection::Pipeline;
use scatter_providers::Registry;
use triplespace_accounts::Secret;
use triplespace_api_ingest::PgIngest;
use triplespace_projections::{EntityProjection, Farm, PgBackend, milestone_pipeline};

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
    /// Proxies whose `X-Forwarded-*` headers are trusted, as the operator wrote them.
    pub trusted_proxies: Vec<String>,
    /// In development mode, the tenant an unregistered host is served as.
    pub dev_tenant: Option<String>,
    /// The farm.
    pub farm: Farm,
    /// `MediaWiki`-compatible `generator` string.
    pub generator: String,
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
}

impl App {
    /// Builds the state over a pool.
    pub fn new(pool: Pool, secret: Secret, config: Config) -> Result<Self, String> {
        let store = PgIngest::new(PgBackend::new(pool.clone()));
        let registry = Registry::default_registry();
        let pipeline = milestone_pipeline(
            scatter_actors::issuer::IssuerRegistry::default_registry().clone(),
            config.farm.clone(),
        )
        .map_err(|e| e.to_string())?;
        // Mirror order: every registered provider, registry order (the tenant's opt-in
        // order is a later refinement, 0018 §6).
        let slugs: Vec<&str> = registry
            .providers()
            .iter()
            .map(|p| p.slug.as_str())
            .collect();
        let order = EntityProjection::new(&slugs, registry);
        Ok(Self {
            inner: Arc::new(Inner {
                pool,
                store,
                pipeline,
                order,
                secret,
                config,
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

    /// The hosts `X-Forwarded-*` is trusted from.
    #[must_use]
    pub fn trusted_proxies(&self) -> BTreeSet<&str> {
        self.inner
            .config
            .trusted_proxies
            .iter()
            .map(String::as_str)
            .collect()
    }
}
