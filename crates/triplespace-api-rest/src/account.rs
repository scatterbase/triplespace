//! `GET /account/preferences`, `PATCH /account/preferences` and
//! `DELETE /account/preferences/{key}` (0027 §6; 0085 §5): the holder's own preferences.
//!
//! Each key is reported with its resolved value and where it came from, and with what
//! `Special:Preferences` needs to draw it: its type, choices, group and default, and
//! whether it follows the farm account or may be held in the browser. Changing needs
//! `editmyoptions`; a save is all or nothing.

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use triplespace_accounts::preferences::{self, PrefError, PreferenceRegistry};
use triplespace_api_action::App;
use triplespace_api_action::auth::Caller;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::prefs;
use triplespace_api_action::tenant::{self, Tenant};

use crate::entity_route::{error, internal};
use crate::settings::check_token;

/// The body of `PATCH /account/preferences`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    /// Values to set, by key.
    #[serde(default)]
    set: BTreeMap<String, Value>,
    /// Keys to reset to their defaults.
    #[serde(default)]
    reset: Vec<String>,
    /// Reset every key first, as `action=options&reset=1` does.
    #[serde(default)]
    reset_all: bool,
}

async fn prelude(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
) -> Result<(Tenant, Caller, String), Response> {
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
    let Some(actor) = caller.identity.actor_key.clone() else {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "notloggedin",
            "preferences belong to an account; log in to read or change them",
        ));
    };
    Ok((tenant, caller, actor))
}

/// The body every route answers with: the holder's preferences, resolved.
async fn body(
    client: &tokio_postgres::Client,
    tenant: &Tenant,
    actor: &str,
    uselang: Option<&str>,
    headers: &HeaderMap,
) -> Result<Value, Response> {
    let accept = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok());
    let resolved = prefs::resolve(client, &tenant.slug, Some(actor), uselang, accept)
        .await
        .map_err(|e| internal(&e))?;
    let registry = PreferenceRegistry::embedded();
    let mut out = Map::new();
    for (key, r) in &resolved.values {
        let Some(def) = registry.lookup(key) else {
            continue;
        };
        let mut row = json!({
            "value": r.value,
            "origin": r.origin,
            "own": resolved.own.contains_key(key),
            "type": def.ty,
            "group": def.group,
            "follows": def.follows,
            "label": def.label_message(),
        });
        if !def.choices.is_empty() {
            row["choices"] = json!(def.choices);
        }
        if let Some(d) = &def.default {
            row["default"] = d.clone();
        }
        if def.browser {
            row["browser"] = Value::Bool(true);
        }
        out.insert(key.clone(), row);
    }
    let groups: Vec<Value> = registry
        .groups()
        .iter()
        .map(|g| json!({"id": g, "label": format!("ts-prefs-group-{g}")}))
        .collect();
    Ok(json!({"groups": groups, "preferences": out}))
}

fn private_json(headers: &HeaderMap, body: &Value) -> Response {
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}

/// `GET /account/preferences`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, _caller, actor) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    match body(
        &client,
        &tenant,
        &actor,
        q.get("uselang").map(String::as_str),
        &headers,
    )
    .await
    {
        Ok(b) => private_json(&headers, &b),
        Err(r) => r,
    }
}

fn refuse(e: &PrefError) -> Response {
    let code = match e {
        PrefError::Unknown(_) => "ts-unknown-preference",
        PrefError::Invalid { .. } => "ts-invalid-preference",
    };
    error(StatusCode::BAD_REQUEST, code, e.to_string())
}

/// `PATCH /account/preferences`.
pub async fn patch(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body_bytes: Bytes,
) -> Response {
    let mut client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if caller.require("editmyoptions").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "changing preferences needs `editmyoptions`",
        );
    }
    let mut object: Map<String, Value> = match serde_json::from_slice(&body_bytes) {
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
    let change: Change = match serde_json::from_value(Value::Object(object)) {
        Ok(c) => c,
        Err(e) => return error(StatusCode::BAD_REQUEST, "badbody", e.to_string()),
    };
    let registry = PreferenceRegistry::embedded();
    for (key, v) in &change.set {
        if let Err(e) = registry.check(key, v) {
            return refuse(&e);
        }
    }
    for key in &change.reset {
        if registry.lookup(key).is_none() {
            return refuse(&PrefError::Unknown(key.clone()));
        }
    }
    let result: Result<(), triplespace_accounts::AccountsError> = async {
        let tx = client.transaction().await?;
        if change.reset_all {
            preferences::reset_all(&*tx, &actor).await?;
        }
        for key in &change.reset {
            preferences::reset(&*tx, &actor, key).await?;
        }
        for (key, v) in &change.set {
            preferences::set(&*tx, &actor, key, v).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        return internal(&e);
    }
    match body(&client, &tenant, &actor, None, &headers).await {
        Ok(b) => private_json(&headers, &b),
        Err(r) => r,
    }
}

/// `DELETE /account/preferences/{key}`: resets one key.
pub async fn delete(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller, actor) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if caller.require("editmyoptions").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "changing preferences needs `editmyoptions`",
        );
    }
    // A DELETE has no body to carry the token, so a session's token comes as `token=`.
    let token: Map<String, Value> = q
        .get("token")
        .map(|t| Map::from_iter([("token".to_string(), Value::String(t.clone()))]))
        .unwrap_or_default();
    if let Err(r) = check_token(&app, &caller, &headers, &token) {
        return r;
    }
    if PreferenceRegistry::embedded().lookup(&key).is_none() {
        return refuse(&PrefError::Unknown(key));
    }
    if let Err(e) = preferences::reset(&**client, &actor, &key).await {
        return internal(&e);
    }
    match body(&client, &tenant, &actor, None, &headers).await {
        Ok(b) => private_json(&headers, &b),
        Err(r) => r,
    }
}
