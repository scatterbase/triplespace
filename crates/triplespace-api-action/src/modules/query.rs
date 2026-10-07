//! `action=query` (mediawiki-compat §5.4): `meta=siteinfo`, `meta=tokens`,
//! `meta=userinfo`. Other `meta`, `prop` and `list` values are warned about and ignored,
//! as MediaWiki does with an unknown parameter value in a module it does serve.

use scatter_providers::Registry;
use scatter_wikibase_model::value::DataType;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use triplespace_accounts::session;

use crate::api::Ctx;
use crate::app::Mode;
use crate::auth::set_cookie;
use crate::response::{ApiError, ApiResponse};

/// Runs `action=query`.
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    let mut query = Map::new();
    let mut response = ApiResponse::default();
    for meta in ctx.params.list("meta") {
        match meta.as_str() {
            "siteinfo" => siteinfo(ctx, &mut query).await?,
            "tokens" => tokens(ctx, &mut query, &mut response).await?,
            "userinfo" => userinfo(ctx, &mut query),
            other => response.warn(
                "query",
                &format!("Unrecognized value for parameter \"meta\": {other}."),
            ),
        }
    }
    // `meta=siteinfo` alone changes only with configuration, which no purge touches, so
    // the web tier may keep it for a minute (0057 §5).
    if ctx.params.list("meta") == ["siteinfo"]
        && ctx.params.list("prop").is_empty()
        && ctx.params.list("list").is_empty()
    {
        ctx.cache_class = crate::http_cache::Class::Stable;
    }
    for p in ctx.params.list("prop") {
        response.warn(
            "query",
            &format!("Unrecognized value for parameter \"prop\": {p}."),
        );
    }
    for l in ctx.params.list("list") {
        response.warn(
            "query",
            &format!("Unrecognized value for parameter \"list\": {l}."),
        );
    }
    let mut body = Map::new();
    if ctx.formatversion == 2 {
        body.insert("batchcomplete".into(), json!(true));
    } else {
        body.insert("batchcomplete".into(), json!(""));
    }
    if !query.is_empty() {
        body.insert("query".into(), Value::Object(query));
    }
    response.body = body;
    Ok(response)
}

/// `meta=tokens`: a token per requested type for the caller's session or key. A caller
/// without either gets a session, so a login token has something to bind to.
async fn tokens(
    ctx: &mut Ctx,
    query: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    let mut types = ctx.params.list("type");
    if types.is_empty() {
        types.push("csrf".into());
    }
    if types.iter().any(|t| t == "*") {
        types = [
            "createaccount",
            "csrf",
            "login",
            "patrol",
            "rollback",
            "userrights",
            "watch",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    }
    if ctx.caller.identity.token_binding().is_none() {
        let s = session::create(ctx.db(), &ctx.tenant.slug, None, None).await?;
        response.set_cookie = Some(set_cookie(
            &ctx.app,
            &s.id,
            session::ANONYMOUS_LIFETIME.as_secs(),
        ));
        ctx.caller.identity.session = Some(s);
    }
    let mut out = Map::new();
    for t in types {
        match t.as_str() {
            "createaccount" | "csrf" | "login" | "patrol" | "rollback" | "userrights" | "watch" => {
                out.insert(format!("{t}token"), json!(ctx.caller.token(&ctx.app, &t)));
            }
            other => response.warn(
                "tokens",
                &format!("Unrecognized value for parameter \"type\": {other}."),
            ),
        }
    }
    query.insert("tokens".into(), Value::Object(out));
    Ok(())
}

/// `meta=userinfo` (0016 §7; 0024 §8): `id`, `name`, `anon`, and by `uiprop` `groups`,
/// `rights`, `blockinfo`, `ratelimits`, plus `operator` and `grants` for a subsidiary.
fn userinfo(ctx: &Ctx, query: &mut Map<String, Value>) {
    let id = &ctx.caller.identity;
    let mut u = Map::new();
    let numeric = id
        .actor_key
        .as_deref()
        .and_then(|k| k.rsplit_once(':'))
        .and_then(|(_, n)| n.parse::<u64>().ok());
    u.insert("id".into(), json!(numeric.unwrap_or(0)));
    if let Some(n) = &id.name {
        u.insert("name".into(), json!(n));
    } else {
        u.insert("name".into(), json!("127.0.0.1"));
        u.insert(
            "anon".into(),
            if ctx.formatversion == 2 {
                json!(true)
            } else {
                json!("")
            },
        );
    }
    let props = ctx.params.list("uiprop");
    let want = |p: &str| props.iter().any(|x| x == p || x == "*");
    if want("groups") {
        u.insert(
            "groups".into(),
            json!(
                ctx.caller
                    .effective
                    .groups
                    .iter()
                    .map(|g| if g == "universe" { "*" } else { g.as_str() })
                    .collect::<Vec<_>>()
            ),
        );
    }
    if want("rights") {
        u.insert("rights".into(), json!(ctx.caller.effective.permissions));
    }
    if want("blockinfo") {
        // No blocks in the milestone's write path; the field is present and empty-shaped.
    }
    if want("ratelimits") {
        u.insert("ratelimits".into(), json!({}));
    }
    if want("hasmsg") {
        // No notifications yet.
    }
    if let Some(op) = &id.operator {
        u.insert("operator".into(), json!(op));
    }
    if let Some(k) = &id.credential {
        u.insert("grants".into(), json!(k.grants));
    }
    query.insert("userinfo".into(), Value::Object(u));
}

#[derive(Deserialize)]
struct NamespaceRow {
    id: i64,
    canonical: String,
    kind: String,
    #[serde(default)]
    case: Option<String>,
    #[serde(default)]
    subpages: Option<bool>,
    #[serde(default)]
    default_model: Option<String>,
    #[serde(default)]
    enabled_by: Option<String>,
}

#[derive(Deserialize)]
struct NamespaceFile {
    namespace: Vec<NamespaceRow>,
}

/// The namespaces `meta=siteinfo` lists (0008 §2–3; 0041 §8): the `pages` and `virtual`
/// namespaces of `namespaces.toml` that no tenant setting gates.
fn namespaces(sitename: &str) -> Value {
    let file: NamespaceFile = toml::from_str(include_str!(
        "../../../../docs/registry/namespaces.toml"
    ))
    .unwrap_or(NamespaceFile {
        namespace: Vec::new(),
    });
    let mut out = Map::new();
    for ns in file.namespace {
        if ns.enabled_by.is_some() || !(ns.kind == "pages" || ns.kind == "virtual") {
            continue;
        }
        let name = match ns.canonical.as_str() {
            "Project" => sitename.to_string(),
            "Project talk" => format!("{sitename} talk"),
            other => other.to_string(),
        };
        let mut row = Map::new();
        row.insert("id".into(), json!(ns.id));
        row.insert(
            "case".into(),
            json!(ns.case.unwrap_or_else(|| "first-letter".into())),
        );
        row.insert("name".into(), json!(name));
        row.insert("subpages".into(), json!(ns.subpages.unwrap_or(false)));
        if !ns.canonical.is_empty() {
            row.insert("canonical".into(), json!(ns.canonical));
        }
        row.insert("content".into(), json!(ns.id == 0));
        row.insert("nonincludable".into(), json!(false));
        if let Some(m) = ns.default_model
            && m != "wikitext"
        {
            row.insert("defaultcontentmodel".into(), json!(m));
        }
        out.insert(ns.id.to_string(), Value::Object(row));
    }
    Value::Object(out)
}

fn property_types() -> Value {
    let mut m = Map::new();
    for dt in [
        DataType::CommonsMedia,
        DataType::GeoShape,
        DataType::TabularData,
        DataType::Url,
        DataType::ExternalId,
        DataType::WikibaseItem,
        DataType::WikibaseProperty,
        DataType::GlobeCoordinate,
        DataType::MonolingualText,
        DataType::Quantity,
        DataType::String,
        DataType::Time,
        DataType::WikibaseDomain,
    ] {
        if let Some(vt) = dt.value_type() {
            m.insert(dt.id().to_string(), json!({"valuetype": vt.name()}));
        }
    }
    Value::Object(m)
}

/// `siprop=providers` (0012 §4): codes, entity types, namespaces, IRI templates, whether
/// upstream history can be fetched, and the colours of each provider's chip (0010 §2).
fn providers() -> Value {
    Registry::default_registry()
        .providers()
        .iter()
        .map(|p| {
            json!({
                "code": p.code,
                "slug": p.slug,
                "name": p.name,
                "number": p.number,
                "revision_ids": p.revision_ids,
                "types": p.types.iter().map(|t| json!({
                    "code": t.code.to_string(),
                    "entity_type": t.entity_type,
                    "namespace": t.namespace,
                    "iri": t.iri,
                    "upstream_prefix": t.upstream_prefix,
                })).collect::<Vec<_>>(),
                "chip": p.chip.as_ref().map(|c| json!({"color": c.color, "background": c.background})),
            })
        })
        .collect()
}

/// The tenant's theme (0034 §1, 0012 A38): the `site:ui.theme` setting's `value`, the
/// tenant's own or else the instance's; `None` when neither is set, and the site then
/// wears its shipped default. Not yet withheld from outsiders of a private tenant,
/// since there are none until 0056 §3 is built.
async fn theme(ctx: &Ctx) -> Result<Option<Value>, ApiError> {
    let row = ctx
        .db()
        .query_opt(
            "SELECT config->'value' FROM view.registry
             WHERE kind = 'site' AND code = 'ui.theme' AND tenant IN ($1, '')
             ORDER BY tenant DESC LIMIT 1",
            &[&ctx.tenant.slug],
        )
        .await?;
    Ok(row
        .and_then(|r| r.get::<_, Option<Value>>(0))
        .filter(Value::is_object))
}

/// The main page's title: the project namespace's `Home`, which the site generates until
/// a page of that title exists.
pub const MAIN_PAGE: &str = "Project:Home";

/// `meta=siteinfo`: `general`, `namespaces`, `namespacealiases`, `extensions`,
/// `statistics`, `usergroups`, `providers`, `triplespace`; unknown `siprop` values warn.
#[allow(clippy::too_many_lines)]
async fn siteinfo(ctx: &mut Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    let mut props = ctx.params.list("siprop");
    if props.is_empty() {
        props.push("general".into());
    }
    let base = ctx.tenant.public_base.trim_end_matches('/').to_string();
    let sitename = ctx.tenant.slug.clone();
    let host = ctx.tenant.host.clone();
    for p in props {
        match p.as_str() {
            "general" => {
                query.insert("general".into(), json!({
                    "mainpage": MAIN_PAGE,
                    "base": format!("{base}/wiki/{}", MAIN_PAGE.replace(' ', "_")),
                    "sitename": sitename,
                    "generator": ctx.app.config().generator,
                    "phpversion": "0",
                    "dbtype": "postgres",
                    "lang": "en",
                    "fallback": [],
                    "case": "first-letter",
                    "legaltitlechars": " %!\"$&'()*,\\-.\\/0-9:;=?@A-Z\\\\^_`a-z~\\x80-\\xFF+",
                    "readonly": false,
                    "writeapi": true,
                    "maxarticlesize": 2_097_152,
                    "timezone": "UTC",
                    "timeoffset": 0,
                    "articlepath": "/wiki/$1",
                    "scriptpath": "/w",
                    "script": "/w/index.php",
                    "variantarticlepath": false,
                    "server": base,
                    "servername": host.split(':').next().unwrap_or(&host),
                    "wikiid": sitename.replace('-', "_"),
                    "time": crate::entity_json::iso8601(crate::edit::now_micros()),
                    "uploadsenabled": false,
                    // Pywikibot's Siteinfo converts these three unconditionally, so they
                    // must be present, as MediaWiki emits them (index-keyed objects, even
                    // in formatversion 2; `magiclinks` a name → bool map). Values are
                    // MediaWiki's defaults; nothing here renders thumbnails.
                    "thumblimits": {"0": 120, "1": 150, "2": 180, "3": 200, "4": 250, "5": 300},
                    "imagelimits": {"0": {"width": 320, "height": 240}, "1": {"width": 640, "height": 480},
                                    "2": {"width": 800, "height": 600}, "3": {"width": 1024, "height": 768},
                                    "4": {"width": 1280, "height": 1024}},
                    "magiclinks": {"ISBN": false, "PMID": false, "RFC": false},
                    "linktrail": "/^([a-z]+)(.*)$/sD",
                    "misermode": false,
                    "centralidlookupprovider": "local",
                    "allcentralidlookupproviders": ["local"],
                    "wikibase-propertytypes": property_types(),
                    "wikibase-conceptbaseuri": format!("{}/entity/", ctx.tenant.base.trim_end_matches('/')),
                }));
            }
            "namespaces" => {
                query.insert("namespaces".into(), namespaces(&sitename));
            }
            "namespacealiases" => {
                query.insert("namespacealiases".into(), json!([]));
            }
            "extensions" => {
                query.insert("extensions".into(), json!([
                    {"type": "wikibase", "name": "WikibaseRepository", "descriptionmsg": "wikibase-desc", "url": "https://github.com/scatterbase/triplespace", "license-name": "GPL-3.0-or-later"},
                    {"type": "other", "name": "Triplespace", "url": "https://github.com/scatterbase/triplespace", "license-name": "GPL-3.0-or-later"}
                ]));
            }
            "statistics" => {
                query.insert("statistics".into(), json!({"pages": 0, "articles": 0, "edits": 0, "images": 0, "users": 0, "activeusers": 0, "admins": 1, "jobs": 0}));
            }
            "usergroups" => {
                let groups = scatter_actors::group::GroupRegistry::default_registry();
                query.insert(
                    "usergroups".into(),
                    json!(
                        groups
                            .groups()
                            .iter()
                            .map(|g| json!({
                                "name": g.mediawiki_name,
                                "rights": g.permissions,
                            }))
                            .collect::<Vec<_>>()
                    ),
                );
            }
            "providers" => {
                query.insert("providers".into(), providers());
            }
            "issuers" => {
                query.insert(
                    "issuers".into(),
                    json!(
                        scatter_actors::issuer::IssuerRegistry::default_registry()
                            .login_providers()
                            .map(|i| json!({"code": i.code, "name": i.name}))
                            .collect::<Vec<_>>()
                    ),
                );
            }
            "triplespace" => {
                let mut t = json!({
                    "tenant": sitename,
                    "farm": ctx.app.config().farm.slug,
                    "search": "postgres",
                    "capabilities": ["wbgetentities", "wbsearchentities", "wbeditentity", "statements", "terms", "login", "clientlogin", "bearer"],
                    "providers": Registry::default_registry().providers().iter().map(|p| p.slug.clone()).collect::<Vec<_>>(),
                    "rate_limits": [],
                    "grants": scatter_actors::grant::GrantRegistry::default_registry().grants().iter().map(|g| g.name.clone()).collect::<Vec<_>>(),
                    "api_version": crate::API_VERSION,
                });
                if let Some(theme) = theme(ctx).await? {
                    t["theme"] = theme;
                }
                if ctx.app.config().mode == Mode::Development {
                    t["insecure"] = json!(true);
                }
                query.insert("triplespace".into(), t);
            }
            other => ctx.warn(
                "siteinfo",
                format!("Unrecognized value for parameter \"siprop\": {other}."),
            ),
        }
    }
    Ok(())
}

/// `action=paraminfo`: enough for a client to see which modules exist.
/// The action modules, as `action=paraminfo` describes them. Every `wb*` write module
/// is one entry here (`write::run` dispatches on the name), so a client that validates
/// against paraminfo, as Pywikibot does, sees each of them.
const ACTION_MODULES: &[(&str, bool)] = &[
    // (name, must be POSTed)
    ("query", false),
    ("paraminfo", false),
    ("help", false),
    ("login", true),
    ("clientlogin", true),
    ("logout", true),
    ("wbgetentities", false),
    ("wbsearchentities", false),
    ("wbeditentity", true),
    ("wbcreateclaim", true),
    ("wbsetclaim", true),
    ("wbremoveclaims", true),
    ("wbsetlabel", true),
    ("wbsetdescription", true),
    ("wbsetaliases", true),
    ("wbsetqualifier", true),
    ("wbremovequalifiers", true),
    ("wbsetreference", true),
    ("wbremovereferences", true),
];

/// `action=query`'s submodules by kind: `(kind, names)`.
const QUERY_MODULES: &[(&str, &[&str])] = &[
    ("meta", &["siteinfo", "tokens", "userinfo"]),
    ("prop", &[]),
    ("list", &[]),
];

fn paraminfo_module(name: &str, path: &str, prefix: &str, parameters: &[Value]) -> Value {
    let mut m = json!({
        "name": name,
        "classname": "Triplespace",
        "path": path,
        "prefix": prefix,
        "source": "Triplespace",
        "sourcename": "Triplespace",
        "licensetag": "GPL-3.0-or-later",
        "parameters": parameters,
    });
    if path.starts_with("query+")
        && let Some((kind, _)) = QUERY_MODULES
            .iter()
            .find(|(_, names)| names.contains(&name))
    {
        m["group"] = json!(kind);
    }
    m
}

fn parameter(index: usize, name: &str, kind: Value) -> Map<String, Value> {
    let mut p = Map::new();
    p.insert("index".into(), json!(index));
    p.insert("name".into(), json!(name));
    p.insert("type".into(), kind);
    p
}

/// A parameter that names submodules: its `type` lists them and `submodules` maps each
/// to its path, which is how Pywikibot learns the module tree.
fn submodule_parameter(index: usize, name: &str, names: &[&str], parent: Option<&str>) -> Value {
    let mut p = parameter(index, name, json!(names));
    let paths: Map<String, Value> = names
        .iter()
        .map(|n| {
            let path = parent.map_or_else(|| (*n).to_string(), |pa| format!("{pa}+{n}"));
            ((*n).to_string(), json!(path))
        })
        .collect();
    p.insert("submodules".into(), Value::Object(paths));
    p.insert("multi".into(), json!(true));
    p.insert("limit".into(), json!(50));
    p.insert("lowlimit".into(), json!(50));
    p.insert("highlimit".into(), json!(500));
    Value::Object(p)
}

/// `action=paraminfo` (mediawiki-compat.md §5): the module tree and each module's
/// parameters, in MediaWiki's shape. `main`, `query` and `paraminfo` are described in
/// full, since Pywikibot's `ParamInfo` reads the module tree from them; the action
/// modules carry their POST requirement; a `query+…` submodule or any other module
/// name is answered with an empty parameter list rather than refused.
pub fn paraminfo(ctx: &Ctx) -> Result<ApiResponse, ApiError> {
    let modules = paraminfo_modules(&ctx.params.list("modules"));
    Ok(ApiResponse::ok(json!({"paraminfo": {"modules": modules}})))
}

/// The `modules` array of `action=paraminfo` for the requested module paths.
#[allow(clippy::too_many_lines)]
pub fn paraminfo_modules(requested: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in requested {
        let module = match m.as_str() {
            "main" => {
                let actions: Vec<&str> = ACTION_MODULES.iter().map(|(n, _)| *n).collect();
                let mut action = submodule_parameter(0, "action", &actions, None);
                action["default"] = json!("help");
                if let Some(o) = action.as_object_mut() {
                    o.remove("multi");
                }
                let mut format = parameter(1, "format", json!(["json"]));
                format.insert("default".into(), json!("jsonfm"));
                format.insert("submodules".into(), json!({"json": "json"}));
                let mut maxlag = parameter(2, "maxlag", json!("integer"));
                maxlag.insert("default".into(), Value::Null);
                paraminfo_module(
                    "main",
                    "main",
                    "",
                    &[
                        action,
                        Value::Object(format),
                        Value::Object(maxlag),
                        Value::Object(parameter(3, "formatversion", json!(["1", "2", "latest"]))),
                    ],
                )
            }
            "paraminfo" => {
                let mut modules = parameter(0, "modules", json!("string"));
                modules.insert("multi".into(), json!(true));
                modules.insert("limit".into(), json!(50));
                modules.insert("lowlimit".into(), json!(50));
                modules.insert("highlimit".into(), json!(500));
                let mut helpformat =
                    parameter(1, "helpformat", json!(["html", "wikitext", "raw", "none"]));
                helpformat.insert("default".into(), json!("none"));
                paraminfo_module(
                    "paraminfo",
                    "paraminfo",
                    "",
                    &[Value::Object(modules), Value::Object(helpformat)],
                )
            }
            "query" => {
                let mut params: Vec<Value> = QUERY_MODULES
                    .iter()
                    .enumerate()
                    .map(|(i, (kind, names))| submodule_parameter(i, kind, names, Some("query")))
                    .collect();
                // Generators are submodules usable as a page set; none yet.
                let mut generator = parameter(params.len(), "generator", json!([]));
                generator.insert("submodules".into(), json!({}));
                params.push(Value::Object(generator));
                paraminfo_module("query", "query", "", &params)
            }
            other => {
                let (name, prefix) = match other.strip_prefix("query+") {
                    Some(sub) => (
                        sub,
                        match sub {
                            "siteinfo" => "si",
                            "userinfo" => "ui",
                            _ => "",
                        },
                    ),
                    None => (other, ""),
                };
                let mut module = paraminfo_module(name, other, prefix, &[]);
                if let Some((_, post)) = ACTION_MODULES.iter().find(|(n, _)| *n == name)
                    && *post
                {
                    module["mustbeposted"] = json!(true);
                    module["writerights"] = json!(true);
                }
                if let Some(sub) = other.strip_prefix("query+")
                    && !QUERY_MODULES.iter().any(|(_, names)| names.contains(&sub))
                    && !ACTION_MODULES.iter().any(|(n, _)| *n == name)
                {
                    module = json!({"name": name, "path": other, "missing": true});
                }
                module
            }
        };
        out.push(module);
    }
    out
}

#[cfg(test)]
mod paraminfo_tests {
    use super::*;

    fn by_path(modules: &[Value], path: &str) -> Value {
        modules
            .iter()
            .find(|m| m["path"] == path)
            .cloned()
            .unwrap_or_else(|| panic!("module {path}"))
    }

    fn param<'a>(module: &'a Value, name: &str) -> &'a Value {
        module["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("parameter {name}"))
    }

    /// The invariants Pywikibot's `ParamInfo._init` and `_generate_submodules` assert.
    #[test]
    fn pywikibot_reads_the_module_tree() {
        let req: Vec<String> = [
            "main",
            "paraminfo",
            "query",
            "query+siteinfo",
            "wbeditentity",
            "query+nothing",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let modules = paraminfo_modules(&req);

        // main: `action` lists the actions and maps each to itself; `format` exists.
        let main = by_path(&modules, "main");
        let action = param(&main, "action");
        let names: Vec<&str> = action["type"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let sub = action["submodules"].as_object().unwrap();
        assert_eq!(names.len(), sub.len());
        for n in &names {
            assert_eq!(sub[*n], json!(n));
        }
        assert!(names.contains(&"query") && names.contains(&"wbeditentity"));
        param(&main, "format");

        // query: prop/list/meta carry type, submodules (`query+name`) and limit; a
        // generator parameter exists and its type is within the submodules.
        let query = by_path(&modules, "query");
        let mut all_sub = std::collections::BTreeSet::new();
        for kind in ["prop", "list", "meta"] {
            let p = param(&query, kind);
            assert!(p["limit"].is_number(), "{kind} limit");
            for (child, path) in p["submodules"].as_object().unwrap() {
                assert_eq!(path.as_str().unwrap(), format!("query+{child}"));
                all_sub.insert(child.clone());
            }
        }
        assert!(all_sub.contains("siteinfo"));
        let generator = param(&query, "generator");
        for g in generator["type"].as_array().unwrap() {
            assert!(all_sub.contains(g.as_str().unwrap()));
        }

        // Submodules and action modules are described; an unknown submodule is `missing`.
        assert_eq!(by_path(&modules, "query+siteinfo")["group"], "meta");
        assert_eq!(by_path(&modules, "query+siteinfo")["prefix"], "si");
        assert_eq!(by_path(&modules, "wbeditentity")["mustbeposted"], true);
        assert_eq!(by_path(&modules, "query+nothing")["missing"], true);
        param(&by_path(&modules, "paraminfo"), "modules");
    }
}
