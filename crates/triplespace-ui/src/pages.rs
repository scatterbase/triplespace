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
//! Entity titles are served by [`crate::entity`]; every other title shows the frame and a
//! notice that it is not served yet.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::Extension;
use axum::body::Body;
use axum::extract::{ConnectInfo, Form, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest as _, Sha256};
use triplespace_client::{CacheInfo, ClientError, Incoming, SiteInfo};

use crate::codex::{Message, MessageKind};
use crate::features::Features;
use crate::frame::{self, Chrome, Page, title_url};
use crate::i18n::{self, Messages};
use crate::routes::{self, Target};
use crate::theme::Theme;
use crate::{MIN_API_VERSION, Site, assets};
use crate::{entity, home, special};

/// The connection address, when the server was started with `ConnectInfo`.
pub type Peer = Option<Extension<ConnectInfo<SocketAddr>>>;

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

/// What every framed page needs from the API, and the caching facts of each response a
/// page draws on; a page adds its own.
pub struct Context {
    /// The site.
    pub site: SiteInfo,
    /// The signed-in viewer's name.
    pub user: Option<String>,
    /// The caching facts of every API response the page used.
    pub inputs: Vec<CacheInfo>,
    /// The interface messages.
    pub m: Messages,
    /// The theme.
    pub theme: Theme,
    /// What the site offers.
    pub features: Features,
}

impl Context {
    /// What the frame needs of the site.
    #[must_use]
    pub fn chrome(&self) -> Chrome<'_> {
        Chrome {
            sitename: &self.site.sitename,
            mainpage: &self.site.mainpage,
            features: &self.features,
        }
    }
}

/// The browser's request, as the client forwards it.
pub fn incoming(headers: &HeaderMap, peer: Peer) -> Incoming {
    Incoming::new(headers, peer.map(|Extension(ConnectInfo(a))| a))
}

/// Asks the API who the site is and who the viewer is. A request without credentials is
/// anonymous whatever the API would say, so it is spared the second call. The interface
/// language is `uselang` where given and valid, else the site's (0034 §9).
///
/// # Errors
///
/// The `503` page when the API cannot be reached or is too old.
pub async fn context(
    site: &Site,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
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
            if !ui.value.anon {
                // A page drawn for a signed-in viewer is theirs alone, whatever its
                // inputs say: it may hold their name, their tokens or their controls.
                inputs.push(CacheInfo::default());
            }
            (!ui.value.anon).then_some(ui.value.name)
        }
        None => None,
    };
    let lang = query
        .get("uselang")
        .and_then(|l| i18n::valid_code(l))
        .unwrap_or_else(|| si.value.lang.clone());
    let m = Messages::for_language(&lang);
    let theme = Theme::with_overrides(si.value.theme.as_ref()).with_chips(&si.value.providers);
    let features = Features::of(&si.value);
    Ok(Context {
        site: si.value,
        user,
        inputs,
        m,
        theme,
        features,
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
    let none = Features::default();
    let chrome = Chrome {
        sitename: &sitename,
        mainpage: DEFAULT_MAINPAGE,
        features: &none,
    };
    let html = match frame::render(&m, &chrome, Theme::shipped(), None, &page) {
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

/// A call a page needed failed after the frame's had succeeded: the API is away (`503`),
/// refused the viewer (`403`), or answered something the site cannot read (`502`). The
/// page says which, in the frame, and keeps nothing.
pub fn api_failed(cx: &Context, headers: &HeaderMap, error: &ClientError) -> Response {
    eprintln!("triplespace-ui: an API call failed: {error}");
    let (status, key) = match error {
        ClientError::Unreachable(_) => (StatusCode::SERVICE_UNAVAILABLE, "ts-api-unavailable"),
        ClientError::Api { code, .. } if code == "permissiondenied" || code == "readapidenied" => {
            (StatusCode::FORBIDDEN, "ts-permission-denied")
        }
        _ => (StatusCode::BAD_GATEWAY, "ts-api-unexpected"),
    };
    let mut r = respond(cx, headers, status, |m| Page {
        title: m.get("ts-api-unavailable-title"),
        body: message(MessageKind::Error, &m.get(key)),
        ..Page::default()
    });
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.remove(header::ETAG);
    h.remove("cache-tag");
    if status == StatusCode::SERVICE_UNAVAILABLE {
        h.insert(header::RETRY_AFTER, HeaderValue::from_static(RETRY_AFTER));
    }
    r
}

fn render_failed(e: &askama::Error) -> Response {
    eprintln!("triplespace-ui: a template failed to render: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error\n").into_response()
}

/// A Codex message, as HTML.
pub fn message(kind: MessageKind, text: &str) -> String {
    use askama::Template as _;
    Message::block(kind, text).render().unwrap_or_default()
}

/// The composed `ETag`, if every input had one.
fn composed_etag(m: &Messages, status: StatusCode, inputs: &[CacheInfo]) -> Option<String> {
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
pub(crate) fn caching(h: &mut HeaderMap, etag: Option<&str>, inputs: &[CacheInfo]) {
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

/// A `304` for a client that holds the response already, when the status allows one.
fn revalidated(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
) -> (Option<String>, Option<Response>) {
    let etag = composed_etag(&cx.m, status, &cx.inputs);
    if status == StatusCode::OK
        && let Some(e) = &etag
        && not_modified(headers, e)
    {
        let mut r = Response::new(Body::empty());
        *r.status_mut() = StatusCode::NOT_MODIFIED;
        caching(r.headers_mut(), Some(e), &cx.inputs);
        return (etag, Some(r));
    }
    (etag, None)
}

/// A page in the frame, with the caching its inputs allow: a `304` before rendering when
/// the client holds it.
pub fn respond(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    build: impl FnOnce(&Messages) -> Page,
) -> Response {
    let (etag, fresh) = revalidated(cx, headers, status);
    if let Some(r) = fresh {
        return r;
    }
    let page = build(&cx.m);
    let html = match frame::render(&cx.m, &cx.chrome(), &cx.theme, cx.user.as_deref(), &page) {
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

/// A region for `action=render` (0057 §8): its HTML alone, with the caching of the page
/// it belongs to.
pub fn fragment(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    build: impl FnOnce() -> String,
) -> Response {
    let (etag, fresh) = revalidated(cx, headers, status);
    if let Some(r) = fresh {
        return r;
    }
    let mut r = Response::new(Body::from(build()));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    caching(r.headers_mut(), etag.as_deref(), &cx.inputs);
    r
}

/// A component built for another release asked for a region: `409` with no body, so it
/// reloads the page instead of swapping in markup its styles do not match (0057 §8).
#[must_use]
pub fn build_skew() -> Response {
    (StatusCode::CONFLICT, [(header::CACHE_CONTROL, "no-store")]).into_response()
}

/// `400` for a region no page has, MediaWiki-shaped.
#[must_use]
pub fn unknown_region(region: &str) -> Response {
    let body = serde_json::json!({"error": {
        "code": "badregion",
        "info": format!("Unrecognized value for parameter \"region\": {region}."),
    }})
    .to_string();
    (
        StatusCode::BAD_REQUEST,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

/// The main page's title when the API does not say (a stock Wikibase says `Main Page`).
pub const DEFAULT_MAINPAGE: &str = "Project:Home";

/// A title from a URL: underscores as spaces; empty for the main page.
fn display_title(raw: &str) -> String {
    raw.replace('_', " ").trim().to_string()
}

/// A title with its first letter upper-cased, as MediaWiki's titles have it.
fn first_upper(t: &str) -> String {
    let mut c = t.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// A title with the project namespace written canonically: `librarybase:Home` and
/// `project:home` are `Project:Home`.
fn canonical(title: &str, sitename: &str) -> String {
    let t = title.replace('_', " ");
    if let Some((ns, rest)) = t.split_once(':')
        && (ns.trim().eq_ignore_ascii_case("project") || ns.trim().eq_ignore_ascii_case(sitename))
    {
        return format!("Project:{}", first_upper(rest.trim()));
    }
    first_upper(t.trim())
}

/// Whether a title is the main page's.
#[must_use]
pub fn is_mainpage(title: &str, site: &SiteInfo) -> bool {
    canonical(title, &site.sitename) == canonical(&site.mainpage, &site.sitename)
}

/// A `302` to a page, cached as its inputs allow.
pub fn redirect_to(cx: &Context, title: &str) -> Response {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::FOUND;
    if let Ok(v) = HeaderValue::from_str(&format!("/wiki/{}", title_url(title))) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    caching(r.headers_mut(), None, &cx.inputs);
    r
}

/// The `404` for a title the site has no page for: a special page it does not serve, or
/// any other title, since the site has no document pages yet (0008).
pub(crate) fn no_page(cx: &Context, headers: &HeaderMap, title: String) -> Response {
    let special = title.starts_with("Special:");
    if cx.site.mainpage != "Main Page" && title.eq_ignore_ascii_case("Main Page") {
        return redirect_to(cx, &cx.site.mainpage.clone());
    }
    respond(cx, headers, StatusCode::NOT_FOUND, |m| {
        let text = if special {
            m.with("ts-no-special-page", &[&title])
        } else {
            m.with("ts-no-page", &[&title])
        };
        Page {
            returnto: Some(title.clone()),
            title,
            body: message(MessageKind::Notice, &text),
            ..Page::default()
        }
    })
}

/// A title: an entity's page, a special page, the main page, or the `404`.
async fn dispatch(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: String,
    query: &BTreeMap<String, String>,
) -> Response {
    if entity::is_entity_title(&title) {
        return entity::serve(site, headers, peer, title, query).await;
    }
    if let Some(r) = special::serve(site, headers, peer, &title, query).await {
        return r;
    }
    let incoming = incoming(headers, peer);
    let cx = match context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if title.is_empty() {
        return redirect_to(&cx, &cx.site.mainpage);
    }
    if is_mainpage(&title, &cx.site) {
        if query.get("action").is_some_and(|a| a == "render") {
            return fragment(&cx, headers, StatusCode::OK, || home::body(&cx));
        }
        return respond(&cx, headers, StatusCode::OK, |_| home::page(&cx));
    }
    if query.get("action").is_some_and(|a| a == "render") {
        return fragment(&cx, headers, StatusCode::NOT_FOUND, String::new);
    }
    no_page(&cx, headers, title)
}

/// Relays the API's `Set-Cookie` headers on a response, which is then the viewer's alone
/// and kept by no one (0057 §4, §6).
#[must_use]
pub fn with_cookies(mut r: Response, cookies: &[HeaderValue]) -> Response {
    if cookies.is_empty() {
        return r;
    }
    let h = r.headers_mut();
    for c in cookies {
        h.append(header::SET_COOKIE, c.clone());
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    h.remove("cache-tag");
    h.remove(header::ETAG);
    r
}

/// A `303` after a form: on to `href`, with the cookies the API set.
#[must_use]
pub fn see_other(href: &str, cookies: &[HeaderValue]) -> Response {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::SEE_OTHER;
    if let Ok(v) = HeaderValue::from_str(href) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    with_cookies(r, cookies)
}

/// A form posted to a title: a special page that takes one, or `405`.
async fn dispatch_post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    if let Some(r) = special::post(site, headers, peer, title, query, form).await {
        return r;
    }
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [
            (header::ALLOW, "GET, HEAD"),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
        .into_response()
}

/// `POST /wiki/{title}`.
pub async fn wiki_post(
    State(site): State<Site>,
    Path(title): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
    Form(form): Form<BTreeMap<String, String>>,
) -> Response {
    dispatch_post(&site, &headers, peer, &display_title(&title), &query, &form).await
}

/// `POST /w/index.php` and `/index.php`: the title in `title=`.
pub async fn index_post(
    State(site): State<Site>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
    Form(form): Form<BTreeMap<String, String>>,
) -> Response {
    let title = display_title(
        query
            .get("title")
            .or_else(|| form.get("title"))
            .map_or("", String::as_str),
    );
    dispatch_post(&site, &headers, peer, &title, &query, &form).await
}

/// `/`: a redirect to the main page.
pub async fn root(State(site): State<Site>, headers: HeaderMap, peer: Peer) -> Response {
    dispatch(&site, &headers, peer, String::new(), &BTreeMap::new()).await
}

/// `/wiki/{title}`.
pub async fn wiki(
    State(site): State<Site>,
    Path(title): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    dispatch(&site, &headers, peer, display_title(&title), &query).await
}

/// `/w/index.php` and `/index.php`: the title in `title=`.
pub async fn index_php(
    State(site): State<Site>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let title = display_title(query.get("title").map_or("", String::as_str));
    dispatch(&site, &headers, peer, title, &query).await
}

/// Everything else: the site's own 404.
pub async fn not_found(State(site): State<Site>, headers: HeaderMap, peer: Peer) -> Response {
    let incoming = incoming(&headers, peer);
    let cx = match context(&site, &incoming, &BTreeMap::new()).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    respond(&cx, &headers, StatusCode::NOT_FOUND, |m| Page {
        title: m.get("ts-not-found-title"),
        body: message(MessageKind::Warning, &m.get("ts-not-found")),
        ..Page::default()
    })
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
        Ok(si) => Theme::with_overrides(si.value.theme.as_ref()).with_chips(&si.value.providers),
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
        let a = composed_etag(&m, StatusCode::OK, &[info(true, Some("\"a\""), &[])]).unwrap();
        let b = composed_etag(&m, StatusCode::OK, &[info(true, Some("\"b\""), &[])]).unwrap();
        let n =
            composed_etag(&m, StatusCode::NOT_FOUND, &[info(true, Some("\"a\""), &[])]).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, n);
        assert!(composed_etag(&m, StatusCode::OK, &[info(true, None, &[])]).is_none());
        let mut req = HeaderMap::new();
        req.insert(header::IF_NONE_MATCH, format!("W/{a}").parse().unwrap());
        assert!(not_modified(&req, &a));
        assert!(!not_modified(&req, &b));
    }

    #[test]
    fn titles() {
        assert_eq!(display_title("Main_Page"), "Main Page");
        assert_eq!(display_title(""), "");
        let site = SiteInfo {
            sitename: "librarybase".into(),
            mainpage: "Project:Home".into(),
            ..SiteInfo::default()
        };
        for t in [
            "Project:Home",
            "project:home",
            "librarybase:Home",
            "Project:_Home",
        ] {
            assert!(is_mainpage(t, &site), "{t}");
        }
        assert!(!is_mainpage("Home", &site));
        assert!(!is_mainpage("Main Page", &site));
    }
}
