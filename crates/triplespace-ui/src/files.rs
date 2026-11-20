//! The site's own files: the built frontend, a skin's files and the theme's stylesheets,
//! each under a name that is the hash of what it serves (0034 §8; 0086 §5; 19 §5.10).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use triplespace_ui_kit::pages::{Peer, incoming};
use triplespace_ui_kit::theme::Theme;
use triplespace_ui_kit::{Site, assets};

/// `/ui/assets/{path}`: a file of the build, kept for a year, since its name is its hash.
///
/// Under a dev server (0034 §8) the request is passed through to it, query and all, and
/// only what it does not have comes from the build.
pub async fn asset(
    State(site): State<Site>,
    Path(path): Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    headers: HeaderMap,
) -> Response {
    if let Some(dev) = site.dev_assets()
        && !path.contains("..")
        && let Some(r) = dev.fetch(&path, query.as_deref(), &headers).await
    {
        return r;
    }
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

/// `/ui/skins/{skin}/{hash}/{path}`: a file of a loaded skin, content-hashed in its URL
/// (19 §5.10). A stale hash gets the current file for a minute, as the theme's
/// stylesheets do.
pub async fn skin_asset(
    State(site): State<Site>,
    Path((skin, hash, path)): Path<(String, String, String)>,
) -> Response {
    let Some(s) = site.skins().get(&skin) else {
        return (StatusCode::NOT_FOUND, "Not found\n").into_response();
    };
    let Some(data) = s.file(&path) else {
        return (StatusCode::NOT_FOUND, "Not found\n").into_response();
    };
    let cache = if hash == s.hash() {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=60"
    };
    (
        [
            (header::CONTENT_TYPE, assets::content_type(&path)),
            (header::CACHE_CONTROL, cache),
        ],
        data.to_vec(),
    )
        .into_response()
}

/// `/ui/theme/{p|t}-{name}-{hash}.css`: a palette's or a type set's stylesheet (0086 §5).
/// Kept for a year when the name is the hash of the text served; a stale name gets the
/// current text for a minute, so that a page cached across a change still looks right.
/// A name from before palettes gets the wiki's palette, briefly.
pub async fn theme_css(
    State(site): State<Site>,
    Path(file): Path<String>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let incoming = incoming(&headers, peer);
    let theme = match site.client().siteinfo(&incoming).await {
        Ok(si) => Theme::from_info(si.value.theme.as_ref()).with_chips(&si.value.providers),
        Err(e) => {
            eprintln!("triplespace-ui: the API is unavailable: {e}");
            Theme::shipped()
        }
    };
    let Some((css, current)) = theme.file(&file) else {
        return (StatusCode::NOT_FOUND, "Not found\n").into_response();
    };
    let cache = if current {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=60"
    };
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, cache),
        ],
        css,
    )
        .into_response()
}
