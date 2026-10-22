//! The `triplespace/v0` REST API (0012 §1.3, §5): data only Triplespace has, served at
//! `rest.php/triplespace/v0` under both MediaWiki paths. `v0` makes no stability promise
//! yet. The normative contract is `docs/api/triplespace-v0.openapi.json`; a route here
//! and its entry there change together.
//!
//! Served so far:
//!
//! - `GET /entity/{id}/provenance` ([`provenance`]): which graphs assert the entity and
//!   each statement, the local corrections, each graph's sync state and history policy,
//!   who minted the ID, retention and the canonical ID (0003 §6; 0012 §5).
//! - `GET /version` and `GET /version/licenses/{id}` ([`version`]): everything
//!   `Special:Version` shows, and one component's licence texts (0077 §10).
//! - `GET /activity` ([`activity`]): a change feed in the row shape of 16 §3.1 — recent
//!   changes, a history or contributions by its target set (0020 §4–5).
//! - `GET /entity/{id}/history` ([`history`]): the one-target feed of an entity, its
//!   local edits, mirror syncs and log events in one list (0012 A71; 19 §3.3).
//! - `GET /entity/{id}/changes/{partition}/{offset}` and `GET /entity/{id}/compare`
//!   ([`diff`]): the structured diff of one change set, and of two local revisions
//!   (0012 §7; 18 §5.1).
//!
//! - `GET /settings/schema`, `GET /settings` and `PATCH /settings` ([`settings`]): the
//!   registered settings, their resolved values with origins, and saving them through the
//!   one validator (0085 §3).
//! - `GET` and `PATCH /account/preferences`, `DELETE /account/preferences/{key}`
//!   ([`account`]): the holder's preferences (0027 §6; 0085 §5).
//! - `GET /account/requests`, `POST /account/requests/{id}/approve` and `…/reject`
//!   ([`account_requests`]): the review of account requests (0087 §6).
//! - `POST /auth/{issuer}/start`, `GET /auth/{issuer}/callback`, `GET /auth/pending` and
//!   `GET /account` ([`auth`]): login through external issuers, the site's door, and the
//!   holder's sign-in methods (0088 §5–§7).
//!
//! - `GET /tenancy` ([`tenancy`]): at the farm base, the farm, the primary tenant, the
//!   tenancy policy and the tenant list (0028 §11; 0046 §9; 0079 §7).
//!
//! - `POST /thread`, `GET /thread/{id}`, `POST /thread/{id}/post`,
//!   `PUT /thread/{id}/post/{revid}`, the thread's `rename`, `move`, `attach`, `detach`,
//!   `pin` and `unpin`, and `GET /page/{id}/threads` ([`threads`]): local threads, read and
//!   written through the one thread writer (0019 §9; 0049 §11; 0069 §9).
//!
//! Every route runs behind the Action API's request layers ([`triplespace_api_action`]):
//! the tenant the host names (421 otherwise), the caller, the redaction of 0012 §8, and
//! the HTTP caching of 0014 §6 ([`triplespace_api_action::http_cache`]). Errors are JSON
//! objects with `code` and `message`, as the Wikibase REST API's are.

#![forbid(unsafe_code)]

pub mod account;
pub mod account_requests;
pub mod activity;
pub mod auth;
pub mod diff;
mod entity_route;
pub mod history;
pub mod provenance;
pub mod settings;
pub mod tenancy;
pub mod threads;
pub mod version;

use axum::Router;
use axum::routing::{delete, get, post, put};
use triplespace_api_action::App;

/// The base paths the REST API answers at.
pub const BASES: [&str; 2] = ["/w/rest.php/triplespace/v0", "/rest.php/triplespace/v0"];

/// The paths served, relative to a base; each is a path of the OpenAPI document.
pub const PATHS: &[&str] = &[
    "/entity/{id}/provenance",
    "/version",
    "/version/licenses/{id}",
    "/activity",
    "/entity/{id}/history",
    "/entity/{id}/changes/{partition}/{offset}",
    "/entity/{id}/compare",
    "/settings/schema",
    "/settings",
    "/account/preferences",
    "/account/preferences/{key}",
    "/account/requests",
    "/account/requests/{id}/approve",
    "/account/requests/{id}/reject",
    "/auth/{issuer}/start",
    "/auth/{issuer}/callback",
    "/auth/pending",
    "/account",
    "/tenancy",
    "/thread",
    "/thread/{id}",
    "/thread/{id}/post",
    "/thread/{id}/post/{revid}",
    "/thread/{id}/rename",
    "/thread/{id}/move",
    "/thread/{id}/attach",
    "/thread/{id}/detach",
    "/thread/{id}/pin",
    "/thread/{id}/unpin",
    "/page/{id}/threads",
];

/// The REST routes, to be merged into the server's router.
pub fn routes() -> Router<App> {
    let mut r = Router::new();
    for base in BASES {
        r = r
            .route(&format!("{base}{}", PATHS[0]), get(provenance::handle))
            .route(&format!("{base}{}", PATHS[1]), get(version::handle))
            .route(&format!("{base}{}", PATHS[2]), get(version::license))
            .route(&format!("{base}{}", PATHS[3]), get(activity::handle))
            .route(&format!("{base}{}", PATHS[4]), get(history::handle))
            .route(&format!("{base}{}", PATHS[5]), get(diff::change))
            .route(&format!("{base}{}", PATHS[6]), get(diff::compare))
            .route(&format!("{base}{}", PATHS[7]), get(settings::schema))
            .route(
                &format!("{base}{}", PATHS[8]),
                get(settings::get).patch(settings::patch),
            )
            .route(
                &format!("{base}{}", PATHS[9]),
                get(account::get).patch(account::patch),
            )
            .route(&format!("{base}{}", PATHS[10]), delete(account::delete))
            .route(&format!("{base}{}", PATHS[11]), get(account_requests::list))
            .route(
                &format!("{base}{}", PATHS[12]),
                post(account_requests::approve),
            )
            .route(
                &format!("{base}{}", PATHS[13]),
                post(account_requests::reject),
            )
            .route(&format!("{base}{}", PATHS[14]), post(auth::start))
            .route(&format!("{base}{}", PATHS[15]), get(auth::callback))
            .route(&format!("{base}{}", PATHS[16]), get(auth::pending))
            .route(&format!("{base}{}", PATHS[17]), get(auth::account))
            .route(&format!("{base}{}", PATHS[18]), get(tenancy::handle))
            .route(&format!("{base}{}", PATHS[19]), post(threads::create))
            .route(&format!("{base}{}", PATHS[20]), get(threads::get))
            .route(&format!("{base}{}", PATHS[21]), post(threads::post))
            .route(&format!("{base}{}", PATHS[22]), put(threads::edit))
            .route(&format!("{base}{}", PATHS[23]), post(threads::rename))
            .route(&format!("{base}{}", PATHS[24]), post(threads::move_to))
            .route(&format!("{base}{}", PATHS[25]), post(threads::attach))
            .route(&format!("{base}{}", PATHS[26]), post(threads::detach))
            .route(&format!("{base}{}", PATHS[27]), post(threads::pin))
            .route(&format!("{base}{}", PATHS[28]), post(threads::unpin))
            .route(&format!("{base}{}", PATHS[29]), get(threads::list));
    }
    r
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_route_is_in_the_contract() {
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/triplespace-v0.openapi.json"
        ))
        .unwrap();
        let paths = doc["paths"].as_object().unwrap();
        for route in super::PATHS {
            assert!(
                paths.contains_key(*route),
                "{route} is not in the OpenAPI document"
            );
        }
        assert_eq!(
            paths.len(),
            super::PATHS.len(),
            "a path in the contract that no route serves"
        );
    }
}
