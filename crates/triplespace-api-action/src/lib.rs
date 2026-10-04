//! The Action API (mediawiki-compat.md; wikibase-compat.md; 0012 §4): the modules a
//! Wikibase bot client needs to log in, read entities and save edits, served the way
//! MediaWiki serves them so that WikibaseIntegrator and Pywikibot work unchanged.
//!
//! - `GET|POST {script path}/api.php` — [`api::handle`]: `action=query` with
//!   `meta=siteinfo|tokens|userinfo`, `action=login` (bot passwords, 0024 §4),
//!   `action=clientlogin` (primary accounts, 0007 §3), `action=logout`, `wbgetentities`,
//!   `wbsearchentities`, and the write modules `wbeditentity`, `wbcreateclaim`,
//!   `wbsetclaim`, `wbremoveclaims`, `wbsetlabel`, `wbsetdescription`, `wbsetaliases`,
//!   `wbsetqualifier`, `wbremovequalifiers`, `wbsetreference`, `wbremovereferences`.
//! - `GET /wiki/Special:EntityData/{id}.json` and `GET /entity/{id}` (0001 §1; 0002 §4).
//!
//! Every request is served for the tenant its `Host` names (0056 §10): an unregistered
//! host is 421, unless the server runs in development mode with a fallback tenant.
//! Writes run the path of 0013 §7 as far as the milestone builds it: identity and
//! permissions ([`triplespace_accounts`]), the CSRF token, the base-offset check and the
//! append with inline projections ([`scatter_ingest::write`]). Rate limits, ACLs beyond
//! the `owner` group and edit filters are not yet in front of the append.

#![forbid(unsafe_code)]

pub mod api;
pub mod app;
pub mod auth;
pub mod edit;
pub mod entity_json;
pub mod forwarded;
pub mod modules;
pub mod params;
pub mod response;
pub mod tenant;

pub use app::{App, Config, Mode};
pub use response::ApiError;

use axum::Router;
use axum::routing::get;

/// The router: `api.php` at both MediaWiki paths, the entity routes, and a health check.
///
/// Every route runs behind [`forwarded::layer`], which works out the client behind
/// trusted proxies. The server must be served with
/// `into_make_service_with_connect_info::<SocketAddr>()` for the peer address to be known.
pub fn router(app: App) -> Router {
    Router::new()
        .route("/api.php", get(api::handle).post(api::handle))
        .route("/w/api.php", get(api::handle).post(api::handle))
        .route(
            "/wiki/Special:EntityData/{file}",
            get(modules::entitydata::special_entity_data),
        )
        .route("/entity/{id}", get(modules::entitydata::concept_uri))
        .route("/healthz", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            app.clone(),
            forwarded::layer,
        ))
        .with_state(app)
}
