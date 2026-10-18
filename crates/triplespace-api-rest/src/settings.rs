//! `GET /settings/schema`, `GET /settings` and `PATCH /settings` (0085 §3).
//!
//! At a tenant's base the routes serve that tenant. With `scope=instance` they serve the
//! instance's settings, and only at the farm base, so that on a single-tenant instance,
//! whose farm base is its tenant's base, the two never share a meaning.
//!
//! Reading needs only what reading the tenant needs, since the `config` partition is
//! public (23 §1.1). Writing needs `ts-config` at the base, an instance right at the farm
//! base (0040 §9), or the entry's stronger `right`; the validator and the writer are the
//! ones `triplespace-cli settings` uses ([`triplespace_api_ingest::settings`]).

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use scatter_actors::evaluate::instance_rights;
use serde_json::{Map, Value, json};
use triplespace_accounts::settings::plan::Target;
use triplespace_accounts::settings::resolve::{Partition, row_json};
use triplespace_accounts::settings::{Patch, Refusal, Scope, Snapshot, load};
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::{self, Tenant, at_farm};
use triplespace_api_ingest::settings::{SaveError, save};

use crate::entity_route::{error, internal};

/// The scope a request asks for: `scope=instance` at the farm base, or the tenant.
#[allow(clippy::result_large_err)] // a ready response, as the async routes return
fn scope_of(app: &App, tenant: &Tenant, q: &BTreeMap<String, String>) -> Result<Scope, Response> {
    match q.get("scope").map(String::as_str) {
        None | Some("tenant") => Ok(Scope::Tenant),
        Some("instance") if at_farm(app, tenant) => Ok(Scope::Instance),
        Some("instance") => Err(error(
            StatusCode::BAD_REQUEST,
            "ts-instance-policy",
            "the instance's settings are served only at the farm base",
        )),
        Some(other) => Err(error(
            StatusCode::BAD_REQUEST,
            "badvalue",
            format!("scope `{other}` is not `tenant` or `instance`"),
        )),
    }
}

/// The tenant and the caller, refusing a caller who may not read the tenant.
async fn prelude(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
) -> Result<(Tenant, Caller), Response> {
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
    if caller.require("read").is_err() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reading needs the \"read\" right",
        ));
    }
    Ok((tenant, caller))
}

/// Whether the caller holds a right at the scope: the tenant's permissions, or at the
/// farm base the instance rights (0040 §9), which on a farm without global groups are
/// what the caller holds on the primary tenant the farm base serves. `owner` is
/// membership in the `owner` group.
fn holds(caller: &Caller, scope: Scope, right: &str) -> bool {
    if right == "owner" {
        return caller.effective.in_group("owner");
    }
    match scope {
        Scope::Tenant => caller.effective.holds(right),
        Scope::Instance => instance_rights(Some(&caller.effective), None).contains(right),
    }
}

fn respond_json(headers: &HeaderMap, method: &Method, caller: &Caller, body: &Value) -> Response {
    let credentialed = caller.has_session() || bearer(headers).is_some();
    let class = Class::Public.permitted(method, credentialed, false, false);
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &[],
    )
}

/// `GET /settings/schema`.
pub async fn schema(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let scope = match scope_of(&app, &tenant, &q) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let (snap, _) = match load(&**client, Some(&tenant.slug)).await {
        Ok(x) => x,
        Err(e) => return internal(&e),
    };
    let own = if scope == Scope::Instance {
        match own_counts(&client).await {
            Ok(c) => Some(c),
            Err(e) => return internal(&e),
        }
    } else {
        None
    };
    let body = schema_body(&snap, scope, own.as_ref());
    respond_json(&headers, &method, &caller, &body)
}

/// How many tenants there are, and how many set each tenant setting themselves: the rest
/// inherit the instance's default (0085 §4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnCounts {
    /// The registered tenants.
    pub tenants: u64,
    /// By key, the tenants with a record of their own.
    pub own: BTreeMap<String, u64>,
}

async fn own_counts(client: &tokio_postgres::Client) -> Result<OwnCounts, tokio_postgres::Error> {
    let tenants: i64 = client
        .query_one(
            "SELECT count(*) FROM view.registry WHERE tenant = '' AND kind = 'tenant'",
            &[],
        )
        .await?
        .get(0);
    let rows = client
        .query(
            "SELECT code, count(*) FROM view.registry
             WHERE kind = 'site' AND tenant <> '' AND NOT instance_act GROUP BY code",
            &[],
        )
        .await?;
    Ok(OwnCounts {
        tenants: u64::try_from(tenants).unwrap_or(0),
        own: rows
            .iter()
            .map(|r| {
                (
                    r.get::<_, String>(0),
                    u64::try_from(r.get::<_, i64>(1)).unwrap_or(0),
                )
            })
            .collect(),
    })
}

/// The body of `GET /settings/schema`. At either scope it reports the tenancy policy as
/// it applies (the switches, the `config.template` mode, the locked keys and the
/// ceilings), which Special:WikiSettings shows read-only; at instance scope it adds every
/// tenant setting with its defaults, its lock and how many tenants inherit it.
#[must_use]
pub fn schema_body(snap: &Snapshot, scope: Scope, own: Option<&OwnCounts>) -> Value {
    let registry = snap.registry();
    let groups: Vec<Value> = registry
        .groups()
        .iter()
        .filter(|g| {
            registry
                .of_scope(scope)
                .any(|d| &d.group == *g && !d.retired)
        })
        .map(|g| json!({"id": g, "label": format!("ts-settings-group-{g}")}))
        .collect();
    let entry = |d: &triplespace_accounts::settings::SettingDef| {
        let mut v = d.to_json();
        if let Some(o) = v.as_object_mut() {
            o.remove("kind");
            o.insert("label".into(), Value::String(d.label_message()));
            o.insert("help".into(), Value::String(d.help_message()));
        }
        v
    };
    let settings: Vec<Value> = registry
        .of_scope(scope)
        .filter(|d| !d.retired)
        .map(entry)
        .collect();
    let mut body = json!({"scope": scope.as_str(), "groups": groups, "settings": settings});
    {
        let registry_switches = scatter_actors::tenancy::TenancyRegistry::default_registry();
        let switches: Map<String, Value> = registry_switches
            .switches()
            .iter()
            .map(|(name, allowed)| {
                let a = match allowed {
                    scatter_actors::tenancy::Allowed::OneOf(v) => json!({"choices": v}),
                    scatter_actors::tenancy::Allowed::Free(text) => json!({"text": text}),
                };
                let value = snap
                    .tenancy
                    .get(name)
                    .map_or(Value::Null, |r| r.value.clone());
                (name.clone(), json!({"allowed": a, "value": value}))
            })
            .collect();
        let ceilings: Map<String, Value> = registry
            .of_scope(Scope::Tenant)
            .filter_map(|d| Some((d.key.clone(), snap.ceiling(d)?.clone())))
            .collect();
        body["tenancy"] = Value::Object(switches);
        body["template"] = Value::String(snap.template_mode().to_string());
        body["locked"] = json!(snap.locked_keys());
        body["ceilings"] = Value::Object(ceilings);
    }
    if scope == Scope::Instance {
        let tenant_settings: Map<String, Value> = registry
            .of_scope(Scope::Tenant)
            .filter(|d| !d.retired)
            .map(|d| {
                let mut row = json!({
                    "registry_default": d.default,
                    "instance_default": snap.defaults.get(&d.key).map(|r| r.value.clone()),
                    "locked": snap.is_locked(&d.key),
                });
                if let Some(o) = own {
                    let set = o.own.get(&d.key).copied().unwrap_or(0);
                    row["inheriting"] = json!(o.tenants.saturating_sub(set));
                }
                (d.key.clone(), row)
            })
            .collect();
        body["tenant_settings"] = Value::Object(tenant_settings);
    }
    body
}

/// `GET /settings`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let scope = match scope_of(&app, &tenant, &q) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let (snap, parts) = match load(&**client, Some(&tenant.slug)).await {
        Ok(x) => x,
        Err(e) => return internal(&e),
    };
    let keys: Option<Vec<&str>> = q.get("keys").map(|k| k.split('|').collect());
    let group = q.get("group");
    if let Some(keys) = &keys
        && let Some(unknown) = keys.iter().find(|k| snap.registry().get(k).is_none())
    {
        return error(
            StatusCode::BAD_REQUEST,
            "ts-unknown-setting",
            format!("`{unknown}` is not a registered setting"),
        );
    }
    let mut settings = Map::new();
    for r in snap.resolve_all(scope) {
        let Some(def) = snap.registry().get(&r.key) else {
            continue;
        };
        if keys.as_ref().is_some_and(|k| !k.contains(&r.key.as_str()))
            || group.is_some_and(|g| &def.group != g)
        {
            continue;
        }
        let can_change =
            holds(&caller, scope, def.write_right()) && !(scope == Scope::Tenant && r.locked);
        let mut row = row_json(&r, can_change);
        if let Some((p, o)) = r.record {
            let id = match p {
                Partition::Instance => Some(parts.instance_config),
                Partition::Tenant => parts.tenant_config,
            };
            row["record"] = json!({"partition": id.map(|i| format!("{i:#018x}")), "offset": o});
        }
        settings.insert(r.key.clone(), row);
    }
    let base = match scope {
        Scope::Tenant => parts.tenant_next.unwrap_or(0),
        Scope::Instance => parts.instance_next,
    };
    let body = json!({"scope": scope.as_str(), "base": base, "settings": settings});
    respond_json(&headers, &method, &caller, &body)
}

/// The HTTP status of a refused save: the first refusal decides.
fn refusal_status(r: &Refusal) -> StatusCode {
    match r.code {
        "permissiondenied" => StatusCode::FORBIDDEN,
        "editconflict" => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    }
}

fn refused(refusals: &[Refusal]) -> Response {
    let first = &refusals[0];
    let mut body = json!({"code": first.code, "message": first.message, "errors": refusals});
    if let Some(k) = &first.key {
        body["key"] = Value::String(k.clone());
    }
    let mut r = error(refusal_status(first), first.code, first.message.clone());
    *r.body_mut() = axum::body::Body::from(body.to_string());
    r
}

/// Checks the CSRF token of a write made with a session cookie (0056 §1): a bearer
/// request needs none, as MediaWiki's REST API has it.
#[allow(clippy::result_large_err)] // a ready response, as the async routes return
pub(crate) fn check_token(
    app: &App,
    caller: &Caller,
    headers: &HeaderMap,
    body: &Map<String, Value>,
) -> Result<(), Response> {
    if bearer(headers).is_some() || !caller.has_session() {
        return Ok(());
    }
    let presented = body.get("token").and_then(Value::as_str);
    if presented == Some(caller.token(app, "csrf").as_str()) {
        Ok(())
    } else {
        Err(error(
            StatusCode::FORBIDDEN,
            "badtoken",
            "a write with a session needs its CSRF token as `token` in the body",
        ))
    }
}

/// `PATCH /settings`.
pub async fn patch(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
    body: Bytes,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let scope = match scope_of(&app, &tenant, &q) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(actor) = caller.identity.actor_key.clone() else {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "changing settings needs an account with `ts-config`",
        );
    };
    let mut object: Map<String, Value> = match serde_json::from_slice(&body) {
        Ok(Value::Object(o)) => o,
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "badbody",
                "the body is a JSON object",
            );
        }
    };
    if let Err(r) = check_token(&app, &caller, &headers, &object) {
        return r;
    }
    object.remove("token");
    let patch: Patch = match serde_json::from_value(Value::Object(object)) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::BAD_REQUEST, "badbody", e.to_string()),
    };
    if patch.is_empty() {
        return error(StatusCode::BAD_REQUEST, "badbody", "nothing to change");
    }
    let target = match scope {
        Scope::Tenant => Target::Tenant,
        Scope::Instance => Target::Instance,
    };
    let rights = |r: &str| holds(&caller, scope, r);
    let now = triplespace_api_action::edit::now_micros();
    match save(
        app.store(),
        app.pipeline(),
        Some(&tenant.slug),
        target,
        &patch,
        &actor,
        &rights,
        now,
    )
    .await
    {
        Ok(saved) => {
            let records: Vec<Value> = saved
                .records
                .iter()
                .map(|(p, o)| json!({"partition": format!("{p:#018x}"), "offset": o}))
                .collect();
            let body = json!({"changes": saved.changes, "records": records, "base": saved.base});
            http_cache::respond(
                &headers,
                StatusCode::OK,
                "application/json; charset=utf-8",
                body.to_string(),
                Class::Private,
                &[],
            )
        }
        Err(SaveError::Refused(r)) => refused(&r),
        Err(e) => internal(&e),
    }
}
