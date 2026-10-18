//! The one validator every writer of a `site` record calls (0085 §3): the REST routes,
//! the settings pages through them, and `triplespace-cli settings`. It turns a request
//! into the records one save appends, all or nothing, or refuses it whole with every
//! reason found:
//!
//! - `ts-unknown-setting`: the key is not registered (or is retired);
//! - `ts-invalid-setting`: the value does not fit the type or choices, or a reference
//!   names nothing the tenant has;
//! - `ts-above-ceiling`: the value exceeds the instance's ceiling;
//! - `ts-locked`: the key is locked;
//! - `ts-instance-policy`: an instance-bound key at a tenant's scope, except a narrowing
//!   of a setting marked `tenant_may_narrow`;
//! - `permissiondenied`: the writer lacks `ts-config` or the entry's `right`;
//! - `editconflict`: a key in the request has a record at or after `base`.
//!
//! The plan is a pure function of a [`Snapshot`]; the writer (`triplespace-api-ingest`'s
//! `settings::save`) appends its records and [`Plan::event`] in one unit of work.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use scatter_actors::tenancy::{Allowed, TenancyRegistry};

use super::registry::{Scope, SettingDef, SettingType};
use super::resolve::{CONFIG_TEMPLATE, LOCKED, Row, Snapshot, is_narrowing};
use super::store::Partitions;
use super::value::check;

/// A save, as `PATCH /settings` takes it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// Values to set, by key.
    #[serde(default)]
    pub set: BTreeMap<String, Value>,
    /// Keys to return to inheriting: the tenant's record is tombstoned.
    #[serde(default)]
    pub unset: Vec<String>,
    /// The edit summary, carried in each record's comment part.
    #[serde(default)]
    pub summary: Option<String>,
    /// The `config` partition's next offset as the writer read it; a key with a record
    /// at or after it is a conflict. Absent: no conflict check.
    #[serde(default)]
    pub base: Option<u64>,
    /// At instance scope: the instance's default for a tenant setting, or `null` to
    /// remove it.
    #[serde(default)]
    pub defaults: BTreeMap<String, Option<Value>>,
    /// At instance scope: lock (`true`) or unlock (`false`) a tenant setting.
    #[serde(default)]
    pub locks: BTreeMap<String, bool>,
    /// At instance scope: tenancy switches.
    #[serde(default)]
    pub tenancy: BTreeMap<String, Value>,
}

impl Patch {
    /// Whether the patch asks for anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
            && self.unset.is_empty()
            && self.defaults.is_empty()
            && self.locks.is_empty()
            && self.tenancy.is_empty()
    }
}

/// Why a save, or part of one, is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// The error code.
    pub code: &'static str,
    /// The key it concerns, if one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Why.
    pub message: String,
}

impl Refusal {
    fn new(code: &'static str, key: &str, message: impl Into<String>) -> Self {
        Self {
            code,
            key: Some(key.to_string()),
            message: message.into(),
        }
    }
}

/// Which `config` a write lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// The tenant's.
    Tenant,
    /// The instance's.
    Instance,
}

/// One record a save appends.
#[derive(Debug, Clone, PartialEq)]
pub struct Write {
    /// Its partition.
    pub target: Target,
    /// The record key, `site:{key}`, `template:site:{key}`, `tenancy:{switch}`.
    pub key: String,
    /// Its content; `None` is a tombstone.
    pub content: Option<Value>,
}

/// One change the `settings/change` event reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    /// What changed: `tenant` or `instance` for a setting, `defaults`, `locks`, `tenancy`.
    pub scope: &'static str,
    /// The setting or switch.
    pub key: String,
    /// The value before; `null` when there was none.
    pub old: Value,
    /// The value after; `null` when there is none.
    pub new: Value,
}

/// What a save will do.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// The records, in the order to append them.
    pub writes: Vec<Write>,
    /// The changes, for the log event and the response.
    pub changes: Vec<Change>,
    /// The summary.
    pub summary: Option<String>,
}

/// Who is writing: whether they hold a right at the scope written. `owner` stands for
/// membership in the `owner` group.
pub trait Rights {
    /// Whether the writer holds `right` (or, for `owner`, is in the group).
    fn holds(&self, right: &str) -> bool;
}

impl<F: Fn(&str) -> bool> Rights for F {
    fn holds(&self, right: &str) -> bool {
        self(right)
    }
}

/// The record content of a `site` setting.
#[must_use]
pub fn site_record(key: &str, value: &Value) -> Value {
    json!({"kind": "site", "name": key, "value": value})
}

/// The record content of an instance default (`template:site:{key}`).
#[must_use]
pub fn template_record(key: &str, value: &Value) -> Value {
    json!({"kind": "template", "record": site_record(key, value)})
}

/// The record content of a tenancy switch, or of `tenancy:locked`.
#[must_use]
pub fn tenancy_record(switch: &str, value: &Value) -> Value {
    json!({"kind": "tenancy", "name": switch, "value": value})
}

/// The canonical namespace names, which a site name may not take (0085 §7).
fn namespace_names() -> &'static BTreeSet<String> {
    static NAMES: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let text = include_str!("../../../../docs/registry/namespaces.toml");
        let table: toml::Table = toml::from_str(text).unwrap_or_default();
        table
            .get("namespace")
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|n| n.get("canonical").and_then(toml::Value::as_str))
                    .filter(|c| !c.is_empty())
                    .map(str::to_lowercase)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Checks a site name as a namespace name (0085 §7): one line, no title separators or
/// forbidden characters, and no clash with a registered namespace's name or with the
/// `{name} talk` the Project talk namespace would take.
pub fn check_site_name(name: &str) -> Result<(), String> {
    if name.trim() != name || name.is_empty() {
        return Err("a site name has no leading or trailing space".into());
    }
    if name.contains("  ") || name.contains('_') {
        return Err("a site name has single spaces and no underscores".into());
    }
    if let Some(c) = name.chars().find(|c| {
        c.is_control() || matches!(c, ':' | '#' | '<' | '>' | '[' | ']' | '|' | '{' | '}' | '/')
    }) {
        return Err(format!("a site name cannot contain `{c}`"));
    }
    if name.len() > 200 {
        return Err("a site name is at most 200 bytes".into());
    }
    let lower = name.to_lowercase();
    let names = namespace_names();
    if lower != "project" && (names.contains(&lower) || names.contains(&format!("{lower} talk"))) {
        return Err(format!("`{name}` is already a namespace's name"));
    }
    Ok(())
}

struct Planner<'a, R: Rights + ?Sized> {
    snap: &'a Snapshot,
    rights: &'a R,
    refusals: Vec<Refusal>,
    plan: Plan,
}

impl<R: Rights + ?Sized> Planner<'_, R> {
    fn refuse(&mut self, code: &'static str, key: &str, message: impl Into<String>) {
        self.refusals.push(Refusal::new(code, key, message));
    }

    /// The registered, unretired definition, or a refusal.
    fn def(&mut self, key: &str) -> Option<SettingDef> {
        match self.snap.registry().get(key) {
            Some(d) if !d.retired => Some(d.clone()),
            Some(_) => {
                self.refuse("ts-unknown-setting", key, format!("`{key}` is retired"));
                None
            }
            None => {
                self.refuse(
                    "ts-unknown-setting",
                    key,
                    format!("`{key}` is not a registered setting"),
                );
                None
            }
        }
    }

    fn right(&mut self, def: &SettingDef) -> bool {
        let right = def.write_right();
        if self.rights.holds(right) {
            true
        } else {
            let msg = if right == "owner" {
                format!("only members of `owner` may change `{}`", def.key)
            } else {
                format!("changing `{}` needs `{right}`", def.key)
            };
            self.refuse("permissiondenied", &def.key, msg);
            false
        }
    }

    fn conflict(&mut self, key: &str, row: Option<&Row>, base: Option<u64>) {
        if let (Some(base), Some(row)) = (base, row)
            && row.offset >= base
        {
            self.refuse(
                "editconflict",
                key,
                format!(
                    "`{key}` changed at offset {} after the base {base}",
                    row.offset
                ),
            );
        }
    }

    /// The type, reference, asset and ceiling checks a value must pass to be stored.
    fn value(&mut self, def: &SettingDef, v: &Value) -> bool {
        if let Err(e) = check(def, v) {
            self.refuse(
                "ts-invalid-setting",
                &def.key,
                format!("`{}`: {e}", def.key),
            );
            return false;
        }
        if def.ty == SettingType::SiteAsset {
            // 0085 §8 needs the version named to exist and be readable by the setter,
            // and the asset route to serve it; neither exists until file storage does.
            self.refuse(
                "ts-invalid-setting",
                &def.key,
                format!(
                    "`{}` names a site asset, and this instance has no file storage yet",
                    def.key
                ),
            );
            return false;
        }
        if def.key == "site.name"
            && let Some(name) = v.as_str()
            && let Err(e) = check_site_name(name)
        {
            self.refuse("ts-invalid-setting", &def.key, e);
            return false;
        }
        if !self.snap.names_something(def, v) {
            self.refuse(
                "ts-invalid-setting",
                &def.key,
                format!("`{}`: {v} names nothing this wiki has", def.key),
            );
            return false;
        }
        if let Some(ceiling) = self.snap.exceeds(def, v) {
            self.refuse(
                "ts-above-ceiling",
                &def.key,
                format!(
                    "`{}`: {v} is above the instance's ceiling {ceiling}",
                    def.key
                ),
            );
            return false;
        }
        true
    }

    fn tenant_set(&mut self, key: &str, v: &Value, base: Option<u64>) {
        let Some(def) = self.def(key) else { return };
        let current = self
            .snap
            .tenant
            .as_ref()
            .and_then(|t| t.site.get(key))
            .cloned();
        if def.scope == Scope::Instance {
            if !def.tenant_may_narrow {
                self.refuse(
                    "ts-instance-policy",
                    key,
                    format!("`{key}` is the instance's; set it at the farm base"),
                );
                return;
            }
            if check(&def, v).is_ok()
                && !is_narrowing(&def, self.snap.instance.get(key).map(|r| &r.value), v)
            {
                self.refuse(
                    "ts-instance-policy",
                    key,
                    format!("`{key}` may only be narrowed on a wiki, not widened"),
                );
                return;
            }
        }
        if self.snap.is_locked(key) {
            self.refuse(
                "ts-locked",
                key,
                format!("`{key}` is locked by the instance"),
            );
            return;
        }
        if !self.right(&def) || !self.value(&def, v) {
            return;
        }
        self.conflict(key, current.as_ref(), base);
        let own = current.as_ref().filter(|r| !r.instance_act);
        if own.is_some_and(|r| &r.value == v) {
            return;
        }
        self.plan.writes.push(Write {
            target: Target::Tenant,
            key: format!("site:{key}"),
            content: Some(site_record(key, v)),
        });
        self.plan.changes.push(Change {
            scope: "tenant",
            key: key.to_string(),
            old: own.map_or(Value::Null, |r| r.value.clone()),
            new: v.clone(),
        });
    }

    fn tenant_unset(&mut self, key: &str, base: Option<u64>) {
        let Some(def) = self.def(key) else { return };
        if def.scope == Scope::Instance && !def.tenant_may_narrow {
            self.refuse(
                "ts-instance-policy",
                key,
                format!("`{key}` is the instance's"),
            );
            return;
        }
        if self.snap.is_locked(key) {
            self.refuse(
                "ts-locked",
                key,
                format!("`{key}` is locked by the instance"),
            );
            return;
        }
        if !self.right(&def) {
            return;
        }
        let Some(row) = self.snap.tenant_row(key).cloned() else {
            return;
        };
        self.conflict(key, Some(&row), base);
        self.plan.writes.push(Write {
            target: Target::Tenant,
            key: format!("site:{key}"),
            content: None,
        });
        self.plan.changes.push(Change {
            scope: "tenant",
            key: key.to_string(),
            old: row.value,
            new: Value::Null,
        });
    }

    fn instance_set(&mut self, key: &str, v: Option<&Value>, base: Option<u64>) {
        let Some(def) = self.def(key) else { return };
        if def.scope != Scope::Instance {
            self.refuse(
                "ts-invalid-setting",
                key,
                format!("`{key}` is a wiki's setting; give the instance's default for it under `defaults`"),
            );
            return;
        }
        if !self.right(&def) {
            return;
        }
        if let Some(v) = v
            && !self.value(&def, v)
        {
            return;
        }
        let current = self.snap.instance.get(key).cloned();
        self.conflict(key, current.as_ref(), base);
        if current.as_ref().map(|r| &r.value) == v {
            return;
        }
        if v.is_none() && current.is_none() {
            return;
        }
        self.plan.writes.push(Write {
            target: Target::Instance,
            key: format!("site:{key}"),
            content: v.map(|v| site_record(key, v)),
        });
        self.plan.changes.push(Change {
            scope: "instance",
            key: key.to_string(),
            old: current.map_or(Value::Null, |r| r.value),
            new: v.cloned().unwrap_or(Value::Null),
        });
    }

    fn default(&mut self, key: &str, v: Option<&Value>, base: Option<u64>) {
        let Some(def) = self.def(key) else { return };
        if def.scope != Scope::Tenant {
            self.refuse(
                "ts-invalid-setting",
                key,
                format!("`{key}` is the instance's own; set it under `set`"),
            );
            return;
        }
        if !self.snap.has_defaults() {
            self.refuse(
                "ts-instance-policy",
                key,
                "`config.template` is `none`, so the instance gives no defaults",
            );
            return;
        }
        if !self.rights.holds("ts-config") {
            self.refuse(
                "permissiondenied",
                key,
                "changing a default needs `ts-config` at the farm base",
            );
            return;
        }
        if let Some(v) = v {
            if let Err(e) = check(&def, v) {
                self.refuse("ts-invalid-setting", key, format!("`{key}`: {e}"));
                return;
            }
            if def.ty == SettingType::SiteAsset {
                self.refuse(
                    "ts-invalid-setting",
                    key,
                    "site assets need file storage, which this instance lacks",
                );
                return;
            }
            if let Some(ceiling) = self.snap.exceeds(&def, v) {
                self.refuse(
                    "ts-above-ceiling",
                    key,
                    format!("`{key}`: {v} is above the instance's ceiling {ceiling}"),
                );
                return;
            }
        }
        let current = self.snap.defaults.get(key).cloned();
        self.conflict(key, current.as_ref(), base);
        if current.as_ref().map(|r| &r.value) == v || (v.is_none() && current.is_none()) {
            return;
        }
        self.plan.writes.push(Write {
            target: Target::Instance,
            key: format!("template:site:{key}"),
            content: v.map(|v| template_record(key, v)),
        });
        self.plan.changes.push(Change {
            scope: "defaults",
            key: key.to_string(),
            old: current.map_or(Value::Null, |r| r.value),
            new: v.cloned().unwrap_or(Value::Null),
        });
    }

    fn locks(&mut self, locks: &BTreeMap<String, bool>, base: Option<u64>) {
        if locks.is_empty() {
            return;
        }
        if self.snap.template_mode() != "locks" {
            self.refuse(
                "ts-instance-policy",
                LOCKED,
                "locks need `config.template = locks`",
            );
            return;
        }
        if !self.rights.holds("ts-config") {
            self.refuse(
                "permissiondenied",
                LOCKED,
                "locking needs `ts-config` at the farm base",
            );
            return;
        }
        let before = self.snap.locked_keys();
        let mut after: BTreeSet<String> = before.iter().cloned().collect();
        for (key, lock) in locks {
            let Some(def) = self.def(key) else { continue };
            if def.scope != Scope::Tenant {
                self.refuse(
                    "ts-invalid-setting",
                    key,
                    format!("`{key}` is the instance's own; it cannot be locked"),
                );
                continue;
            }
            let k = format!("site:{key}");
            if *lock {
                after.insert(k);
            } else {
                after.remove(&k);
            }
        }
        let after: Vec<String> = after.into_iter().collect();
        let mut before_sorted = before.clone();
        before_sorted.sort();
        if after == before_sorted {
            return;
        }
        self.conflict(LOCKED, self.snap.tenancy.get(LOCKED), base);
        self.plan.writes.push(Write {
            target: Target::Instance,
            key: format!("tenancy:{LOCKED}"),
            content: Some(tenancy_record(LOCKED, &json!(after))),
        });
        self.plan.changes.push(Change {
            scope: "locks",
            key: LOCKED.to_string(),
            old: json!(before),
            new: json!(after),
        });
    }

    fn switch(&mut self, switch: &str, v: &Value, base: Option<u64>) {
        let registry = TenancyRegistry::default_registry();
        let Some(allowed) = registry.switches().get(switch) else {
            self.refuse(
                "ts-unknown-setting",
                switch,
                format!("`{switch}` is not a tenancy switch"),
            );
            return;
        };
        let Some(text) = v.as_str() else {
            self.refuse(
                "ts-invalid-setting",
                switch,
                format!("`{switch}` takes a string"),
            );
            return;
        };
        if let Allowed::OneOf(values) = allowed
            && !values.iter().any(|a| a == text)
        {
            self.refuse(
                "ts-invalid-setting",
                switch,
                format!(
                    "`{switch}` cannot be `{text}`; it is one of {}",
                    values.join(", ")
                ),
            );
            return;
        }
        // 08 §3: the identity switches are hard to reverse, so only `owner` changes them.
        let right = if switch.starts_with("identity.") {
            "owner"
        } else {
            "ts-config"
        };
        if !self.rights.holds(right) {
            self.refuse(
                "permissiondenied",
                switch,
                format!("changing `{switch}` needs `{right}`"),
            );
            return;
        }
        let current = self.snap.tenancy.get(switch).cloned();
        self.conflict(switch, current.as_ref(), base);
        if current.as_ref().is_some_and(|r| &r.value == v) {
            return;
        }
        self.plan.writes.push(Write {
            target: Target::Instance,
            key: format!("tenancy:{switch}"),
            content: Some(tenancy_record(switch, v)),
        });
        self.plan.changes.push(Change {
            scope: "tenancy",
            key: switch.to_string(),
            old: current.map_or(Value::Null, |r| r.value),
            new: v.clone(),
        });
    }
}

/// Plans a save at a tenant's base (`Target::Tenant`) or, with `scope=instance`, at the
/// farm base (`Target::Instance`). `rights` answers for the scope written: the
/// tenant's permissions, or the instance rights (0040 §9).
pub fn plan<R: Rights + ?Sized>(
    snap: &Snapshot,
    target: Target,
    patch: &Patch,
    rights: &R,
) -> Result<Plan, Vec<Refusal>> {
    let mut p = Planner {
        snap,
        rights,
        refusals: Vec::new(),
        plan: Plan {
            summary: patch.summary.clone(),
            ..Plan::default()
        },
    };
    for key in &patch.unset {
        if patch.set.contains_key(key) {
            p.refuse(
                "ts-invalid-setting",
                key,
                format!("`{key}` is both set and unset"),
            );
        }
    }
    match target {
        Target::Tenant => {
            if snap.tenant.is_none() {
                p.refusals.push(Refusal {
                    code: "ts-instance-policy",
                    key: None,
                    message: "no tenant to write to".into(),
                });
            }
            for (key, label) in patch
                .defaults
                .keys()
                .map(|k| (k, "defaults"))
                .chain(patch.locks.keys().map(|k| (k, "locks")))
                .chain(patch.tenancy.keys().map(|k| (k, "tenancy")))
            {
                p.refuse(
                    "ts-instance-policy",
                    key,
                    format!("`{label}` is written with scope=instance at the farm base"),
                );
            }
            for (key, v) in &patch.set {
                p.tenant_set(key, v, patch.base);
            }
            for key in &patch.unset {
                p.tenant_unset(key, patch.base);
            }
        }
        Target::Instance => {
            for (switch, v) in &patch.tenancy {
                if switch == LOCKED {
                    p.refuse(
                        "ts-invalid-setting",
                        switch,
                        "the locked list is changed through `locks`",
                    );
                    continue;
                }
                p.switch(switch, v, patch.base);
            }
            // A save that changes `config.template` and a default together is judged by
            // the mode it leaves; otherwise by the mode in force.
            let adjusted;
            let snap_for_rest = if let Some(mode) = patch.tenancy.get(CONFIG_TEMPLATE) {
                let mut s = snap.clone();
                s.tenancy
                    .insert(CONFIG_TEMPLATE.into(), Row::of(mode.clone(), u64::MAX));
                adjusted = s;
                &adjusted
            } else {
                snap
            };
            let mut q = Planner {
                snap: snap_for_rest,
                rights,
                refusals: Vec::new(),
                plan: Plan::default(),
            };
            for (key, v) in &patch.set {
                q.instance_set(key, Some(v), patch.base);
            }
            for key in &patch.unset {
                q.instance_set(key, None, patch.base);
            }
            for (key, v) in &patch.defaults {
                q.default(key, v.as_ref(), patch.base);
            }
            q.locks(&patch.locks, patch.base);
            p.refusals.extend(q.refusals);
            p.plan.writes.extend(q.plan.writes);
            p.plan.changes.extend(q.plan.changes);
        }
    }
    if p.refusals.is_empty() {
        Ok(p.plan)
    } else {
        Err(p.refusals)
    }
}

impl Plan {
    /// The `settings/change` log event's content (16 §2.3), or `None` for a save that
    /// changed nothing.
    #[must_use]
    pub fn event(&self, target: Target) -> Option<Value> {
        if self.changes.is_empty() {
            return None;
        }
        Some(json!({
            "type": "settings",
            "action": "change",
            "params": {
                "scope": target,
                "changes": self.changes,
            },
        }))
    }

    /// Which partition each write goes to.
    #[must_use]
    pub fn partition_of(target: Target, parts: &Partitions) -> Option<u64> {
        match target {
            Target::Instance => Some(parts.instance_config),
            Target::Tenant => parts.tenant_config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::resolve::TenantRows;

    fn snap() -> Snapshot {
        Snapshot {
            tenant: Some(TenantRows {
                slug: "librarybase".into(),
                ..TenantRows::default()
            }),
            ..Snapshot::default()
        }
    }

    fn all(_: &str) -> bool {
        true
    }

    fn config_only(r: &str) -> bool {
        r == "ts-config"
    }

    fn nothing(_: &str) -> bool {
        false
    }

    fn codes(r: &Result<Plan, Vec<Refusal>>) -> Vec<&'static str> {
        r.as_ref()
            .err()
            .map(|v| v.iter().map(|x| x.code).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_tenant_save() {
        let mut s = snap();
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("site.language".into(), Row::of(json!("en"), 4));
        let patch = Patch {
            set: BTreeMap::from([
                ("site.name".into(), json!("Librarybase")),
                ("site.language".into(), json!("de")),
            ]),
            unset: vec!["site.language".into()],
            ..Patch::default()
        };
        // Set and unset together is refused.
        assert_eq!(
            codes(&plan(&s, Target::Tenant, &patch, &all)),
            vec!["ts-invalid-setting"]
        );
        let patch = Patch {
            set: BTreeMap::from([
                ("site.name".into(), json!("Librarybase")),
                ("site.language".into(), json!("de")),
            ]),
            summary: Some("name it".into()),
            ..Patch::default()
        };
        let p = plan(&s, Target::Tenant, &patch, &all).unwrap();
        assert_eq!(p.writes.len(), 2);
        assert_eq!(p.writes[0].key, "site:site.language");
        assert_eq!(p.changes[0].old, json!("en"));
        let ev = p.event(Target::Tenant).unwrap();
        assert_eq!(ev["type"], "settings");
        assert_eq!(ev["params"]["changes"][1]["new"], "Librarybase");
        // Setting a value to what it is writes nothing.
        let same = Patch {
            set: BTreeMap::from([("site.language".into(), json!("en"))]),
            ..Patch::default()
        };
        assert!(
            plan(&s, Target::Tenant, &same, &all)
                .unwrap()
                .writes
                .is_empty()
        );
    }

    #[test]
    fn refusals() {
        let mut s = snap();
        s.tenancy
            .insert(CONFIG_TEMPLATE.into(), Row::of(json!("locks"), 1));
        s.tenancy
            .insert(LOCKED.into(), Row::of(json!(["site:site.language"]), 2));
        s.tenancy
            .insert("query.timeout".into(), Row::of(json!("PT1M"), 3));
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("site.name".into(), Row::of(json!("Old"), 9));
        let p = Patch {
            set: BTreeMap::from([
                ("no.such".into(), json!(1)),
                ("site.language".into(), json!("de")),
                ("query.timeout".into(), json!("PT5M")),
                ("account.approval".into(), json!("yes")),
                ("version.services".into(), json!("off")),
                ("site.name".into(), json!("User")),
            ]),
            base: Some(5),
            ..Patch::default()
        };
        let mut c = codes(&plan(&s, Target::Tenant, &p, &all));
        c.sort_unstable();
        assert_eq!(
            c,
            vec![
                "ts-above-ceiling",
                "ts-instance-policy",
                "ts-invalid-setting",
                "ts-invalid-setting",
                "ts-locked",
                "ts-unknown-setting"
            ]
        );
        let p = Patch {
            set: BTreeMap::from([("site.name".into(), json!("New"))]),
            base: Some(5),
            ..Patch::default()
        };
        assert_eq!(
            codes(&plan(&s, Target::Tenant, &p, &all)),
            vec!["editconflict"]
        );
        assert_eq!(
            codes(&plan(&s, Target::Tenant, &p, &nothing)),
            vec!["permissiondenied"]
        );
        let p = Patch {
            set: BTreeMap::from([("site.name".into(), json!("New"))]),
            base: Some(10),
            ..Patch::default()
        };
        assert!(plan(&s, Target::Tenant, &p, &all).is_ok());
        // `files.separation` is the owners'.
        let p = Patch {
            set: BTreeMap::from([("files.separation".into(), json!("tenant"))]),
            ..Patch::default()
        };
        assert_eq!(
            codes(&plan(&s, Target::Instance, &p, &config_only)),
            vec!["permissiondenied"]
        );
    }

    #[test]
    fn unset_returns_to_inheriting() {
        let mut s = snap();
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("site.language".into(), Row::of(json!("de"), 4));
        let p = Patch {
            unset: vec!["site.language".into(), "site.name".into()],
            ..Patch::default()
        };
        let plan = plan(&s, Target::Tenant, &p, &all).unwrap();
        assert_eq!(
            plan.writes,
            vec![Write {
                target: Target::Tenant,
                key: "site:site.language".into(),
                content: None
            }]
        );
    }

    #[test]
    fn instance_scope() {
        let mut s = Snapshot::default();
        let p = Patch {
            defaults: BTreeMap::from([("site.language".into(), Some(json!("de")))]),
            ..Patch::default()
        };
        // No defaults under `config.template = none`.
        assert_eq!(
            codes(&plan(&s, Target::Instance, &p, &all)),
            vec!["ts-instance-policy"]
        );
        // Turning defaults on in the same save allows them.
        let p = Patch {
            tenancy: BTreeMap::from([(CONFIG_TEMPLATE.into(), json!("locks"))]),
            defaults: BTreeMap::from([("site.language".into(), Some(json!("de")))]),
            locks: BTreeMap::from([("site.language".into(), true)]),
            set: BTreeMap::from([("version.services".into(), json!("off"))]),
            ..Patch::default()
        };
        let planned = plan(&s, Target::Instance, &p, &all).unwrap();
        let keys: Vec<&str> = planned.writes.iter().map(|w| w.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "tenancy:config.template",
                "site:version.services",
                "template:site:site.language",
                "tenancy:locked"
            ]
        );
        assert_eq!(
            planned.writes[2].content.as_ref().unwrap()["record"]["value"],
            "de"
        );
        // A switch value the registry does not allow; an identity switch without owner.
        s.tenancy
            .insert(CONFIG_TEMPLATE.into(), Row::of(json!("defaults"), 1));
        let p = Patch {
            tenancy: BTreeMap::from([
                ("wikitext.ceiling".into(), json!("everything")),
                ("identity.shared_names".into(), json!("on")),
            ]),
            ..Patch::default()
        };
        let mut c = codes(&plan(&s, Target::Instance, &p, &config_only));
        c.sort_unstable();
        assert_eq!(c, vec!["permissiondenied", "ts-invalid-setting"]);
        // A tenant setting under `set` at instance scope.
        let p = Patch {
            set: BTreeMap::from([("site.language".into(), json!("de"))]),
            ..Patch::default()
        };
        assert_eq!(
            codes(&plan(&s, Target::Instance, &p, &all)),
            vec!["ts-invalid-setting"]
        );
    }

    #[test]
    fn narrowing_an_instance_list() {
        let mut s = snap();
        s.instance
            .insert("files.types".into(), Row::of(json!(["png", "svg"]), 3));
        let narrow = Patch {
            set: BTreeMap::from([("files.types".into(), json!(["png"]))]),
            ..Patch::default()
        };
        assert!(plan(&s, Target::Tenant, &narrow, &all).is_ok());
        let widen = Patch {
            set: BTreeMap::from([("files.types".into(), json!(["png", "pdf"]))]),
            ..Patch::default()
        };
        assert_eq!(
            codes(&plan(&s, Target::Tenant, &widen, &all)),
            vec!["ts-instance-policy"]
        );
    }

    #[test]
    fn site_names() {
        assert!(check_site_name("Librarybase").is_ok());
        assert!(check_site_name("Project").is_ok());
        assert!(check_site_name("Library base").is_ok());
        assert!(check_site_name("Talk").is_err());
        assert!(check_site_name("User").is_err());
        assert!(check_site_name("File").is_err());
        assert!(check_site_name("A:B").is_err());
        assert!(check_site_name(" X").is_err());
        assert!(check_site_name("A_B").is_err());
    }
}
