//! What the site forwards to the API for a browser's request (0057 §4).
//!
//! Over HTTP the web tier is a proxy hop: it forwards the viewer's credentials and
//! language, sends the host the browser asked for as `X-Forwarded-Host`, appends the
//! address that connected to it to `X-Forwarded-For` and its forwarder key, if it has one,
//! to `Triplespace-Forwarder`. It judges none of these; only the API decides what to
//! believe (0057 §10). In process the browser's request is passed as it arrived, with its
//! `Host` and whatever forwarded headers it carried, and the connection address travels
//! beside it ([`crate::ApiRequest::peer`]).

use std::net::SocketAddr;

use axum::http::{HeaderMap, HeaderName, HeaderValue, header};

use crate::transport::Transport;

/// The parts of the browser's request the site forwards.
#[derive(Debug, Clone, Default)]
pub struct Incoming {
    /// The browser's request headers.
    pub headers: HeaderMap,
    /// The address of the connection the request came on.
    pub peer: Option<SocketAddr>,
}

impl Incoming {
    /// From a request's headers and connection address.
    #[must_use]
    pub fn new(headers: &HeaderMap, peer: Option<SocketAddr>) -> Self {
        Self {
            headers: headers.clone(),
            peer,
        }
    }

    /// The host the browser asked for.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.headers.get(header::HOST).and_then(|v| v.to_str().ok())
    }

    /// Whether the browser sent credentials (a cookie or a bearer header).
    #[must_use]
    pub fn credentialed(&self) -> bool {
        self.headers.contains_key(header::COOKIE)
            || self.headers.contains_key(header::AUTHORIZATION)
    }
}

/// The headers passed on in every case.
const PASSED: [HeaderName; 5] = [
    header::COOKIE,
    header::AUTHORIZATION,
    header::ACCEPT_LANGUAGE,
    header::USER_AGENT,
    HeaderName::from_static("x-request-id"),
];

/// The forwarded headers a request may already carry.
const XFF: HeaderName = HeaderName::from_static("x-forwarded-for");
const XFH: HeaderName = HeaderName::from_static("x-forwarded-host");
const XFP: HeaderName = HeaderName::from_static("x-forwarded-proto");
const FORWARDER: HeaderName = HeaderName::from_static("triplespace-forwarder");

fn joined(headers: &HeaderMap, name: &HeaderName) -> Vec<String> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

/// The headers for a call to the API on behalf of `incoming`, as `transport` sends them.
#[must_use]
pub fn headers(incoming: &Incoming, transport: &dyn Transport) -> HeaderMap {
    let mut out = HeaderMap::new();
    for name in &PASSED {
        for v in incoming.headers.get_all(name) {
            out.append(name.clone(), v.clone());
        }
    }
    if transport.embedded() {
        // As it arrived: the API judges the forwarded headers against the real peer.
        if let Some(h) = incoming.headers.get(header::HOST) {
            out.insert(header::HOST, h.clone());
        }
        for name in [&XFF, &XFH, &XFP, &FORWARDER] {
            for v in incoming.headers.get_all(name) {
                out.append(name.clone(), v.clone());
            }
        }
        return out;
    }
    // A proxy hop.
    if let Some(h) = incoming.headers.get(header::HOST) {
        out.insert(XFH, h.clone());
    }
    if let Some(p) = incoming.headers.get(&XFP) {
        out.insert(XFP, p.clone());
    }
    let mut xff = joined(&incoming.headers, &XFF);
    if let Some(peer) = incoming.peer {
        xff.push(peer.ip().to_canonical().to_string());
    }
    if !xff.is_empty()
        && let Ok(v) = HeaderValue::from_str(&xff.join(", "))
    {
        out.insert(XFF, v);
    }
    let mut keys = joined(&incoming.headers, &FORWARDER);
    if let Some(k) = transport.forwarder_key() {
        keys.push(k.to_string());
    }
    if !keys.is_empty()
        && let Ok(v) = HeaderValue::from_str(&keys.join(", "))
    {
        out.insert(FORWARDER, v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{ApiRequest, ApiResponse, BoxFuture, ClientError};

    struct Fake {
        embedded: bool,
        key: Option<&'static str>,
    }

    impl Transport for Fake {
        fn send(&self, _: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
            unreachable!()
        }
        fn embedded(&self) -> bool {
            self.embedded
        }
        fn forwarder_key(&self) -> Option<&str> {
            self.key
        }
    }

    fn incoming() -> Incoming {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "librarybase.org".parse().unwrap());
        h.insert(header::COOKIE, "triplespace_session=abc".parse().unwrap());
        h.insert(header::ACCEPT_LANGUAGE, "de".parse().unwrap());
        h.insert("x-forwarded-for", "198.51.100.7".parse().unwrap());
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        h.insert("triplespace-forwarder", "kE".parse().unwrap());
        h.insert("x-unrelated", "dropped".parse().unwrap());
        Incoming::new(&h, Some("10.0.0.2:41000".parse().unwrap()))
    }

    #[test]
    fn a_proxy_hop_appends_and_judges_nothing() {
        let h = headers(
            &incoming(),
            &Fake {
                embedded: false,
                key: Some("kW"),
            },
        );
        assert_eq!(h["x-forwarded-host"], "librarybase.org");
        assert!(
            h.get(header::HOST).is_none(),
            "the transport sets the API's own host"
        );
        assert_eq!(h["x-forwarded-for"], "198.51.100.7, 10.0.0.2");
        assert_eq!(h["x-forwarded-proto"], "https");
        assert_eq!(h["triplespace-forwarder"], "kE, kW");
        assert_eq!(h[header::COOKIE], "triplespace_session=abc");
        assert_eq!(h[header::ACCEPT_LANGUAGE], "de");
        assert!(h.get("x-unrelated").is_none());
    }

    #[test]
    fn in_process_the_request_is_passed_as_it_arrived() {
        let h = headers(
            &incoming(),
            &Fake {
                embedded: true,
                key: None,
            },
        );
        assert_eq!(h[header::HOST], "librarybase.org");
        assert_eq!(h["x-forwarded-for"], "198.51.100.7");
        assert_eq!(h["triplespace-forwarder"], "kE");
        assert!(h.get("x-forwarded-host").is_none());
    }
}
