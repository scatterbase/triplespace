//! The Action API (mediawiki-compat.md; wikibase-compat.md; 0012 §4): the modules a
//! Wikibase bot client needs to log in, read entities and save edits, served the way
//! MediaWiki serves them so that WikibaseIntegrator and Pywikibot work unchanged.
//!
//! - `GET|POST {script path}/api.php` — [`api::handle`]: `action=query` with
//!   `meta=siteinfo|tokens|userinfo`, `action=login` (bot passwords, 0024 §4),
//!   `action=clientlogin` (primary accounts, 0007 §3), `action=logout`, `wbgetentities`,
//!   `wbsearchentities`, and the write modules `wbeditentity`, `wbcreateclaim`,
//!   `wbsetclaim`, `wbremoveclaims`, `wbsetlabel`, `wbsetdescription`, `wbsetaliases`,
//!   `wbsetqualifier`, `wbremovequalifiers`, `wbsetreference`, `wbremovereferences`;
//!   `list=recentchanges`, `list=usercontribs`, `list=logevents` ([`modules::changes`]);
//!   `prop=revisions` ([`modules::revisions`]).
//! - `GET /wiki/Special:EntityData/{id}.json` and `GET /entity/{id}` (0001 §1; 0002 §4).
//!
//! Every request is served for the tenant its `Host` names (0056 §10): an unregistered
//! host is 421, unless the server runs in development mode with a fallback tenant.
//! Writes run the path of 0013 §7 as far as the milestone builds it: identity and
//! permissions ([`triplespace_accounts`]), the CSRF token, the base-offset check and the
//! append with inline projections ([`scatter_ingest::write`]). Rate limits, ACLs beyond
//! the `owner` group and edit filters are not yet in front of the append.

#![forbid(unsafe_code)]

pub mod acl_api;
pub mod activity;
pub mod api;
pub mod app;
pub mod auth;
pub mod edit;
pub mod entity_json;
pub mod external;
pub mod forwarded;
pub mod http_cache;
pub mod modules;
pub mod pages;
pub mod params;
pub mod prefs;
pub mod response;
pub mod tenant;
pub mod thread_view;
pub mod version;

pub use app::{App, Config, Mode};

/// The API level the site checks against the minimum it was built for (0057 §9),
/// reported as `api_version` in `meta=siteinfo&siprop=triplespace`. It rises whenever a
/// route, module or field the site relies on is added; it never falls.
///
/// - 1: `siprop=triplespace` with `api_version` and `theme`; `ETag`, `Cache-Control`,
///   `Cache-Tag` and `304` on public responses; `GET /entity/{id}/provenance` in REST v0.
/// - 2: `GET /version` and `GET /version/licenses/{id}` in REST v0; `siprop=libraries`;
///   `general.dbversion`; `build` in `siprop=triplespace` (0077 §10).
/// - 3: `list=recentchanges`, `list=usercontribs` and `list=logevents`; `GET /activity`
///   in REST v0; the `recentchanges` and `activity` capabilities (0020 §5).
/// - 4: `GET /entity/{id}/history` in REST v0, `source` on `GET /activity`, and the
///   `history` capability (0012 A71).
/// - 5: `prop=revisions` with `rvprop=content|record`; `GET /entity/{id}/changes/{partition}/{offset}`
///   and `GET /entity/{id}/compare` in REST v0; the `revisions` and `diff` capabilities
///   (0012 §4, §7; 0083 A1).
/// - 6: login through external issuers: `POST /auth/{issuer}/start`,
///   `GET /auth/{issuer}/callback`, `GET /auth/pending` and `GET /account` in REST v0;
///   `login` in `siprop=triplespace`; `action=linkaccount` and `action=unlinkaccount`;
///   `createpreservestate` (0088).
pub const API_VERSION: u32 = 6;
pub use response::ApiError;

use axum::Router;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};

/// The characters a `/wiki/{title}` segment keeps, as MediaWiki's `wfUrlencode` does.
pub const TITLE_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'-')
    .remove(b'.')
    .remove(b':')
    .remove(b'/')
    .remove(b'(')
    .remove(b')')
    .remove(b'~')
    .remove(b'!')
    .remove(b'*')
    .remove(b';')
    .remove(b'@')
    .remove(b'$')
    .remove(b',');

/// The characters a query value keeps.
pub const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'-')
    .remove(b'.')
    .remove(b'~');

/// HTML-escapes text, for `parsedcomment`.
#[must_use]
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}
use axum::routing::get;

/// The router: `api.php` at both MediaWiki paths, the entity routes, and a health check.
pub fn router(app: App) -> Router {
    router_with(app, Router::new())
}

/// The router with more routes merged in (the REST API's), all behind the same layers.
///
/// Every route runs behind [`forwarded::layer`], which works out the client behind
/// trusted proxies. The server must be served with
/// `into_make_service_with_connect_info::<SocketAddr>()` for the peer address to be known.
pub fn router_with(app: App, extra: Router<App>) -> Router {
    Router::new()
        .route("/api.php", get(api::handle).post(api::handle))
        .route("/w/api.php", get(api::handle).post(api::handle))
        .route(
            "/wiki/Special:EntityData/{file}",
            get(modules::entitydata::special_entity_data),
        )
        .route("/entity/{id}", get(modules::entitydata::concept_uri))
        .route("/healthz", get(|| async { "ok" }))
        .merge(extra)
        .layer(axum::middleware::from_fn_with_state(
            app.clone(),
            forwarded::layer,
        ))
        .with_state(app)
}
