//! `GET /suggest?q=&language=&limit=` (0012 §5; 0014 §8; 03 §11.2, §11.6; 19 §2.1): what
//! the search box offers for a text, in one call.
//!
//! ```json
//! { "q": "lib",
//!   "language": ["en", "mul"],
//!   "goto": {"kind": "entity", "id": "Q42", "title": "Item:Q42"},
//!   "groups": [
//!     {"kind": "item", "hits": [
//!       {"id": "Q7", "title": "Item:Q7", "label": {"language": "en", "value": "library"},
//!        "description": {"language": "en", "value": "a collection of books"},
//!        "match": {"type": "label", "language": "en", "text": "library"}},
//!       {"id": "WDQ7075", "title": "Item:WDQ7075", "provider": "wikidata",
//!        "match": {"type": "alias", "language": "en", "text": "libraries"}}]},
//!     {"kind": "page", "hits": [{"title": "Project:Library", "ns": 4, "pageid": 12}]}] }
//! ```
//!
//! - **Groups** come in a fixed order: items, properties, domains, then pages. A group
//!   with no hit is left out. Each holds at most `limit` hits (4 by default, 20 at most).
//! - **Entities** match by a prefix of a label or alias along the language chain, one per
//!   identity cluster, an exact match first and then the most referred to
//!   ([`triplespace_api_action::modules::search::entity_hits`]). Mirrored entities are
//!   found with local ones; a foreign hit names its `provider`.
//! - **Pages** match by a prefix of their title, over every namespace that holds pages
//!   unless the text begins with a namespace prefix.
//! - **`goto`** is present when the text names an entity by its ID or key (`Q42`,
//!   `wdq42`, `Item:Q42`, `wikipedia.org`), which resolves through its cluster, or is the
//!   exact title of a page; the entity is tried first (10 §3.3). The index is not asked
//!   for it.
//! - `language` is the chain, most wanted first, separated by `|`; where it is absent,
//!   the site's language and its fallbacks.
//! - What the caller may not read is not suggested (09 §5.3; 03 §11.5), so the response
//!   is public only for a caller without credentials. It carries the `Cache-Tag` of each
//!   entity it names.

use axum::extract::{Extension, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::id::IdForm;
use serde_json::{Value, json};
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::modules::entities::parse_id;
use triplespace_api_action::modules::search::{self, EntityHit};
use triplespace_api_action::pages::Site;
use triplespace_api_action::{App, entity_json, tenant};
use triplespace_projections::acl_read::{self, Hidden};

use crate::entity_route::{error, internal};

/// The kinds of entity suggested, in the order of their groups.
pub const KINDS: [&str; 3] = ["item", "property", "domain"];

/// Hits per group unless `limit` says otherwise, and the most it may ask for.
pub const PER_GROUP: u32 = 4;
/// The most hits per group.
pub const MAX_PER_GROUP: u32 = 20;

/// The longest text suggested for.
pub const MAX_TEXT: usize = 255;

/// The parameters: the text, the language chain and the limit, as given.
fn params(query: Option<&str>) -> (String, Vec<String>, Option<String>) {
    let (mut q, mut languages, mut limit) = (String::new(), Vec::new(), None);
    for (k, v) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match k.as_ref() {
            "q" => q = v.trim().to_string(),
            "language" => {
                for l in v.split('|').map(str::trim).filter(|l| !l.is_empty()) {
                    if !languages.iter().any(|x| x == l) {
                        languages.push(l.to_string());
                    }
                }
            }
            "limit" => limit = Some(v.into_owned()),
            _ => {}
        }
    }
    (q, languages, limit)
}

/// Whether a string is shaped as a language code: letters, digits and hyphens.
fn language_code(s: &str) -> bool {
    !s.is_empty() && s.len() <= 42 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The IDs a text may name (19 §2.1): itself as an ID or an entity's title, and, for a
/// dotted word, the domain whose key it is.
fn candidate_ids(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    if q.is_empty() || q.chars().any(char::is_whitespace) {
        return out;
    }
    if let Ok(id) = parse_id(q) {
        out.push(id.as_str().to_string());
    }
    let dotted = q.contains('.')
        && !q.starts_with('.')
        && !q.ends_with('.')
        && q.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-'));
    if dotted && let Ok(id) = parse_id(&format!("domain:{q}")) {
        out.push(id.as_str().to_string());
    }
    out.dedup();
    out
}

/// The entity a text names, where the instance holds it and the caller may read it: the
/// canonical member of its cluster, as `(id, type)`.
async fn named_entity(
    db: &tokio_postgres::Client,
    tenant: &str,
    registry: &scatter_providers::Registry,
    hidden: &Hidden,
    q: &str,
) -> Result<Option<(String, String)>, tokio_postgres::Error> {
    first_held(db, tenant, registry, hidden, candidate_ids(q)).await
}

/// The first of `ids` the instance holds and the caller may read: the canonical member of
/// its cluster, as `(id, type)`.
pub(crate) async fn first_held(
    db: &tokio_postgres::Client,
    tenant: &str,
    registry: &scatter_providers::Registry,
    hidden: &Hidden,
    ids: Vec<String>,
) -> Result<Option<(String, String)>, tokio_postgres::Error> {
    if hidden.every_entity() {
        return Ok(None);
    }
    for raw in ids {
        let Ok(id) = scatter_wikibase_model::id::EntityId::parse(&raw) else {
            continue;
        };
        let id = entity_json::canonical_in(id, registry);
        if id.form() == IdForm::Foreign && registry.parse_foreign_id(id.as_str()).is_err() {
            continue;
        }
        let row = db
            .query_opt(
                "SELECT r.id, r.type
                 FROM view.entity e
                 JOIN LATERAL (
                   SELECT x.id, x.type FROM view.entity x
                   WHERE x.tenant IN ($1, '') AND x.id IN (e.canonical_id, e.id) AND NOT x.deleted
                     AND NOT (x.id = ANY($3)) AND NOT (x.type = ANY($4))
                   ORDER BY (x.id = e.canonical_id) DESC, (x.tenant = $1) DESC
                   LIMIT 1) r ON true
                 WHERE e.tenant IN ($1, '') AND e.id = $2 AND NOT e.deleted
                   AND NOT (e.id = ANY($3)) AND NOT (e.type = ANY($4))
                 ORDER BY (e.tenant = $1) DESC
                 LIMIT 1",
                &[
                    &tenant,
                    &id.as_str(),
                    &hidden.entity_ids(),
                    &hidden.entity_types(),
                ],
            )
            .await?;
        if let Some(r) = row {
            return Ok(Some((r.get(0), r.get(1))));
        }
    }
    Ok(None)
}

/// One entity hit, as the response lists it.
fn entity_entry(h: &EntityHit, registry: &scatter_providers::Registry) -> Value {
    let mut entry = json!({
        "id": h.id,
        "title": h.title(),
        "match": {
            "type": if h.alias { "alias" } else { "label" },
            "language": h.lang,
            "text": h.text,
        },
    });
    if let Some((language, value)) = &h.label {
        entry["label"] = json!({"language": language, "value": value});
    }
    if let Some((language, value)) = &h.description {
        entry["description"] = json!({"language": language, "value": value});
    }
    if !h.id.contains(':')
        && let Ok(f) = registry.parse_foreign_id(&h.id)
    {
        entry["provider"] = json!(f.provider().slug);
    }
    entry
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
    let bad = |code: &str, message: String| error(StatusCode::BAD_REQUEST, code, message);
    let (q, mut chain, limit) = params(query.as_deref());
    if q.is_empty() {
        return bad("param-missing", "`q` is the text to suggest for".into());
    }
    if q.len() > MAX_TEXT {
        return bad("toolong", format!("`q` takes at most {MAX_TEXT} bytes"));
    }
    if let Some(l) = chain.iter().find(|l| !language_code(l)) {
        return bad(
            "invalid-language",
            format!("\"{l}\" is not a language code"),
        );
    }
    if chain.len() > crate::labels::MAX_LANGUAGES {
        return bad(
            "toomanyvalues",
            format!(
                "`language` takes at most {} languages",
                crate::labels::MAX_LANGUAGES
            ),
        );
    }
    let limit = match limit.as_deref().map(str::parse::<u32>) {
        None => PER_GROUP,
        Some(Ok(n)) if (1..=MAX_PER_GROUP).contains(&n) => n,
        Some(_) => {
            return bad(
                "invalid-limit",
                format!("`limit` is a number from 1 to {MAX_PER_GROUP}"),
            );
        }
    };
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
    if chain.is_empty() {
        chain = search::language_chain(&site.lang, false);
    }
    let at = scatter_actors::Timestamp(triplespace_api_action::edit::now_micros());
    let hidden = match acl_read::hidden(client, &tenant.slug, &caller.effective, at).await {
        Ok(h) => h,
        Err(e) => return internal(&e),
    };

    let mut tags: Vec<String> = Vec::new();
    let mut body = json!({"q": q, "language": chain, "groups": []});

    // Go to: the entity the text names, else the page it is the exact title of.
    let named = match named_entity(client, &tenant.slug, registry, &hidden, &q).await {
        Ok(n) => n,
        Err(e) => return internal(&e),
    };
    let pages = match search::page_title_hits(
        client,
        &tenant.slug,
        &site,
        &hidden,
        &q,
        Some(Vec::new()),
        limit,
        0,
    )
    .await
    {
        Ok((p, _)) => p,
        Err(e) => return internal(&e),
    };
    if let Some((id, entity_type)) = named {
        let title = scatter_wikibase_model::id::EntityId::parse(&id).map_or_else(
            |_| id.clone(),
            |e| {
                entity_json::title_of(
                    &e,
                    &scatter_wikibase_model::entity::EntityType::parse(&entity_type),
                )
                .1
            },
        );
        tags.push(format!("entity:{id}"));
        body["goto"] = json!({"kind": "entity", "id": id, "title": title});
    } else if let Some(p) = pages.iter().find(|p| search::names_page(&site, &q, p)) {
        body["goto"] = json!({
            "kind": "page",
            "title": site.prefixed(p.ns, &p.title),
            "ns": p.ns,
            "pageid": p.page_id,
        });
    }

    let mut groups: Vec<Value> = Vec::new();
    for kind in KINDS {
        let hits = match search::entity_hits(
            client,
            &tenant.slug,
            &hidden,
            &chain,
            &q,
            &[kind.to_string()],
            limit,
            0,
        )
        .await
        {
            Ok((h, _)) => h,
            Err(e) => return internal(&e),
        };
        if hits.is_empty() {
            continue;
        }
        tags.extend(hits.iter().map(|h| format!("entity:{}", h.id)));
        let hits: Vec<Value> = hits.iter().map(|h| entity_entry(h, registry)).collect();
        groups.push(json!({"kind": kind, "hits": hits}));
    }
    if !pages.is_empty() {
        let hits: Vec<Value> = pages
            .iter()
            .map(|p| json!({"title": site.prefixed(p.ns, &p.title), "ns": p.ns, "pageid": p.page_id}))
            .collect();
        groups.push(json!({"kind": "page", "hits": hits}));
    }
    body["groups"] = Value::Array(groups);
    tags.sort();
    tags.dedup();

    let credentialed = caller.has_session() || bearer(&headers).is_some();
    let class = Class::Public.permitted(&method, credentialed, false, false);
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &tags,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0012 §5: `q`, a `|`-separated chain, and the limit as given.
    #[test]
    fn the_parameters_are_read() {
        let (q, languages, limit) = params(Some("q=%20lib%20&language=de|mul|de&limit=6"));
        assert_eq!(q, "lib");
        assert_eq!(languages, ["de", "mul"]);
        assert_eq!(limit.as_deref(), Some("6"));
        assert_eq!(params(None), (String::new(), vec![], None));
    }

    // 0012 §5: the route is in the contract, and a response has what the contract
    // requires of one.
    #[test]
    fn the_contract_names_the_route_and_a_hit_meets_it() {
        const OPENAPI: &str = include_str!("../../../docs/api/triplespace-v0.openapi.json");
        let doc: Value = serde_json::from_str(OPENAPI).expect("the OpenAPI document parses");
        let op = &doc["paths"]["/suggest"]["get"];
        assert_eq!(
            op["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Suggestions"
        );
        let hit = EntityHit {
            id: "WDQ42".into(),
            entity_type: "item".into(),
            page_id: 7,
            alias: true,
            lang: "en".into(),
            text: "DNA".into(),
            label: Some(("en".into(), "Douglas Adams".into())),
            description: None,
        };
        let entry = entity_entry(&hit, scatter_providers::Registry::default_registry());
        for key in doc["components"]["schemas"]["SuggestedEntity"]["required"]
            .as_array()
            .unwrap()
        {
            assert!(
                entry.get(key.as_str().unwrap()).is_some(),
                "{key} is missing"
            );
        }
        assert_eq!(entry["title"], "Item:WDQ42");
        assert_eq!(entry["provider"], "wikidata");
        assert_eq!(
            entry["match"],
            json!({"type": "alias", "language": "en", "text": "DNA"})
        );
        assert_eq!(
            entry["label"],
            json!({"language": "en", "value": "Douglas Adams"})
        );
        assert!(entry.get("description").is_none());
    }

    // 19 §2.1: an ID, an entity's title and a bare domain key name an entity; plain
    // words do not.
    #[test]
    fn a_text_may_name_an_entity() {
        assert_eq!(candidate_ids("q42"), ["Q42"]);
        assert_eq!(candidate_ids("Item:Q42"), ["Q42"]);
        // A dotted word may be a domain's key, whatever else its shape suggests.
        assert_eq!(
            candidate_ids("Wikipedia.org").last().map(String::as_str),
            Some("domain:wikipedia.org")
        );
        assert!(candidate_ids("open library").is_empty());
        assert!(candidate_ids("").is_empty());
    }
}
