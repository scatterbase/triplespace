//! `GET /backup-keys` and `PUT /backup-keys` (0103 A3, A5): the backup key set, at the farm
//! base. Reading needs only what reading needs, since the `config` partition is public and
//! the set holds public keys. Replacing it needs `ts-backup-keys`, an instance right held by
//! the instance's `owner` alone; on an instance with no owner account the operator registers
//! the set through `triplespace backup-key register` instead.
//!
//! The body of `PUT` is `{"keys": ["backup-key:…", …], "threshold": k, "token": …}`, with
//! `threshold` 1 when absent, so that any one key restores.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_actors::evaluate::instance_rights;
use scatter_integrity::backup_key::{KeySet, PublicKey};
use scatter_projection::Backend as _;
use serde_json::{Map, Value, json};
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::tenant::at_farm;
use triplespace_api_ingest::backup_keys;

use crate::acl::ok;
use crate::entity_route::{error, internal};
use crate::settings::{check_token, prelude};

/// The right `PUT` needs.
pub const RIGHT: &str = "ts-backup-keys";

fn set_json(r: &backup_keys::Registered) -> Value {
    let mut v = r.set.to_json();
    if let Some(o) = v.as_object_mut() {
        o.insert(
            "key_ids".into(),
            json!(
                r.set
                    .keys()
                    .iter()
                    .map(PublicKey::key_id)
                    .collect::<Vec<_>>()
            ),
        );
        o.insert("offset".into(), json!(r.offset));
        o.insert("actor".into(), json!(r.actor));
    }
    v
}

#[allow(clippy::result_large_err)] // a ready response, as the async routes return
fn at_farm_base(
    app: &App,
    tenant: &triplespace_api_action::tenant::Tenant,
) -> Result<(), Response> {
    if at_farm(app, tenant) {
        Ok(())
    } else {
        Err(error(
            StatusCode::BAD_REQUEST,
            "ts-instance-policy",
            "the backup key set is the instance's, served only at the farm base",
        ))
    }
}

/// `GET /backup-keys`: the set in force, `{"set": null}` when none is registered.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, _) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Err(r) = at_farm_base(&app, &tenant) {
        return r;
    }
    match backup_keys::current(&**client).await {
        Ok(Some(r)) => ok(&headers, &json!({"set": set_json(&r)})),
        Ok(None) => ok(&headers, &json!({"set": null})),
        Err(e) => internal(&e),
    }
}

/// `PUT /backup-keys`: registers or replaces the set, as the instance's `owner`.
pub async fn put(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
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
    if let Err(r) = at_farm_base(&app, &tenant) {
        return r;
    }
    let Some(actor) = caller.identity.actor_key.clone() else {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "replacing the backup key set needs the instance's owner",
        );
    };
    if !instance_rights(Some(&caller.effective), None).contains(RIGHT) {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "replacing the backup key set needs `ts-backup-keys`, which the instance's owner alone holds",
        );
    }
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
    let set = match KeySet::from_json(&Value::Object(object)) {
        Ok(s) => s,
        Err(e) => {
            return error(
                StatusCode::BAD_REQUEST,
                "ts-backup-keys-invalid",
                e.to_string(),
            );
        }
    };
    drop(client);
    let store = app.store();
    let now = triplespace_api_action::edit::now_micros();
    let mut cx = match store.begin().await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let offset = match backup_keys::register(
        store,
        app.pipeline(),
        &mut cx,
        &set,
        &actor,
        Some("backup key set"),
        now,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            let _ = store.rollback(cx).await;
            return internal(&e);
        }
    };
    if let Err(e) = store.commit(cx).await {
        return internal(&e);
    }
    let registered = backup_keys::Registered { set, offset, actor };
    ok(&headers, &json!({"set": set_json(&registered)}))
}
