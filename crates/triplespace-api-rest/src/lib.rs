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
//!
//! Every route runs behind the Action API's request layers ([`triplespace_api_action`]):
//! the tenant the host names (421 otherwise), the caller, the redaction of 0012 §8, and
//! the HTTP caching of 0014 §6 ([`triplespace_api_action::http_cache`]). Errors are JSON
//! objects with `code` and `message`, as the Wikibase REST API's are.

#![forbid(unsafe_code)]

pub mod provenance;

use axum::Router;
use axum::routing::get;
use triplespace_api_action::App;

/// The base paths the REST API answers at.
pub const BASES: [&str; 2] = ["/w/rest.php/triplespace/v0", "/rest.php/triplespace/v0"];

/// The paths served, relative to a base; each is a path of the OpenAPI document.
pub const PATHS: &[&str] = &["/entity/{id}/provenance"];

/// The REST routes, to be merged into the server's router.
pub fn routes() -> Router<App> {
    let mut r = Router::new();
    for base in BASES {
        r = r.route(&format!("{base}{}", PATHS[0]), get(provenance::handle));
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
            1,
            "a path in the contract that no route serves"
        );
    }
}
