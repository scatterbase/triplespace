//! `GET /entity/{id}/history` (0012 §5, A71; 0020 §4; 19 §3.3): an entity's history as
//! activity rows, the one-target feed of [`crate::activity`] with the target fixed.
//!
//! Served from `view.activity`'s target index for now: every record keyed to the entity
//! in any partition the tenant reads, local edits and mirror syncs alike (`syncs` is on
//! for a history, 16 §4.2), and the log events keyed to it. The design's one backing,
//! `view.entity_history` — one row per record of every cluster member and per upstream
//! log event, written by composition — takes over when composition writes it and
//! clusters exist; the route and its parameters do not change.
//!
//! Parameters are `GET /activity`'s, less `target` and `actor`'s set-making meaning:
//! `source` (the source switch of 19 §3.5), `kind`, `since`, `until`, `dir`, `limit`,
//! `continue`, `lang`. The ID takes any form the Action API accepts; an entity that does
//! not exist, or that the viewer may not see, is `404` alike (0056 §5).

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, Method};
use axum::response::Response;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_projections::feed::{TargetSet, target_kind};

use crate::entity_route;

/// The handler.
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id_text): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    // Does the entity exist for this viewer? The same answer as `/provenance` gives.
    let id = {
        let client = match app.pool().get().await {
            Ok(c) => c,
            Err(e) => return entity_route::internal(&e),
        };
        match entity_route::resolve(&app, &client, &origin, &headers, &id_text).await {
            Ok(r) => r.id,
            Err(r) => return r,
        }
    };
    let preset = TargetSet::Target {
        kind: target_kind::ENTITY,
        id: id.as_str().to_string(),
    };
    crate::activity::serve(app, origin, &method, &headers, &q, Some(preset)).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_contract_names_the_route() {
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/triplespace-v0.openapi.json"
        ))
        .unwrap();
        let get = &doc["paths"]["/entity/{id}/history"]["get"];
        assert_eq!(get["operationId"], "getEntityHistory");
        let names: Vec<&str> = get["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["name"].as_str())
            .collect();
        for p in [
            "id", "source", "kind", "since", "until", "dir", "limit", "continue", "lang",
        ] {
            assert!(names.contains(&p), "{p} is not in the contract");
        }
        assert_eq!(
            get["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ActivityPage"
        );
    }
}
