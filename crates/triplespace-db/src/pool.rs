//! Connection pools (0033 §4): `deadpool-postgres` over `tokio-postgres`.

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use tokio_postgres::NoTls;

/// Why a pool could not be built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PoolError {
    /// The URL does not parse as a connection string.
    #[error("database URL: {0}")]
    Url(#[from] tokio_postgres::Error),
    /// The pool refused its configuration.
    #[error("pool: {0}")]
    Build(String),
}

/// A pool of at most `size` connections to `url`, each verified with a fast check when
/// recycled. TLS is the deployment's proxy's job for now (0033 §4 leaves `rustls` for
/// the server's own listeners).
pub fn pool(url: &str, size: usize) -> Result<Pool, PoolError> {
    let config: tokio_postgres::Config = url.parse()?;
    let manager = Manager::from_config(
        config,
        NoTls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        },
    );
    Pool::builder(manager)
        .max_size(size)
        .build()
        .map_err(|e| PoolError::Build(e.to_string()))
}
