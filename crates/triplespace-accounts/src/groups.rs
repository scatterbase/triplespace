//! A tenant's groups (09 §3.2): the registry defaults, overlaid by the tenant's `group`
//! config records as `view.registry` holds them, with the tenant's suppression group.
//!
//! A record replaces the default group of its name wholly, as a `config` record replaces
//! what came before; a record that no longer passes the registry's rules (say a scoped
//! group whose permission has since lost its target) is left out, so that a group fails
//! closed rather than granting what its record no longer may.

use scatter_actors::group::{Group, GroupRegistry};
use tokio_postgres::GenericClient;

/// The groups of `tenant`.
pub async fn registry<C: GenericClient>(
    client: &C,
    tenant: &str,
) -> Result<GroupRegistry, tokio_postgres::Error> {
    let mut reg = GroupRegistry::default_registry().clone();
    let rows = client
        .query(
            "SELECT code, config FROM view.registry WHERE tenant = $1 AND kind = 'group' ORDER BY code",
            &[&tenant],
        )
        .await?;
    for r in rows {
        let name: String = r.get(0);
        let config: serde_json::Value = r.get(1);
        if let Ok(g) = Group::from_config(&name, &config, &reg) {
            let _ = reg.set_group(g);
        }
    }
    Ok(reg)
}

/// The tenant's suppression group (`moderation.suppress_group`), which `includes = ["*"]`
/// leaves out.
pub async fn suppress_group<C: GenericClient>(
    client: &C,
    tenant: &str,
) -> Result<String, tokio_postgres::Error> {
    let (s, _) = crate::settings::load(client, Some(tenant)).await?;
    Ok(s.value("moderation.suppress_group")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| scatter_actors::group::SUPPRESS.to_string()))
}

/// The tenant's registry with its suppression group set, for evaluating a principal in
/// `groups`: the setting is read only when one of those groups includes `"*"`, the one
/// case it matters.
pub async fn registry_for<C: GenericClient>(
    client: &C,
    tenant: &str,
    groups: impl IntoIterator<Item = &String>,
) -> Result<GroupRegistry, tokio_postgres::Error> {
    let mut reg = registry(client, tenant).await?;
    let every = groups
        .into_iter()
        .filter_map(|g| reg.by_name(g))
        .any(|g| g.includes.every);
    if every {
        let s = suppress_group(client, tenant).await?;
        reg.set_suppress_group(&s);
    }
    Ok(reg)
}
