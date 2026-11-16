//! Reading a [`Snapshot`] from `view.registry` and `log.partition`: the rows that bear
//! on one tenant's settings (or the instance's alone) in one query, and the partitions a
//! save writes to in another.

use std::collections::BTreeMap;

use serde_json::Value;
use tokio_postgres::GenericClient;

use super::registry::{SETTING_KIND, SettingsRegistry};
use super::resolve::{Row, Snapshot, TenantRows};

/// The `config` and `log` partitions a save writes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Partitions {
    /// The instance `config`, partition 0.
    pub instance_config: u64,
    /// The instance `config`'s next offset: the base of a later instance-scope write.
    pub instance_next: u64,
    /// The instance `log`, if it exists.
    pub instance_log: Option<u64>,
    /// The tenant's `config`.
    pub tenant_config: Option<u64>,
    /// The tenant `config`'s next offset: the base of a later tenant-scope write.
    pub tenant_next: Option<u64>,
    /// The tenant's `log`.
    pub tenant_log: Option<u64>,
}

fn u64_of(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// The value a configuration row holds, by kind: a `site` or `tenancy` record's
/// `value`; a `template:site:` record's inner record's `value` (`{kind: "template",
/// record: {kind: "site", name, value}}`), or a bare `value` beside `kind`.
#[must_use]
pub fn value_of(kind: &str, config: &Value) -> Option<Value> {
    match kind {
        "template" => config
            .get("record")
            .and_then(|r| r.get("value"))
            .or_else(|| config.get("value"))
            .cloned(),
        _ => config.get("value").cloned(),
    }
}

/// Reads the snapshot for a tenant (`Some(slug)`) or for the instance alone (`None`),
/// with the partitions a save would write to.
pub async fn load<C: GenericClient>(
    client: &C,
    tenant: Option<&str>,
) -> Result<(Snapshot, Partitions), tokio_postgres::Error> {
    let slug = tenant.unwrap_or("");
    let rows = client
        .query(
            "SELECT tenant, kind, code, config, \"offset\", actor, instance_act FROM view.registry
             WHERE (tenant = '' AND kind IN ('site', 'template', 'tenancy', 'setting'))
                OR ($1 <> '' AND tenant = $1 AND kind IN ('site', 'group'))",
            &[&slug],
        )
        .await?;
    let mut snap = Snapshot::default();
    let mut setting_records = Vec::new();
    let mut tenant_rows = tenant.map(|t| TenantRows {
        slug: t.to_string(),
        ..TenantRows::default()
    });
    for r in &rows {
        let row_tenant: String = r.get(0);
        let kind: String = r.get(1);
        let code: String = r.get(2);
        let config: Value = r.get(3);
        let offset = u64_of(r.get(4));
        let actor: Option<String> = r.get(5);
        let instance_act: bool = r.get(6);
        let row = |value: Value| Row {
            value,
            offset,
            actor: actor.clone(),
            instance_act,
        };
        if row_tenant.is_empty() {
            match kind.as_str() {
                "site" => {
                    if let Some(v) = value_of(&kind, &config) {
                        snap.instance.insert(code, row(v));
                    }
                }
                "template" => {
                    if let Some(key) = code.strip_prefix("site:")
                        && let Some(v) = value_of(&kind, &config)
                    {
                        snap.defaults.insert(key.to_string(), row(v));
                    }
                }
                "tenancy" => {
                    if let Some(v) = value_of(&kind, &config) {
                        snap.tenancy.insert(code, row(v));
                    }
                }
                SETTING_KIND => setting_records.push(config),
                _ => {}
            }
        } else if let Some(t) = tenant_rows.as_mut() {
            match kind.as_str() {
                "site" => {
                    if let Some(v) = value_of(&kind, &config) {
                        t.site.insert(code, row(v));
                    }
                }
                "group" => {
                    t.groups.insert(code);
                }
                _ => {}
            }
        }
    }
    if !setting_records.is_empty() {
        snap.registry = Some(SettingsRegistry::embedded().overlay(&setting_records));
    }
    let p = partitions(client, tenant).await?;
    snap.instance_offset = Some(p.instance_next);
    if let Some(t) = tenant_rows.as_mut() {
        t.offset = p.tenant_next;
    }
    snap.tenant = tenant_rows;
    Ok((snap, p))
}

/// The `config` and `log` partitions of the instance and, with a slug, of the tenant.
pub async fn partitions<C: GenericClient>(
    client: &C,
    tenant: Option<&str>,
) -> Result<Partitions, tokio_postgres::Error> {
    let slug = tenant.unwrap_or("");
    let parts = client
        .query(
            "SELECT partition, tenant, name, next_offset FROM log.partition
             WHERE name IN ('config', 'log') AND (tenant IS NULL OR tenant = $1)",
            &[&slug],
        )
        .await?;
    let mut p = Partitions::default();
    let mut by: BTreeMap<(bool, String), (u64, u64)> = BTreeMap::new();
    for r in &parts {
        let partition: i64 = r.get(0);
        let t: Option<String> = r.get(1);
        let name: String = r.get(2);
        let next: i64 = r.get(3);
        by.insert(
            (t.is_some(), name),
            (scatter_log_id(partition), u64_of(next)),
        );
    }
    if let Some((id, next)) = by.get(&(false, "config".to_string())) {
        p.instance_config = *id;
        p.instance_next = *next;
    }
    p.instance_log = by.get(&(false, "log".to_string())).map(|(id, _)| *id);
    if tenant.is_some() {
        p.tenant_config = by.get(&(true, "config".to_string())).map(|(id, _)| *id);
        p.tenant_next = by.get(&(true, "config".to_string())).map(|(_, n)| *n);
        p.tenant_log = by.get(&(true, "log".to_string())).map(|(id, _)| *id);
    }
    Ok(p)
}

/// A partition ID as `log.partition` stores it (a signed `bigint` holding 64 random
/// bits) back to the log's `u64`.
fn scatter_log_id(db: i64) -> u64 {
    u64::from_ne_bytes(db.to_ne_bytes())
}

/// The instance's `setting:` records over the embedded registry, for a reader that needs
/// the registry without a snapshot.
pub async fn registry<C: GenericClient>(
    client: &C,
) -> Result<SettingsRegistry, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT config FROM view.registry WHERE tenant = '' AND kind = 'setting'",
            &[],
        )
        .await?;
    let records: Vec<Value> = rows.iter().map(|r| r.get(0)).collect();
    Ok(SettingsRegistry::embedded().overlay(&records))
}
