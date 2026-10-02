//! The tenancy policy (0028 §1, §8; 0056 §11): `docs/registry/tenancy.toml`.
//!
//! The policy is a set of switches, each a `config` record of kind `tenancy` in the
//! instance `config`. Three presets ship in the registry; an instance starts from one and
//! may change each switch, under the registry's constraints: anything that names a farm
//! actor or a global group needs farm identity. Templates and locks (0028 §8) are the
//! `config.template` switch and the `tenancy:locked` list.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::acl::Target;

/// Why `tenancy.toml` or a policy is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TenancyError {
    /// The TOML did not parse or did not have the expected shape.
    #[error("tenancy.toml: {0}")]
    Toml(String),
    /// A registry version this crate does not know.
    #[error("tenancy.toml: version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// A switch the registry does not list.
    #[error("unknown tenancy switch `{0}`")]
    UnknownSwitch(String),
    /// A value the switch does not allow.
    #[error("tenancy switch `{switch}` cannot be `{value}`")]
    BadValue {
        /// The switch.
        switch: String,
        /// The value.
        value: String,
    },
    /// A switch needs farm identity and the policy has none.
    #[error("tenancy switch `{0}` needs `identity.farm_issuer` to be `optional` or `required`")]
    NeedsFarm(String),
    /// A preset the registry does not ship.
    #[error("unknown tenancy preset `{0}`")]
    UnknownPreset(String),
    /// A preset leaves a switch unset.
    #[error("preset `{preset}` does not set `{switch}`")]
    Incomplete {
        /// The preset.
        preset: String,
        /// The switch.
        switch: String,
    },
    /// A tenant `config` record for a locked key (`ts-locked`).
    #[error("ts-locked: `{0}` is locked by the instance")]
    Locked(String),
    /// A confidential restriction the policy does not allow (`ts-policy`).
    #[error(
        "ts-policy: `security.restrictions` is `{policy}`, which does not allow a restriction on a {target} target"
    )]
    Policy {
        /// The policy's value.
        policy: String,
        /// The target kind.
        target: &'static str,
    },
}

/// What values a switch allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Allowed {
    /// One of these.
    OneOf(Vec<String>),
    /// Free text with a description, such as "issuer code or none".
    Free(String),
}

/// A global group a preset ships (0028 §3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetGlobalGroup {
    /// The name.
    pub name: String,
    /// Its permissions.
    pub permissions: Vec<String>,
}

/// A preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    /// The name: `isolated`, `community`, `enterprise`.
    pub name: String,
    /// The switch values.
    pub switches: BTreeMap<String, String>,
    /// The global groups it ships.
    pub global_groups: Vec<PresetGlobalGroup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    switches: BTreeMap<String, toml::Value>,
    #[serde(default)]
    preset: BTreeMap<String, toml::Table>,
}

/// The switch that gives the farm identity.
pub const FARM_ISSUER: &str = "identity.farm_issuer";
/// The switch of 0056 §11.
pub const SECURITY_RESTRICTIONS: &str = "security.restrictions";
/// The switch of 0028 §8.
pub const CONFIG_TEMPLATE: &str = "config.template";

/// Switches that need farm identity when set to anything but their first value.
const NEED_FARM: [&str; 5] = [
    "identity.shared_names",
    "identity.auto_link",
    "groups.global",
    "blocks.global",
    "identity.required_issuer",
];

/// The registry: the switches and the presets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenancyRegistry {
    version: u32,
    switches: BTreeMap<String, Allowed>,
    presets: Vec<Preset>,
}

impl TenancyRegistry {
    /// Parses a `tenancy.toml`.
    pub fn parse(text: &str) -> Result<Self, TenancyError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| TenancyError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(TenancyError::Version(raw.version));
        }
        let mut switches = BTreeMap::new();
        for (name, v) in raw.switches {
            let allowed = match v {
                toml::Value::Array(items) => Allowed::OneOf(
                    items
                        .into_iter()
                        .map(|i| {
                            i.as_str().map(str::to_string).ok_or_else(|| {
                                TenancyError::Toml(format!("switch `{name}`: values are strings"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                ),
                toml::Value::String(s) => Allowed::Free(s),
                _ => {
                    return Err(TenancyError::Toml(format!(
                        "switch `{name}`: a list of values or a description"
                    )));
                }
            };
            switches.insert(name, allowed);
        }
        if !switches.contains_key(FARM_ISSUER) {
            return Err(TenancyError::UnknownSwitch(FARM_ISSUER.to_string()));
        }
        let mut presets = Vec::new();
        for (name, table) in raw.preset {
            let mut values = BTreeMap::new();
            let mut global_groups = Vec::new();
            for (k, v) in table {
                if k == "global_group" {
                    global_groups = v
                        .try_into::<Vec<PresetGlobalGroup>>()
                        .map_err(|e| TenancyError::Toml(format!("preset `{name}`: {e}")))?;
                } else {
                    let v = v.as_str().ok_or_else(|| {
                        TenancyError::Toml(format!("preset `{name}`: `{k}` is a string"))
                    })?;
                    values.insert(k, v.to_string());
                }
            }
            let preset = Preset {
                name: name.clone(),
                switches: values,
                global_groups,
            };
            let reg = Self {
                version: raw.version,
                switches: switches.clone(),
                presets: Vec::new(),
            };
            for s in reg.switches.keys() {
                if !preset.switches.contains_key(s) {
                    return Err(TenancyError::Incomplete {
                        preset: name.clone(),
                        switch: s.clone(),
                    });
                }
            }
            // `set-at-creation` stands for a value the operator supplies.
            reg.policy_from_values(&preset.switches)?;
            presets.push(preset);
        }
        Ok(Self {
            version: raw.version,
            switches,
            presets,
        })
    }

    /// The embedded `docs/registry/tenancy.toml`.
    ///
    /// # Panics
    ///
    /// If the embedded registry is invalid, which the crate's tests rule out.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: std::sync::OnceLock<TenancyRegistry> = std::sync::OnceLock::new();
        DEFAULT.get_or_init(|| Self::parse(crate::TENANCY_TOML).expect("embedded tenancy.toml"))
    }

    /// The registry version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The switches and what each allows.
    #[must_use]
    pub fn switches(&self) -> &BTreeMap<String, Allowed> {
        &self.switches
    }

    /// The presets.
    #[must_use]
    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    /// A preset by name.
    #[must_use]
    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name == name)
    }

    /// The policy a preset writes; `set-at-creation` switches are left for
    /// [`Self::set`] (the enterprise preset's `identity.required_issuer`).
    pub fn policy_from_preset(&self, name: &str) -> Result<TenancyPolicy, TenancyError> {
        let preset = self
            .preset(name)
            .ok_or_else(|| TenancyError::UnknownPreset(name.to_string()))?;
        let mut policy = self.policy_from_values(&preset.switches)?;
        policy.preset = Some(name.to_string());
        Ok(policy)
    }

    /// A policy from a whole set of values at once, checked together, so that the order
    /// of the switches does not matter.
    pub fn policy_from_values(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<TenancyPolicy, TenancyError> {
        let mut policy = TenancyPolicy::default();
        for (k, v) in values {
            if v == "set-at-creation" {
                continue;
            }
            let allowed = self
                .switches
                .get(k)
                .ok_or_else(|| TenancyError::UnknownSwitch(k.clone()))?;
            if let Allowed::OneOf(choices) = allowed
                && !choices.iter().any(|c| c == v)
            {
                return Err(TenancyError::BadValue {
                    switch: k.clone(),
                    value: v.clone(),
                });
            }
            policy.switches.insert(k.clone(), v.clone());
        }
        self.check_constraints(&policy, "")?;
        Ok(policy)
    }

    /// Sets one switch, as a `tenancy:{switch}` record does, under the registry's
    /// constraints.
    pub fn set(
        &self,
        policy: &mut TenancyPolicy,
        switch: &str,
        value: &str,
    ) -> Result<(), TenancyError> {
        let allowed = self
            .switches
            .get(switch)
            .ok_or_else(|| TenancyError::UnknownSwitch(switch.to_string()))?;
        if let Allowed::OneOf(values) = allowed
            && !values.iter().any(|v| v == value)
        {
            return Err(TenancyError::BadValue {
                switch: switch.to_string(),
                value: value.to_string(),
            });
        }
        policy
            .switches
            .insert(switch.to_string(), value.to_string());
        self.check_constraints(policy, switch)
    }

    fn check_constraints(&self, policy: &TenancyPolicy, changed: &str) -> Result<(), TenancyError> {
        if policy.has_farm_identity() {
            return Ok(());
        }
        // A switch is "off" at its first listed value (`none` for free text).
        let is_on = |switch: &str| -> bool {
            let Some(value) = policy.switches.get(switch) else {
                return false;
            };
            match self.switches.get(switch) {
                Some(Allowed::OneOf(values)) => values.first().is_some_and(|first| first != value),
                Some(Allowed::Free(_)) => value != "none",
                None => false,
            }
        };
        let offenders: Vec<&str> = NEED_FARM.into_iter().filter(|s| is_on(s)).collect();
        match offenders.first() {
            None => Ok(()),
            Some(first) => {
                let name = if offenders.contains(&changed) {
                    changed
                } else {
                    first
                };
                Err(TenancyError::NeedsFarm(name.to_string()))
            }
        }
    }
}

/// An instance's tenancy policy: the current value of every switch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TenancyPolicy {
    /// The preset it started from, if any.
    pub preset: Option<String>,
    /// The switches.
    pub switches: BTreeMap<String, String>,
}

impl TenancyPolicy {
    /// A switch's value.
    #[must_use]
    pub fn get(&self, switch: &str) -> Option<&str> {
        self.switches.get(switch).map(String::as_str)
    }

    /// Whether the farm has identity: `identity.farm_issuer` is not `none`.
    #[must_use]
    pub fn has_farm_identity(&self) -> bool {
        self.get(FARM_ISSUER).is_some_and(|v| v != "none")
    }

    /// Whether global groups are inherited (0028 §3).
    #[must_use]
    pub fn global_groups(&self) -> bool {
        self.get("groups.global") == Some("inherited")
    }

    /// Whether farm-actor blocks apply (0028 §4).
    #[must_use]
    pub fn global_blocks(&self) -> bool {
        self.get("blocks.global") == Some("farm-actor")
    }

    /// The `config.template` mode (0028 §8).
    #[must_use]
    pub fn template_mode(&self) -> &str {
        self.get(CONFIG_TEMPLATE).unwrap_or("none")
    }

    /// Whether a tenant may write a `config` record under `key` given the instance's
    /// locked list (0028 §8): refused with `ts-locked` under `locks`.
    pub fn check_locked(&self, locked: &[String], key: &str) -> Result<(), TenancyError> {
        if self.template_mode() == "locks" && locked.iter().any(|k| k == key) {
            return Err(TenancyError::Locked(key.to_string()));
        }
        Ok(())
    }

    /// Whether a confidential `read` restriction on `target` is allowed by
    /// `security.restrictions` (0056 §11): `none` allows none, `tenant` only the tenant
    /// itself, `any` all.
    pub fn check_restriction(&self, target: &Target) -> Result<(), TenancyError> {
        let policy = self.get(SECURITY_RESTRICTIONS).unwrap_or("any");
        let allowed = match policy {
            "any" => true,
            "tenant" => matches!(target, Target::Tenant(_)),
            _ => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(TenancyError::Policy {
                policy: policy.to_string(),
                target: target.kind(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_parse_and_constrain() {
        let r = TenancyRegistry::default_registry();
        assert_eq!(r.presets().len(), 3);
        let isolated = r.policy_from_preset("isolated").unwrap();
        assert!(!isolated.has_farm_identity());
        assert!(!isolated.global_groups());
        assert_eq!(isolated.get(SECURITY_RESTRICTIONS), Some("any"));
        let community = r.policy_from_preset("community").unwrap();
        assert!(community.global_groups() && community.global_blocks());
        assert_eq!(community.get(SECURITY_RESTRICTIONS), Some("none"));
        assert_eq!(
            r.preset("community")
                .unwrap()
                .global_groups
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>(),
            ["steward", "global-sysop"]
        );
        let enterprise = r.policy_from_preset("enterprise").unwrap();
        assert_eq!(
            enterprise.get("identity.required_issuer"),
            None,
            "set at creation"
        );
        let mut e = enterprise;
        r.set(&mut e, "identity.required_issuer", "corp-sso")
            .unwrap();
        assert!(matches!(
            r.policy_from_preset("nope"),
            Err(TenancyError::UnknownPreset(_))
        ));
        // Constraints.
        let mut p = r.policy_from_preset("isolated").unwrap();
        assert!(matches!(
            r.set(&mut p, "groups.global", "inherited"),
            Err(TenancyError::NeedsFarm(_))
        ));
        assert!(matches!(
            r.set(&mut p, "groups.global", "sometimes"),
            Err(TenancyError::BadValue { .. })
        ));
        assert!(matches!(
            r.set(&mut p, "nothing.here", "x"),
            Err(TenancyError::UnknownSwitch(_))
        ));
        r.set(&mut p, FARM_ISSUER, "optional").unwrap();
        r.set(&mut p, "groups.global", "inherited").unwrap();
        assert!(
            matches!(
                r.set(&mut p, FARM_ISSUER, "none"),
                Err(TenancyError::NeedsFarm(_))
            ),
            "cannot drop farm identity while a switch needs it"
        );
    }

    #[test]
    fn locks_and_restriction_policy() {
        let r = TenancyRegistry::default_registry();
        let enterprise = r.policy_from_preset("enterprise").unwrap();
        let locked = vec!["site:sitelink-policy".to_string()];
        assert!(matches!(
            enterprise.check_locked(&locked, "site:sitelink-policy"),
            Err(TenancyError::Locked(_))
        ));
        assert!(enterprise.check_locked(&locked, "site:name").is_ok());
        let community = r.policy_from_preset("community").unwrap();
        assert!(
            community
                .check_locked(&locked, "site:sitelink-policy")
                .is_ok(),
            "defaults, not locks"
        );
        assert!(matches!(
            community.check_restriction(&Target::Page(1)),
            Err(TenancyError::Policy { .. })
        ));
        assert!(matches!(
            community.check_restriction(&Target::Tenant("t".into())),
            Err(TenancyError::Policy { .. })
        ));
        let mut tenant_only = community;
        r.set(&mut tenant_only, SECURITY_RESTRICTIONS, "tenant")
            .unwrap();
        assert!(
            tenant_only
                .check_restriction(&Target::Tenant("t".into()))
                .is_ok()
        );
        assert!(tenant_only.check_restriction(&Target::Set(1)).is_err());
        let isolated = r.policy_from_preset("isolated").unwrap();
        assert!(isolated.check_restriction(&Target::Set(1)).is_ok());
    }

    #[test]
    fn rejects_bad_registries() {
        assert!(matches!(
            TenancyRegistry::parse("version = 2\n[switches]\n"),
            Err(TenancyError::Version(2))
        ));
        assert!(matches!(
            TenancyRegistry::parse("version = 1\n[switches]\n\"a.b\" = [\"x\"]\n"),
            Err(TenancyError::UnknownSwitch(_))
        ));
        let incomplete = crate::TENANCY_TOML.replace(
            "[preset.isolated]\n\"security.restrictions\"       = \"any\"\n",
            "[preset.isolated]\n",
        );
        assert!(matches!(
            TenancyRegistry::parse(&incomplete),
            Err(TenancyError::Incomplete { .. })
        ));
    }
}
