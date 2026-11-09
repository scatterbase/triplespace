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
//! - `GET` and `POST /auth/email`, `POST /auth/email/verify`, `…/primary` and
//!   `…/remove` ([`email`]): the login's email addresses and their verification codes
//!   (0100 §7), and `POST /mail/postmark`, Postmark's bounce and complaint webhook
//!   (0100 A2).
//!
//! - `GET /tenancy` ([`tenancy`]): at the farm base, the farm, the primary tenant, the
//!   tenancy policy and the tenant list (0028 §11; 0046 §9; 0079 §7).
//!
//! - `POST /thread`, `GET /thread/{id}`, `POST /thread/{id}/post`,
//!   `PUT /thread/{id}/post/{revid}`, the thread's `rename`, `move`, `attach`, `detach`,
//!   `pin` and `unpin`, and `GET /page/{id}/threads` ([`threads`]): local threads, read and
//!   written through the one thread writer (0019 §9; 0049 §11; 0069 §9).
//! - `GET /reports` and `GET /reports/{name}` ([`reports`]): every report with its state, and
//!   one report's rows, as `list=querypage` serves them (0047 §11).
//!
//! - `GET /record/{partition}/{offset}`, its `checkpoint` and `proof`, and
//!   `GET /record/proofs` ([`record`]): one record of a published partition as an export
//!   shows it, the checkpoint that first covers it, and inclusion proofs, one or a range's
//!   with their shared nodes once (0012 §5; 17 §1.1).
//!
//! - `POST /export` ([`export`]): the `records` export of selected pages and local
//!   entities, with their proofs (0047 §8; 21 §4.2).
//!
//! Every route runs behind the Action API's request layers ([`triplespace_api_action`]):
//! the tenant the host names (421 otherwise), the caller, the redaction of 0012 §8, and
//! the HTTP caching of 0014 §6 ([`triplespace_api_action::http_cache`]). Errors are JSON
//! objects with `code` and `message`, as the Wikibase REST API's are.

#![forbid(unsafe_code)]

pub mod account;
pub mod account_requests;
pub mod acl;
pub mod activity;
pub mod auth;
pub mod diff;
pub mod email;
mod entity_route;
pub mod export;
pub mod groups;
pub mod history;
pub mod jobs;
pub mod provenance;
pub mod purges;
pub mod record;
pub mod reports;
pub mod settings;
pub mod tenancy;
pub mod threads;
pub mod version;
pub mod watchlist;

use axum::Router;
use axum::extract::{MatchedPath, Request};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use triplespace_api_action::App;

/// The base paths the REST API answers at.
pub const BASES: [&str; 2] = ["/w/rest.php/triplespace/v0", "/rest.php/triplespace/v0"];

/// Declares every REST operation once, `method "path" => handler`, the path relative to a
/// base and as the OpenAPI document spells it. [`OPERATIONS`], which the contract test reads,
/// and the router [`routes`] builds are both generated from the one table, so a route cannot be
/// served without being listed, or listed without being served (0012 §5; 18 §3.2).
macro_rules! rest_operations {
    ($( $method:ident $path:literal => $handler:path ;)+) => {
        /// Every operation served, `(METHOD, path)`, the path relative to a base.
        pub const OPERATIONS: &[(&str, &str)] = &[$((rest_operations!(@upper $method), $path),)+];

        /// The REST routes, to be merged into the server's router.
        pub fn routes() -> Router<App> {
            let mut r = Router::new();
            for base in BASES {
                $( r = r.route(&format!("{base}{}", $path), $method($handler)); )+
            }
            r.route_layer(middleware::from_fn(report_route))
        }
    };
    (@upper get) => { "GET" };
    (@upper post) => { "POST" };
    (@upper put) => { "PUT" };
    (@upper patch) => { "PATCH" };
    (@upper delete) => { "DELETE" };
}

rest_operations! {
    get "/entity/{id}/provenance" => provenance::handle;
    get "/version" => version::handle;
    get "/version/licenses/{id}" => version::license;
    get "/activity" => activity::handle;
    get "/entity/{id}/history" => history::handle;
    get "/entity/{id}/changes/{partition}/{offset}" => diff::change;
    get "/entity/{id}/compare" => diff::compare;
    get "/settings/schema" => settings::schema;
    get "/settings" => settings::get;
    patch "/settings" => settings::patch;
    get "/account/preferences" => account::get;
    patch "/account/preferences" => account::patch;
    delete "/account/preferences/{key}" => account::delete;
    get "/account/requests" => account_requests::list;
    post "/account/requests/{id}/approve" => account_requests::approve;
    post "/account/requests/{id}/reject" => account_requests::reject;
    post "/auth/{issuer}/start" => auth::start;
    get "/auth/{issuer}/callback" => auth::callback;
    get "/auth/pending" => auth::pending;
    get "/account" => auth::account;
    get "/tenancy" => tenancy::handle;
    post "/thread" => threads::create;
    get "/thread/{id}" => threads::get;
    post "/thread/{id}/post" => threads::post;
    put "/thread/{id}/post/{revid}" => threads::edit;
    post "/thread/{id}/rename" => threads::rename;
    post "/thread/{id}/move" => threads::move_to;
    post "/thread/{id}/attach" => threads::attach;
    post "/thread/{id}/detach" => threads::detach;
    post "/thread/{id}/pin" => threads::pin;
    post "/thread/{id}/unpin" => threads::unpin;
    get "/page/{id}/threads" => threads::list;
    get "/page/{id}/history" => threads::history;
    get "/acl/{kind}/{id}" => acl::get;
    put "/acl/{kind}/{id}" => acl::put;
    delete "/acl/{kind}/{id}" => acl::delete;
    get "/acl/tenant/{slug}" => acl::get_tenant;
    put "/acl/tenant/{slug}" => acl::put_tenant;
    delete "/acl/tenant/{slug}" => acl::delete_tenant;
    post "/acl/set" => acl::create_set;
    post "/acl/set/{id}/members" => acl::add_member;
    delete "/acl/set/{id}/members/{kind}/{member}" => acl::remove_member;
    get "/group/{name}" => groups::get;
    put "/group/{name}" => groups::put;
    delete "/group/{name}" => groups::delete;
    get "/acl/empty-groups" => acl::empty_groups;
    get "/purges" => purges::list;
    get "/watchlist" => watchlist::list;
    post "/watchlist/seen" => watchlist::seen;
    put "/watchlist/{kind}/{id}" => watchlist::put;
    delete "/watchlist/{kind}/{id}" => watchlist::delete;
    get "/reports" => reports::list;
    get "/reports/{name}" => reports::get;
    get "/auth/email" => email::list;
    post "/auth/email" => email::add;
    post "/auth/email/verify" => email::verify;
    post "/auth/email/primary" => email::primary;
    post "/auth/email/remove" => email::remove;
    post "/mail/postmark" => email::postmark_webhook;
    get "/record/{partition}/{offset}" => record::get;
    get "/record/{partition}/{offset}/checkpoint" => record::checkpoint;
    get "/record/{partition}/{offset}/proof" => record::proof;
    get "/record/proofs" => record::proofs;
    post "/export" => export::post;
    get "/jobs" => jobs::list;
    get "/jobs/{id}" => jobs::get;
    post "/jobs/{id}/reject" => jobs::reject;
    post "/jobs/{id}/revert" => jobs::revert;
    post "/jobs/{id}/close" => jobs::close;
}

/// The observer of the routes served a process may install (0108 §1): called with
/// `{METHOD} {path}`, the path relative to the base as `routes.toml` spells it, for every
/// request a route matched.
static OBSERVER: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();

/// Installs the observer of the routes served, once per process (implements 0108 §1.2):
/// the server sets it when asked to probe, so that the routes a run used can be compared
/// with `routes.toml`. False if one was already installed.
pub fn observe_routes(observer: fn(&str)) -> bool {
    OBSERVER.set(observer).is_ok()
}

/// The route a request matched, as `routes.toml` spells it: `GET /account`.
fn route_name(method: &str, matched: &str) -> String {
    let path = BASES
        .iter()
        .find_map(|b| matched.strip_prefix(b))
        .unwrap_or(matched);
    format!("{method} {path}")
}

async fn report_route(req: Request, next: Next) -> Response {
    if let (Some(observe), Some(matched)) = (OBSERVER.get(), req.extensions().get::<MatchedPath>())
    {
        observe(&route_name(req.method().as_str(), matched.as_str()));
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    // 0108 §1.2: a matched route is named relative to either base, in routes.toml's spelling.
    #[test]
    fn routes_are_named_as_the_catalogue_names_them() {
        assert_eq!(
            super::route_name("GET", "/w/rest.php/triplespace/v0/account"),
            "GET /account"
        );
        assert_eq!(
            super::route_name("POST", "/rest.php/triplespace/v0/jobs/{id}/close"),
            "POST /jobs/{id}/close"
        );
    }

    #[test]
    fn every_route_is_in_the_contract() {
        // Operations, not only paths: a method the router serves and the contract lacks, or
        // the reverse, fails here (0012 §5; 18 §3.2).
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/triplespace-v0.openapi.json"
        ))
        .unwrap();
        let mut contract = std::collections::BTreeSet::new();
        for (path, item) in doc["paths"].as_object().unwrap() {
            for method in item.as_object().unwrap().keys() {
                if ["get", "post", "put", "patch", "delete"].contains(&method.as_str()) {
                    contract.insert((method.to_uppercase(), path.clone()));
                }
            }
        }
        let served: std::collections::BTreeSet<(String, String)> = super::OPERATIONS
            .iter()
            .map(|(m, p)| ((*m).to_owned(), (*p).to_owned()))
            .collect();
        assert_eq!(
            served.len(),
            super::OPERATIONS.len(),
            "an operation declared twice"
        );
        let unlisted: Vec<_> = served.difference(&contract).collect();
        let unserved: Vec<_> = contract.difference(&served).collect();
        assert!(
            unlisted.is_empty(),
            "served but not in the OpenAPI document: {unlisted:?}"
        );
        assert!(
            unserved.is_empty(),
            "in the OpenAPI document but not served: {unserved:?}"
        );
    }
}
