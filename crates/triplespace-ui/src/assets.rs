//! The built frontend (0034 §8): `ui/dist`, which `npm run build` in `ui/` writes, embedded
//! in the binary in a release build and read from disk in a debug one. Its manifest names
//! the hashed files a page links to.
//!
//! In development the files can come from a running Vite dev server instead ([`DevServer`];
//! 22 §5.1): a page then links Vite's client and the source entries, requests under
//! `/ui/assets/` are passed through to it, and a change under `ui/src` reaches the open
//! page without a rebuild.
//!
//! A build without `ui/dist` (no Node on the machine, as in the Rust CI job) still
//! compiles and serves pages; they link no stylesheet or script, which a page reads
//! without.

use std::borrow::Cow;
use std::sync::LazyLock;

use sha2::{Digest as _, Sha256};

#[derive(rust_embed::Embed)]
#[folder = "../../ui/dist"]
#[allow_missing = true]
struct Dist;

/// The entries of the manifest the site links to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entries {
    /// The stylesheet, left to right.
    pub css: Option<String>,
    /// The stylesheet, right to left.
    pub css_rtl: Option<String>,
    /// The script.
    pub js: Option<String>,
    /// The blocking script in `<head>` that applies a reader's stored appearance before
    /// the first paint (0085 §6).
    pub head_js: Option<String>,
    /// Whether the head script is a module: only under a dev server, which serves the
    /// source with its imports, where the build wraps one classic script.
    pub head_module: bool,
    /// Vite's client, the module that keeps the page current with the sources; only
    /// under a dev server.
    pub dev_client: Option<String>,
}

fn entry(manifest: &serde_json::Value, src: &str) -> Option<String> {
    manifest
        .get(src)
        .and_then(|e| e.get("file"))
        .and_then(serde_json::Value::as_str)
        .map(|f| format!("/ui/assets/{f}"))
}

static MANIFEST: LazyLock<(Entries, String)> = LazyLock::new(|| {
    let raw = Dist::get("manifest.json").map(|f| f.data.into_owned());
    let manifest: serde_json::Value = raw
        .as_deref()
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or(serde_json::Value::Null);
    let entries = Entries {
        css: entry(&manifest, "src/site.css"),
        css_rtl: entry(&manifest, "src/site-rtl.css"),
        js: entry(&manifest, "src/main.js"),
        head_js: entry(&manifest, "src/head.js"),
        head_module: false,
        dev_client: None,
    };
    // The build ID: the crate's version and the manifest's hash, so that a new frontend is
    // a new build even when the Rust is unchanged (0057 §8).
    let mut h = Sha256::new();
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(raw.as_deref().unwrap_or_default());
    let id = crate::hex(&h.finalize()[..6]);
    (entries, format!("{}-{id}", env!("CARGO_PKG_VERSION")))
});

/// The manifest's entries.
#[must_use]
pub fn entries() -> &'static Entries {
    &MANIFEST.0
}

/// The path every file of the build is served under, and Vite's `base` (`ui/vite.config.js`).
pub const BASE: &str = "/ui/assets/";

/// A Vite dev server the site takes its assets from in development (22 §5.1), in place
/// of the hashed files of `ui/dist`. With the embedded build above, this implements
/// 0034 §8 (A19).
///
/// The page links the server's client and the source entries under [`BASE`], on the
/// site's own origin; the site passes those requests through ([`DevServer::fetch`]), so
/// only the client's WebSocket goes to the dev server directly. What Vite does not serve
/// (a type set's built font files) still comes from `ui/dist`.
#[derive(Debug, Clone)]
pub struct DevServer {
    /// The server's origin, `http://localhost:5173`: scheme, host and port, no path.
    origin: String,
    entries: Entries,
    http: reqwest::Client,
}

/// Why a dev server's URL is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevServerError(String);

impl std::fmt::Display for DevServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DevServerError {}

/// The request headers passed to the dev server: what Vite reads to choose a
/// representation (a stylesheet asked for by a `<link>` is served as CSS, by an `import`
/// as a module) and to answer `304`.
const DEV_REQUEST_HEADERS: [&str; 5] = [
    "accept",
    "sec-fetch-dest",
    "sec-fetch-mode",
    "if-none-match",
    "if-modified-since",
];

/// The response headers passed back from it.
const DEV_RESPONSE_HEADERS: [&str; 4] = ["content-type", "etag", "last-modified", "cache-control"];

impl DevServer {
    /// The dev server at a URL: `http` or `https`, with a host, and nothing after it but
    /// `/`, since Vite's `base` is [`BASE`] and the site names every path itself.
    ///
    /// # Errors
    /// A URL that is not one, has another scheme, or carries a path, query or credentials.
    pub fn new(url: &str) -> Result<Self, DevServerError> {
        let bad = |why: &str| DevServerError(format!("the Vite dev server `{url}` {why}"));
        let parsed = url::Url::parse(url).map_err(|e| bad(&format!("is not a URL: {e}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(bad("is not an http or https URL"));
        }
        if parsed.host_str().is_none_or(str::is_empty) {
            return Err(bad("names no host"));
        }
        if !matches!(parsed.path(), "" | "/")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(bad("must be an origin only, such as http://localhost:5173"));
        }
        let at = |src: &str| Some(format!("{BASE}{src}"));
        Ok(Self {
            // The serialization of an origin holds a scheme, a host and a port: nothing
            // that could end a directive of the Content Security Policy.
            origin: parsed.origin().ascii_serialization(),
            entries: Entries {
                css: at("src/site.css"),
                css_rtl: at("src/site-rtl.css"),
                js: at("src/main.js"),
                head_js: at("src/head.js"),
                head_module: true,
                dev_client: at("@vite/client"),
            },
            // The dev server is on this machine or beside it: never through a proxy the
            // environment names.
            http: reqwest::Client::builder()
                .no_proxy()
                .build()
                .map_err(|e| bad(&format!("cannot be reached: {e}")))?,
        })
    }

    /// The server's origin.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The origin of the WebSocket Vite's client opens to the server: the same host and
    /// port, `ws` for `http` and `wss` for `https`.
    #[must_use]
    pub fn socket_origin(&self) -> String {
        match self.origin.split_once("://") {
            Some(("https", rest)) => format!("wss://{rest}"),
            Some((_, rest)) => format!("ws://{rest}"),
            None => self.origin.clone(),
        }
    }

    /// The entries a page links under this server.
    #[must_use]
    pub fn entries(&self) -> &Entries {
        &self.entries
    }

    /// The Content Security Policy of a response under this server: `policy` with the
    /// server's WebSocket among the sources a script may connect to, and with `<style>`
    /// elements allowed, which is how Vite applies a stylesheet a module imports, and
    /// with fonts from `data:` URLs, which is how it serves a small one. Nothing else is
    /// widened: scripts and stylesheets still come from the site's own origin.
    #[must_use]
    pub fn widen_csp(&self, policy: &str) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut connect = false;
        for directive in policy.split(';').map(str::trim).filter(|d| !d.is_empty()) {
            let name = directive.split_whitespace().next().unwrap_or_default();
            match name {
                "style-src" if !directive.contains("'unsafe-inline'") => {
                    out.push(format!("{directive} 'unsafe-inline'"));
                }
                // Vite's dev server inlines a small font into the stylesheet as a
                // `data:` URL, where the build links a hashed file.
                "font-src" if !directive.contains("data:") => {
                    out.push(format!("{directive} data:"));
                }
                "connect-src" => {
                    connect = true;
                    out.push(format!("{directive} {}", self.socket_origin()));
                }
                _ => out.push(directive.to_string()),
            }
        }
        if !connect {
            out.push(format!("connect-src 'self' {}", self.socket_origin()));
        }
        out.join("; ")
    }

    /// A file under [`BASE`] from the dev server: its status, the headers a browser
    /// needs, and its body. `None` when the server has no such file, or cannot be
    /// reached, and the caller falls back to `ui/dist`.
    pub async fn fetch(
        &self,
        path: &str,
        query: Option<&str>,
        headers: &axum::http::HeaderMap,
    ) -> Option<axum::response::Response> {
        use axum::response::IntoResponse as _;
        let mut url = format!("{}{BASE}{path}", self.origin);
        if let Some(q) = query {
            url.push('?');
            url.push_str(q);
        }
        let mut request = self.http.get(&url);
        for name in DEV_REQUEST_HEADERS {
            if let Some(v) = headers.get(name) {
                request = request.header(name, v.as_bytes());
            }
        }
        let answer = match request.send().await {
            Ok(a) => a,
            Err(e) => {
                eprintln!(
                    "triplespace-ui: the Vite dev server at {} did not answer: {e}",
                    self.origin
                );
                return None;
            }
        };
        let status = answer.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return None;
        }
        let mut out = axum::http::HeaderMap::new();
        for name in DEV_RESPONSE_HEADERS {
            if let Some(v) = answer.headers().get(name)
                && let Ok(v) = axum::http::HeaderValue::from_bytes(v.as_bytes())
            {
                out.insert(axum::http::HeaderName::from_static(name), v);
            }
        }
        let body = answer.bytes().await.ok()?;
        let status = axum::http::StatusCode::from_u16(status.as_u16()).ok()?;
        Some((status, out, body.to_vec()).into_response())
    }
}

/// The `@font-face` rules of a shipped type set's typefaces (0086 §3), built from
/// `ui/src/typesets/{name}.css`, whose `url()`s name the hashed font files under
/// `/ui/assets/`; `None` for a type set the build has none for.
#[must_use]
pub fn typeset_fonts(name: &str) -> Option<String> {
    built_css("src/typesets/", name)
}

/// The `@font-face` rules of a fallback layer (0093 §3), built from
/// `ui/src/fallbacks/{name}.css`, which `ui/scripts/fallbacks.js` writes from the registry;
/// `None` for a layer the build has none for.
#[must_use]
pub fn fallback_fonts(name: &str) -> Option<String> {
    built_css("src/fallbacks/", name)
}

/// A built stylesheet of the build's `@font-face` rules, by its source directory and name.
fn built_css(dir: &str, name: &str) -> Option<String> {
    static FONTS: LazyLock<std::collections::BTreeMap<String, String>> = LazyLock::new(|| {
        let manifest: serde_json::Value = Dist::get("manifest.json")
            .and_then(|f| serde_json::from_slice(&f.data).ok())
            .unwrap_or(serde_json::Value::Null);
        let Some(entries) = manifest.as_object() else {
            return std::collections::BTreeMap::new();
        };
        entries
            .iter()
            .filter(|(src, _)| {
                src.starts_with("src/typesets/") || src.starts_with("src/fallbacks/")
            })
            .filter_map(|(src, e)| {
                let src = src.strip_suffix(".css")?;
                let file = e.get("file")?.as_str()?;
                let css = String::from_utf8(Dist::get(file)?.data.into_owned()).ok()?;
                Some((src.to_string(), without_header(&css).to_string()))
            })
            .collect()
    });
    FONTS.get(&format!("{dir}{name}")).cloned()
}

/// A built stylesheet without the licence header the build opens every file with
/// (21 §5.3): the rules alone, for a sheet the site composes from them.
fn without_header(css: &str) -> &str {
    css.strip_prefix("/*!")
        .and_then(|rest| rest.split_once("*/"))
        .map_or(css, |(_, rules)| rules.trim_start_matches(['\r', '\n']))
}

/// The build ID, which a page's `ETag` and the components' build check carry.
#[must_use]
pub fn build_id() -> &'static str {
    &MANIFEST.1
}

/// A file of the build, by its path under `/ui/assets/`, with its content type.
#[must_use]
pub fn get(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    if path == "manifest.json" || path.contains("..") {
        return None;
    }
    let file = Dist::get(path)?;
    Some((file.data, content_type(path)))
}

/// The content type of a file, by its extension.
#[must_use]
pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0034 §8: under a dev server a page links Vite's client and the sources.
    #[test]
    fn a_dev_server_names_the_sources_and_widens_only_what_vite_needs() {
        let dev = DevServer::new("http://localhost:5173/").unwrap();
        assert_eq!(dev.origin(), "http://localhost:5173");
        let e = dev.entries();
        assert_eq!(e.dev_client.as_deref(), Some("/ui/assets/@vite/client"));
        assert_eq!(e.js.as_deref(), Some("/ui/assets/src/main.js"));
        assert_eq!(e.css.as_deref(), Some("/ui/assets/src/site.css"));
        assert_eq!(e.css_rtl.as_deref(), Some("/ui/assets/src/site-rtl.css"));
        assert!(e.head_module);
        let csp = dev.widen_csp(crate::pages::CSP);
        assert!(csp.contains("style-src 'self' 'unsafe-inline';"), "{csp}");
        assert!(
            csp.contains("connect-src 'self' ws://localhost:5173"),
            "{csp}"
        );
        // Scripts still come from the site's own origin alone.
        assert!(csp.contains("script-src 'self';"), "{csp}");
        assert!(csp.contains("style-src-attr 'unsafe-inline'"), "{csp}");
        assert!(csp.contains("font-src 'self' data:;"), "{csp}");
        assert_eq!(
            DevServer::new("https://vite.test").unwrap().socket_origin(),
            "wss://vite.test"
        );
        // The built entries never carry the dev server's pieces.
        assert!(entries().dev_client.is_none());
        assert!(!entries().head_module);
    }

    #[test]
    fn a_dev_server_url_is_an_origin() {
        for bad in [
            "localhost:5173",
            "ftp://localhost",
            "http://localhost:5173/ui",
            "http://localhost:5173/?x=1",
            "http://user:pw@localhost:5173",
            "not a url",
        ] {
            assert!(DevServer::new(bad).is_err(), "{bad}");
        }
    }

    // 0077 §3: the header is the build's, not part of the rules a composed sheet takes.
    #[test]
    fn a_composed_sheet_takes_the_rules_without_the_licence_header() {
        assert_eq!(
            without_header("/*! @license GPL-3.0-or-later. x */\n@font-face{a:b}"),
            "@font-face{a:b}"
        );
        assert_eq!(without_header("@font-face{a:b}"), "@font-face{a:b}");
        assert_eq!(without_header("/* note */a{}"), "/* note */a{}");
    }

    #[test]
    fn content_types_and_refusals() {
        assert_eq!(content_type("site-abc.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("x.woff2"), "font/woff2");
        assert!(get("manifest.json").is_none());
        assert!(get("../Cargo.toml").is_none());
        assert!(build_id().starts_with(env!("CARGO_PKG_VERSION")));
    }
}
