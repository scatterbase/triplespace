//! `GET /resolve` (18 §3.3): what a string names, for the resolver pages of 06 §3.3.

use axum::http::StatusCode;
use percent_encoding::utf8_percent_encode;
use serde::Deserialize;

use crate::api::{CacheInfo, Fetched};
use crate::entity::QUERY;
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// An entity a resolver names.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Named {
    /// Its ID, the canonical member of its cluster.
    pub id: String,
    /// Its page's title.
    pub title: String,
    /// Its values for the resolver's properties, as keys; empty for a `target`.
    #[serde(default)]
    pub values: Vec<String>,
}

/// What a string names: `kind` is `entity-source`, `entity`, `resolver` or `page`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Resolution {
    /// What answered.
    pub kind: String,
    /// `entity` and `page`: the page's title.
    #[serde(default)]
    pub title: Option<String>,
    /// `resolver`: its name (`doi`).
    #[serde(default)]
    pub resolver: Option<String>,
    /// `resolver`: its display name (`DOI`).
    #[serde(default)]
    pub label: Option<String>,
    /// `resolver`: its namespace (`DOI`).
    #[serde(default)]
    pub namespace: Option<String>,
    /// `resolver`: the normalized key.
    #[serde(default)]
    pub key: Option<String>,
    /// `resolver`: where the key resolves outside the instance.
    #[serde(default)]
    pub external: Option<String>,
    /// `resolver`: the properties bound to it.
    #[serde(default)]
    pub properties: Vec<String>,
    /// `resolver`: the one entity the key names.
    #[serde(default)]
    pub target: Option<Named>,
    /// `resolver`, without a target: the entities holding the key.
    #[serde(default)]
    pub candidates: Vec<Named>,
    /// `resolver`: why the string is not a valid key; `entity-source`: not one of the
    /// source's IDs.
    #[serde(default)]
    pub invalid: Option<String>,
    /// `entity-source`: the source's name (`mhc`).
    #[serde(default)]
    pub source: Option<String>,
    /// `entity-source`: the source entity's canonical ID (`mhc:Q1`).
    #[serde(default)]
    pub id: Option<String>,
    /// `entity-source`, without a target: whether the entity is queued for the source's
    /// mirror.
    #[serde(default)]
    pub fetching: bool,
}

impl Client {
    /// `GET /resolve?q=`: what the string names, or `None` where it names nothing.
    ///
    /// # Errors
    /// The API's, other than `404 unresolved`.
    pub async fn resolve(
        &self,
        incoming: &Incoming,
        q: &str,
    ) -> Result<Fetched<Option<Resolution>>, ClientError> {
        let path = format!(
            "/w/rest.php/triplespace/v0/resolve?q={}",
            utf8_percent_encode(q, QUERY)
        );
        let r = self.get(incoming, &path).await?;
        let cache = CacheInfo::of(&r);
        let value = match r.status {
            StatusCode::NOT_FOUND => None,
            StatusCode::OK => Some(
                serde_json::from_slice(&r.body)
                    .map_err(|e| ClientError::Unexpected(format!("resolve: {e}")))?,
            ),
            _ => return Err(crate::entity::rest_error(&r)),
        };
        Ok(Fetched { value, cache })
    }
}
