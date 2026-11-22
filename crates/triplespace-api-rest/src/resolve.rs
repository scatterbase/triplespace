//! `GET /resolve?q=` (18 §3.3; 06 §3.3): any string to the page it names, through the
//! title resolver.
//!
//! ```json
//! { "q": "doi:10.1000/XYZ", "kind": "resolver", "resolver": "doi", "label": "DOI",
//!   "namespace": "DOI", "key": "10.1000/xyz", "external": "https://doi.org/10.1000/xyz",
//!   "properties": ["WDP356"],
//!   "target": {"id": "WDQ5", "title": "Item:WDQ5"} }
//! ```
//!
//! The forms are tried in order, and the first that names something answers:
//!
//! 1. **An entity ID** the instance holds (`WDQ65`, `Item:Q6`, `domain:example.org`),
//!    reached through its cluster: `kind: entity`.
//! 2. **A notation key** (`osm:amenity=cafe`), the one bare key that resolves (0029 A9):
//!    `kind: entity`.
//! 3. **A resolver-prefixed string** (`doi:10.1000/xyz`, `DOI:10.1000/xyz`), then **a bare
//!    string** that is a valid key of exactly one resolver (`10.1000/xyz`): `kind:
//!    resolver`, with `target` when exactly one entity holds exactly one value for the
//!    key, and `candidates` otherwise: none, several entities, or one entity with each of
//!    its values (`values`). A prefixed string that is not a valid key has `invalid`, why.
//! 4. **A main-namespace title**: `kind: page`. Where an entity answered first, a
//!    main-namespace page with the same title is named in `also`.
//!
//! Nothing named is `404 unresolved`, with `suggestions`: the keyed-type ID a bare key
//! would be (`domain:example.org`), since bare keys are not IDs (0017 §1). Only the
//! resolvers bound to a property serve here; one bound to sitelinks (`URL:`) is 0029 §5's.
//! What the caller may not read is not resolved to, so the response is public only for a
//! caller without credentials. It carries the `Cache-Tag` of each entity it names.

use axum::extract::{Extension, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::entity::EntityType;
use scatter_wikibase_model::id::EntityId;
use serde_json::{Value, json};
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::modules::entities::parse_id;
use triplespace_api_action::modules::search;
use triplespace_api_action::pages::Site;
use triplespace_api_action::{App, entity_json, tenant};
use triplespace_projections::acl_read;
use triplespace_projections::resolvers::{self, Found, Keyed};

use crate::entity_route::{error, internal};
use crate::suggest::first_held;

/// The longest string resolved.
pub const MAX_TEXT: usize = 512;

/// The notation entity a string names as a key (`osm:amenity=cafe`; 0048 §2).
fn notation_id(q: &str) -> Option<String> {
    resolvers::is_notation_key(q)
        .then(|| parse_id(&format!("notation:{q}")).ok())
        .flatten()
        .map(|id| id.as_str().to_string())
}

/// The keyed-type IDs a bare key would be, offered rather than resolved (0017 §1).
fn suggestions(q: &str) -> Vec<String> {
    let dotted = q.contains('.')
        && !q.contains(':')
        && !q.starts_with('.')
        && !q.ends_with('.')
        && q.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-'));
    if !dotted {
        return Vec::new();
    }
    parse_id(&format!("domain:{q}"))
        .ok()
        .map(|id| vec![id.as_str().to_string()])
        .unwrap_or_default()
}

/// An entity's title, by its ID and type.
fn title(id: &str, entity_type: &str) -> String {
    EntityId::parse(id).map_or_else(
        |_| id.to_string(),
        |e| entity_json::title_of(&e, &EntityType::parse(entity_type)).1,
    )
}

/// The response for a resolver's key (06 §3.3 step 3).
fn resolver_body(
    q: &str,
    r: &resolvers::Resolver,
    properties: &[&str],
    key: &str,
    found: &[Found],
) -> Value {
    let mut body = json!({
        "q": q,
        "kind": "resolver",
        "resolver": r.name,
        "label": r.label,
        "namespace": r.namespace,
        "key": key,
        "external": r.external_iri(key),
        "properties": properties,
    });
    match found {
        [one] if one.values.len() <= 1 => {
            body["target"] = json!({"id": one.id, "title": title(&one.id, &one.entity_type)});
        }
        _ => {
            body["candidates"] = found
                .iter()
                .map(|f| {
                    json!({
                        "id": f.id,
                        "title": title(&f.id, &f.entity_type),
                        "values": f.values,
                    })
                })
                .collect();
        }
    }
    body
}

/// The handler.
#[allow(clippy::too_many_lines)]
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let q = url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .find(|(k, _)| k == "q")
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default();
    if q.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "param-missing",
            "`q` is the string to resolve",
        );
    }
    if q.len() > MAX_TEXT {
        return error(
            StatusCode::BAD_REQUEST,
            "toolong",
            format!("`q` takes at most {MAX_TEXT} bytes"),
        );
    }
    let pooled = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let client: &tokio_postgres::Client = &pooled;
    let tenant = match tenant::resolve(&app, client, &origin, &headers).await {
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
    let caller = match Caller::resolve(client, &tenant.slug, &headers).await {
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
    let registry = match app.registry_for(client, &tenant.slug).await {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let site = match Site::load_for(client, &tenant.slug).await {
        Ok(s) => s,
        Err(e) => return internal(&e.info),
    };
    let at = scatter_actors::Timestamp(triplespace_api_action::edit::now_micros());
    let hidden = match acl_read::hidden(client, &tenant.slug, &caller.effective, at).await {
        Ok(h) => h,
        Err(e) => return internal(&e),
    };
    let credentialed = caller.has_session() || bearer(&headers).is_some();
    let class = Class::Public.permitted(&method, credentialed, false, false);
    let answer = |body: Value, tags: &[String]| {
        http_cache::respond(
            &headers,
            StatusCode::OK,
            "application/json; charset=utf-8",
            body.to_string(),
            class,
            tags,
        )
    };

    // The main-namespace page with this exact title, tried last or named in `also`.
    let page = match search::page_title_hits(
        client,
        &tenant.slug,
        &site,
        &hidden,
        &q,
        Some(vec![0]),
        1,
        0,
    )
    .await
    {
        Ok((p, _)) => p
            .into_iter()
            .find(|p| p.ns == 0 && search::names_page(&site, &q, p))
            .map(|p| json!({"kind": "page", "title": site.prefixed(p.ns, &p.title), "ns": p.ns, "pageid": p.page_id})),
        Err(e) => return internal(&e),
    };

    // 1–2. An entity ID, then a notation key.
    let mut ids = Vec::new();
    if !q.chars().any(char::is_whitespace)
        && let Ok(id) = parse_id(&q)
    {
        ids.push(id.as_str().to_string());
    }
    ids.extend(notation_id(&q));
    match first_held(client, &tenant.slug, registry, &hidden, ids).await {
        Ok(Some((id, entity_type))) => {
            let mut body = json!({
                "q": q,
                "kind": "entity",
                "id": id,
                "title": title(&id, &entity_type),
            });
            if let Some(p) = page {
                body["also"] = p;
            }
            return answer(body, &[format!("entity:{id}")]);
        }
        Ok(None) => {}
        Err(e) => return internal(&e),
    }

    // 3. A resolver's key, prefixed or bare.
    let bound = resolvers::bound();
    match resolvers::keyed(&q, &resolvers::serving()) {
        Some(Keyed::Key(r, key)) => {
            let properties: Vec<&str> = bound.properties(&r.name).collect();
            let found =
                match resolvers::lookup(client, &tenant.slug, &hidden, &properties, &key).await {
                    Ok(f) => f,
                    Err(e) => return internal(&e),
                };
            let tags: Vec<String> = found.iter().map(|f| format!("entity:{}", f.id)).collect();
            return answer(resolver_body(&q, r, &properties, &key, &found), &tags);
        }
        Some(Keyed::Invalid(r, why)) => {
            return answer(
                json!({
                    "q": q,
                    "kind": "resolver",
                    "resolver": r.name,
                    "label": r.label,
                    "namespace": r.namespace,
                    "invalid": why,
                }),
                &[],
            );
        }
        None => {}
    }

    // 4. A main-namespace title.
    if let Some(mut p) = page {
        p["q"] = json!(q);
        return answer(p, &[]);
    }
    let mut r = error(
        StatusCode::NOT_FOUND,
        "unresolved",
        format!("\"{q}\" names nothing this wiki holds"),
    );
    let s = suggestions(&q);
    if !s.is_empty() {
        let body = json!({
            "code": "unresolved",
            "message": format!("\"{q}\" names nothing this wiki holds"),
            "suggestions": s,
        })
        .to_string();
        *r.body_mut() = axum::body::Body::from(body);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doi() -> &'static resolvers::Resolver {
        resolvers::serving()[0]
    }

    // 0017 §1: a bare domain name is offered as the keyed-type ID, not resolved.
    #[test]
    fn a_bare_domain_is_a_suggestion() {
        assert_eq!(suggestions("Example.org"), ["domain:example.org"]);
        assert!(suggestions("Douglas Adams").is_empty());
        assert!(suggestions("doi:10.1000/x").is_empty());
    }

    // 06 §3.3 step 3: one entity with one value is the target; anything else lists.
    #[test]
    fn only_one_entity_with_one_value_is_a_target() {
        let r = doi();
        let f = |id: &str, values: &[&str]| Found {
            id: id.into(),
            entity_type: "item".into(),
            values: values.iter().map(|v| (*v).to_string()).collect(),
        };
        let one = resolver_body(
            "q",
            r,
            &["WDP356"],
            "10.1000/x",
            &[f("WDQ5", &["10.1000/x"])],
        );
        assert_eq!(one["target"], json!({"id": "WDQ5", "title": "Item:WDQ5"}));
        assert_eq!(one["external"], "https://doi.org/10.1000/x");
        let two_values = resolver_body(
            "q",
            r,
            &["WDP356"],
            "10.1000/x",
            &[f("WDQ5", &["10.1000/x", "10.1000/y"])],
        );
        assert!(two_values.get("target").is_none());
        assert_eq!(
            two_values["candidates"][0]["values"],
            json!(["10.1000/x", "10.1000/y"])
        );
        let none = resolver_body("q", r, &["WDP356"], "10.1000/x", &[]);
        assert_eq!(none["candidates"], json!([]));
    }
}
