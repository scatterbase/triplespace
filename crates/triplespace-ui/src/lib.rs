//! The site (0034; 0057): pages rendered on the server from the public API, in Codex's
//! CSS-only markup, readable without JavaScript.
//!
//! One router serves the site wherever it runs: from `triplespace-web` over an HTTP
//! [`triplespace_client::HttpTransport`], or from `triplespace-server` (`server.ui =
//! embedded`) over an in-process [`triplespace_client::ServiceTransport`] into the API's
//! own router (0057 §1.4, §2). The site holds no state, reaches no store and depends on
//! no API crate (0005 §3 rule 10).
//!
//! - [`routes`]: which paths the site serves and which the API does, the one list the
//!   edge proxy's configuration and the embedded mode both come from (0057 §3).
//! - [`pages`]: the handlers, and the caching a page composes from the API responses it
//!   used (0057 §6).
//! - [`entity`]: entity pages and their `action=render` regions (0010 §2; 0057 §8).
//! - [`special`]: the special pages served so far (0047), starting with `Special:Search`.
//! - [`home`]: the main page, `Project:Home`, generated until a page of that title exists.
//! - [`features`]: what the site offers, from what it has built and what the API serves;
//!   nothing links to a feature that is not offered.
//! - [`frame`]: the page frame of 0010 §2, with the [`codex`] builder's components.
//! - [`theme`]: the shipped default theme and a tenant's overrides, as a stylesheet of
//!   Codex token values (0034 §1).
//! - [`assets`]: the built frontend (`ui/dist`), embedded, and its manifest.
//! - [`i18n`]: interface messages from `i18n/` (0034 §9).
//! - [`html`]: escaping, for the pieces too small for a template.

#![forbid(unsafe_code)]

pub mod assets;
pub mod codex;
pub mod entity;
pub mod features;
pub mod frame;
pub mod home;
pub mod html;
pub mod i18n;
pub mod pages;
pub mod routes;
pub mod special;
pub mod theme;

use axum::Router;
use axum::routing::get;
use triplespace_client::Client;

/// Lower-case hex of some bytes.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The lowest `api_version` this build of the site works with (0057 §9).
pub const MIN_API_VERSION: u32 = 1;

/// The site's state: its client of the API. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Site {
    client: Client,
}

impl Site {
    /// The site over a client.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// The client.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }
}

/// The site's router: every path of [`routes::TABLE`] the site serves, with its own 404
/// page for everything else but the API's paths, which it refuses with a plain 404, so
/// that it can be the fallback of the API's router (0057 §3).
pub fn router(client: Client) -> Router {
    Router::new()
        .route("/", get(pages::root))
        .route("/wiki/", get(pages::root))
        .route("/wiki/{*title}", get(pages::wiki).post(pages::wiki_post))
        .route(
            "/w/index.php",
            get(pages::index_php).post(pages::index_post),
        )
        .route("/index.php", get(pages::index_php).post(pages::index_post))
        .route("/ui/assets/{*path}", get(pages::asset))
        .route("/ui/theme/{file}", get(pages::theme_css))
        .fallback(pages::not_found)
        .with_state(Site::new(client))
        .layer(axum::middleware::from_fn(pages::api_paths_refused))
        .layer(axum::middleware::from_fn(pages::security_headers))
}
