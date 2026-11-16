//! The store's errors (0033 §14: one `thiserror` enum per crate boundary).

use crate::key::KeyError;

/// What a store operation can fail with.
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    /// No object at the key.
    #[error("no object at {location}")]
    NotFound {
        /// The key, as a store path.
        location: String,
    },
    /// A streamed write exceeded its size limit (0039 §3, §5); nothing was stored.
    #[error("the bytes exceed the size limit of {limit} bytes")]
    TooLarge {
        /// The limit that was enforced.
        limit: u64,
    },
    /// A key, scope or path did not follow the layout.
    #[error(transparent)]
    Key(#[from] KeyError),
    /// The stream feeding a write failed.
    #[error("the source stream failed: {0}")]
    Source(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    /// The backend failed.
    #[error(transparent)]
    Store(object_store::Error),
}

impl From<object_store::Error> for BlobError {
    fn from(e: object_store::Error) -> Self {
        match e {
            object_store::Error::NotFound { path, .. } => Self::NotFound { location: path },
            other => Self::Store(other),
        }
    }
}

impl BlobError {
    /// Whether this is [`BlobError::NotFound`].
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}
