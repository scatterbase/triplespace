//! HTTP caching of API responses (0014 §6; 0057 §5–6).
//!
//! - A `GET` answered for a request without credentials (no session, no bearer), with no
//!   `Set-Cookie` and no error, is the public form, and is sent `public` with
//!   `s-maxage=60`: a proxy keeps it for a minute and a client revalidates every time.
//! - A response no purge, erasure or moderation can change (`meta=siteinfo` alone) is
//!   [`Class::Stable`]: it also carries `max-age`, so the web tier may reuse it without
//!   revalidating (0057 §5).
//! - Everything else is `private, no-cache`.
//!
//! Every response carries a strong `ETag`, a hash of its body, and a request whose
//! `If-None-Match` names it gets `304` with no body. Public responses also carry the
//! `Cache-Tag` of every entity they drew on (`entity:Q42`), which erasure purges
//! (0014 §5).
//!
//! The `ETag` is computed from the body, so a `304` saves sending and parsing the body,
//! not building it. An `ETag` from the versions a response read (0014 §3) is the later
//! refinement that saves the building too.

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use sha2::{Digest as _, Sha256};

/// How long a proxy keeps a public response (0014 §6).
pub const S_MAXAGE: u32 = 60;

/// How long a client may keep a [`Class::Stable`] response without revalidating, and the
/// ceiling of the web tier's `web.cache.max_ttl` (0057 §5).
pub const STABLE_MAX_AGE: u32 = 60;

/// The cache class of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Class {
    /// Only the client that asked may keep it, and must revalidate.
    #[default]
    Private,
    /// The public form: proxies keep it for [`S_MAXAGE`]; clients revalidate.
    Public,
    /// The public form of something no purge can change: also kept by clients for
    /// [`STABLE_MAX_AGE`].
    Stable,
}

impl Class {
    /// The `Cache-Control` value.
    #[must_use]
    pub fn cache_control(self) -> String {
        match self {
            Self::Private => "private, no-cache".to_string(),
            Self::Public => {
                format!("public, max-age=0, s-maxage={S_MAXAGE}, stale-while-revalidate=300")
            }
            Self::Stable => format!(
                "public, max-age={STABLE_MAX_AGE}, s-maxage={S_MAXAGE}, stale-while-revalidate=300"
            ),
        }
    }

    /// The class a request permits: a response may be public only for a credential-free
    /// `GET` that sets no cookie and did not fail.
    #[must_use]
    pub fn permitted(
        self,
        method: &Method,
        credentialed: bool,
        sets_cookie: bool,
        failed: bool,
    ) -> Self {
        if method == Method::GET && !credentialed && !sets_cookie && !failed {
            self
        } else {
            Self::Private
        }
    }
}

/// A strong `ETag` for a body: the first 128 bits of its SHA-256, quoted.
#[must_use]
pub fn etag(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    let mut hex = String::with_capacity(32);
    for b in &digest[..16] {
        let _ = write!(hex, "{b:02x}");
    }
    format!("\"{hex}\"")
}

/// Whether `If-None-Match` names the `ETag` (`*` matches anything; weak validators
/// compare by their opaque part, as RFC 9110 §13.1.2 has it for `If-None-Match`).
#[must_use]
pub fn not_modified(request: &HeaderMap, etag: &str) -> bool {
    let want = etag.trim_start_matches("W/");
    request
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|t| t == "*" || t.trim_start_matches("W/") == want)
}

/// Builds the response: status, content type, body, the caching headers, and `304`
/// instead when the client already holds this body.
#[must_use]
pub fn respond(
    request: &HeaderMap,
    status: StatusCode,
    content_type: &'static str,
    body: String,
    class: Class,
    tags: &[String],
) -> Response {
    let tag = etag(body.as_bytes());
    let fresh = status == StatusCode::OK && not_modified(request, &tag);
    let mut r = if fresh {
        let mut r = Response::new(Body::empty());
        *r.status_mut() = StatusCode::NOT_MODIFIED;
        r
    } else {
        let mut r = Response::new(Body::from(body));
        *r.status_mut() = status;
        r.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        r
    };
    let h = r.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&tag) {
        h.insert(header::ETAG, v);
    }
    if let Ok(v) = HeaderValue::from_str(&class.cache_control()) {
        h.insert(header::CACHE_CONTROL, v);
    }
    h.insert(
        header::VARY,
        HeaderValue::from_static("Accept, Accept-Language"),
    );
    if class != Class::Private && !tags.is_empty() {
        let mut uniq: Vec<&str> = tags.iter().map(String::as_str).collect();
        uniq.sort_unstable();
        uniq.dedup();
        if let Ok(v) = HeaderValue::from_str(&uniq.join(", ")) {
            h.insert("cache-tag", v);
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert_eq!(Class::Private.cache_control(), "private, no-cache");
        assert!(
            Class::Public
                .cache_control()
                .contains("max-age=0, s-maxage=60")
        );
        assert!(
            Class::Stable
                .cache_control()
                .starts_with("public, max-age=60")
        );
        let get = Method::GET;
        assert_eq!(
            Class::Public.permitted(&get, false, false, false),
            Class::Public
        );
        assert_eq!(
            Class::Public.permitted(&get, true, false, false),
            Class::Private
        );
        assert_eq!(
            Class::Public.permitted(&get, false, true, false),
            Class::Private
        );
        assert_eq!(
            Class::Public.permitted(&get, false, false, true),
            Class::Private
        );
        assert_eq!(
            Class::Stable.permitted(&Method::POST, false, false, false),
            Class::Private
        );
    }

    #[test]
    fn etags_and_revalidation() {
        let t = etag(b"{}");
        assert!(t.starts_with('"') && t.ends_with('"') && t.len() == 34);
        assert_ne!(t, etag(b"{ }"));
        let mut h = HeaderMap::new();
        assert!(!not_modified(&h, &t));
        h.insert(
            header::IF_NONE_MATCH,
            format!("\"x\", W/{t}").parse().unwrap(),
        );
        assert!(not_modified(&h, &t));
        let mut star = HeaderMap::new();
        star.insert(header::IF_NONE_MATCH, "*".parse().unwrap());
        assert!(not_modified(&star, &t));
    }

    #[test]
    fn respond_sends_304_and_tags() {
        let body = "{\"a\":1}".to_string();
        let tag = etag(body.as_bytes());
        let tags = vec!["entity:Q6".to_string(), "entity:Q6".to_string()];
        let r = respond(
            &HeaderMap::new(),
            StatusCode::OK,
            "application/json; charset=utf-8",
            body.clone(),
            Class::Public,
            &tags,
        );
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()["etag"], tag.as_str());
        assert_eq!(r.headers()["cache-tag"], "entity:Q6");
        let mut h = HeaderMap::new();
        h.insert(header::IF_NONE_MATCH, tag.parse().unwrap());
        let r = respond(
            &h,
            StatusCode::OK,
            "application/json; charset=utf-8",
            body,
            Class::Private,
            &tags,
        );
        assert_eq!(r.status(), StatusCode::NOT_MODIFIED);
        assert!(
            r.headers().get("cache-tag").is_none(),
            "private responses carry no tags"
        );
    }
}
