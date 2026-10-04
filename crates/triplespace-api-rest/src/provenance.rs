//! `GET /entity/{id}/provenance` (0003 §6; 0012 §5): where an entity and each of its
//! statements come from.
//!
//! Served from what the serving tables hold today: the graphs that hold a record for the
//! entity, with their statement counts, sync state and history policy; the dominant
//! graph, the one asserting the most statements (a tie goes to the earlier graph, local
//! first; none when no graph asserts any); the graphs that
//! assert each statement of the resolved view, with the member statements; the local
//! corrections; who minted the ID; retention; the canonical ID. The parts of 0012 §5
//! whose projections are not built yet (cluster members, constraint results, the backlink
//! count) are absent rather than empty, so a client can tell the difference.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use scatter_wikibase_model::id::{EntityId, IdForm, StatementId};
use scatter_wikibase_model::statement::Rank;
use scatter_wikibase_resolve::Correction;
use serde_json::{Map, Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::entity_json::iso8601;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::modules::entities::parse_id;
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_projections::read::{self, Current, Standing};

/// A REST error: `{"code": …, "message": …}` with a status.
fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = json!({"code": code, "message": message.into()}).to_string();
    let mut r = (
        status,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        body,
    )
        .into_response();
    if status == StatusCode::NOT_FOUND {
        // A missing entity and a confidential one answer alike (0056 §5).
        r.headers_mut().insert(
            header::VARY,
            header::HeaderValue::from_static("Accept, Accept-Language"),
        );
    }
    r
}

fn micros(t: SystemTime) -> u64 {
    u64::try_from(t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_micros())).unwrap_or(0)
}

fn rank_name(r: Rank) -> &'static str {
    match r {
        Rank::Preferred => "preferred",
        Rank::Normal => "normal",
        Rank::Deprecated => "deprecated",
    }
}

/// Who minted the ID: the tenant (and the wiki it was adopted from), a provider, or no
/// one, for a keyed ID that names the thing itself (0017 §1).
async fn minted_by(
    app: &App,
    client: &tokio_postgres::Client,
    tenant: &Tenant,
    id: &EntityId,
) -> Result<Value, tokio_postgres::Error> {
    Ok(match id.form() {
        IdForm::Local => {
            let adopted: Option<String> = client
                .query_opt(
                    "SELECT config->>'adopted_from' FROM view.registry
                     WHERE tenant = '' AND kind = 'tenant' AND code = $1",
                    &[&tenant.slug],
                )
                .await?
                .and_then(|r| r.get(0));
            let mut v = json!({"kind": "tenant", "tenant": tenant.slug});
            if let Some(a) = adopted {
                v["adopted_from"] = json!(a);
            }
            v
        }
        IdForm::Foreign => match app.registry().parse_foreign_id(id.as_str()) {
            Ok(f) => json!({
                "kind": "provider",
                "provider": f.provider().slug,
                "code": f.provider().code,
                "upstream_id": f.upstream_id(),
            }),
            Err(_) => json!({"kind": "provider"}),
        },
        IdForm::Keyed => json!({
            "kind": "keyed",
            "type": id.as_str().split(':').next().unwrap_or(""),
        }),
    })
}

/// The response body.
fn body(id: &EntityId, current: Option<&Current>, standing: &Standing, minted: &Value) -> Value {
    let mut graphs = Vec::new();
    let mut dominant: Option<(String, usize)> = None;
    for g in &standing.graphs {
        let statements = current.map_or(0, |c| {
            c.contributions
                .iter()
                .find(|k| k.graph == g.graph)
                .map_or(0, |k| k.entity.all_statements().count())
        });
        if statements > dominant.as_ref().map_or(0, |d| d.1) {
            dominant = Some((g.graph.clone(), statements));
        }
        let mut row = json!({
            "graph": g.graph,
            "statements": statements,
            "synced_at": iso8601(micros(g.synced_at)),
        });
        if let Some(v) = &g.upstream_version {
            row["upstream_version"] = json!(v);
        }
        if let Some(j) = g.job_id {
            row["job"] = json!(j);
        }
        if let Some(h) = &g.history {
            row["history"] = json!(h);
        }
        graphs.push(row);
    }
    let mut statements = Map::new();
    let mut corrections = Map::new();
    if let Some(c) = current {
        for (s, p) in c
            .resolved
            .entity
            .all_statements()
            .zip(&c.resolved.statements)
        {
            if let Some(sid) = &s.id {
                statements.insert(
                    sid.as_str().to_string(),
                    json!({
                        "graphs": p.graphs,
                        "members": p.members.iter().map(StatementId::as_str).collect::<Vec<_>>(),
                    }),
                );
            }
        }
        for (sid, corr) in &c.corrections {
            corrections.insert(
                sid.as_str().to_string(),
                match corr {
                    Correction::Rank(r) => json!({"rank": rank_name(*r)}),
                    Correction::Suppress => json!({"suppressed": true}),
                },
            );
        }
    }
    let mut out = json!({
        "id": id.as_str(),
        "canonical": standing.canonical_id.clone().unwrap_or_else(|| id.as_str().to_string()),
        "minted_by": minted,
        "graphs": graphs,
        "statements": statements,
        "corrections": corrections,
    });
    if let Some(c) = current {
        out["type"] = json!(c.resolved.entity.entity_type.name());
        out["page_id"] = json!(c.page_id);
        out["lastrevid"] = json!(c.lastrevid);
        if c.modified > 0 {
            out["modified"] = json!(iso8601(c.modified));
        }
    } else if let Some((t, _)) = id.as_str().split_once(':') {
        out["type"] = json!(t);
    }
    if let Some((g, _)) = dominant {
        out["dominant"] = json!(g);
    }
    if let Some(t) = standing.first_seen {
        out["first_seen"] = json!(iso8601(micros(t)));
    }
    if let Some(r) = &standing.retention {
        out["retention"] = json!(r);
    }
    out
}

/// The handler.
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id_text): Path<String>,
) -> Response {
    let internal = |e: &dyn std::fmt::Display| {
        error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
    };
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let tenant = match tenant::resolve(&app, &**client, &origin, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return error(
                StatusCode::MISDIRECTED_REQUEST,
                "misdirected",
                "not a tenant of this instance",
            );
        }
        Err(e) => return internal(&e),
    };
    let caller = match Caller::resolve(&**client, &tenant.slug, &headers).await {
        Ok(c) => c,
        Err(e) => return internal(&e.info),
    };
    if caller.require("read").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reading needs the \"read\" right",
        );
    }
    let id = match parse_id(&id_text) {
        Ok(id) => id,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid-entity-id", e.info),
    };
    if id.form() == IdForm::Foreign && app.registry().parse_foreign_id(id.as_str()).is_err() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid-entity-id",
            format!("\"{}\" names no registered provider", id.as_str()),
        );
    }
    let current = match read::current(&client, app.order(), &tenant.slug, &id).await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let standing = match read::standing(&client, &tenant.slug, &id).await {
        Ok(s) => s,
        Err(e) => return internal(&e),
    };
    // Absent, deleted or (later) confidential: one answer (0056 §5). A valid keyed ID no
    // graph holds is an empty entity, as `wbgetentities` serves it (0009 §4).
    if standing.deleted || (current.is_none() && id.form() != IdForm::Keyed) {
        return error(
            StatusCode::NOT_FOUND,
            "entity-not-found",
            format!("no entity {}", id.as_str()),
        );
    }
    let minted = match minted_by(&app, &client, &tenant, &id).await {
        Ok(v) => v,
        Err(e) => return internal(&e),
    };
    let body = body(&id, current.as_ref(), &standing, &minted).to_string();
    let credentialed = caller.has_session() || bearer(&headers).is_some();
    let class = Class::Public.permitted(&method, credentialed, false, false);
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body,
        class,
        &[format!("entity:{}", id.as_str())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENAPI: &str = include_str!("../../../docs/api/triplespace-v0.openapi.json");

    #[test]
    fn the_contract_names_the_route_and_the_body_meets_it() {
        let doc: Value = serde_json::from_str(OPENAPI).expect("the OpenAPI document parses");
        assert!(doc["paths"].get("/entity/{id}/provenance").is_some());
        let required: Vec<&str> = doc["components"]["schemas"]["Provenance"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let id = EntityId::parse("domain:example.org").unwrap();
        let b = body(
            &id,
            None,
            &Standing::default(),
            &json!({"kind": "keyed", "type": "domain"}),
        );
        for key in required {
            assert!(b.get(key).is_some(), "the body lacks `{key}`: {b}");
        }
        assert_eq!(b["type"], "domain");
        assert!(b.get("dominant").is_none());
    }
}
