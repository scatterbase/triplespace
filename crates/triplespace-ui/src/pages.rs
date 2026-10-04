//! The site's handlers, and the caching a page composes from the API responses it used
//! (0057 §6).
//!
//! A page is `public` only if every API response it drew on was; its `ETag` is a hash of
//! the build, the interface language, the status and those responses' `ETag`s, so a
//! revalidation costs the API calls but not the rendering; its `Cache-Tag`s are theirs.
//! It varies on `Cookie` and `Accept-Language`, the two inputs a shared cache can see.
//!
//! When the API cannot be reached, or is older than [`MIN_API_VERSION`], the site says so
//! with a `503` and `Retry-After`, in the frame and the shipped theme, and keeps nothing.
//!
//! Phase 0 serves the frame only: every title shows the frame and a notice that it is not
//! served yet.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::Extension;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest as _, Sha256};
use triplespace_client::{CacheInfo, ClientError, Incoming, SiteInfo};

use crate::codex::{Message, MessageKind};
use crate::frame::{self, Page, Tab, title_url};
use crate::i18n::Messages;
use crate::routes::{self, Target};
use crate::theme::Theme;
use crate::{MIN_API_VERSION, Site, assets};

/// The connection address, when the server was started with `ConnectInfo`.
type Peer = Option<Extension<ConnectInfo<SocketAddr>>>;

/// How long a shared cache keeps a public page.
const S_MAXAGE: u32 = 60;

/// How long a client waits before trying again after a `503`.
const RETRY_AFTER: &str = "30";

/// The Content Security Policy of every response the site makes (0034 §8; 0057 §7): the
/// site's own origin only, no inline script or style.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
    img-src 'self' data:; font-src 'self'; object-src 'none'; base-uri 'self'; \
    frame-ancestors 'self'; form-action 'self'";

/// Adds the security headers to every response.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut r = next.run(request).await;
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    r
}

/// Refuses the API's paths: the site never answers for them, even where it is the
/// fallback of the API's own router (0057 §3).
pub async fn api_paths_refused(request: Request, next: Next) -> Response {
    if routes::target(request.uri().path()) == Target::Api {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Not found\n",
        )
            .into_response();
    }
    next.run(request).await
}

/// What every framed page needs from the API.
struct Context {
    site: SiteInfo,
    user: Option<String>,
    inputs: Vec<CacheInfo>,
    m: Messages,
    theme: Theme,
}

fn incoming(headers: &HeaderMap, peer: Peer) -> Incoming {
    Incoming::new(headers, peer.map(|Extension(ConnectInfo(a))| a))
}

/// Asks the API who the site is and who the viewer is. A request without credentials is
/// anonymous whatever the API would say, so it is spared the second call.
async fn context(site: &Site, incoming: &Incoming) -> Result<Context, Response> {
    let client = site.client();
    let (si, ui) = if incoming.credentialed() {
        let (si, ui) = tokio::join!(client.siteinfo(incoming), client.userinfo(incoming));
        (si, Some(ui))
    } else {
        (client.siteinfo(incoming).await, None)
    };
    let si = si.map_err(|e| unavailable(incoming, &e, "ts-api-unavailable"))?;
    if si.value.api_version < MIN_API_VERSION {
        return Err(unavailable(
            incoming,
            &ClientError::Unexpected(format!(
                "the API's api_version is {}; this site needs {MIN_API_VERSION}",
                si.value.api_version
            )),
            "ts-api-too-old",
        ));
    }
    let mut inputs = vec![si.cache.clone()];
    let user = match ui {
        Some(ui) => {
            let ui = ui.map_err(|e| unavailable(incoming, &e, "ts-api-unavailable"))?;
            inputs.push(ui.cache);
            (!ui.value.anon).then_some(ui.value.name)
        }
        None => None,
    };
    let m = Messages::for_language(&si.value.lang);
    let theme = Theme::with_overrides(si.value.theme.as_ref());
    Ok(Context {
        site: si.value,
        user,
        inputs,
        m,
        theme,
    })
}

/// The `503` page: the frame, the shipped theme, and the reason.
fn unavailable(incoming: &Incoming, error: &ClientError, key: &str) -> Response {
    eprintln!("triplespace-ui: the API is unavailable: {error}");
    let m = Messages::for_language("en");
    let sitename = incoming.host().unwrap_or("Triplespace").to_string();
    let text = m.get(key);
    let page = Page {
        title: m.get("ts-api-unavailable-title"),
        body: message(MessageKind::Error, &text),
        ..Page::default()
    };
    let html = match frame::render(m, &sitename, Theme::shipped(), None, &page) {
        Ok(h) => h,
        Err(e) => return render_failed(&e),
    };
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::RETRY_AFTER, RETRY_AFTER),
        ],
        html,
    )
        .into_response()
}

fn render_failed(e: &askama::Error) -> Response {
    eprintln!("triplespace-ui: a template failed to render: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error\n").into_response()
}

fn message(kind: MessageKind, text: &str) -> String {
    use askama::Template as _;
    Message::block(kind, text).render().unwrap_or_default()
}

/// The composed `ETag`, if every input had one.
fn composed_etag(m: Messages, status: StatusCode, inputs: &[CacheInfo]) -> Option<String> {
    let mut h = Sha256::new();
    for part in [assets::build_id(), m.lang(), status.as_str()] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    for i in inputs {
        h.update(i.etag.as_deref()?.as_bytes());
        h.update([0]);
    }
    Some(format!("\"{}\"", crate::hex(&h.finalize()[..16])))
}

/// Whether `If-None-Match` names the `ETag`.
fn not_modified(request: &HeaderMap, etag: &str) -> bool {
    let want = etag.trim_start_matches("W/");
    request
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|t| t == "*" || t.trim_start_matches("W/") == want)
}

/// The caching headers of a page drawn from `inputs`.
fn caching(h: &mut HeaderMap, etag: Option<&str>, inputs: &[CacheInfo]) {
    let public = inputs.iter().all(|i| i.public);
    let cc = if public {
        format!("public, max-age=0, s-maxage={S_MAXAGE}, stale-while-revalidate=300")
    } else {
        "private, no-cache".to_string()
    };
    if let Ok(v) = HeaderValue::from_str(&cc) {
        h.insert(header::CACHE_CONTROL, v);
    }
    if let Some(v) = etag.and_then(|e| HeaderValue::from_str(e).ok()) {
        h.insert(header::ETAG, v);
    }
    h.insert(
        header::VARY,
        HeaderValue::from_static("Cookie, Accept-Language"),
    );
    if public {
        let mut tags: Vec<&str> = inputs
            .iter()
            .flat_map(|i| i.tags.iter().map(String::as_str))
            .collect();
        tags.sort_unstable();
        tags.dedup();
        if !tags.is_empty()
            && let Ok(v) = HeaderValue::from_str(&tags.join(", "))
        {
            h.insert("cache-tag", v);
        }
    }
}

/// A page in the frame, with its caching.
async fn framed(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    status: StatusCode,
    build: impl FnOnce(Messages) -> Page,
) -> Response {
    let incoming = incoming(headers, peer);
    let cx = match context(site, &incoming).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let etag = composed_etag(cx.m, status, &cx.inputs);
    if status == StatusCode::OK
        && let Some(e) = &etag
        && not_modified(headers, e)
    {
        let mut r = Response::new(Body::empty());
        *r.status_mut() = StatusCode::NOT_MODIFIED;
        caching(r.headers_mut(), Some(e), &cx.inputs);
        return r;
    }
    let page = build(cx.m);
    let html = match frame::render(
        cx.m,
        &cx.site.sitename,
        &cx.theme,
        cx.user.as_deref(),
        &page,
    ) {
        Ok(h) => h,
        Err(e) => return render_failed(&e),
    };
    let mut r = Response::new(Body::from(html));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    caching(r.headers_mut(), etag.as_deref(), &cx.inputs);
    r
}

/// A title from a URL: underscores as spaces; the main page when empty.
fn display_title(raw: &str) -> String {
    let t = raw.replace('_', " ");
    let t = t.trim();
    if t.is_empty() {
        "Main Page".to_string()
    } else {
        t.to_string()
    }
}

/// The placeholder page of Phase 0 for a title.
fn placeholder(m: Messages, title: String, query: &BTreeMap<String, String>) -> Page {
    let special = title.starts_with("Special:");
    let history = query.get("action").is_some_and(|a| a == "history");
    let tabs = if special {
        Vec::new()
    } else {
        let url = title_url(&title);
        vec![
            Tab {
                label: m.get("ts-tab-read"),
                href: format!("/wiki/{url}"),
                current: !history,
            },
            Tab {
                label: m.get("ts-tab-history"),
                href: format!("/w/index.php?title={url}&action=history"),
                current: history,
            },
        ]
    };
    Page {
        returnto: Some(title.clone()),
        title,
        tabs,
        body: message(MessageKind::Notice, &m.get("ts-not-served")),
        search: query.get("search").cloned().unwrap_or_default(),
        ..Page::default()
    }
}

/// `/`: the main page.
pub async fn root() -> Response {
    (StatusCode::FOUND, [(header::LOCATION, "/wiki/Main_Page")]).into_response()
}

/// `/wiki/{title}`.
pub async fn wiki(
    State(site): State<Site>,
    Path(title): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let title = display_title(&title);
    framed(&site, &headers, peer, StatusCode::OK, |m| {
        placeholder(m, title, &query)
    })
    .await
}

/// `/w/index.php` and `/index.php`: the title in `title=`.
pub async fn index_php(
    State(site): State<Site>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let title = display_title(query.get("title").map_or("", String::as_str));
    framed(&site, &headers, peer, StatusCode::OK, |m| {
        placeholder(m, title, &query)
    })
    .await
}

/// Everything else: the site's own 404.
pub async fn not_found(State(site): State<Site>, headers: HeaderMap, peer: Peer) -> Response {
    framed(&site, &headers, peer, StatusCode::NOT_FOUND, |m| Page {
        title: m.get("ts-not-found-title"),
        body: message(MessageKind::Warning, &m.get("ts-not-found")),
        ..Page::default()
    })
    .await
}

/// `/ui/assets/{path}`: a file of the build, kept for a year, since its name is its hash.
pub async fn asset(Path(path): Path<String>) -> Response {
    match assets::get(&path) {
        Some((data, content_type)) => (
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            data.into_owned(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Not found\n").into_response(),
    }
}

/// `/ui/theme/{hash}.css`: the tenant's theme. Kept for a year when the name is the hash
/// of the theme it serves; a stale name gets the current theme for a minute, so that a
/// page cached across a theme change still looks right.
pub async fn theme_css(
    State(site): State<Site>,
    Path(file): Path<String>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let incoming = incoming(&headers, peer);
    let theme = match site.client().siteinfo(&incoming).await {
        Ok(si) => Theme::with_overrides(si.value.theme.as_ref()),
        Err(e) => {
            eprintln!("triplespace-ui: the API is unavailable: {e}");
            Theme::shipped().clone()
        }
    };
    let cache = if file == format!("{}.css", theme.hash()) {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=60"
    };
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, cache),
        ],
        theme.css(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(public: bool, etag: Option<&str>, tags: &[&str]) -> CacheInfo {
        CacheInfo {
            public,
            etag: etag.map(str::to_string),
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
        }
    }

    #[test]
    fn a_page_is_public_only_if_every_input_was() {
        let mut h = HeaderMap::new();
        caching(
            &mut h,
            Some("\"x\""),
            &[
                info(true, Some("\"a\""), &["entity:Q2"]),
                info(true, Some("\"b\""), &["entity:Q1", "entity:Q2"]),
            ],
        );
        assert!(
            h[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .starts_with("public")
        );
        assert_eq!(h["cache-tag"], "entity:Q1, entity:Q2");
        assert_eq!(h[header::VARY], "Cookie, Accept-Language");
        let mut h = HeaderMap::new();
        caching(
            &mut h,
            None,
            &[info(true, None, &["entity:Q2"]), info(false, None, &[])],
        );
        assert_eq!(h[header::CACHE_CONTROL], "private, no-cache");
        assert!(h.get("cache-tag").is_none());
        assert!(h.get(header::ETAG).is_none());
    }

    #[test]
    fn the_composed_etag_needs_every_input_etag() {
        let m = Messages::for_language("en");
        let a = composed_etag(m, StatusCode::OK, &[info(true, Some("\"a\""), &[])]).unwrap();
        let b = composed_etag(m, StatusCode::OK, &[info(true, Some("\"b\""), &[])]).unwrap();
        let n = composed_etag(m, StatusCode::NOT_FOUND, &[info(true, Some("\"a\""), &[])]).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, n);
        assert!(composed_etag(m, StatusCode::OK, &[info(true, None, &[])]).is_none());
        let mut req = HeaderMap::new();
        req.insert(header::IF_NONE_MATCH, format!("W/{a}").parse().unwrap());
        assert!(not_modified(&req, &a));
        assert!(!not_modified(&req, &b));
    }

    #[test]
    fn titles() {
        assert_eq!(display_title("Main_Page"), "Main Page");
        assert_eq!(display_title(""), "Main Page");
        let m = Messages::for_language("en");
        let q: BTreeMap<String, String> = [("action".to_string(), "history".to_string())].into();
        let p = placeholder(m, "Item:Q6".into(), &q);
        assert_eq!(p.tabs.len(), 2);
        assert!(p.tabs[1].current && !p.tabs[0].current);
        assert_eq!(p.tabs[1].href, "/w/index.php?title=Item:Q6&action=history");
        assert!(
            placeholder(m, "Special:Search".into(), &BTreeMap::new())
                .tabs
                .is_empty()
        );
    }
}
