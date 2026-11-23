//! `GET /entity/{id}/page`, the page bundle (0012 A70; 19 §5.1): everything an entity
//! page is drawn from but the labels of what it links to, in one response, so that the
//! page reaches its first byte within four API calls.
//!
//! ```json
//! { "id": "Q6",
//!   "entity": { … },          // as `wbgetentities` serves it, every part and language
//!   "provenance": { … },      // as `GET /entity/{id}/provenance` serves it
//!   "datatypes": { "P7": "external-id" },   // only for snaks that do not say theirs
//!   "visibility": { … },      // only for a restricted entity, as `ts-visibility`
//!   "badges": { "Q6": ["WDQ142241643"] },   // `class` badges of it and its properties
//!   "redirects": { "from": "XDQ9", "to": "domain:example.org" } }   // only for an alias
//! ```
//!
//! A snak carries its property's data type when it was written with one; `datatypes`
//! gives it for the properties of the snaks that were not, so that the page need not
//! fetch each property to tell an identifier from a statement.
//!
//! `badges` gives the `class` badges (0114 §2) of the entity, of each property its
//! statements use and of each item they name as a value (at most 100 of these), as
//! `wbgetentities` reports them in `ts-badges`, for those that carry any: a retired
//! property's row is drawn with its "deprecated" badge.
//!
//! The provenance carries the identity-line facts (who minted the ID, the canonical ID,
//! each graph's sync state and history policy, retention). It answers as
//! `/provenance` does for an entity that is absent or not the caller's to read, and is
//! cached as that response is: by its content, which changes with the entity's
//! `resolved_version`, under the tag `entity:{id}`.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use std::collections::{BTreeMap, BTreeSet};

use scatter_wikibase_ids::IdForm;
use scatter_wikibase_model::id::EntityId;
use serde_json::{Value, json};
use triplespace_api_action::auth::bearer;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::modules::entities::parse_id;
use triplespace_api_action::{App, entity_json};
use triplespace_projections::read;

use crate::entity_route::{self, internal};
use crate::provenance;
use scatter_wikibase_ids::TriplespaceId as _;

/// The properties of the snaks of an entity's JSON that carry no `datatype`: main
/// snaks, qualifiers and references.
fn untyped_properties(entity: &Value) -> BTreeSet<String> {
    fn snak(s: &Value, out: &mut BTreeSet<String>) {
        if s.get("datatype").is_none()
            && let Some(p) = s["property"].as_str()
        {
            out.insert(p.to_string());
        }
    }
    fn by_property(snaks: &Value, out: &mut BTreeSet<String>) {
        for list in snaks.as_object().into_iter().flat_map(|m| m.values()) {
            for s in list.as_array().into_iter().flatten() {
                snak(s, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    for statements in entity["claims"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
    {
        for st in statements.as_array().into_iter().flatten() {
            snak(&st["mainsnak"], &mut out);
            by_property(&st["qualifiers"], &mut out);
            for r in st["references"].as_array().into_iter().flatten() {
                by_property(&r["snaks"], &mut out);
            }
        }
    }
    out
}

/// The bundle's body, from the entity's JSON and its provenance.
fn body(
    id: &str,
    entity: Value,
    provenance: Value,
    datatypes: &BTreeMap<String, String>,
    visibility: Option<Value>,
    badges: &BTreeMap<String, Vec<String>>,
    alias: Option<&str>,
) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("id".into(), json!(id));
    out.insert("entity".into(), entity);
    out.insert("provenance".into(), provenance);
    if !datatypes.is_empty() {
        out.insert("datatypes".into(), json!(datatypes));
    }
    let mut out = Value::Object(out);
    if let Some(v) = visibility {
        out["visibility"] = v;
    }
    if !badges.is_empty() {
        out["badges"] = json!(badges);
    }
    if let Some(from) = alias {
        out["redirects"] = json!({"from": from, "to": id});
    }
    out
}

/// The handler: with [`crate::labels`], the two routes of the four-call budget
/// (0012 §5, A70).
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id_text): Path<String>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    // The one answer every entity route gives for an entity that is absent, restricted or
    // deleted for this viewer (`entity_route::resolve`; 09 §5.3).
    let r = match entity_route::resolve(&app, &client, &origin, &headers, &id_text).await {
        Ok(r) => r,
        // A caller with no credential is told the same of an absent entity as anyone
        // else with none: that answer is public, tagged with the entity so that creating
        // it purges it, as `wbgetentities` serves `missing` (0014 §6). A page drawn
        // from it, the site's own 404, is then public too.
        Err(r)
            if r.status() == StatusCode::NOT_FOUND
                && !headers.contains_key(header::COOKIE)
                && bearer(&headers).is_none() =>
        {
            let Ok(id) = parse_id(&id_text) else {
                return r;
            };
            return http_cache::respond(
                &headers,
                StatusCode::NOT_FOUND,
                "application/json; charset=utf-8",
                json!({"code": "entity-not-found", "message": format!("no entity {}", id.as_str())})
                    .to_string(),
                Class::Public.permitted(&method, false, false, false),
                &[format!("entity:{}", id.as_str())],
            );
        }
        Err(r) => return r,
    };
    let (provenance, current) = match provenance::assemble(&app, &client, &r.tenant, &r.id).await {
        Ok(p) => p,
        Err(e) => return internal(&e),
    };
    let registry = app
        .registry_for(&client, &r.tenant.slug)
        .await
        .unwrap_or_else(|_| app.registry());
    // A keyed ID no graph holds is an empty entity, as `wbgetentities` serves it (0009 §4).
    let entity = current.as_ref().map_or_else(
        || entity_json::empty_keyed(&r.id),
        |c| entity_json::present(c, registry, &r.tenant.base),
    );
    // The data type of each property a snak names without saying it, from the
    // property's current state.
    let mut datatypes = BTreeMap::new();
    for p in untyped_properties(&entity) {
        let Ok(property) = EntityId::parse(&p) else {
            continue;
        };
        match read::current(&client, app.order(), &r.tenant.slug, &property).await {
            Ok(Some(c)) => {
                if let Some(d) = &c.resolved.entity.datatype {
                    datatypes.insert(p, d.id().to_string());
                }
            }
            Ok(None) => {}
            Err(e) => return internal(&e),
        }
    }
    // The visibility of a restricted local or keyed entity, for the Restricted pill
    // (19 §1.3), as `wbgetentities` reports it in `ts-visibility`.
    let visibility = if r.id.form() == IdForm::Foreign {
        None
    } else {
        match triplespace_projections::acl_read::entity_chain(
            &client,
            &r.tenant.slug,
            r.id.as_str(),
            None,
        )
        .await
        {
            Ok(chain) => triplespace_api_action::pages::visibility_of(&chain),
            Err(e) => return internal(&e),
        }
    };
    let badges = match class_badges(&app, &client, &r.tenant.slug, registry, current.as_ref()).await
    {
        Ok(b) => b,
        Err(e) => return internal(&e),
    };
    let body = body(
        r.id.as_str(),
        entity,
        provenance,
        &datatypes,
        visibility,
        &badges,
        r.alias_of.as_ref().map(EntityId::as_str),
    )
    .to_string();
    let class = Class::Public.permitted(&method, r.credentialed, false, false);
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body,
        class,
        &[format!("entity:{}", r.id.as_str())],
    )
}

/// The most entities, beyond the entity itself, whose `class` badges one bundle works out:
/// its properties first, then the items its statements name as values.
const MAX_BADGED: usize = 100;

/// The properties and item values an entity's statements use, properties first, each once,
/// the entity itself left out.
fn badged_ids(entity: &scatter_wikibase_model::entity::Entity) -> Vec<EntityId> {
    use scatter_wikibase_model::value::DataValue;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut push = |id: &EntityId| {
        if *id != entity.id && seen.insert(id.clone()) {
            out.push(id.clone());
        }
    };
    for p in entity.statements.keys() {
        push(p);
    }
    for s in entity.statements.values().flatten() {
        if let Some(DataValue::EntityId(v)) = s.mainsnak.data_value() {
            push(&v.id);
        }
    }
    out
}

/// The `class` badges of the entity, of each property its statements use and of each item
/// they name as a value, by ID, for those that carry any, at most [`MAX_BADGED`] of the
/// others (implements 0114 §2 (A2)).
async fn class_badges(
    app: &App,
    client: &deadpool_postgres::Object,
    tenant: &str,
    providers: &scatter_providers::Registry,
    current: Option<&read::Current>,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out = BTreeMap::new();
    let Some(current) = current else {
        return Ok(out);
    };
    let badges = triplespace_projections::badges::registry(client, tenant)
        .await?
        .unwrap_or_else(|_| {
            scatter_wikibase_fusion::badges::BadgeRegistry::default_registry().clone()
        });
    let mut walk =
        triplespace_projections::badges::ClassWalk::new(client, tenant, providers, &badges).await?;
    let entity = &current.resolved.entity;
    let mut put = |id: &EntityId, found: Vec<EntityId>| {
        if !found.is_empty() {
            out.insert(
                id.as_str().to_string(),
                found.iter().map(|b| b.as_str().to_string()).collect(),
            );
        }
    };
    let own = walk.badges(client, app.order(), tenant, entity).await?;
    put(&entity.id, own);
    for id in badged_ids(entity).into_iter().take(MAX_BADGED) {
        if let Some(c) = read::current(client, app.order(), tenant, &id).await? {
            let found = walk
                .badges(client, app.order(), tenant, &c.resolved.entity)
                .await?;
            put(&id, found);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_properties_of_snaks_without_a_data_type_are_found() {
        let entity = json!({"claims": {
            "P7": [{"mainsnak": {"snaktype": "value", "property": "P7"},
                    "qualifiers": {"P9": [{"property": "P9", "datatype": "time"}],
                                   "P4": [{"property": "P4"}]},
                    "references": [{"snaks": {"P5": [{"property": "P5"}]}}]}],
            "P2": [{"mainsnak": {"property": "P2", "datatype": "wikibase-item"}}]
        }});
        let found: Vec<String> = untyped_properties(&entity).into_iter().collect();
        assert_eq!(found, ["P4", "P5", "P7"]);
        assert!(untyped_properties(&json!({"id": "Q1"})).is_empty());
    }

    const OPENAPI: &str = include_str!("../../../docs/api/triplespace-v0.openapi.json");

    // 0012 §5 (A70): the bundle's body is what the contract says.
    #[test]
    fn the_contract_names_the_route_and_the_body_meets_it() {
        let doc: Value = serde_json::from_str(OPENAPI).expect("the OpenAPI document parses");
        let op = &doc["paths"]["/entity/{id}/page"]["get"];
        assert_eq!(
            op["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/PageBundle"
        );
        let schema = &doc["components"]["schemas"]["PageBundle"];
        let plain = body(
            "Q6",
            json!({"id": "Q6"}),
            json!({"id": "Q6"}),
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
            None,
        );
        for key in schema["required"].as_array().unwrap() {
            assert!(
                plain.get(key.as_str().unwrap()).is_some(),
                "{key} is missing"
            );
        }
        assert!(plain.get("visibility").is_none());
        assert!(plain.get("datatypes").is_none());
        assert!(plain.get("redirects").is_none());
        assert!(plain.get("badges").is_none());
        let aliased = body(
            "domain:example.org",
            json!({}),
            json!({}),
            &BTreeMap::from([("P7".to_string(), "external-id".to_string())]),
            Some(json!({"groups": ["staff"]})),
            &BTreeMap::from([("P4".to_string(), vec!["WDQ142241643".to_string()])]),
            Some("XDQ9"),
        );
        assert_eq!(
            aliased["redirects"],
            json!({"from": "XDQ9", "to": "domain:example.org"})
        );
        assert_eq!(aliased["datatypes"], json!({"P7": "external-id"}));
        assert_eq!(aliased["badges"], json!({"P4": ["WDQ142241643"]}));
        for key in aliased.as_object().unwrap().keys() {
            assert!(
                schema["properties"].get(key).is_some(),
                "{key} is not in the contract"
            );
        }
    }
}
