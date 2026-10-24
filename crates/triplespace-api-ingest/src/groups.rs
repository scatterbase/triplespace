//! The one writer of `group` records (09 §3.2): `PUT /group/{name}` and `DELETE`.
//!
//! A group is a `config` record keyed `group:{name}` in the tenant's `config` partition,
//! whose content is the whole definition ([`Group::to_config`]); `null` retires it, and a
//! default group then falls back to the registry's definition. Writing one needs
//! `ts-config`; a write that changes `includes`, `adds`, `removes`, `adds_self`,
//! `removes_self` or `within` needs `userrights` as well, since each changes who may read
//! or who may enter a group. The definition must pass the registry's rules
//! ([`GroupRegistry::validate`]).

use scatter_actors::evaluate::Effective;
use scatter_actors::group::{Group, GroupRegistry, GroupRegistryError};
use scatter_ingest::store::IngestStore as _;
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::registry::PAYLOAD_CONFIG;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use triplespace_projections::PgCx;

use crate::PgIngest;

/// Why a group write was refused; the caller rolls back.
#[derive(Debug, thiserror::Error)]
pub enum GroupWriteError {
    /// The writer lacks a right.
    #[error("this needs the \"{0}\" right")]
    Denied(&'static str),
    /// The definition breaks a rule.
    #[error("{0}")]
    Invalid(#[from] GroupRegistryError),
    /// There is no record to retire.
    #[error("group `{0}` has no record of its own")]
    NothingToRetire(String),
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

fn store_err(e: impl std::fmt::Display) -> GroupWriteError {
    GroupWriteError::Store(e.to_string())
}

/// Whether two definitions differ in a field that needs `userrights` to change.
fn delegation_differs(old: Option<&Group>, new: Option<&Group>) -> bool {
    let empty = Group::default();
    let (a, b) = (old.unwrap_or(&empty), new.unwrap_or(&empty));
    a.includes != b.includes
        || a.adds != b.adds
        || a.removes != b.removes
        || a.adds_self != b.adds_self
        || a.removes_self != b.removes_self
        || a.within != b.within
}

/// Writes (`Some`) or retires (`None`) a group's record. `registry` is the tenant's as it
/// stands ([`triplespace_accounts::groups::registry`]).
///
/// # Errors
///
/// A [`GroupWriteError`].
#[allow(clippy::too_many_arguments)]
pub async fn write(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor: &str,
    who: &Effective,
    registry: &GroupRegistry,
    name: &str,
    group: Option<Group>,
    reason: &str,
    now: u64,
) -> Result<(), GroupWriteError> {
    if !who.holds("ts-config") {
        return Err(GroupWriteError::Denied("ts-config"));
    }
    let old = registry.by_name(name);
    let own_record = cx
        .conn()
        .query_opt(
            "SELECT 1 FROM view.registry WHERE tenant = $1 AND kind = 'group' AND code = $2",
            &[&tenant, &name],
        )
        .await
        .map_err(store_err)?
        .is_some();
    if group.is_none() && !own_record {
        return Err(GroupWriteError::NothingToRetire(name.to_string()));
    }
    let fallback = scatter_actors::group::GroupRegistry::default_registry().by_name(name);
    let after = group.as_ref().or(fallback);
    if delegation_differs(old, after) && !who.holds("userrights") {
        return Err(GroupWriteError::Denied("userrights"));
    }
    if let Some(g) = &group {
        registry.validate(g)?;
    }
    let config = store
        .partition(cx, tenant, "config")
        .await
        .map_err(store_err)?
        .ok_or_else(|| store_err(format!("no `config` partition for `{tenant}`")))?;
    let content = group
        .as_ref()
        .map_or(Value::Null, |g| Value::from_json(&g.to_config()));
    let draft = Draft {
        appended_at: now,
        payload_type: PAYLOAD_CONFIG.into(),
        key: Some(format!("group:{name}")),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &content,
            &Value::text(reason),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(store_err)?,
    };
    append_projected(store, pipeline, cx, config, draft, Budget::NONE)
        .await
        .map_err(store_err)?;
    Ok(())
}
