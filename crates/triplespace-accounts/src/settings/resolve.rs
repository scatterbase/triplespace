//! Resolving a setting (0085 §2.1).
//!
//! A tenant setting resolves, first match wins: the tenant's own `site` record; the
//! instance's default, its `template:site:{key}` record, read when the value is needed;
//! the registry default. A lock makes the instance's value the value; a ceiling caps it.
//! An instance setting resolves to the instance's record, then the registry default, and
//! a tenant record narrows one marked `tenant_may_narrow`. Every resolved value carries
//! its origin.
//!
//! Resolution is a pure function of a [`Snapshot`], the configuration rows that bear on
//! one tenant, which [`super::store`] reads in two queries.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};

use super::registry::{Bound, Scope, SettingDef, SettingType, SettingsRegistry};
use super::value::magnitude;

/// The `config.template` switch (0028 §8).
pub const CONFIG_TEMPLATE: &str = "config.template";
/// The code of the `tenancy:locked` list.
pub const LOCKED: &str = "locked";
/// The switch that caps `wikitext.expansion` and `wikitext.lua` (0042 §2).
pub const WIKITEXT_CEILING: &str = "wikitext.ceiling";

/// One configuration row: the value it holds and who wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The value: a `site` or `tenancy` record's `value`, or a `template:site:` record's
    /// inner record's `value`.
    pub value: Value,
    /// The record's offset in its `config` partition.
    pub offset: u64,
    /// The attestation's actor.
    pub actor: Option<String>,
    /// Whether the record is an instance act (08 §8.3).
    pub instance_act: bool,
}

impl Row {
    /// A row with no attestation details, for tests and for values computed elsewhere.
    #[must_use]
    pub fn of(value: Value, offset: u64) -> Self {
        Self {
            value,
            offset,
            actor: None,
            instance_act: false,
        }
    }
}

/// The configuration that bears on one tenant's settings, or on the instance's alone.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    /// The registry, the embedded file with the instance's `setting:` records over it.
    pub registry: Option<SettingsRegistry>,
    /// The instance's own `site` records, by key.
    pub instance: BTreeMap<String, Row>,
    /// The instance's defaults, `template:site:{key}`, by key.
    pub defaults: BTreeMap<String, Row>,
    /// The tenancy switches, `tenancy:{switch}`, by switch; `locked` among them.
    pub tenancy: BTreeMap<String, Row>,
    /// The instance `config`'s last offset.
    pub instance_offset: Option<u64>,
    /// The tenant, when the snapshot is of one.
    pub tenant: Option<TenantRows>,
}

/// A tenant's part of a snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TenantRows {
    /// The slug.
    pub slug: String,
    /// The tenant's `site` records, by key, instance acts included.
    pub site: BTreeMap<String, Row>,
    /// The tenant's groups, which a `group` reference must name.
    pub groups: BTreeSet<String>,
    /// The tenant `config`'s last offset.
    pub offset: Option<u64>,
}

/// Where a resolved value came from (0085 §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// The registry default.
    Registry,
    /// The instance: its default for a tenant setting, or its record for its own.
    Instance,
    /// The tenant's own record.
    Tenant,
    /// The instance's value, locked.
    Locked,
}

/// Which `config` partition a record is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Partition {
    /// The instance's, partition 0.
    Instance,
    /// The tenant's.
    Tenant,
}

/// A resolved setting.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    /// The key.
    pub key: String,
    /// The value; `None` where the setting is unset and has no default.
    pub value: Option<Value>,
    /// Where it came from.
    pub origin: Origin,
    /// The ceiling, where one capped the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capped: Option<Value>,
    /// The record that set it: its partition and offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<(Partition, u64)>,
    /// For a tenant setting: the instance's default, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_default: Option<Value>,
    /// The instance's default names nothing this tenant has, so the registry default
    /// applies (0085 §2.1).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unresolved_default: bool,
    /// The key is locked by the instance.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

impl Snapshot {
    /// The registry in force.
    #[must_use]
    pub fn registry(&self) -> &SettingsRegistry {
        self.registry
            .as_ref()
            .unwrap_or_else(|| SettingsRegistry::embedded())
    }

    /// The `config.template` mode: `none`, `defaults` or `locks`; `none` when unset.
    #[must_use]
    pub fn template_mode(&self) -> &str {
        self.tenancy
            .get(CONFIG_TEMPLATE)
            .and_then(|r| r.value.as_str())
            .unwrap_or("none")
    }

    /// Whether instance defaults apply at all.
    #[must_use]
    pub fn has_defaults(&self) -> bool {
        self.template_mode() != "none"
    }

    /// Whether a tenant setting is locked: `config.template = locks` and `tenancy:locked`
    /// names `site:{key}`.
    #[must_use]
    pub fn is_locked(&self, key: &str) -> bool {
        self.template_mode() == "locks" && self.locked_keys().contains(&format!("site:{key}"))
    }

    /// The `tenancy:locked` list.
    #[must_use]
    pub fn locked_keys(&self) -> Vec<String> {
        self.tenancy
            .get(LOCKED)
            .and_then(|r| r.value.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The instance's default for a tenant setting, if defaults apply and it has one.
    #[must_use]
    pub fn default_row(&self, key: &str) -> Option<&Row> {
        if self.has_defaults() {
            self.defaults.get(key)
        } else {
            None
        }
    }

    /// The tenant's own record for a setting: not an instance act (0085 §2.1).
    #[must_use]
    pub fn tenant_row(&self, key: &str) -> Option<&Row> {
        self.tenant
            .as_ref()
            .and_then(|t| t.site.get(key))
            .filter(|r| !r.instance_act)
    }

    /// The ceiling on a value of a `bound = ceiling` setting, where the instance sets one.
    #[must_use]
    pub fn ceiling(&self, def: &SettingDef) -> Option<&Value> {
        let switch = def.ceiling_switch()?;
        self.tenancy.get(switch).map(|r| &r.value)
    }

    /// Resolves one setting; `None` for a key the registry does not know.
    #[must_use]
    pub fn resolve(&self, key: &str) -> Option<Resolved> {
        let def = self.registry().get(key)?;
        Some(match def.scope {
            Scope::Tenant => self.resolve_tenant(def),
            Scope::Instance => self.resolve_instance(def),
        })
    }

    /// Resolves every setting of a scope.
    #[must_use]
    pub fn resolve_all(&self, scope: Scope) -> Vec<Resolved> {
        self.registry()
            .of_scope(scope)
            .filter(|d| !d.retired)
            .map(|d| match scope {
                Scope::Tenant => self.resolve_tenant(d),
                Scope::Instance => self.resolve_instance(d),
            })
            .collect()
    }

    /// The value of a setting, resolved; a convenience over [`Self::resolve`].
    #[must_use]
    pub fn value(&self, key: &str) -> Option<Value> {
        self.resolve(key).and_then(|r| r.value)
    }

    fn registry_default(&self, def: &SettingDef) -> Option<Value> {
        def.default.clone().or_else(|| self.computed_default(def))
    }

    /// The defaults the registry describes as computed (`no_default` with a rule).
    fn computed_default(&self, def: &SettingDef) -> Option<Value> {
        match def.key.as_str() {
            // 0085 §7: the default is the slug.
            "site.name" => self.tenant.as_ref().map(|t| Value::String(t.slug.clone())),
            _ => None,
        }
    }

    /// Whether the tenant has what a reference-typed value names. Only the kinds whose
    /// records a snapshot holds are checked; the rest are taken as named.
    #[must_use]
    pub fn names_something(&self, def: &SettingDef, v: &Value) -> bool {
        use super::registry::RefKind;
        match (def.ty, &self.tenant) {
            (SettingType::Reference(RefKind::Group), Some(t)) => {
                v.as_str().is_some_and(|g| t.groups.contains(g))
            }
            _ => true,
        }
    }

    fn resolve_tenant(&self, def: &SettingDef) -> Resolved {
        let key = def.key.clone();
        let default_row = self.default_row(&key);
        let mut instance_default = default_row.map(|r| r.value.clone());
        let mut unresolved_default = false;
        if let Some(d) = &instance_default
            && !self.names_something(def, d)
        {
            unresolved_default = true;
            instance_default = None;
        }
        let locked = self.is_locked(&key);
        let (value, origin, record) = if locked {
            match (default_row, &instance_default) {
                (Some(r), Some(v)) => (
                    Some(v.clone()),
                    Origin::Locked,
                    Some((Partition::Instance, r.offset)),
                ),
                _ => (self.registry_default(def), Origin::Locked, None),
            }
        } else if let Some(r) = self.tenant_row(&key) {
            (
                Some(r.value.clone()),
                Origin::Tenant,
                Some((Partition::Tenant, r.offset)),
            )
        } else if let (Some(r), Some(v)) = (default_row, &instance_default) {
            (
                Some(v.clone()),
                Origin::Instance,
                Some((Partition::Instance, r.offset)),
            )
        } else {
            (self.registry_default(def), Origin::Registry, None)
        };
        let (value, capped) = match value {
            Some(v) if def.bound == Bound::Ceiling => match self.cap(def, &v) {
                Some((capped_value, ceiling)) => (Some(capped_value), Some(ceiling)),
                None => (Some(v), None),
            },
            v => (v, None),
        };
        Resolved {
            key,
            value,
            origin,
            capped,
            record,
            instance_default,
            unresolved_default,
            locked,
        }
    }

    fn resolve_instance(&self, def: &SettingDef) -> Resolved {
        let key = def.key.clone();
        let (mut value, mut origin, mut record) = match self.instance.get(&key) {
            Some(r) => (
                Some(r.value.clone()),
                Origin::Instance,
                Some((Partition::Instance, r.offset)),
            ),
            None => (def.default.clone(), Origin::Registry, None),
        };
        if def.tenant_may_narrow
            && let Some(r) = self.tenant_row(&key)
        {
            let narrowed = match &value {
                Some(wide) => narrow(def, wide, &r.value),
                None => Some(r.value.clone()),
            };
            if let Some(n) = narrowed {
                value = Some(n);
                origin = Origin::Tenant;
                record = Some((Partition::Tenant, r.offset));
            }
        }
        Resolved {
            key,
            value,
            origin,
            capped: None,
            record,
            instance_default: None,
            unresolved_default: false,
            locked: false,
        }
    }

    /// Whether a value exceeds the setting's ceiling; `Some(ceiling)` when it does.
    #[must_use]
    pub fn exceeds(&self, def: &SettingDef, v: &Value) -> Option<Value> {
        self.cap(def, v).map(|(_, ceiling)| ceiling)
    }

    /// Caps a value: `Some((capped value, ceiling))` when the ceiling changed it.
    fn cap(&self, def: &SettingDef, v: &Value) -> Option<(Value, Value)> {
        let switch = def.ceiling_switch()?;
        let ceiling = self.tenancy.get(switch)?.value.clone();
        if switch == WIKITEXT_CEILING {
            // 23 §3.4: subset < expansion < lua; `wikitext.expansion` needs at least
            // `expansion`, `wikitext.lua` needs `lua` (the order 0085 Q4 notes is implicit).
            let level = |s: &str| match s {
                "subset" => 0,
                "expansion" => 1,
                _ => 2,
            };
            let need = if def.key == "wikitext.lua" { 2 } else { 1 };
            let have = level(ceiling.as_str().unwrap_or("lua"));
            return (v.as_bool() == Some(true) && have < need)
                .then_some((Value::Bool(false), ceiling));
        }
        match def.ty {
            SettingType::LanguageList | SettingType::StringList => {
                let allowed: BTreeSet<&str> = ceiling
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .collect();
                let items = v.as_array()?;
                let kept: Vec<Value> = items
                    .iter()
                    .filter(|x| x.as_str().is_some_and(|s| allowed.contains(s)))
                    .cloned()
                    .collect();
                (kept.len() < items.len()).then(|| (Value::Array(kept), ceiling.clone()))
            }
            _ => {
                let have = magnitude(def, v)?;
                let max = magnitude(def, &ceiling)?;
                (have > max).then(|| (ceiling.clone(), ceiling.clone()))
            }
        }
    }
}

/// A tenant's narrowing of an instance setting marked `tenant_may_narrow`: for a list,
/// the tenant's items the instance's list has; for a magnitude, the smaller. `None`
/// when the tenant's value is not a narrowing the type allows.
#[must_use]
pub fn narrow(def: &SettingDef, wide: &Value, tenant: &Value) -> Option<Value> {
    match def.ty {
        SettingType::LanguageList | SettingType::StringList => {
            let allowed: BTreeSet<&str> =
                wide.as_array()?.iter().filter_map(Value::as_str).collect();
            Some(Value::Array(
                tenant
                    .as_array()?
                    .iter()
                    .filter(|x| x.as_str().is_some_and(|s| allowed.contains(s)))
                    .cloned()
                    .collect(),
            ))
        }
        _ => {
            let (w, t) = (magnitude(def, wide)?, magnitude(def, tenant)?);
            Some(if t < w { tenant.clone() } else { wide.clone() })
        }
    }
}

/// Whether a tenant value only narrows the instance's (for the write check).
#[must_use]
pub fn is_narrowing(def: &SettingDef, wide: Option<&Value>, tenant: &Value) -> bool {
    let Some(wide) = wide else {
        // Nothing to narrow: an unset instance list allows everything.
        return true;
    };
    match def.ty {
        SettingType::LanguageList | SettingType::StringList => {
            let (Some(w), Some(t)) = (wide.as_array(), tenant.as_array()) else {
                return false;
            };
            t.iter().all(|x| w.contains(x))
        }
        _ => matches!((magnitude(def, wide), magnitude(def, tenant)), (Some(w), Some(t)) if t <= w),
    }
}

/// The JSON a `GET /settings` row carries beyond [`Resolved`]: nothing yet; kept as one
/// place to add viewer-dependent fields.
#[must_use]
pub fn row_json(r: &Resolved, can_change: bool) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.remove("key");
        if let Some((p, o)) = r.record {
            obj.insert("record".into(), json!({"partition": p, "offset": o}));
        }
        obj.insert("can_change".into(), Value::Bool(can_change));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            tenant: Some(TenantRows {
                slug: "librarybase".into(),
                ..TenantRows::default()
            }),
            ..Snapshot::default()
        }
    }

    fn tenancy(s: &mut Snapshot, switch: &str, v: Value) {
        s.tenancy.insert(switch.into(), Row::of(v, 1));
    }

    #[test]
    fn registry_then_instance_then_tenant() {
        let mut s = snap();
        let r = s.resolve("site.language").unwrap();
        assert_eq!((r.value, r.origin), (Some(json!("en")), Origin::Registry));
        // A default is ignored under `config.template = none`.
        s.defaults
            .insert("site.language".into(), Row::of(json!("de"), 5));
        assert_eq!(s.resolve("site.language").unwrap().origin, Origin::Registry);
        tenancy(&mut s, CONFIG_TEMPLATE, json!("defaults"));
        let r = s.resolve("site.language").unwrap();
        assert_eq!((r.value, r.origin), (Some(json!("de")), Origin::Instance));
        assert_eq!(r.record, Some((Partition::Instance, 5)));
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("site.language".into(), Row::of(json!("fr"), 9));
        let r = s.resolve("site.language").unwrap();
        assert_eq!((r.value, r.origin), (Some(json!("fr")), Origin::Tenant));
        assert_eq!(r.instance_default, Some(json!("de")));
    }

    #[test]
    fn an_instance_act_is_not_the_tenants_value() {
        let mut s = snap();
        tenancy(&mut s, CONFIG_TEMPLATE, json!("defaults"));
        s.defaults
            .insert("site.language".into(), Row::of(json!("de"), 5));
        s.tenant.as_mut().unwrap().site.insert(
            "site.language".into(),
            Row {
                instance_act: true,
                ..Row::of(json!("nl"), 3)
            },
        );
        let r = s.resolve("site.language").unwrap();
        assert_eq!((r.value, r.origin), (Some(json!("de")), Origin::Instance));
    }

    #[test]
    fn a_lock_takes_the_instances_value() {
        let mut s = snap();
        tenancy(&mut s, CONFIG_TEMPLATE, json!("locks"));
        tenancy(&mut s, LOCKED, json!(["site:site.language"]));
        s.defaults
            .insert("site.language".into(), Row::of(json!("de"), 5));
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("site.language".into(), Row::of(json!("fr"), 9));
        let r = s.resolve("site.language").unwrap();
        assert_eq!(
            (r.value, r.origin, r.locked),
            (Some(json!("de")), Origin::Locked, true)
        );
        // Unlocked, the tenant's own value is back.
        tenancy(&mut s, LOCKED, json!([]));
        assert_eq!(s.resolve("site.language").unwrap().origin, Origin::Tenant);
    }

    #[test]
    fn the_site_name_defaults_to_the_slug() {
        let s = snap();
        assert_eq!(s.value("site.name"), Some(json!("librarybase")));
    }

    #[test]
    fn ceilings_cap() {
        let mut s = snap();
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("wikitext.lua".into(), Row::of(json!(true), 2));
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("wikitext.expansion".into(), Row::of(json!(true), 3));
        assert_eq!(s.value("wikitext.lua"), Some(json!(true)));
        tenancy(&mut s, WIKITEXT_CEILING, json!("expansion"));
        let r = s.resolve("wikitext.lua").unwrap();
        assert_eq!(
            (r.value, r.capped),
            (Some(json!(false)), Some(json!("expansion")))
        );
        assert_eq!(s.value("wikitext.expansion"), Some(json!(true)));
        tenancy(&mut s, WIKITEXT_CEILING, json!("subset"));
        assert_eq!(s.value("wikitext.expansion"), Some(json!(false)));

        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("query.timeout".into(), Row::of(json!("PT5M"), 4));
        tenancy(&mut s, "query.timeout", json!("PT1M"));
        let r = s.resolve("query.timeout").unwrap();
        assert_eq!(
            (r.value, r.capped),
            (Some(json!("PT1M")), Some(json!("PT1M")))
        );
        let def = s.registry().get("query.timeout").unwrap().clone();
        assert_eq!(s.exceeds(&def, &json!("PT30S")), None);
        assert_eq!(s.exceeds(&def, &json!("PT2M")), Some(json!("PT1M")));

        s.tenant.as_mut().unwrap().site.insert(
            "entities.term_languages".into(),
            Row::of(json!(["en", "de", "fr"]), 5),
        );
        tenancy(&mut s, "entities.term_languages", json!(["en", "fr"]));
        assert_eq!(
            s.value("entities.term_languages"),
            Some(json!(["en", "fr"]))
        );
    }

    #[test]
    fn instance_settings_and_narrowing() {
        let mut s = snap();
        assert_eq!(
            s.resolve("version.services").unwrap().origin,
            Origin::Registry
        );
        s.instance.insert(
            "files.types".into(),
            Row::of(json!(["png", "svg", "pdf"]), 7),
        );
        let r = s.resolve("files.types").unwrap();
        assert_eq!(
            (r.value, r.origin),
            (Some(json!(["png", "svg", "pdf"])), Origin::Instance)
        );
        s.tenant
            .as_mut()
            .unwrap()
            .site
            .insert("files.types".into(), Row::of(json!(["png", "gif"]), 8));
        let r = s.resolve("files.types").unwrap();
        assert_eq!((r.value, r.origin), (Some(json!(["png"])), Origin::Tenant));
        let def = s.registry().get("files.types").unwrap();
        assert!(is_narrowing(
            def,
            Some(&json!(["png", "svg"])),
            &json!(["png"])
        ));
        assert!(!is_narrowing(
            def,
            Some(&json!(["png"])),
            &json!(["png", "gif"])
        ));
    }

    #[test]
    fn a_group_default_the_tenant_lacks_is_unresolved() {
        let mut s = snap();
        tenancy(&mut s, CONFIG_TEMPLATE, json!("defaults"));
        let key = s
            .registry()
            .of_scope(Scope::Tenant)
            .find(|d| d.ty == SettingType::Reference(super::super::registry::RefKind::Group))
            .map(|d| d.key.clone())
            .unwrap();
        s.defaults
            .insert(key.clone(), Row::of(json!("reviewers"), 5));
        let r = s.resolve(&key).unwrap();
        assert!(r.unresolved_default);
        assert_eq!(r.origin, Origin::Registry);
        s.tenant.as_mut().unwrap().groups.insert("reviewers".into());
        let r = s.resolve(&key).unwrap();
        assert_eq!(
            (r.value, r.origin),
            (Some(json!("reviewers")), Origin::Instance)
        );
    }
}
