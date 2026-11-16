//! `/group/{name}` (09 §3.2): read, define and retire a tenant's group, written through
//! [`triplespace_api_ingest::groups`]. The REST routes wrap these.
//!
//! - **`GET`**: the group's definition as its record carries it, whether that comes from
//!   a tenant record or the registry default (`source`), and its member count.
//! - **`PUT`**: the whole definition (`permissions`, `includes`, `adds`, `removes`,
//!   `adds_self`, `removes_self`, `within`, `notify_joins`, `mediawiki_name`), with a
//!   `reason`. Needs `ts-config`, and `userrights` too when it changes what a group
//!   includes, delegates or is scoped to.
//! - **`DELETE`**: retires the tenant's record; a default group falls back to the
//!   registry's definition.

use scatter_actors::group::{Group, GroupRegistryError};
use scatter_projection::Backend as _;
use serde_json::{Map, Value, json};
use triplespace_api_ingest::groups::{GroupWriteError, write as write_group};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::response::ApiError;

fn refused(e: GroupWriteError) -> ApiError {
    match e {
        GroupWriteError::Denied(r) => ApiError::permission_denied(r),
        GroupWriteError::Invalid(GroupRegistryError::UnknownPermission { permission, .. }) => {
            ApiError::bad_value("permissions", &permission)
        }
        GroupWriteError::Invalid(e) => ApiError::new("ts-invalid-group", e.to_string()),
        GroupWriteError::NothingToRetire(n) => ApiError::new(
            "ts-no-group-record",
            format!("Group `{n}` has no record of its own to retire."),
        ),
        GroupWriteError::Store(m) => ApiError::internal(m),
    }
}

fn check_name(name: &str) -> Result<(), ApiError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(ApiError::new(
            "ts-invalid-group",
            "A group name is 1–64 lower-case letters, digits, `-` and `_`.",
        ))
    }
}

/// `GET /group/{name}`.
///
/// # Errors
///
/// `ts-no-group` for a group the tenant does not have.
pub async fn read(ctx: &Ctx, name: &str) -> Result<Value, ApiError> {
    ctx.caller.require("read")?;
    let registry = triplespace_accounts::groups::registry(ctx.db(), &ctx.tenant.slug).await?;
    let own = ctx
        .db()
        .query_opt(
            "SELECT 1 FROM view.registry WHERE tenant = $1 AND kind = 'group' AND code = $2",
            &[&ctx.tenant.slug, &name],
        )
        .await?
        .is_some();
    let members: i64 = ctx
        .db()
        .query_one(
            "SELECT count(*) FROM view.membership
             WHERE tenant = $1 AND \"group\" = $2 AND layer = 'tenant'
               AND (expires IS NULL OR expires > $3)",
            &[
                &ctx.tenant.slug,
                &name,
                &i64::try_from(now_micros()).unwrap_or(i64::MAX),
            ],
        )
        .await?
        .get(0);
    let Some(g) = registry.by_name(name) else {
        if members > 0 {
            return Ok(
                json!({"group": {"kind": "group", "name": name, "permissions": []}, "source": "members", "members": members}),
            );
        }
        return Err(ApiError::new(
            "ts-no-group",
            format!("There is no group `{name}`."),
        ));
    };
    let mut config = g.to_config();
    if g.implicit.is_some() {
        config["implicit"] = json!(true);
    }
    Ok(json!({
        "group": config,
        "source": if own { "tenant" } else { "registry" },
        "members": members,
    }))
}

/// `PUT /group/{name}`.
///
/// # Errors
///
/// `ts-invalid-group`, `badvalue`, `permissiondenied`.
pub async fn write(ctx: &Ctx, name: &str, body: &Map<String, Value>) -> Result<Value, ApiError> {
    check_name(name)?;
    let registry = triplespace_accounts::groups::registry(ctx.db(), &ctx.tenant.slug).await?;
    let mut config = body.clone();
    config.remove("reason");
    config.insert("kind".into(), json!("group"));
    config.insert("name".into(), json!(name));
    let group = Group::from_config(name, &Value::Object(config), &registry)
        .map_err(|e| ApiError::new("ts-invalid-group", e.to_string()))?;
    save(ctx, name, Some(group), body).await?;
    read(ctx, name).await
}

/// `DELETE /group/{name}`.
///
/// # Errors
///
/// `ts-no-group-record` when the tenant has no record for the group.
pub async fn retire(ctx: &Ctx, name: &str, body: &Map<String, Value>) -> Result<Value, ApiError> {
    save(ctx, name, None, body).await?;
    match read(ctx, name).await {
        Ok(v) => Ok(v),
        Err(e) if e.code == "ts-no-group" => Ok(json!({"group": null, "source": null})),
        Err(e) => Err(e),
    }
}

async fn save(
    ctx: &Ctx,
    name: &str,
    group: Option<Group>,
    body: &Map<String, Value>,
) -> Result<(), ApiError> {
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("ts-config"))?;
    let registry = triplespace_accounts::groups::registry(ctx.db(), &ctx.tenant.slug).await?;
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let store = ctx.app.store();
    let mut cx = store.begin().await.map_err(ApiError::internal)?;
    match write_group(
        store,
        ctx.app.pipeline(),
        &mut cx,
        &ctx.tenant.slug,
        &actor,
        &ctx.caller.effective,
        &registry,
        name,
        group,
        reason,
        now_micros(),
    )
    .await
    {
        Ok(()) => store.commit(cx).await.map_err(ApiError::internal),
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(refused(e))
        }
    }
}
