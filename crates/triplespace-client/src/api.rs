//! The typed calls the site makes, each returning its value with the caching facts a page
//! composes its own from (0057 §6).

use axum::http::{StatusCode, header};
use serde_json::{Map, Value};

use crate::transport::{ApiResponse, ClientError};
use crate::{Client, Incoming};

/// The caching facts of one API response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheInfo {
    /// Whether the API sent it `public`.
    pub public: bool,
    /// Its `ETag`.
    pub etag: Option<String>,
    /// Its `Cache-Tag`s.
    pub tags: Vec<String>,
}

impl CacheInfo {
    /// Read from a response's headers.
    #[must_use]
    pub fn of(response: &ApiResponse) -> Self {
        let h = &response.headers;
        let public = h
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|d| d.trim() == "public"));
        let etag = h
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let tags = h
            .get("cache-tag")
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Self { public, etag, tags }
    }
}

/// A value and the caching facts of the response it came from.
#[derive(Debug, Clone)]
pub struct Fetched<T> {
    /// The value.
    pub value: T,
    /// The caching facts.
    pub cache: CacheInfo,
}

/// What the site needs of `meta=siteinfo`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SiteInfo {
    /// `general.sitename`.
    pub sitename: String,
    /// `general.server`: the tenant base the request is seen under.
    pub server: String,
    /// `general.lang`.
    pub lang: String,
    /// `triplespace.api_version`; 0 against a stock Wikibase, which has no such field.
    pub api_version: u32,
    /// `triplespace.theme`: the tenant's or instance's token overrides, if any.
    pub theme: Option<Map<String, Value>>,
    /// Whether the API reports itself in development mode (`triplespace.insecure`).
    pub insecure: bool,
}

/// What the site needs of `meta=userinfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserInfo {
    /// The name; for an anonymous viewer, what the API reports in its place.
    pub name: String,
    /// The user ID; 0 when anonymous.
    pub id: u64,
    /// Whether the viewer is anonymous.
    pub anon: bool,
}

/// What happens when the API refuses the request outright.
fn check(response: &ApiResponse) -> Result<Value, ClientError> {
    match response.status {
        StatusCode::OK => serde_json::from_slice(&response.body)
            .map_err(|e| ClientError::Unexpected(format!("not JSON: {e}"))),
        StatusCode::MISDIRECTED_REQUEST => Err(ClientError::Unexpected(
            "the API does not serve this host (421)".into(),
        )),
        s => Err(ClientError::Unexpected(format!("HTTP {s}"))),
    }
}

impl Client {
    /// `meta=siteinfo&siprop=general|triplespace`, alone, so that the API may mark it
    /// stable and the response is the same for every viewer (0057 §5).
    pub async fn siteinfo(&self, incoming: &Incoming) -> Result<Fetched<SiteInfo>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=siteinfo&siprop=general%7Ctriplespace&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        if let Some(e) = v.get("error") {
            return Err(ClientError::Unexpected(format!("siteinfo: {e}")));
        }
        let general = &v["query"]["general"];
        let ts = &v["query"]["triplespace"];
        let value = SiteInfo {
            sitename: general["sitename"].as_str().unwrap_or_default().to_string(),
            server: general["server"].as_str().unwrap_or_default().to_string(),
            lang: general["lang"].as_str().unwrap_or("en").to_string(),
            api_version: ts["api_version"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            theme: ts["theme"].as_object().cloned(),
            insecure: ts["insecure"].as_bool().unwrap_or(false),
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `meta=userinfo`: who the viewer is.
    pub async fn userinfo(&self, incoming: &Incoming) -> Result<Fetched<UserInfo>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=userinfo&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        if let Some(e) = v.get("error") {
            return Err(ClientError::Unexpected(format!("userinfo: {e}")));
        }
        let u = &v["query"]["userinfo"];
        let anon = match &u["anon"] {
            Value::Bool(b) => *b,
            Value::String(_) => true,
            _ => u["id"].as_u64().unwrap_or(0) == 0,
        };
        let value = UserInfo {
            name: u["name"].as_str().unwrap_or_default().to_string(),
            id: u["id"].as_u64().unwrap_or(0),
            anon,
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn cache_info_reads_the_headers() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CACHE_CONTROL,
            "public, max-age=0, s-maxage=60".parse().unwrap(),
        );
        h.insert(header::ETAG, "\"abc\"".parse().unwrap());
        h.insert("cache-tag", "entity:Q6, entity:Q7".parse().unwrap());
        let c = CacheInfo::of(&ApiResponse {
            status: StatusCode::OK,
            headers: h,
            body: Vec::new(),
        });
        assert!(c.public);
        assert_eq!(c.etag.as_deref(), Some("\"abc\""));
        assert_eq!(c.tags, vec!["entity:Q6", "entity:Q7"]);
        let p = CacheInfo::of(&ApiResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Vec::new(),
        });
        assert!(!p.public);
    }
}
