//! How a request reaches the API: over HTTP, or in process (0057 §1.4).

use std::convert::Infallible;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt as _;
use tower::ServiceExt as _;

/// A request to the API.
#[derive(Debug, Clone)]
pub struct ApiRequest {
    /// The method.
    pub method: Method,
    /// The path and query, `/w/api.php?action=query&…`.
    pub path_and_query: String,
    /// The headers, already built for the transport ([`crate::forward::headers`]).
    pub headers: HeaderMap,
    /// The body.
    pub body: Vec<u8>,
    /// The browser's connection address, which an in-process call hands the API.
    pub peer: Option<SocketAddr>,
}

/// The API's answer.
#[derive(Debug, Clone)]
pub struct ApiResponse {
    /// The status.
    pub status: StatusCode,
    /// The headers.
    pub headers: HeaderMap,
    /// The body.
    pub body: Vec<u8>,
}

/// Why a call failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    /// The API could not be reached, or did not answer in time.
    #[error("the API is unreachable: {0}")]
    Unreachable(String),
    /// The API answered with something the client cannot read.
    #[error("the API answered unexpectedly: {0}")]
    Unexpected(String),
    /// The configuration is refused: plain HTTP to an address that is not internal.
    #[error("{0}")]
    Config(String),
    /// The API refused the request with a MediaWiki error code (`invalid-entity-id`,
    /// `permissiondenied`, …), which the page shows as the API meant it.
    #[error("the API refused the request: {code}: {info}")]
    Api {
        /// The error code.
        code: String,
        /// The message.
        info: String,
    },
}

/// A boxed future, as the transport trait returns it.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A way to send requests to the API.
pub trait Transport: Send + Sync {
    /// Sends one request.
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>>;

    /// Whether this transport calls the API in process, so the browser's request is
    /// passed as it arrived rather than forwarded by a proxy hop.
    fn embedded(&self) -> bool;

    /// The forwarder key this hop appends, if it has one (0057 §10).
    fn forwarder_key(&self) -> Option<&str> {
        None
    }
}

// ---------------------------------------------------------------------------------------
// In process
// ---------------------------------------------------------------------------------------

/// The API's own router, called in process: no network hop, the same middleware.
#[derive(Clone)]
pub struct ServiceTransport<S> {
    service: S,
}

impl<S> ServiceTransport<S> {
    /// Wraps a service, typically the API's `axum::Router`.
    pub fn new(service: S) -> Self {
        Self { service }
    }
}

impl<S> Transport for ServiceTransport<S>
where
    S: tower::Service<Request<Body>, Response = Response, Error = Infallible>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send,
{
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        let service = self.service.clone();
        Box::pin(async move {
            let mut req = Request::builder()
                .method(request.method)
                .uri(&request.path_and_query)
                .body(Body::from(request.body))
                .map_err(|e| ClientError::Unexpected(e.to_string()))?;
            *req.headers_mut() = request.headers;
            if let Some(peer) = request.peer {
                req.extensions_mut().insert(ConnectInfo(peer));
            }
            let Ok(response) = service.oneshot(req).await;
            let status = response.status();
            let headers = response.headers().clone();
            let body = response
                .into_body()
                .collect()
                .await
                .map_err(|e| ClientError::Unexpected(e.to_string()))?
                .to_bytes()
                .to_vec();
            Ok(ApiResponse {
                status,
                headers,
                body,
            })
        })
    }

    fn embedded(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------------------
// Over HTTP
// ---------------------------------------------------------------------------------------

/// Whether an address is internal: loopback, link-local, private (RFC 1918, RFC 4193) or
/// shared (RFC 6598, which clusters and overlay networks use). Plain HTTP to the API is
/// allowed only to these (0057 §4).
#[must_use]
pub fn is_internal(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (o[0] == 100 && (o[1] & 0b1100_0000) == 64)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Resolves names for plain-HTTP calls, refusing a name any of whose addresses is not
/// internal, at every resolution, so a name that starts resolving publicly is refused
/// from then on (0057 §4).
#[derive(Debug, Default)]
struct InternalOnly;

impl reqwest::dns::Resolve for InternalOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if addrs.is_empty() {
                return Err(format!("`{host}` resolved to nothing").into());
            }
            if let Some(a) = addrs.iter().find(|a| !is_internal(a.ip())) {
                return Err(format!(
                    "`{host}` resolves to {}, which is not internal; plain HTTP to the API is allowed only to internal addresses (0057 §4)",
                    a.ip()
                )
                .into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// The API over the network.
#[derive(Clone)]
pub struct HttpTransport {
    client: reqwest::Client,
    base: url::Url,
    forwarder_key: Option<Arc<str>>,
}

impl std::fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpTransport")
            .field("base", &self.base.as_str())
            .field("forwarder_key", &self.forwarder_key.is_some())
            .finish_non_exhaustive()
    }
}

impl HttpTransport {
    /// A transport to the API at `base` (`https://api.internal:8080`, `http://api.svc`).
    /// Plain `http` is refused unless the host is an internal address or resolves only to
    /// internal addresses, now and at every later resolution (0057 §4).
    pub async fn new(base: &str, forwarder_key: Option<String>) -> Result<Self, ClientError> {
        let base = url::Url::parse(base)
            .map_err(|e| ClientError::Config(format!("the API's URL `{base}`: {e}")))?;
        let host = base
            .host()
            .ok_or_else(|| ClientError::Config(format!("the API's URL `{base}` names no host")))?;
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        match base.scheme() {
            "https" => {}
            "http" => {
                match host {
                    url::Host::Ipv4(ip) if !is_internal(IpAddr::V4(ip)) => {
                        return Err(ClientError::Config(format!(
                            "plain HTTP to {ip}, which is not internal; use https (0057 §4)"
                        )));
                    }
                    url::Host::Ipv6(ip) if !is_internal(IpAddr::V6(ip)) => {
                        return Err(ClientError::Config(format!(
                            "plain HTTP to {ip}, which is not internal; use https (0057 §4)"
                        )));
                    }
                    url::Host::Domain(name) => {
                        // Checked now, so a misconfiguration fails at start, and again at
                        // every resolution.
                        let port = base.port_or_known_default().unwrap_or(80);
                        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name, port))
                            .await
                            .map_err(|e| ClientError::Config(format!("resolving `{name}`: {e}")))?
                            .collect();
                        if addrs.is_empty() || addrs.iter().any(|a| !is_internal(a.ip())) {
                            return Err(ClientError::Config(format!(
                                "plain HTTP to `{name}`, which does not resolve only to internal addresses; use https (0057 §4)"
                            )));
                        }
                    }
                    _ => {}
                }
                builder = builder.dns_resolver(Arc::new(InternalOnly));
            }
            other => {
                return Err(ClientError::Config(format!(
                    "the API's URL has scheme `{other}`; http or https"
                )));
            }
        }
        let client = builder
            .build()
            .map_err(|e| ClientError::Config(format!("the HTTP client: {e}")))?;
        Ok(Self {
            client,
            base,
            forwarder_key: forwarder_key.map(Into::into),
        })
    }

    /// The API's base URL.
    #[must_use]
    pub fn base(&self) -> &url::Url {
        &self.base
    }
}

impl Transport for HttpTransport {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let url = self
                .base
                .join(request.path_and_query.trim_start_matches('/'))
                .map_err(|e| ClientError::Unexpected(e.to_string()))?;
            // Idempotent GETs are retried once on a connection failure; writes never.
            let attempts = if request.method == Method::GET { 2 } else { 1 };
            let mut last = None;
            for _ in 0..attempts {
                let r = self
                    .client
                    .request(request.method.clone(), url.clone())
                    .headers(request.headers.clone())
                    .body(request.body.clone())
                    .send()
                    .await;
                match r {
                    Ok(resp) => {
                        let status = resp.status();
                        let headers = resp.headers().clone();
                        let body = resp
                            .bytes()
                            .await
                            .map_err(|e| ClientError::Unreachable(e.to_string()))?
                            .to_vec();
                        return Ok(ApiResponse {
                            status,
                            headers,
                            body,
                        });
                    }
                    Err(e) if e.is_connect() || e.is_timeout() => last = Some(e),
                    Err(e) => return Err(ClientError::Unreachable(e.to_string())),
                }
            }
            Err(ClientError::Unreachable(
                last.map_or_else(|| "no attempt".into(), |e| e.to_string()),
            ))
        })
    }

    fn embedded(&self) -> bool {
        false
    }

    fn forwarder_key(&self) -> Option<&str> {
        self.forwarder_key.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn internal_addresses() {
        for a in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_internal(ip(a)), "{a}");
        }
        for a in [
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "2001:db8::1",
            "198.51.100.7",
        ] {
            assert!(!is_internal(ip(a)), "{a}");
        }
    }

    #[tokio::test]
    async fn plain_http_only_to_internal_addresses() {
        assert!(
            HttpTransport::new("http://127.0.0.1:8080", None)
                .await
                .is_ok()
        );
        assert!(
            HttpTransport::new("http://localhost:8080", None)
                .await
                .is_ok()
        );
        assert!(
            HttpTransport::new("https://203.0.113.5", None)
                .await
                .is_ok()
        );
        assert!(matches!(
            HttpTransport::new("http://203.0.113.5", None).await,
            Err(ClientError::Config(_))
        ));
        assert!(matches!(
            HttpTransport::new("ftp://api.internal", None).await,
            Err(ClientError::Config(_))
        ));
    }
}
