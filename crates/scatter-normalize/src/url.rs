//! The URL normalizer (ADR 0026 §1): the identity of a sitelink and the key of the `url`
//! data type.
//!
//! 1. parse as a WHATWG URL; reject anything that does not parse;
//! 2. the scheme must be in the allowed set (`sitelinks.schemes`; default `https` and `http`);
//! 3. lowercase the scheme; normalize the host with the Domain normalizer, so it is a valid
//!    Domain key in A-label form; drop a default port;
//! 4. normalize percent-encoding in the path, query and fragment (RFC 3986 §6.2.2:
//!    uppercase hex digits, decode unreserved characters); an empty path becomes `/`;
//! 5. keep the query and the fragment.
//!
//! Nothing else is folded: `/Foo` and `/Foo/` are different URLs.

use std::fmt::Write as _;

use url::{Host, Url};

use crate::domain::{self, DomainError};

/// The schemes allowed when a tenant has not set `sitelinks.schemes`.
pub const DEFAULT_SCHEMES: &[&str] = &["https", "http"];

/// Why a string is not a normalizable URL.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UrlError {
    /// Not a WHATWG URL.
    #[error("`{0}` is not a URL")]
    Parse(String),
    /// A scheme outside the allowed set.
    #[error("scheme `{0}` is not allowed")]
    Scheme(String),
    /// No host, as in `mailto:` or a relative reference.
    #[error("`{0}` has no host")]
    NoHost(String),
    /// An IP address rather than a domain name; the host of a sitelink is a Domain.
    #[error("host `{0}` is not a domain name")]
    HostNotDomain(String),
    /// The host is not a valid Domain key.
    #[error("host: {0}")]
    Domain(#[from] DomainError),
}

/// Normalizes `input` under the given allowed schemes.
///
/// ```
/// use scatter_normalize::url::{normalize_url, DEFAULT_SCHEMES};
/// let n = |s| normalize_url(s, DEFAULT_SCHEMES).unwrap();
/// assert_eq!(n("HTTPS://EN.Wikipedia.org:443/wiki/Caf%c3%a9"), "https://en.wikipedia.org/wiki/Caf%C3%A9");
/// assert_eq!(n("https://example.org"), "https://example.org/");
/// assert_eq!(n("https://example.org/a%2Fb?q=%41#s%65c"), "https://example.org/a%2Fb?q=A#sec");
/// assert_ne!(n("https://example.org/Foo"), n("https://example.org/Foo/"));
/// assert!(normalize_url("ftp://example.org/", DEFAULT_SCHEMES).is_err());
/// ```
pub fn normalize_url(input: &str, schemes: &[&str]) -> Result<String, UrlError> {
    let url = Url::parse(input).map_err(|_| UrlError::Parse(input.to_string()))?;
    let scheme = url.scheme(); // the url crate lowercases it
    if !schemes.iter().any(|s| s.eq_ignore_ascii_case(scheme)) {
        return Err(UrlError::Scheme(scheme.to_string()));
    }
    let host = match url.host() {
        None => return Err(UrlError::NoHost(input.to_string())),
        Some(Host::Domain(d)) => domain::normalize(d)?,
        Some(other) => return Err(UrlError::HostNotDomain(other.to_string())),
    };

    let mut out = String::with_capacity(input.len() + 1);
    out.push_str(scheme);
    out.push_str("://");
    if !url.username().is_empty() || url.password().is_some() {
        out.push_str(url.username());
        if let Some(p) = url.password() {
            out.push(':');
            out.push_str(p);
        }
        out.push('@');
    }
    out.push_str(&host);
    // `port()` is `None` for the scheme's default port, which is what "drop a default
    // port" asks for.
    if let Some(port) = url.port() {
        let _ = write!(out, ":{port}");
    }
    let path = url.path();
    if path.is_empty() {
        out.push('/');
    } else {
        out.push_str(&normalize_percent_encoding(path));
    }
    if let Some(q) = url.query() {
        out.push('?');
        out.push_str(&normalize_percent_encoding(q));
    }
    if let Some(f) = url.fragment() {
        out.push('#');
        out.push_str(&normalize_percent_encoding(f));
    }
    Ok(out)
}

/// Whether `s` is already a normalized URL under the default schemes.
#[must_use]
pub fn is_normalized(s: &str) -> bool {
    normalize_url(s, DEFAULT_SCHEMES).as_deref() == Ok(s)
}

/// RFC 3986 §6.2.2: uppercase the hex digits of every percent-encoded octet, and decode
/// the ones that encode unreserved characters. Everything else is left as it is.
#[must_use]
pub fn normalize_percent_encoding(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let (Some(h), Some(l)) = (b.get(i + 1), b.get(i + 2))
            && let (Some(hv), Some(lv)) = (hex(*h), hex(*l))
        {
            let byte = hv << 4 | lv;
            if is_unreserved(byte) {
                out.push(byte as char);
            } else {
                let _ = write!(out, "%{byte:02X}");
            }
            i += 3;
            continue;
        }
        // The input is a `str`, so any non-ASCII sequence is valid UTF-8; copy it through.
        let ch = s[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> String {
        normalize_url(s, DEFAULT_SCHEMES).unwrap()
    }

    #[test]
    fn scheme_host_and_port() {
        assert_eq!(n("HTTP://Example.ORG:80/"), "http://example.org/");
        assert_eq!(n("https://example.org:8443/"), "https://example.org:8443/");
        assert_eq!(
            n("https://bücher.example/"),
            "https://xn--bcher-kva.example/"
        );
        assert_eq!(
            n("https://user:pw@example.org/"),
            "https://user:pw@example.org/"
        );
    }

    #[test]
    fn percent_encoding() {
        assert_eq!(normalize_percent_encoding("%7euser"), "~user");
        assert_eq!(normalize_percent_encoding("a%2fb"), "a%2Fb");
        assert_eq!(normalize_percent_encoding("100%"), "100%");
        assert_eq!(normalize_percent_encoding("%zz"), "%zz");
        assert_eq!(normalize_percent_encoding("caf\u{e9}"), "caf\u{e9}");
        assert_eq!(
            n("https://example.org/wiki/Caf%c3%a9"),
            "https://example.org/wiki/Caf%C3%A9"
        );
        // The url crate percent-encodes non-ASCII on parse; the encoding is then uppercase.
        assert_eq!(
            n("https://example.org/wiki/Café"),
            "https://example.org/wiki/Caf%C3%A9"
        );
    }

    #[test]
    fn what_is_kept() {
        assert_eq!(
            n("https://example.org/a?b=c#d"),
            "https://example.org/a?b=c#d"
        );
        assert_eq!(n("https://example.org/a?"), "https://example.org/a?");
        assert_ne!(n("https://example.org/Foo"), n("https://example.org/foo"));
    }

    #[test]
    fn rejections() {
        assert!(matches!(n_err("not a url"), UrlError::Parse(_)));
        assert!(matches!(n_err("ftp://example.org/"), UrlError::Scheme(_)));
        assert!(matches!(
            n_err("https://127.0.0.1/"),
            UrlError::HostNotDomain(_)
        ));
        assert!(matches!(
            n_err("https://[::1]/"),
            UrlError::HostNotDomain(_)
        ));
        assert!(matches!(
            n_err("https://exa_mple.org/"),
            UrlError::Domain(_)
        ));
        assert!(matches!(
            normalize_url("mailto:x@example.org", &["mailto"]).unwrap_err(),
            UrlError::NoHost(_)
        ));
    }

    fn n_err(s: &str) -> UrlError {
        normalize_url(s, DEFAULT_SCHEMES).unwrap_err()
    }

    #[test]
    fn idempotent() {
        for s in [
            "https://en.wikipedia.org/wiki/Caf%C3%A9",
            "https://example.org/",
            "http://example.org:8080/p?q#f",
        ] {
            assert!(is_normalized(s), "{s}");
        }
    }
}
