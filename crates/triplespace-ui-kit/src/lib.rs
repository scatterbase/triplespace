//! The site's rendering kit (0113 §1): what every page draws and answers with, and the
//! lowest of the crates `triplespace-ui` is assembled from.
//!
//! It names no page kind. The page kinds (`triplespace-ui-pages`), the special pages
//! (`triplespace-ui-accounts`, `-admin`, `-special`) and the router (`triplespace-ui`)
//! are built on it, and what two of them draw alike lives here. It reaches the instance
//! only through `triplespace-client` (0005 §3 rule 10).
//!
//! - [`Site`]: the site's state, its client of the API and the skins it has.
//! - [`pages`]: what every handler shares, and the caching a page composes from the API
//!   responses it used (0057 §6).
//! - [`frame`]: the page frame of 0010 §2, drawn by the skin, and [`codex`], the Codex
//!   components' models.
//! - [`skin`]: the skins (0097 §8), Plain and Worksheet embedded and the operator's
//!   installed, and the renderer that draws a page through one.
//! - [`theme`]: a stylesheet per palette and per type set (0086 §5), and [`appearance`],
//!   the Appearance menu and the reader's choices on `<html>` (0085 §6).
//! - [`assets`]: the built frontend (`ui/dist`), embedded, and its manifest.
//! - [`i18n`]: interface messages from `i18n/` (0034 §9).
//! - [`features`]: what the site offers, from what the API serves; nothing links to a
//!   feature that is not offered.
//! - [`controls`], [`choice`] and [`changelist`]: form controls from a type, links in a
//!   switch or a pager, and the rows of a list of changes.
//! - [`comment`], [`inline`], [`diffview`] and [`watch`]: parsed summaries, inline
//!   markup, a document's diff views and the watch star.
//! - [`values`] and [`titles`]: values as a form or a cell writes them, and titles read
//!   against the namespace registry.
//! - [`html`]: escaping, for the pieces too small for a template.

#![forbid(unsafe_code)]

pub mod appearance;
pub mod assets;
pub mod changelist;
pub mod choice;
pub mod codex;
pub mod comment;
pub mod controls;
pub mod diffview;
pub mod features;
pub mod frame;
pub mod html;
pub mod i18n;
pub mod inline;
pub mod pages;
pub mod skin;
pub mod theme;
pub mod titles;
pub mod values;
pub mod watch;

use triplespace_client::Client;

/// Lower-case hex of some bytes.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
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

/// The site's state: its client of the API, the skins it has, and, in the web tier, the
/// tier's own build. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Site {
    client: Client,
    build: Option<std::sync::Arc<triplespace_client::version::Build>>,
    skins: std::sync::Arc<skin::Skins>,
    dev_assets: Option<std::sync::Arc<assets::DevServer>>,
}

impl Site {
    /// The site over a client, with the shipped skins.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            build: None,
            skins: std::sync::Arc::new(skin::Skins::load(None)),
            dev_assets: None,
        }
    }

    /// The site taking its assets from a Vite dev server (`server.vite`, `web.vite`;
    /// 0034 §8), for development only: its pages link the sources, and its Content
    /// Security Policy admits the server's WebSocket and `<style>` elements.
    #[must_use]
    pub fn with_dev_assets(mut self, dev: Option<assets::DevServer>) -> Self {
        self.dev_assets = dev.map(std::sync::Arc::new);
        self
    }

    /// The dev server the assets come from, in development.
    #[must_use]
    pub fn dev_assets(&self) -> Option<&assets::DevServer> {
        self.dev_assets.as_deref()
    }

    /// The assets a page links: the dev server's sources, else the build's hashed files.
    #[must_use]
    pub fn entries(&self) -> &assets::Entries {
        match self.dev_assets.as_deref() {
            Some(dev) => dev.entries(),
            None => assets::entries(),
        }
    }

    /// The site with the skins installed under a directory beside the shipped ones
    /// (`server.skins`, `web.skins`), read now.
    #[must_use]
    pub fn with_skins_dir(mut self, dir: Option<&std::path::Path>) -> Self {
        self.skins = std::sync::Arc::new(skin::Skins::load(dir));
        for r in self.skins.refused() {
            eprintln!(
                "triplespace-ui: the skin `{}` ({}) is not loaded: {}",
                r.name, r.origin, r.reason
            );
        }
        self
    }

    /// The skins.
    #[must_use]
    pub fn skins(&self) -> &skin::Skins {
        &self.skins
    }

    /// The site of a separate web tier, whose own build `Special:Version` shows beside
    /// the API server's (0077 §1, §4): `version_json` is the binary's `version.json`.
    #[must_use]
    pub fn with_build(mut self, version_json: &str) -> Self {
        self.build = serde_json::from_str::<serde_json::Value>(version_json)
            .ok()
            .and_then(|v| serde_json::from_value(v["build"].clone()).ok())
            .map(std::sync::Arc::new);
        self
    }

    /// The client.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The web tier's own build, where the site runs in one.
    #[must_use]
    pub fn build(&self) -> Option<&triplespace_client::version::Build> {
        self.build.as_deref()
    }
}
