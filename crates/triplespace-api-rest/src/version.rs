//! `GET /version` and `GET /version/licenses/{id}` (ADR 0077 §10): everything
//! `Special:Version` shows, and one component's licence texts.
//!
//! Built from three sources (0077 §1):
//!
//! - `docs/registry/version.toml`, embedded: the developer, contributors, funders, AI
//!   agents, services, feature switches and extension pages;
//! - the binary's build and components ([`triplespace_api_action::version::BuildInfo`]);
//! - the registries that record an `origin` (`special-pages.toml`, `content-models.toml`,
//!   `wikitext-functions.toml`), embedded, for the lineage of §8 and the wikitext of §7;
//!
//! and from the instance's settings and the services' last probes. The response names no
//! ADRs (0077 A1), and lists only the services the instance is configured to use (0077
//! A2). Rendering it contacts no service: the probes are the server's (0077 §4).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_api_action::version::{self as v, Disclosure};

const VERSION_TOML: &str = include_str!("../../../docs/registry/version.toml");
const SPECIAL_PAGES_TOML: &str = include_str!("../../../docs/registry/special-pages.toml");
const CONTENT_MODELS_TOML: &str = include_str!("../../../docs/registry/content-models.toml");
const WIKITEXT_TOML: &str = include_str!("../../../docs/registry/wikitext-functions.toml");

/// The product.
pub const PRODUCT: &str = "Triplespace";
/// Its licence.
pub const LICENSE: &str = "GPL-3.0-or-later";

/// The embedded registries, parsed once. A registry that does not parse fails the tests,
/// not a request.
struct Registries {
    version: toml::Value,
    special_pages: toml::Value,
    content_models: toml::Value,
    wikitext: toml::Value,
}

fn registries() -> &'static Registries {
    static R: OnceLock<Registries> = OnceLock::new();
    R.get_or_init(|| Registries {
        version: toml::from_str(VERSION_TOML).expect("version.toml parses"),
        special_pages: toml::from_str(SPECIAL_PAGES_TOML).expect("special-pages.toml parses"),
        content_models: toml::from_str(CONTENT_MODELS_TOML).expect("content-models.toml parses"),
        wikitext: toml::from_str(WIKITEXT_TOML).expect("wikitext-functions.toml parses"),
    })
}

fn to_json(t: &toml::Value) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

fn table<'a>(t: &'a toml::Value, key: &str) -> &'a [toml::Value] {
    t.get(key)
        .and_then(toml::Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn s<'a>(t: &'a toml::Value, key: &str) -> Option<&'a str> {
    t.get(key).and_then(toml::Value::as_str)
}

/// A JSON object of the keys of `t` named, leaving out the absent ones.
fn pick(t: &toml::Value, keys: &[&str]) -> Value {
    let mut m = Map::new();
    for k in keys {
        if let Some(v) = t.get(*k) {
            m.insert((*k).to_string(), to_json(v));
        }
    }
    Value::Object(m)
}

/// The credit line's parts: the developer, contributors (in order of first contribution)
/// and funders (0077 §2).
fn credits() -> Value {
    let r = &registries().version;
    let dev = r
        .get("developer")
        .map(|d| pick(d, &["name", "place", "source"]))
        .unwrap_or_default();
    let mut contributors: Vec<&toml::Value> = table(r, "contributor").iter().collect();
    contributors.sort_by_key(|c| c.get("since").map(ToString::to_string).unwrap_or_default());
    json!({
        "product": PRODUCT,
        "developer": dev,
        "contributors": contributors.iter().map(|c| pick(c, &["name", "since", "url"])).collect::<Vec<_>>(),
        "funders": table(r, "funder").iter().map(|f| pick(f, &["name", "funded", "since", "until", "url"])).collect::<Vec<_>>(),
    })
}

/// The AI agents (0077 §9).
fn agents() -> Vec<Value> {
    table(&registries().version, "agent")
        .iter()
        .map(|a| pick(a, &["name", "maker", "versions"]))
        .collect()
}

/// The roles of the services, by name, from the registry.
fn service_role(name: &str) -> Option<String> {
    table(&registries().version, "service")
        .iter()
        .find(|s2| s(s2, "name") == Some(name))
        .and_then(|s2| s(s2, "role"))
        .map(str::to_string)
}

/// The configured services, with their states and, where disclosed, versions (0077 §4).
fn services(app: &App, disclosure: Disclosure) -> Vec<Value> {
    if !disclosure.services() {
        return Vec::new();
    }
    app.probes()
        .services()
        .into_iter()
        .map(|(name, probe)| {
            let mut row = json!({
                "name": name,
                "role": service_role(name),
                "state": if probe.connected { "connected" } else { "unreachable" },
            });
            if disclosure.versions()
                && let Some(ver) = probe.version
            {
                row["version"] = json!(ver);
            }
            row
        })
        .collect()
}

/// What came from one origin.
#[derive(Default)]
struct Row {
    special_pages: Vec<String>,
    content_models: Vec<String>,
    implemented: Vec<String>,
    recognized: Vec<String>,
    inspired_by: Vec<String>,
}

impl Row {
    fn is_empty(&self) -> bool {
        self.special_pages.is_empty()
            && self.content_models.is_empty()
            && self.implemented.is_empty()
            && self.recognized.is_empty()
            && self.inspired_by.is_empty()
    }
}

/// Every origin in the registries, with what came from it. Only entries that are served
/// (special pages) or implemented (content models, wikitext), or only recognized
/// (wikitext `chip` and `ignored`), are gathered; deferred, declined and reserved names
/// are not.
fn gather() -> BTreeMap<String, Row> {
    let r = registries();
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for p in table(&r.special_pages, "page") {
        if let (Some(name), Some(origin), Some("served")) =
            (s(p, "name"), s(p, "origin"), s(p, "status"))
            && origin != "triplespace"
        {
            rows.entry(origin.to_string())
                .or_default()
                .special_pages
                .push(name.to_string());
        }
    }
    for m in table(&r.content_models, "model") {
        if let (Some(id), Some(origin), Some("implemented")) =
            (s(m, "id"), s(m, "origin"), s(m, "status"))
            && origin != "triplespace"
            && origin != "generic"
        {
            rows.entry(origin.to_string())
                .or_default()
                .content_models
                .push(id.to_string());
        }
    }
    for kind in ["variable", "function", "tag", "switch"] {
        let Some(entries) = r.wikitext.get(kind).and_then(toml::Value::as_table) else {
            continue;
        };
        for (name, e) in entries {
            let Some(origin) = s(e, "origin").filter(|o| *o != "triplespace") else {
                continue;
            };
            let shown = as_written(kind, name);
            let row = rows.entry(origin.to_string()).or_default();
            match s(e, "status") {
                Some("implemented") => row.implemented.push(shown),
                Some("chip" | "ignored") => row.recognized.push(shown),
                _ => {}
            }
        }
    }
    for e in table(&r.version, "extension") {
        let Some(name) = s(e, "name") else { continue };
        for i in table(e, "inspired_by") {
            if let Some(what) = s(i, "what") {
                rows.entry(name.to_string())
                    .or_default()
                    .inspired_by
                    .push(what.to_string());
            }
        }
    }
    rows
}

/// The lineage of §8: MediaWiki core, then each extension in alphabetical order, with
/// what came from it and its page on mediawiki.org.
fn lineage() -> Vec<Value> {
    let urls: BTreeMap<&str, &str> = table(&registries().version, "extension")
        .iter()
        .filter_map(|e| Some((s(e, "name")?, s(e, "url")?)))
        .collect();
    let mut out: Vec<(bool, String, Value)> = gather()
        .into_iter()
        .filter(|(_, row)| !row.is_empty())
        .map(|(origin, mut row)| {
            row.special_pages.sort();
            row.content_models.sort();
            row.implemented.sort();
            row.recognized.sort();
            let core = origin == "mediawiki";
            let (name, url) = if core {
                (
                    "MediaWiki".to_string(),
                    Some("https://www.mediawiki.org/wiki/MediaWiki"),
                )
            } else {
                (origin.clone(), urls.get(origin.as_str()).copied())
            };
            (
                !core,
                name.to_lowercase(),
                json!({
                    "name": name,
                    "url": url,
                    "special_pages": row.special_pages,
                    "content_models": row.content_models,
                    "wikitext": {"implemented": row.implemented, "recognized": row.recognized},
                    "inspired_by": row.inspired_by,
                }),
            )
        })
        .collect();
    out.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    out.into_iter().map(|(_, _, v)| v).collect()
}

/// A wikitext entry as it is written on a page: `{{PAGENAME}}`, `{{#if:…}}`, `<ref>`,
/// `__NOTOC__`; the redirect, which the registry keeps among the functions, as
/// `#REDIRECT [[…]]`.
#[must_use]
pub fn as_written(kind: &str, name: &str) -> String {
    match kind {
        "variable" => format!("{{{{{name}}}}}"),
        "function" if name.eq_ignore_ascii_case("#redirect") => format!("{name} [[…]]"),
        "function" => format!("{{{{{name}:…}}}}"),
        "tag" => format!("<{name}>"),
        "switch" => format!("__{name}__"),
        _ => name.to_string(),
    }
}

/// The wikitext of §7, with whether each entry is active given the tenant's settings.
/// Every entry needs template expansion; without it, the list is empty.
fn wikitext(settings: &BTreeMap<String, Value>) -> Vec<Value> {
    let on = |key: &str| {
        matches!(settings.get(key), Some(Value::String(x)) if x == "on")
            || settings.get(key) == Some(&Value::Bool(true))
    };
    if !on("wikitext.expansion") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for kind in ["tag", "function", "variable", "switch"] {
        let Some(entries) = registries()
            .wikitext
            .get(kind)
            .and_then(toml::Value::as_table)
        else {
            continue;
        };
        for (name, e) in entries {
            let status = s(e, "status").unwrap_or("");
            if !matches!(status, "implemented" | "chip" | "ignored") {
                continue;
            }
            let needs: Vec<&str> = ["requires", "enabled_by"]
                .iter()
                .filter_map(|k| s(e, k))
                .collect();
            out.push(json!({
                "kind": kind,
                "name": name,
                "written": as_written(kind, name),
                "origin": s(e, "origin"),
                "status": status,
                "active": needs.iter().all(|k| on(k)),
            }));
        }
    }
    out
}

/// The feature switches of §11 for one scope, with each setting's value, the tenant's or
/// the instance's where set, else the registry's default.
fn features(settings: &BTreeMap<String, Value>, scope: &str) -> Vec<Value> {
    table(&registries().version, "feature")
        .iter()
        .filter(|f| s(f, "scope") == Some(scope))
        .filter_map(|f| {
            let key = s(f, "key")?;
            let set = settings.get(key).cloned();
            let mut row = json!({"key": key, "set": set.is_some()});
            if let Some(val) = set.or_else(|| f.get("default").map(to_json)) {
                row["value"] = val;
            }
            Some(row)
        })
        .collect()
}

/// The keys of every feature switch.
fn feature_keys() -> Vec<&'static str> {
    table(&registries().version, "feature")
        .iter()
        .filter_map(|f| s(f, "key"))
        .collect()
}

/// The entry points of §5 that are served, at the base that serves them.
fn entry_points(tenant: &Tenant) -> Vec<Value> {
    let base = tenant.public_base.trim_end_matches('/');
    let concept = tenant.base.trim_end_matches('/');
    vec![
        json!({"name": "article", "url": format!("{base}/wiki/$1")}),
        json!({"name": "script", "url": format!("{base}/w")}),
        json!({"name": "api.php", "url": format!("{base}/w/api.php")}),
        json!({"name": "rest.php", "url": format!("{base}/w/rest.php")}),
        json!({"name": "triplespace/v0", "url": format!("{base}/w/rest.php/triplespace/v0")}),
        json!({"name": "entity", "url": format!("{concept}/entity/")}),
        json!({"name": "entitydata", "url": format!("{base}/wiki/Special:EntityData/")}),
    ]
}

fn host_of(base: &str) -> &str {
    let rest = base.split_once("://").map_or(base, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest)
}

/// The source of this build (0077 §3): `instance.source_url` if set, else the repository
/// at the commit for a build from a clean tree, else none.
fn source(build: &Value, source_url: Option<&str>) -> Option<String> {
    if let Some(u) = source_url.filter(|u| !u.is_empty()) {
        return Some(u.to_string());
    }
    let repo = s(registries().version.get("developer")?, "source")?;
    let commit = build["commit"].as_str().unwrap_or_default();
    (!commit.is_empty() && build["modified"] != Value::Bool(true))
        .then(|| format!("{repo}/tree/{commit}"))
}

/// What the page needs that depends on the request: its tenant, its forms, the settings
/// and the viewer's rights.
pub struct Request<'a> {
    /// The tenant the host selects.
    pub tenant: &'a Tenant,
    /// Whether the request is at the farm base (also true where the two coincide).
    pub at_farm: bool,
    /// Whether the request is at a tenant base (also true where the two coincide).
    pub at_tenant: bool,
    /// The settings the page reads, the tenant's over the instance's.
    pub settings: BTreeMap<String, Value>,
    /// `version.services`, or `full` for a holder of `ts-config`.
    pub disclosure: Disclosure,
    /// The providers the instance mirrors, which the tenant reads.
    pub providers: Vec<String>,
}

/// The response body.
#[must_use]
pub fn body(app: &App, req: &Request<'_>) -> Value {
    let build_info = app.build();
    let build = build_info.build().clone();
    let source_url = req
        .settings
        .get("instance.source_url")
        .and_then(Value::as_str);
    let mut out = credits();
    let o = out.as_object_mut().expect("an object");
    o.insert(
        "license".into(),
        json!({
            "spdx": LICENSE,
            "source": source(&build, source_url),
            "modified": build["modified"] == Value::Bool(true),
        }),
    );
    o.insert("build".into(), build);
    o.insert("services".into(), json!(services(app, req.disclosure)));
    let listed = build_info.listed();
    o.insert("components_listed".into(), json!(listed));
    let strip = |list: &str| -> Vec<Value> {
        build_info.json()[list]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let mut c = c.clone();
                if let Some(m) = c.as_object_mut() {
                    m.remove("texts");
                }
                c
            })
            .collect()
    };
    for list in ["workspace", "crates", "packages", "vendored"] {
        o.insert(list.into(), json!(strip(list)));
    }
    let mut feats = Map::new();
    if req.at_tenant {
        feats.insert("tenant".into(), json!(features(&req.settings, "tenant")));
        feats.insert("providers".into(), json!(req.providers));
    }
    if req.at_farm {
        feats.insert(
            "instance".into(),
            json!(features(&req.settings, "instance")),
        );
    }
    o.insert("features".into(), Value::Object(feats));
    o.insert("entry_points".into(), json!(entry_points(req.tenant)));
    o.insert(
        "wikitext".into(),
        json!(if req.at_tenant {
            wikitext(&req.settings)
        } else {
            Vec::new()
        }),
    );
    o.insert("inspired_by".into(), json!(lineage()));
    o.insert("agents".into(), json!(agents()));
    let mut instance = json!({"name": app.config().farm.slug});
    if let Some(owner) = req.settings.get("instance.owner_of_record") {
        instance["owner_of_record"] = owner.clone();
    }
    o.insert("instance".into(), instance);
    out
}

/// A REST error: `{"code": …, "message": …}` with a status.
fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = json!({"code": code, "message": message.into()}).to_string();
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        body,
    )
        .into_response()
}

/// Resolves the tenant and the caller, as every route does.
async fn prelude(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
) -> Result<(Tenant, Caller), Response> {
    let internal = |e: &dyn std::fmt::Display| {
        error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
    };
    let tenant = match tenant::resolve(app, client, origin, headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(error(
                StatusCode::MISDIRECTED_REQUEST,
                "misdirected",
                "not a tenant of this instance",
            ));
        }
        Err(e) => return Err(internal(&e)),
    };
    let caller = Caller::resolve(client, &tenant.slug, headers)
        .await
        .map_err(|e| internal(&e.info))?;
    Ok((tenant, caller))
}

/// `GET /version`.
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let internal = |e: &dyn std::fmt::Display| {
        error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
    };
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let mut keys: Vec<&str> = feature_keys();
    keys.extend(["instance.source_url", "instance.owner_of_record"]);
    let mut settings = BTreeMap::new();
    for key in keys {
        match v::setting(&client, &tenant.slug, key).await {
            Ok(Some(val)) => {
                settings.insert(key.to_string(), val);
            }
            Ok(None) => {}
            Err(e) => return internal(&e),
        }
    }
    let disclosure = if caller.require("ts-config").is_ok() {
        Disclosure::Full
    } else {
        match v::disclosure(&client).await {
            Ok(d) => d,
            Err(e) => return internal(&e),
        }
    };
    // The providers the instance mirrors: its `mirror/{slug}` graphs. A tenant's own
    // opt-in is a later refinement (0018 §6); until then it reads every mirror.
    let providers: Vec<String> = match client
        .query(
            "SELECT substr(code, 8) FROM view.registry
             WHERE tenant = '' AND kind = 'graph' AND code LIKE 'mirror/%' ORDER BY 1",
            &[],
        )
        .await
    {
        Ok(rows) => rows.iter().map(|r| r.get(0)).collect(),
        Err(e) => return internal(&e),
    };
    let farm_base = &app.config().farm.base;
    let same_base = farm_base.trim_end_matches('/') == tenant.base.trim_end_matches('/');
    let at_farm = same_base || host_of(farm_base).eq_ignore_ascii_case(&tenant.host);
    let at_tenant = same_base || !at_farm;
    let req = Request {
        tenant: &tenant,
        at_farm,
        at_tenant,
        settings,
        disclosure,
        providers,
    };
    let body = body(&app, &req).to_string();
    let credentialed = caller.has_session() || bearer(&headers).is_some();
    let class = if disclosure == Disclosure::Full && caller.require("ts-config").is_ok() {
        // What a holder of ts-config sees may be more than the public form.
        Class::Private
    } else {
        Class::Public.permitted(&method, credentialed, false, false)
    };
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body,
        class,
        &[],
    )
}

/// `GET /version/licenses/{id}`: one component's licence, copyright and notice files.
pub async fn license(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let internal = |e: &dyn std::fmt::Display| {
        error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
    };
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (_tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Some(component) = app.build().component(&id) else {
        return error(
            StatusCode::NOT_FOUND,
            "component-not-found",
            format!("no component {id}"),
        );
    };
    let credentialed = caller.has_session() || bearer(&headers).is_some();
    let class = Class::Stable.permitted(&method, credentialed, false, false);
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        component.to_string(),
        class,
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENAPI: &str = include_str!("../../../docs/api/triplespace-v0.openapi.json");

    #[test]
    fn the_registries_parse() {
        let r = registries();
        assert!(!table(&r.version, "agent").is_empty());
        assert!(!table(&r.special_pages, "page").is_empty());
    }

    #[test]
    fn credits_name_the_developer() {
        let c = credits();
        assert_eq!(c["product"], "Triplespace");
        assert_eq!(c["developer"]["name"], "Scatter LLC");
        assert_eq!(c["developer"]["place"], "Portland, Oregon");
        assert!(c["contributors"].as_array().unwrap().is_empty());
        assert!(c["funders"].as_array().unwrap().is_empty());
    }

    #[test]
    fn agents_come_from_the_registry() {
        let names: Vec<String> = agents()
            .iter()
            .map(|a| a["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["Claude Opus", "Claude Fable", "Claude Sonnet"]);
    }

    #[test]
    fn lineage_starts_with_mediawiki_and_names_no_adrs() {
        let l = lineage();
        assert_eq!(l[0]["name"], "MediaWiki");
        let wikibase = l.iter().find(|r| r["name"] == "Wikibase").unwrap();
        assert!(
            wikibase["special_pages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "EntityData")
        );
        assert!(
            wikibase["content_models"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "wikibase-item")
        );
        assert_eq!(
            wikibase["url"],
            "https://www.mediawiki.org/wiki/Extension:Wikibase_Repository"
        );
        let cirrus = l.iter().find(|r| r["name"] == "CirrusSearch").unwrap();
        assert!(!cirrus["inspired_by"].as_array().unwrap().is_empty());
        let pf = l.iter().find(|r| r["name"] == "ParserFunctions").unwrap();
        assert!(
            pf["wikitext"]["implemented"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f == "{{#if:…}}")
        );
        let core = &l[0]["wikitext"]["implemented"];
        assert!(
            core.as_array().unwrap().iter().any(|f| f == "__NOTOC__"),
            "{core}"
        );
        assert!(
            core.as_array()
                .unwrap()
                .iter()
                .any(|f| f == "#REDIRECT [[…]]"),
            "{core}"
        );
        // Declined pages are not lineage: LockDB is MediaWiki's, declined.
        let core = &l[0]["special_pages"];
        assert!(!core.as_array().unwrap().iter().any(|p| p == "LockDB"));
        // The page names no ADRs (0077 A1): no `adr` field, no ADR number.
        let text = serde_json::to_string(&l).unwrap();
        assert!(!text.contains("\"adr\""), "{text}");
        assert!(!text.contains("ADR") && !text.contains(" §"), "{text}");
    }

    #[test]
    fn wikitext_needs_expansion() {
        let mut settings = BTreeMap::new();
        assert!(wikitext(&settings).is_empty());
        settings.insert("wikitext.expansion".into(), json!("on"));
        let w = wikitext(&settings);
        assert!(w.iter().any(|e| e["name"] == "#if" && e["active"] == true));
        assert!(
            w.iter().any(|e| e["active"] == false),
            "Lua's entries need wikitext.lua"
        );
    }

    #[test]
    fn features_fall_back_to_defaults() {
        let mut settings = BTreeMap::new();
        settings.insert("query.enabled".into(), json!(true));
        let f = features(&settings, "tenant");
        let q = f.iter().find(|r| r["key"] == "query.enabled").unwrap();
        assert_eq!(
            (q["value"].clone(), q["set"].clone()),
            (json!(true), json!(true))
        );
        let e = f.iter().find(|r| r["key"] == "wikitext.expansion").unwrap();
        assert_eq!(
            (e["value"].clone(), e["set"].clone()),
            (json!("off"), json!(false))
        );
        let l = f.iter().find(|r| r["key"] == "content.licence").unwrap();
        assert!(l.get("value").is_none(), "no default: not set");
        assert!(
            features(&settings, "instance")
                .iter()
                .any(|r| r["key"] == "files.separation")
        );
    }

    #[test]
    fn the_source_of_a_build() {
        let clean = json!({"commit": "abc", "modified": false});
        assert_eq!(
            source(&clean, None).as_deref(),
            Some("https://github.com/scatterbase/triplespace/tree/abc")
        );
        let dirty = json!({"commit": "abc", "modified": true});
        assert_eq!(source(&dirty, None), None);
        assert_eq!(
            source(&dirty, Some("https://example.org/src")).as_deref(),
            Some("https://example.org/src")
        );
    }

    #[test]
    fn hosts() {
        assert_eq!(host_of("https://scatter.example"), "scatter.example");
        assert_eq!(
            host_of("https://scatter.example:8443/x"),
            "scatter.example:8443"
        );
    }

    #[test]
    fn the_contract_names_the_routes() {
        let doc: Value = serde_json::from_str(OPENAPI).unwrap();
        assert!(doc["paths"].get("/version").is_some());
        assert!(doc["paths"].get("/version/licenses/{id}").is_some());
        assert!(doc["components"]["schemas"].get("Version").is_some());
    }
}
