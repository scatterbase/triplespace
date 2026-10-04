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
                    "mainpage": "Main Page",
                    "base": format!("{base}/wiki/Main_Page"),
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
pub fn paraminfo(ctx: &Ctx) -> Result<ApiResponse, ApiError> {
    let modules: Vec<Value> = ctx
        .params
        .list("modules")
        .into_iter()
        .map(|m| json!({"name": m, "classname": "Triplespace", "path": m, "parameters": []}))
        .collect();
    Ok(ApiResponse::ok(json!({"paraminfo": {"modules": modules}})))
}
