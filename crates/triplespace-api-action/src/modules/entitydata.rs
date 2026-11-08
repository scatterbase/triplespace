//! `Special:EntityData/{id}.json`, `.ttl` and `.nt` (0001 §1, §3; 0009 §6;
//! special-pages.toml) and the concept URI `/entity/{id}` (0002 §4), which answers 303 to
//! the data document, as Wikibase does, choosing the format by `Accept`.
//!
//! The RDF forms are the entity's triples in Wikibase's flavors (wikibase-compat.md §5;
//! 02 §4.2), rendered by `scatter-wikibase-rdf` from the same document the `.json` form
//! serves, so they answer for the same reader and the same entity. `flavor=` takes
//! `simple`, `long`, `dump` or `full`; absent or empty it is `full`, which adds a stub for
//! every property and entity the statements mention, each read as `.json` would read it,
//! in the reader's label languages; any other value is a 400, as in Wikibase. `.json`
//! ignores `flavor=` (0001 A21).

use std::collections::{BTreeSet, HashSet, VecDeque};

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use scatter_wikibase_model::EntityId;
use scatter_wikibase_model::{Entity, Hasher};
use scatter_wikibase_rdf::{Flavor, IriScheme, Namespaces, Renderer, to_ntriples, to_turtle};

use crate::api::Ctx;
use crate::app::App;
use crate::auth::Caller;
use crate::forwarded::Origin;
use crate::http_cache::{self, Class};
use crate::modules::entities::{entity_json, parse_id};
use crate::params::Params;
use crate::response::ApiError;
use crate::tenant;

fn plain(status: StatusCode, text: String) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        text,
    )
        .into_response()
}

async fn context(app: App, origin: Origin, headers: &HeaderMap) -> Result<Ctx, Response> {
    let client = app
        .pool()
        .get()
        .await
        .map_err(|e| plain(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let tenant = match tenant::resolve(&app, &**client, &origin, headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(plain(
                StatusCode::MISDIRECTED_REQUEST,
                "not a tenant of this instance\n".into(),
            ));
        }
        Err(e) => return Err(plain(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    };
    let caller = Caller::resolve(&**client, &tenant.slug, headers)
        .await
        .map_err(|e| plain(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Ctx {
        app,
        client,
        tenant,
        origin,
        params: Params::default(),
        caller,
        formatversion: 2,
        set_cookie: None,
        warnings: Vec::new(),
        cache_class: crate::http_cache::Class::Public,
        cache_tags: Vec::new(),
        accept_language: None,
    })
}

/// The forms the data document is served in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Turtle,
    NTriples,
}

impl Format {
    /// The format a file name asks for, and the ID before its extension.
    fn of(file: &str) -> Option<(&str, Self)> {
        let (id, ext) = file.rsplit_once('.')?;
        let format = match ext {
            "json" => Self::Json,
            "ttl" => Self::Turtle,
            "nt" => Self::NTriples,
            _ => return None,
        };
        Some((id, format))
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Turtle => "ttl",
            Self::NTriples => "nt",
        }
    }

    const fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json; charset=utf-8",
            Self::Turtle => "text/turtle; charset=utf-8",
            Self::NTriples => "application/n-triples; charset=utf-8",
        }
    }

    /// The format an `Accept` header prefers among those served, JSON by default.
    fn accepted(headers: &HeaderMap) -> Self {
        let accept = headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        let mut best = (0.0_f32, Self::Json);
        for item in accept.split(',') {
            let mut parts = item.split(';').map(str::trim);
            let media = parts.next().unwrap_or_default().to_ascii_lowercase();
            let q = parts
                .find_map(|p| p.strip_prefix("q="))
                .and_then(|q| q.parse::<f32>().ok())
                .unwrap_or(1.0);
            let format = match media.as_str() {
                "application/json" => Self::Json,
                "text/turtle" => Self::Turtle,
                "application/n-triples" => Self::NTriples,
                _ => continue,
            };
            if q > best.0 {
                best = (q, format);
            }
        }
        best.1
    }
}

/// The languages a stub's labels and descriptions are written in: the reader's label
/// languages, else their language, then English, standing in for Wikibase's fallback
/// chain (`getFetchLanguageCodes`). `uselang` sets the language for one request.
async fn stub_languages(ctx: &Ctx, uselang: Option<&str>) -> Vec<String> {
    let resolved = crate::prefs::resolve(
        ctx.db(),
        &ctx.tenant.slug,
        ctx.caller.identity.actor_key.as_deref(),
        uselang,
        ctx.accept_language.as_deref(),
    )
    .await
    .ok();
    let value = |key: &str| {
        resolved
            .as_ref()
            .and_then(|r| r.values.get(key))
            .and_then(|p| p.value.clone())
    };
    let mut languages: Vec<String> = value("label-languages")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if languages.is_empty()
        && let Some(lang) = uselang
            .map(str::to_string)
            .or_else(|| value("language").and_then(|v| v.as_str().map(str::to_string)))
    {
        languages.push(lang);
    }
    if !languages.iter().any(|l| l == "en") {
        languages.push("en".to_string());
    }
    languages
}

/// The entity's triples as Turtle or N-Triples, from its Wikibase JSON, in `flavor`; under
/// `full`, followed by a stub for each mention, read as `.json` reads it: a mention the
/// reader may not see, or that does not exist, gets nothing, and one that is an alias gets
/// `owl:sameAs` to its target and the target's stub (Wikibase's
/// `resolveMentionedEntities`).
async fn rdf(
    ctx: &Ctx,
    registry: &scatter_providers::Registry,
    entity: Value,
    format: Format,
    flavor: Flavor,
    uselang: Option<&str>,
) -> Result<String, String> {
    // Rendered from the resolved view itself, as `.json` is, with the resolver's record of
    // which graphs assert each statement, so that a mirrored statement is named as its
    // upstream names it (0002 A43). An entity with no record of its own (a keyed entity
    // nothing has written to) is rendered from the document.
    let parsed = Entity::from_value(entity).map_err(|e| e.to_string())?;
    let current = crate::edit::current(ctx, &parsed.entity.id)
        .await
        .map_err(|e| format!("{}: {}", e.code, e.info))?;
    let (entity, page, upstream) = match &current {
        Some(current) => {
            let (entity, page) = crate::entity_json::served(current);
            let upstream = crate::entity_json::upstream_statements(current, registry);
            (entity, Some(page), upstream)
        }
        None => (parsed.entity, parsed.page, BTreeSet::new()),
    };
    let iris = IriScheme::new(
        Namespaces::triplespace(&ctx.tenant.base),
        registry,
        scatter_normalize::KeyedRegistry::default_registry(),
    );
    let hasher = Hasher::local();
    let renderer = Renderer::new(&iris, &hasher, flavor.produce()).with_upstream(&upstream);
    let mut triples = renderer.entity(&entity, page.as_ref());

    let mut queue: VecDeque<EntityId> = renderer.mentions(&entity).into();
    if !queue.is_empty() {
        let languages = stub_languages(ctx, uselang).await;
        let mut resolved: HashSet<EntityId> = HashSet::from([entity.id.clone()]);
        while let Some(id) = queue.pop_front() {
            if !resolved.insert(id.clone()) {
                continue;
            }
            let Ok(v) = entity_json(ctx, &id, id.as_str()).await else {
                continue;
            };
            if v.get("missing").is_some() {
                continue;
            }
            let Ok(mentioned) = Entity::from_value(v) else {
                continue;
            };
            let target = mentioned.entity.id.clone();
            if target != id {
                triples.push(renderer.redirect(&id, &target));
                if !resolved.insert(target) {
                    continue;
                }
            }
            triples.extend(renderer.stub(&mentioned.entity, &languages));
        }
    }
    Ok(match format {
        Format::NTriples => to_ntriples(&triples),
        _ => to_turtle(&iris, &triples),
    })
}

/// The query parameters `Special:EntityData` reads.
#[derive(Debug, Default, serde::Deserialize)]
pub struct DataQuery {
    /// The RDF flavor.
    flavor: Option<String>,
    /// The language stubs are written in, for this request.
    uselang: Option<String>,
}

/// `GET /wiki/Special:EntityData/{id}.json`, `.ttl` or `.nt`: the entity document, or 404.
pub async fn special_entity_data(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(file): Path<String>,
    Query(query): Query<DataQuery>,
) -> Response {
    let Some((id_text, format)) = Format::of(&file) else {
        return plain(
            StatusCode::NOT_ACCEPTABLE,
            "the .json, .ttl and .nt forms are served\n".into(),
        );
    };
    // `flavor=` is read for the RDF forms only, as Wikibase reads it; an unknown one is a
    // 400 naming the four it knows.
    let flavor = if format == Format::Json {
        Flavor::Full
    } else {
        match Flavor::parse(query.flavor.as_deref()) {
            Some(f) => f,
            None => {
                return plain(
                    StatusCode::BAD_REQUEST,
                    format!(
                        "unknown flavor `{}`; the flavors are {}\n",
                        query.flavor.as_deref().unwrap_or_default(),
                        Flavor::NAMES.join("|")
                    ),
                );
            }
        }
    };
    let ctx = match context(app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let id = match parse_id(id_text) {
        Ok(id) => id,
        Err(e) => return plain(StatusCode::BAD_REQUEST, format!("{}\n", e.info)),
    };
    match entity_json(&ctx, &id, id_text).await {
        Ok(v) if v.get("missing").is_some() => plain(
            StatusCode::NOT_FOUND,
            format!("no entity {}\n", id.as_str()),
        ),
        Ok(v) => {
            // Keyed by the entity's own ID: an alias (`XDQ9`) answers as its target.
            let key = v
                .get("id")
                .and_then(Value::as_str)
                .map_or_else(|| id.as_str().to_string(), str::to_string);
            let body = if format == Format::Json {
                json!({"entities": {key: v}}).to_string()
            } else {
                let registry = match ctx.registry().await {
                    Ok(r) => r,
                    Err(ApiError { code, info, .. }) => {
                        return plain(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("{code}: {info}\n"),
                        );
                    }
                };
                match rdf(&ctx, registry, v, format, flavor, query.uselang.as_deref()).await {
                    Ok(text) => text,
                    Err(e) => return plain(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}\n")),
                }
            };
            let credentialed = ctx.caller.has_session() || crate::auth::bearer(&headers).is_some();
            let class = Class::Public.permitted(&Method::GET, credentialed, false, false);
            http_cache::respond(
                &headers,
                StatusCode::OK,
                format.content_type(),
                body,
                class,
                &[format!("entity:{}", id.as_str())],
            )
        }
        Err(ApiError { code, info, .. }) => plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{code}: {info}\n"),
        ),
    }
}

/// `GET /entity/{id}`: 303 to the data document, in the format `Accept` prefers.
pub async fn concept_uri(headers: HeaderMap, Path(id): Path<String>) -> Response {
    match parse_id(&id) {
        Ok(parsed) => (
            StatusCode::SEE_OTHER,
            [
                (
                    header::LOCATION,
                    format!(
                        "/wiki/Special:EntityData/{}.{}",
                        parsed.as_str(),
                        Format::accepted(&headers).extension()
                    ),
                ),
                (header::VARY, "Accept".to_string()),
            ],
        )
            .into_response(),
        Err(e) => plain(StatusCode::BAD_REQUEST, format!("{}\n", e.info)),
    }
}
