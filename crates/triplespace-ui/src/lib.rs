//! The site (0034; 0057): pages rendered on the server from the public API, in Codex's
//! CSS-only markup, readable without JavaScript.
//!
//! One router serves the site wherever it runs: from `triplespace-web` over an HTTP
//! [`triplespace_client::HttpTransport`], or from `triplespace-server` (`server.ui =
//! embedded`) over an in-process [`triplespace_client::ServiceTransport`] into the API's
//! own router (0057 §1.4, §2). The site holds no state, reaches no store and depends on
//! no API crate (0005 §3 rule 10).
//!
//! This crate is the router over the five crates the rendering is split into (0113 §1):
//! `triplespace-ui-kit`, what every page draws and answers with; `triplespace-ui-pages`,
//! the page kinds a title names; and the special pages in `triplespace-ui-accounts`,
//! `triplespace-ui-admin` and `triplespace-ui-special`. It is the one crate that names
//! them all, and the one the binaries depend on.
//!
//! - [`routes`]: which paths the site serves and which the API does, the one list the
//!   edge proxy's configuration and the embedded mode both come from (0057 §3).
//! - [`dispatch`]: which page kind serves a title.
//! - [`files`]: the built frontend, the skins' files and the theme's stylesheets.
//! - [`parity`]: two renderings of a page compared as a browser reads them, for the
//!   check that Worksheet is Plain with its stylesheets (0097 §16, A12).
//! - [`Site`] and the kit's modules the binaries and the tests use, re-exported.

#![forbid(unsafe_code)]

pub mod dispatch;
pub mod files;
pub mod parity;
pub mod routes;

pub use triplespace_ui_kit::{
    MIN_API_VERSION, Site, assets, codex, controls, frame, i18n, inline, pages, skin, theme,
};
pub use triplespace_ui_pages::entity;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use triplespace_client::Client;

/// The site's router: every path of [`routes::TABLE`] the site serves, with its own 404
/// page for everything else but the API's paths, which it refuses with a plain 404, so
/// that it can be the fallback of the API's router (0057 §3).
pub fn router(client: Client) -> Router {
    router_for(Site::new(client))
}

/// The router of a separate web tier: [`router`], with the tier's own build for
/// `Special:Version` (`version_json`, its binary's `version.json`).
pub fn router_with_build(client: Client, version_json: &str) -> Router {
    router_for(Site::new(client).with_build(version_json))
}

/// The most a posted form may be: `Special:Import`'s upload, bounded by the wiki's
/// `import.max_upload`, whose ceiling this is (0047 A36).
pub const BODY_LIMIT: usize = 1024 * 1024 * 1024;

/// The router over a site configured by hand: the skins directory and the build.
pub fn router_for(site: Site) -> Router {
    Router::new()
        .route("/", get(dispatch::root))
        .route("/wiki/", get(dispatch::root))
        .route(
            "/wiki/{*title}",
            get(dispatch::wiki)
                .post(dispatch::wiki_post)
                .layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route(
            "/w/index.php",
            get(dispatch::index_php)
                .post(dispatch::index_post)
                .layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route(
            "/index.php",
            get(dispatch::index_php)
                .post(dispatch::index_post)
                .layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route("/ui/assets/{*path}", get(files::asset))
        .route("/ui/skins/{skin}/{hash}/{*path}", get(files::skin_asset))
        .route("/ui/theme/{file}", get(files::theme_css))
        .fallback(dispatch::not_found)
        .with_state(site.clone())
        .layer(axum::middleware::from_fn(dispatch::api_paths_refused))
        .layer(axum::middleware::from_fn_with_state(
            site,
            pages::security_headers,
        ))
}
