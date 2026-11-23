//! Which page kind serves a title (0057 §3): the handlers of `/`, `/wiki/{title}` and
//! `/w/index.php`, read and posted to.
//!
//! Entity titles are served by [`entity`], special pages by the three special-page crates,
//! each asked in turn (0113 §1), and every other title, where the API serves pages, by
//! [`discussion`] and [`document`]; without pages, the main page is the generated one
//! and every other title a `404`. This is the one module that names every page kind: the
//! page kinds call [`triplespace_ui_kit::pages`], never this. A title in a resolver
//! namespace, the registry's (`DOI:`) or an entity source's (05 §6.5), is served by
//! [`resolver`] when no `action` is asked for.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use triplespace_ui_kit::Site;
use triplespace_ui_kit::codex::MessageKind;
use triplespace_ui_kit::features::Feature;
use triplespace_ui_kit::frame::Page;
use triplespace_ui_kit::pages::{
    Params, Peer, context, display_title, fragment, incoming, is_mainpage, no_page, redirect_to,
    respond,
};
use triplespace_ui_kit::titles::special_name;
use triplespace_ui_kit::watch;
use triplespace_ui_pages::{discussion, document, entity, home, moderation, resolver};

use crate::routes::{self, Target};

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

/// Serves a special page: each special-page crate is asked in turn, and a name is served
/// by exactly one of them (implements 0113 §1). `None` for a special page the site does not serve.
async fn special_serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Option<Response> {
    if let Some(r) = triplespace_ui_accounts::serve(site, headers, peer, title, params).await {
        return Some(r);
    }
    if let Some(r) = triplespace_ui_admin::serve(site, headers, peer, title, params).await {
        return Some(r);
    }
    triplespace_ui_special::serve(site, headers, peer, title, params).await
}

/// Takes a form posted to a special page, asking each special-page crate in turn.
async fn special_post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Option<Response> {
    if let Some(r) = triplespace_ui_accounts::post(site, headers, peer, title, query, form).await {
        return Some(r);
    }
    if let Some(r) = triplespace_ui_admin::post(site, headers, peer, title, query, form).await {
        return Some(r);
    }
    triplespace_ui_special::post(site, headers, peer, title, query, form).await
}

/// A title: an entity's page, a special page, the main page, or the `404`.
async fn dispatch(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: String,
    params: &Params,
) -> Response {
    let query = &params.map;
    if let Some(form) = query.get("action").and_then(|a| moderation::Form::of(a))
        && !title.is_empty()
        && special_name(&title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        return moderation::serve(site, headers, &incoming, cx, form, title, params).await;
    }
    if entity::is_entity_title(&title) {
        return entity::serve(site, headers, peer, title, query).await;
    }
    if let Some(r) = special_serve(site, headers, peer, &title, params).await {
        return r;
    }
    let incoming = incoming(headers, peer);
    let cx = match context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    // An entity source's namespace, the tenant's own resolver namespace (05 §6.5).
    if !query.contains_key("action")
        && resolver::is_own_resolver_title(&title, &cx.site.own_namespaces)
    {
        return resolver::serve(site, headers, peer, &title, params).await;
    }
    if cx.features.has(Feature::Pages) {
        // A talk page or a thread (19 §6.1); every other title is a document page's.
        let cx = match discussion::serve(site, headers, &incoming, cx, &title, params).await {
            Ok(r) => return r,
            Err(cx) => cx,
        };
        return document::serve(site, headers, &incoming, cx, title, params).await;
    }
    if title.is_empty() {
        return redirect_to(&cx, &cx.site.mainpage);
    }
    if is_mainpage(&title, &cx.site) {
        if query.get("action").is_some_and(|a| a == "render") {
            return fragment(&cx, headers, StatusCode::OK, || home::content(&cx));
        }
        return respond(&cx, headers, StatusCode::OK, |_| home::page(&cx));
    }
    if query.get("action").is_some_and(|a| a == "render") {
        return fragment(&cx, headers, StatusCode::NOT_FOUND, String::new);
    }
    no_page(&cx, headers, title)
}

/// A form posted to a title: a document page's edit form, a special page that takes one,
/// or `405`.
async fn dispatch_post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let action = query.get("action").or_else(|| form.map.get("action"));
    if let Some(which) = action.and_then(|a| moderation::Form::of(a))
        && !title.is_empty()
        && special_name(title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        return moderation::post(site, headers, &incoming, cx, which, title.to_string(), form)
            .await;
    }
    if !title.is_empty()
        && special_name(title).is_none()
        && !entity::is_entity_title(title)
        && action.is_some_and(|a| a == "submit" || a == "edit")
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        if cx.features.has(Feature::Pages) {
            return document::post(site, headers, &incoming, cx, title, form).await;
        }
        return no_page(&cx, headers, title.to_string());
    }
    if let Some(a) = action.filter(|a| *a == "watch" || *a == "unwatch")
        && !title.is_empty()
        && special_name(title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        return watch::post(site, headers, &incoming, cx, title, a, form).await;
    }
    if let Some(a) = action.filter(|a| matches!(a.as_str(), "addtopic" | "reply" | "thread"))
        && !title.is_empty()
        && special_name(title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        if cx.user.is_none() || !cx.features.has(Feature::Pages) {
            return no_page(&cx, headers, title.to_string());
        }
        return match discussion::post(site, headers, &incoming, cx, title, a, form).await {
            Ok(r) => r,
            Err(cx) => no_page(&cx, headers, title.to_string()),
        };
    }
    if let Some(r) = special_post(site, headers, peer, title, query, form).await {
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
    body: axum::body::Bytes,
) -> Response {
    let form = match Params::posted(&headers, body).await {
        Ok(f) => f,
        Err(r) => return r,
    };
    dispatch_post(&site, &headers, peer, &display_title(&title), &query, &form).await
}

/// `POST /w/index.php` and `/index.php`: the title in `title=`.
pub async fn index_post(
    State(site): State<Site>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
    body: axum::body::Bytes,
) -> Response {
    let form = match Params::posted(&headers, body).await {
        Ok(f) => f,
        Err(r) => return r,
    };
    let title = display_title(
        query
            .get("title")
            .or_else(|| form.map.get("title"))
            .map_or("", String::as_str),
    );
    dispatch_post(&site, &headers, peer, &title, &query, &form).await
}

/// `/`: a redirect to the main page.
pub async fn root(State(site): State<Site>, headers: HeaderMap, peer: Peer) -> Response {
    dispatch(&site, &headers, peer, String::new(), &Params::default()).await
}

/// `/wiki/{title}`.
pub async fn wiki(
    State(site): State<Site>,
    Path(title): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let params = Params::new(query);
    if resolver::is_resolver_title(&title) && !params.map.contains_key("action") {
        return resolver::serve(&site, &headers, peer, &title, &params).await;
    }
    dispatch(&site, &headers, peer, display_title(&title), &params).await
}

/// `/w/index.php` and `/index.php`: the title in `title=`.
pub async fn index_php(
    State(site): State<Site>,
    Query(query): Query<Vec<(String, String)>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let params = Params::new(query);
    let raw = params.map.get("title").map_or("", String::as_str);
    if resolver::is_resolver_title(raw) && !params.map.contains_key("action") {
        let raw = raw.to_string();
        return resolver::serve(&site, &headers, peer, &raw, &params).await;
    }
    let title = display_title(raw);
    dispatch(&site, &headers, peer, title, &params).await
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
        body: cx.message(MessageKind::Warning, &m.get("ts-not-found")),
        ..Page::default()
    })
}
