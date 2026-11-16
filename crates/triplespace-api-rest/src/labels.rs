//! `GET /labels?ids=&language=`, bulk labels (0012 A70; 19 §5.1): for up to 1,000 entity
//! and property IDs, each one's label in a language chain and nothing else, so that an
//! entity page names everything it links to in one call.
//!
//! ```json
//! { "language": ["de", "mul", "en"],
//!   "labels": { "Q6": {"language": "de", "value": "Haus"}, "P31": {"language": "en", "value": "instance of"} } }
//! ```
//!
//! - `ids` are separated by `|`, in any form the Action API accepts, and the answer is
//!   keyed by each ID as it was given. An ID with no label in the chain, or that names
//!   no entity, is left out.
//! - `language` is the chain, most wanted first, separated by `|`: `de|mul|en`. The route
//!   applies no fallbacks of its own, so the response is a function of the two
//!   parameters.
//! - Each ID is resolved through its identity cluster (0004 §4): where the entity has no
//!   label in a language, the cluster's canonical member's is taken, then another
//!   member's.
//! - **The public form only.** The labels are those anyone may read: an entity under a
//!   `read` restriction, a deleted one, and every entity of a private wiki is left out,
//!   whoever asks. The response is therefore cacheable by its URL and not by viewer.
//! - Labels come from `view.term` (03 §5). A mirrored entity has rows there in the
//!   instance's term languages only; where the chain asks for another language first,
//!   the entity's record is read for it, as `wbgetentities` reads it.
//! - A keyed entity's derived label (0009 §5) stands in for a `mul` label it lacks.

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::{Extension, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::entity::{Entity, EntityType};
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde_json::{Map, Value, json};
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::modules::entities::parse_id;
use triplespace_api_action::{App, entity_json, tenant};
use triplespace_projections::acl_read::{self, Hidden};
use triplespace_projections::entities::SHARED_TERM_LANGUAGES;
use triplespace_projections::read;

use crate::entity_route::{error, internal};

/// The most IDs one call takes.
pub const MAX_IDS: usize = 1000;

/// The most languages a chain holds.
pub const MAX_LANGUAGES: usize = 24;

/// The parameters of a request: `ids` and `language`, each a `|`-separated list.
fn params(query: Option<&str>) -> (Vec<String>, Vec<String>) {
    let (mut ids, mut languages) = (Vec::new(), Vec::new());
    for (k, v) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        let into = match k.as_ref() {
            "ids" => &mut ids,
            "language" => &mut languages,
            _ => continue,
        };
        for part in v.split('|').map(str::trim).filter(|p| !p.is_empty()) {
            if !into.iter().any(|x| x == part) {
                into.push(part.to_string());
            }
        }
    }
    (ids, languages)
}

/// Whether a string is shaped as a language code: letters, digits and hyphens.
fn language_code(s: &str) -> bool {
    !s.is_empty() && s.len() <= 42 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A label and where it stands in the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Label {
    /// The position of its language in the chain.
    at: usize,
    text: String,
}

/// The better of a label found and a candidate: the earlier in the chain; a tie keeps
/// what was found.
fn better(found: Option<Label>, candidate: Option<Label>) -> Option<Label> {
    match (found, candidate) {
        (Some(f), Some(c)) if c.at < f.at => Some(c),
        (Some(f), _) => Some(f),
        (None, c) => c,
    }
}

/// The first label along the chain among an entity's.
fn along(labels: &BTreeMap<String, String>, chain: &[String]) -> Option<Label> {
    chain.iter().enumerate().find_map(|(at, l)| {
        labels.get(l).map(|text| Label {
            at,
            text: text.clone(),
        })
    })
}

/// A keyed entity's derived label (0009 §5), as the `mul` label of the chain.
fn derived(id: &EntityId, chain: &[String]) -> Option<Label> {
    let (t, _) = id.keyed_parts()?;
    let mut entity = Entity::new(id.clone(), EntityType::parse(t));
    entity_json::add_derived_terms(&mut entity);
    along(&entity.labels, chain)
}

/// One row of the lookup: an ID asked for that the instance holds.
struct Row {
    id: String,
    /// Whether the row serving it is the shared one, a mirrored entity's.
    shared: bool,
    graph: Option<String>,
    label: Option<Label>,
}

/// The labels of the entities held, from `view.term`: the tenant's own row over the
/// shared one, the entity's own label over its cluster's.
async fn lookup(
    client: &tokio_postgres::Client,
    tenant: &str,
    ids: &[String],
    chain: &[String],
) -> Result<Vec<Row>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT a.id, e.tenant = '' AS shared,
                    (SELECT s.graph FROM view.entity_source s
                     WHERE s.entity_id = a.id AND s.tenant IN ($1, '')
                     ORDER BY s.graph <> 'local', s.graph LIMIT 1) AS graph,
                    t.lang, t.text
             FROM unnest($2::text[]) AS a(id)
             JOIN LATERAL (
               SELECT x.tenant, x.cluster_id, x.canonical_id FROM view.entity x
               WHERE x.id = a.id AND x.tenant IN ($1, '') AND NOT x.deleted
               ORDER BY (x.tenant = $1) DESC LIMIT 1) e ON true
             LEFT JOIN LATERAL (
               SELECT x.lang, x.text FROM view.term x
               WHERE x.tenant IN ($1, '') AND x.kind = 1 AND x.lang = ANY($3)
                 AND x.entity_id = ANY(
                   ARRAY[a.id, e.canonical_id] || CASE WHEN e.cluster_id IS NULL
                     THEN ARRAY[]::text[]
                     ELSE ARRAY(SELECT m.id FROM view.entity m
                                WHERE m.cluster_id = e.cluster_id AND m.tenant IN ($1, '')
                                  AND NOT m.deleted)
                   END)
               ORDER BY array_position($3, x.lang), (x.entity_id = a.id) DESC,
                        (x.entity_id = e.canonical_id) DESC, (x.tenant = $1) DESC, x.entity_id
               LIMIT 1) t ON true",
            &[&tenant, &ids, &chain],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let lang: Option<String> = r.get(3);
            let text: Option<String> = r.get(4);
            let label = lang.zip(text).and_then(|(lang, text)| {
                chain
                    .iter()
                    .position(|l| *l == lang)
                    .map(|at| Label { at, text })
            });
            Row {
                id: r.get(0),
                shared: r.get(1),
                graph: r.get(2),
                label,
            }
        })
        .collect())
}

/// The languages a mirrored entity's labels are held in `view.term` for (05 §5.8).
async fn shared_languages(
    client: &tokio_postgres::Client,
) -> Result<BTreeSet<String>, tokio_postgres::Error> {
    let rows = client
        .query(
            &format!("WITH {SHARED_TERM_LANGUAGES} SELECT lang FROM shared_term_languages"),
            &[],
        )
        .await?;
    Ok(rows.iter().filter_map(|r| r.get(0)).collect())
}

/// The response body.
fn body(chain: &[String], labels: &BTreeMap<String, Label>) -> Value {
    let mut out = Map::new();
    for (id, l) in labels {
        out.insert(
            id.clone(),
            json!({"language": chain[l.at], "value": l.text}),
        );
    }
    json!({"language": chain, "labels": out})
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
    let (given, chain) = params(query.as_deref());
    if given.is_empty() {
        return bad(
            "param-missing",
            "`ids` names the entities, separated by `|`".into(),
        );
    }
    if given.len() > MAX_IDS {
        return bad(
            "toomanyvalues",
            format!(
                "`ids` takes at most {MAX_IDS} IDs, and {} were given",
                given.len()
            ),
        );
    }
    if chain.is_empty() {
        return bad(
            "param-missing",
            "`language` names the language chain, most wanted first, separated by `|`".into(),
        );
    }
    if chain.len() > MAX_LANGUAGES {
        return bad(
            "toomanyvalues",
            format!("`language` takes at most {MAX_LANGUAGES} languages"),
        );
    }
    if let Some(l) = chain.iter().find(|l| !language_code(l)) {
        return bad(
            "invalid-language",
            format!("\"{l}\" is not a language code"),
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
    // Each ID as given, with the entity it names: a key-mapped provider's own item ID
    // stands for the keyed entity it was written under (05 §4.1).
    let mut asked: Vec<(String, EntityId)> = Vec::with_capacity(given.len());
    for g in &given {
        let id = match parse_id(g) {
            Ok(id) => entity_json::canonical_in(id, registry),
            Err(e) => return bad("invalid-entity-id", e.info),
        };
        if id.form() == IdForm::Foreign && registry.parse_foreign_id(id.as_str()).is_err() {
            return bad(
                "invalid-entity-id",
                format!("\"{}\" names no registered provider", id.as_str()),
            );
        }
        asked.push((g.clone(), id));
    }
    // What anyone may read (09 §5.3): the hidden set of a caller with no account.
    let at = scatter_actors::Timestamp(triplespace_api_action::edit::now_micros());
    let hidden: Hidden = match acl_read::hidden(
        client,
        &tenant.slug,
        &scatter_actors::Effective::default(),
        at,
    )
    .await
    {
        Ok(h) => h,
        Err(e) => return internal(&e),
    };
    let mut labels: BTreeMap<String, Label> = BTreeMap::new();
    if !hidden.every_entity() {
        let mut ids: Vec<String> = asked
            .iter()
            .map(|(_, id)| id.as_str().to_string())
            .collect();
        ids.sort();
        ids.dedup();
        let mut rows = match lookup(client, &tenant.slug, &ids, &chain).await {
            Ok(r) => r,
            Err(e) => return internal(&e),
        };
        // An ID the instance holds nothing under may be a key-mapped provider's item.
        let held: BTreeSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let mut aliases: BTreeMap<String, String> = BTreeMap::new();
        for (_, id) in &asked {
            if held.contains(id.as_str()) || id.form() != IdForm::Foreign {
                continue;
            }
            match read::mapped_alias(client, registry, &tenant.slug, id).await {
                Ok(Some(target)) => {
                    aliases.insert(id.as_str().to_string(), target.as_str().to_string());
                }
                Ok(None) => {}
                Err(e) => return internal(&e),
            }
        }
        if !aliases.is_empty() {
            let mut targets: Vec<String> = aliases
                .values()
                .filter(|t| !held.contains(t.as_str()))
                .cloned()
                .collect();
            targets.sort();
            targets.dedup();
            if !targets.is_empty() {
                match lookup(client, &tenant.slug, &targets, &chain).await {
                    Ok(more) => rows.extend(more),
                    Err(e) => return internal(&e),
                }
            }
        }
        // The chain's languages `view.term` does not hold for a mirrored entity: where
        // one comes before the label found, the record has the last word.
        let shared = match shared_languages(client).await {
            Ok(s) => s,
            Err(e) => return internal(&e),
        };
        let first_unheld = chain.iter().position(|l| !shared.contains(l));
        let mut by_id: BTreeMap<String, Option<Label>> = BTreeMap::new();
        for row in rows {
            if hidden.entity(&row.id, row.graph.as_deref()) {
                continue;
            }
            let mut label = row.label;
            let Ok(id) = EntityId::parse(&row.id) else {
                continue;
            };
            let unread = first_unheld.filter(|u| label.as_ref().is_none_or(|l| l.at > *u));
            if row.shared && unread.is_some() {
                match read::current(client, app.order(), &tenant.slug, &id).await {
                    Ok(Some(current)) => {
                        label = better(label, along(&current.resolved.entity.labels, &chain));
                    }
                    Ok(None) => {}
                    Err(e) => return internal(&e),
                }
            }
            label = better(label, derived(&id, &chain));
            by_id.insert(row.id, label);
        }
        for (given, id) in &asked {
            let key = aliases.get(id.as_str()).map_or(id.as_str(), String::as_str);
            let label = match by_id.get(key) {
                Some(l) => l.clone(),
                // A keyed ID no graph holds is still an entity (0009 §4); a source's
                // is not.
                None if id.form() == IdForm::Keyed
                    && !entity_json::is_source_id(id, registry)
                    && !hidden.entity(id.as_str(), None) =>
                {
                    derived(id, &chain)
                }
                None => None,
            };
            if let Some(l) = label {
                labels.insert(given.clone(), l);
            }
        }
    }
    let body = body(&chain, &labels).to_string();
    let credentialed = caller.has_session() || bearer(&headers).is_some();
    let class = Class::Public.permitted(&method, credentialed, false, false);
    // No tag per entity: a label's staleness is accepted up to `s-maxage`, and a tag
    // for each of a thousand entities would exceed every proxy's header limit (20 §6).
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body,
        class,
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENAPI: &str = include_str!("../../../docs/api/triplespace-v0.openapi.json");

    fn chain(c: &[&str]) -> Vec<String> {
        c.iter().map(|s| (*s).to_string()).collect()
    }

    // 0012 §5 (A70): the parameters are lists separated by `|`, each value once.
    #[test]
    fn the_parameters_are_pipe_separated_lists() {
        let (ids, languages) = params(Some("ids=Q1%7CP2|Q1||%20Q3&language=de|mul|en&x=1"));
        assert_eq!(ids, ["Q1", "P2", "Q3"]);
        assert_eq!(languages, ["de", "mul", "en"]);
        assert_eq!(params(None), (vec![], vec![]));
        assert!(language_code("zh-hant"));
        assert!(!language_code("de;x"));
        assert!(!language_code(""));
    }

    #[test]
    fn the_earlier_language_wins_and_a_keyed_id_has_its_derived_label() {
        let c = chain(&["de", "mul", "en"]);
        let mut labels = BTreeMap::new();
        labels.insert("en".to_string(), "house".to_string());
        let en = along(&labels, &c);
        assert_eq!(en.as_ref().map(|l| l.at), Some(2));
        labels.insert("de".to_string(), "Haus".to_string());
        let de = along(&labels, &c);
        assert_eq!(better(en.clone(), de.clone()), de);
        assert_eq!(better(de.clone(), en.clone()), de);
        assert_eq!(better(None, en.clone()), en);
        let id = EntityId::parse("domain:example.org").unwrap();
        let d = derived(&id, &c).expect("a domain has a derived label");
        assert_eq!((d.at, d.text.as_str()), (1, "example.org"));
        assert!(derived(&id, &chain(&["de"])).is_none());
        assert!(derived(&EntityId::parse("Q6").unwrap(), &c).is_none());
    }

    #[test]
    fn the_contract_names_the_route_and_the_body_meets_it() {
        let doc: Value = serde_json::from_str(OPENAPI).expect("the OpenAPI document parses");
        let op = &doc["paths"]["/labels"]["get"];
        assert_eq!(
            op["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Labels"
        );
        let schema = &doc["components"]["schemas"]["Labels"];
        let c = chain(&["de", "en"]);
        let mut labels = BTreeMap::new();
        labels.insert(
            "Q6".to_string(),
            Label {
                at: 1,
                text: "house".into(),
            },
        );
        let b = body(&c, &labels);
        for key in schema["required"].as_array().unwrap() {
            assert!(b.get(key.as_str().unwrap()).is_some(), "{key} is missing");
        }
        assert_eq!(b["language"], json!(["de", "en"]));
        assert_eq!(
            b["labels"]["Q6"],
            json!({"language": "en", "value": "house"})
        );
        let label = &doc["components"]["schemas"]["Label"];
        for key in label["required"].as_array().unwrap() {
            assert!(b["labels"]["Q6"].get(key.as_str().unwrap()).is_some());
        }
    }
}
