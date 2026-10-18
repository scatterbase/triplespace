//! Preferences (0027 §1; 0085 §1, §2.2): key–value pairs an account owns, in
//! `private.preference`, the keys registered in `docs/registry/preferences.toml`.
//!
//! A preference resolves, first match wins:
//!
//! 1. for a key marked `negotiated`, `uselang`, which overrides everything for one
//!    request;
//! 2. the account's own value;
//! 3. for a key marked `negotiated`, what the request's `Accept-Language` asks for,
//!    matched against the languages the interface has messages for;
//! 4. the tenant's `preferences.defaults`, merged key by key over the instance's default
//!    for it; for `language`, `site.language` unless `preferences.defaults` names
//!    another;
//! 5. the registry default.
//!
//! A key marked `follows = farm` or `choice` belongs to the farm account under farm
//! identity (0028 §2). Farm accounts are not built yet, so every key is the tenant
//! account's for now, which is what 0085 §1 says applies without farm identity.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio_postgres::GenericClient;

use crate::AccountsError;
use crate::settings::Snapshot;
use crate::settings::value::{duration_seconds, is_language};

/// The embedded registry file.
pub const PREFERENCES_TOML: &str = include_str!("../../../docs/registry/preferences.toml");

/// The setting that holds a tenant's (and the instance's) defaults.
pub const DEFAULTS_SETTING: &str = "preferences.defaults";

/// A preference's value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrefType {
    /// `true` or `false`.
    Boolean,
    /// One line of text.
    String,
    /// An integer.
    Integer,
    /// An ISO 8601 duration.
    Duration,
    /// A language code.
    Language,
    /// A list of language codes.
    LanguageList,
    /// One of `choices`.
    Choice,
    /// Any JSON.
    Json,
}

/// Whose a preference's value is under farm identity (0085 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Follows {
    /// The tenant account's.
    #[default]
    Account,
    /// The farm account's.
    Farm,
    /// The tenant account's unless the reader chose "Use on every wiki".
    Choice,
}

/// Where a preference's value may come from in the request (0085 §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Negotiated {
    /// `uselang`, then `Accept-Language`.
    AcceptLanguage,
}

/// One registered preference key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefDef {
    /// The key; with `pattern`, `*` stands for one segment.
    pub key: String,
    /// The type.
    #[serde(rename = "type")]
    pub ty: PrefType,
    /// For `choice`: the permitted values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// The registry default; absent where the key's behaviour without one is described.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// The tab of `Special:Preferences`.
    pub group: String,
    /// Whose it is under farm identity.
    #[serde(default)]
    pub follows: Follows,
    /// Whether a reader who is not logged in may hold it in the browser.
    #[serde(default)]
    pub browser: bool,
    /// Whether the request supplies it where the reader has not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negotiated: Option<Negotiated>,
    /// Whether `*` in the key stands for a segment.
    #[serde(default)]
    pub pattern: bool,
    /// The ADR that defined it.
    pub from: String,
}

impl PrefDef {
    /// Whether the key matches a requested key: equal, or for a pattern, equal segment by
    /// segment with `*` matching any one segment.
    #[must_use]
    pub fn matches(&self, key: &str) -> bool {
        if !self.pattern {
            return self.key == key;
        }
        let want: Vec<&str> = self.key.split('.').collect();
        let have: Vec<&str> = key.split('.').collect();
        want.len() == have.len()
            && want
                .iter()
                .zip(&have)
                .all(|(w, h)| (*w == "*" && !h.is_empty()) || w == h)
    }

    /// The message key of the label.
    #[must_use]
    pub fn label_message(&self) -> String {
        format!("ts-pref-{}", self.key.replace('.', "-"))
    }

    /// Checks a value against the type.
    pub fn check(&self, v: &Value) -> Result<(), String> {
        match self.ty {
            PrefType::Boolean => v
                .as_bool()
                .map(drop)
                .ok_or_else(|| "not true or false".into()),
            PrefType::Integer => v.as_i64().map(drop).ok_or_else(|| "not an integer".into()),
            PrefType::String => match v.as_str() {
                Some(s) if !s.contains(['\n', '\r']) && s.len() <= 1_000 => Ok(()),
                _ => Err("not one line of text".into()),
            },
            PrefType::Duration => match v.as_str() {
                Some(s) if duration_seconds(s).is_some() => Ok(()),
                _ => Err("not an ISO 8601 duration".into()),
            },
            PrefType::Language => match v.as_str() {
                Some(s) if is_language(s) => Ok(()),
                _ => Err("not a language code".into()),
            },
            PrefType::LanguageList => match v.as_array() {
                Some(a) if a.iter().all(|x| x.as_str().is_some_and(is_language)) => Ok(()),
                _ => Err("not a list of language codes".into()),
            },
            PrefType::Choice => match v.as_str() {
                Some(s) if self.choices.iter().any(|c| c == s) => Ok(()),
                _ => Err(format!("not one of {}", self.choices.join(", "))),
            },
            PrefType::Json => Ok(()),
        }
    }
}

#[derive(Deserialize)]
struct RawRegistry {
    version: u32,
    #[serde(default)]
    group: Vec<RawGroup>,
    #[serde(default)]
    preference: Vec<PrefDef>,
}

#[derive(Deserialize)]
struct RawGroup {
    id: String,
}

/// The preference registry.
#[derive(Debug, Clone, PartialEq)]
pub struct PreferenceRegistry {
    groups: Vec<String>,
    prefs: Vec<PrefDef>,
}

impl PreferenceRegistry {
    /// Parses a `preferences.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| format!("preferences.toml: {e}"))?;
        if raw.version != 1 {
            return Err(format!(
                "preferences.toml: version {} is not supported",
                raw.version
            ));
        }
        let groups: Vec<String> = raw.group.into_iter().map(|g| g.id).collect();
        for p in &raw.preference {
            if !groups.contains(&p.group) {
                return Err(format!(
                    "preference `{}`: group `{}` is not declared",
                    p.key, p.group
                ));
            }
            if (p.ty == PrefType::Choice) == p.choices.is_empty() {
                return Err(format!(
                    "preference `{}`: `choices` goes with type `choice`",
                    p.key
                ));
            }
            if let Some(d) = &p.default {
                p.check(d)
                    .map_err(|e| format!("preference `{}`: the default is {e}", p.key))?;
            }
        }
        Ok(Self {
            groups,
            prefs: raw.preference,
        })
    }

    /// The registry this build ships.
    pub fn embedded() -> &'static Self {
        static EMBEDDED: std::sync::OnceLock<PreferenceRegistry> = std::sync::OnceLock::new();
        EMBEDDED.get_or_init(|| Self::parse(PREFERENCES_TOML).expect("embedded preferences.toml"))
    }

    /// The groups, in tab order.
    #[must_use]
    pub fn groups(&self) -> &[String] {
        &self.groups
    }

    /// Every entry, in file order.
    #[must_use]
    pub fn entries(&self) -> &[PrefDef] {
        &self.prefs
    }

    /// The entry a key falls under: an exact entry before a pattern.
    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<&PrefDef> {
        self.prefs
            .iter()
            .find(|p| !p.pattern && p.key == key)
            .or_else(|| self.prefs.iter().find(|p| p.pattern && p.matches(key)))
    }

    /// Checks a key and value as `action=options` and `PATCH /account/preferences`
    /// would store them.
    pub fn check(&self, key: &str, v: &Value) -> Result<(), PrefError> {
        let def = self
            .lookup(key)
            .ok_or_else(|| PrefError::Unknown(key.to_string()))?;
        def.check(v).map_err(|e| PrefError::Invalid {
            key: key.to_string(),
            problem: e,
        })
    }
}

/// Why a preference was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrefError {
    /// Not a registered key.
    #[error("`{0}` is not a registered preference")]
    Unknown(String),
    /// The value does not fit.
    #[error("`{key}`: {problem}")]
    Invalid {
        /// The key.
        key: String,
        /// Why.
        problem: String,
    },
}

// ---------------------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------------------

/// An account's own values.
pub async fn own<C: GenericClient>(
    client: &C,
    actor_key: &str,
) -> Result<BTreeMap<String, Value>, AccountsError> {
    let rows = client
        .query(
            "SELECT key, value FROM private.preference WHERE actor_key = $1",
            &[&actor_key],
        )
        .await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// Sets one value; the caller has checked it ([`PreferenceRegistry::check`]).
pub async fn set<C: GenericClient>(
    client: &C,
    actor_key: &str,
    key: &str,
    value: &Value,
) -> Result<(), AccountsError> {
    client
        .execute(
            "INSERT INTO private.preference (actor_key, key, value) VALUES ($1, $2, $3)
             ON CONFLICT (actor_key, key) DO UPDATE SET value = EXCLUDED.value, updated = now()",
            &[&actor_key, &key, value],
        )
        .await?;
    Ok(())
}

/// Resets one key to its default; `false` when the account had no value of its own.
pub async fn reset<C: GenericClient>(
    client: &C,
    actor_key: &str,
    key: &str,
) -> Result<bool, AccountsError> {
    Ok(client
        .execute(
            "DELETE FROM private.preference WHERE actor_key = $1 AND key = $2",
            &[&actor_key, &key],
        )
        .await?
        > 0)
}

/// Resets every key, as `action=options&reset=1` does.
pub async fn reset_all<C: GenericClient>(
    client: &C,
    actor_key: &str,
) -> Result<u64, AccountsError> {
    Ok(client
        .execute(
            "DELETE FROM private.preference WHERE actor_key = $1",
            &[&actor_key],
        )
        .await?)
}

// ---------------------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------------------

/// Where a resolved preference came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrefOrigin {
    /// The account's own value.
    Account,
    /// The request: `uselang` or `Accept-Language`.
    Request,
    /// The tenant's `preferences.defaults` (or, for `language`, `site.language`).
    Tenant,
    /// The instance's default for `preferences.defaults`.
    Instance,
    /// The registry default.
    Registry,
}

/// The defaults a tenant gives preferences: its `preferences.defaults` merged key by key
/// over the instance's, and its `site.language`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Defaults {
    /// The instance's map.
    pub instance: Map<String, Value>,
    /// The tenant's map.
    pub tenant: Map<String, Value>,
    /// The tenant's `site.language`.
    pub site_language: Option<String>,
}

impl Defaults {
    /// Reads them from a settings snapshot of the tenant (0085 §2.2): the tenant's own
    /// record over the instance's default for `preferences.defaults`; a lock makes the
    /// instance's the only map.
    #[must_use]
    pub fn of(snap: &Snapshot) -> Self {
        let as_map = |v: Option<&Value>| v.and_then(Value::as_object).cloned().unwrap_or_default();
        let instance = as_map(snap.default_row(DEFAULTS_SETTING).map(|r| &r.value));
        let tenant = if snap.is_locked(DEFAULTS_SETTING) {
            Map::new()
        } else {
            as_map(snap.tenant_row(DEFAULTS_SETTING).map(|r| &r.value))
        };
        let site_language = snap
            .value("site.language")
            .and_then(|v| v.as_str().map(str::to_owned));
        Self {
            instance,
            tenant,
            site_language,
        }
    }

    fn get(&self, key: &str) -> Option<(Value, PrefOrigin)> {
        self.tenant
            .get(key)
            .map(|v| (v.clone(), PrefOrigin::Tenant))
            .or_else(|| {
                self.instance
                    .get(key)
                    .map(|v| (v.clone(), PrefOrigin::Instance))
            })
    }
}

/// What the request says about language (0085 §2.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request<'a> {
    /// `uselang`.
    pub uselang: Option<&'a str>,
    /// The `Accept-Language` header.
    pub accept_language: Option<&'a str>,
    /// The languages the interface has messages for; empty means any.
    pub interface_languages: &'a [&'a str],
}

/// `Accept-Language`'s languages in order of preference: valid codes only, lower-cased,
/// `q=0` and `*` dropped, ties kept in header order.
#[must_use]
pub fn accept_languages(header: &str) -> Vec<String> {
    let mut items: Vec<(f32, usize, String)> = header
        .split(',')
        .enumerate()
        .filter_map(|(i, part)| {
            let mut bits = part.split(';');
            let tag = bits.next()?.trim().to_ascii_lowercase();
            let q = bits
                .find_map(|b| {
                    b.trim()
                        .strip_prefix("q=")
                        .and_then(|q| q.trim().parse::<f32>().ok())
                })
                .unwrap_or(1.0);
            (q > 0.0 && is_language(&tag)).then_some((q, i, tag))
        })
        .collect();
    items.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<String> = Vec::new();
    for (_, _, tag) in items {
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

impl Request<'_> {
    fn available(&self, code: &str) -> Option<String> {
        if self.interface_languages.is_empty() {
            return Some(code.to_string());
        }
        if self.interface_languages.contains(&code) {
            return Some(code.to_string());
        }
        // `de-at` falls back to `de` when only that has messages.
        let primary = code.split('-').next()?;
        self.interface_languages
            .contains(&primary)
            .then(|| primary.to_string())
    }

    fn uselang(&self) -> Option<String> {
        self.uselang
            .map(str::to_ascii_lowercase)
            .filter(|s| is_language(s))
    }

    /// The value the request gives a negotiated key, from `Accept-Language`.
    fn negotiate(&self, key: &str) -> Option<Value> {
        let langs = accept_languages(self.accept_language?);
        match key {
            "language" => langs
                .iter()
                .find_map(|l| self.available(l))
                .map(Value::String),
            "label-languages" if !langs.is_empty() => {
                Some(Value::Array(langs.into_iter().map(Value::String).collect()))
            }
            _ => None,
        }
    }
}

/// One resolved preference.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedPref {
    /// The value; `None` where the key has no default and nothing gave one.
    pub value: Option<Value>,
    /// Where it came from.
    pub origin: PrefOrigin,
}

/// Resolves one preference (0085 §2.2).
#[must_use]
pub fn resolve(
    def: &PrefDef,
    key: &str,
    own: Option<&Value>,
    request: &Request<'_>,
    defaults: &Defaults,
) -> ResolvedPref {
    let negotiated = def.negotiated.is_some();
    if negotiated && let Some(lang) = request.uselang() {
        let value = match key {
            "label-languages" => Value::Array(vec![Value::String(lang)]),
            _ => Value::String(lang),
        };
        return ResolvedPref {
            value: Some(value),
            origin: PrefOrigin::Request,
        };
    }
    if let Some(v) = own.filter(|v| def.check(v).is_ok()) {
        return ResolvedPref {
            value: Some(v.clone()),
            origin: PrefOrigin::Account,
        };
    }
    if negotiated && let Some(v) = request.negotiate(key) {
        return ResolvedPref {
            value: Some(v),
            origin: PrefOrigin::Request,
        };
    }
    if let Some((v, origin)) = defaults.get(key).filter(|(v, _)| def.check(v).is_ok()) {
        return ResolvedPref {
            value: Some(v),
            origin,
        };
    }
    if key == "language"
        && let Some(lang) = &defaults.site_language
    {
        return ResolvedPref {
            value: Some(Value::String(lang.clone())),
            origin: PrefOrigin::Tenant,
        };
    }
    ResolvedPref {
        value: def.default.clone(),
        origin: PrefOrigin::Registry,
    }
}

/// Every registered (non-pattern) key resolved, with the account's own values for
/// pattern keys beside them: what `meta=userinfo&uiprop=options` and
/// `GET /account/preferences` report.
#[must_use]
pub fn resolve_all(
    registry: &PreferenceRegistry,
    own: &BTreeMap<String, Value>,
    request: &Request<'_>,
    defaults: &Defaults,
) -> BTreeMap<String, ResolvedPref> {
    let mut out = BTreeMap::new();
    for def in registry.entries().iter().filter(|d| !d.pattern) {
        out.insert(
            def.key.clone(),
            resolve(def, &def.key, own.get(&def.key), request, defaults),
        );
    }
    for (key, v) in own {
        if out.contains_key(key) {
            continue;
        }
        if let Some(def) = registry.lookup(key) {
            out.insert(key.clone(), resolve(def, key, Some(v), request, defaults));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reg() -> &'static PreferenceRegistry {
        PreferenceRegistry::embedded()
    }

    #[test]
    fn the_registry_parses_and_patterns_match() {
        let r = reg();
        assert!(r.lookup("language").is_some());
        assert_eq!(
            r.lookup("notifications.talk.email").unwrap().key,
            "notifications.talk.email"
        );
        assert_eq!(
            r.lookup("notifications.mention.email").unwrap().key,
            "notifications.*.email"
        );
        assert!(r.lookup("notifications.email").is_none());
        assert!(r.lookup("nonsense").is_none());
        assert!(r.check("ui.mode", &json!("dark")).is_ok());
        assert_eq!(
            r.check("ui.mode", &json!("purple")),
            Err(PrefError::Invalid {
                key: "ui.mode".into(),
                problem: "not one of auto, light, dark".into()
            })
        );
        assert!(matches!(
            r.check("bogus", &json!(1)),
            Err(PrefError::Unknown(_))
        ));
    }

    #[test]
    fn accept_language_order() {
        assert_eq!(
            accept_languages("de-AT, de;q=0.9, en;q=0.8, *;q=0.1"),
            vec!["de-at", "de", "en"]
        );
        assert_eq!(accept_languages("en;q=0.5, fr"), vec!["fr", "en"]);
        assert_eq!(accept_languages("fr;q=0, it"), vec!["it"]);
        assert!(accept_languages("").is_empty());
    }

    #[test]
    fn language_resolution_order() {
        let def = reg().lookup("language").unwrap();
        let langs = ["en", "de", "fr"];
        let defaults = Defaults {
            site_language: Some("fr".into()),
            ..Defaults::default()
        };
        let none = Request {
            interface_languages: &langs,
            ..Request::default()
        };
        // Nothing asked: the site's language.
        let r = resolve(def, "language", None, &none, &defaults);
        assert_eq!((r.value, r.origin), (Some(json!("fr")), PrefOrigin::Tenant));
        // Accept-Language, falling back from a regional code to its primary.
        let header = Request {
            accept_language: Some("de-CH, en;q=0.5"),
            interface_languages: &langs,
            ..Request::default()
        };
        let r = resolve(def, "language", None, &header, &defaults);
        assert_eq!(
            (r.value, r.origin),
            (Some(json!("de")), PrefOrigin::Request)
        );
        // The account's own choice beats the header.
        let r = resolve(def, "language", Some(&json!("en")), &header, &defaults);
        assert_eq!(
            (r.value, r.origin),
            (Some(json!("en")), PrefOrigin::Account)
        );
        // uselang beats everything.
        let uselang = Request {
            uselang: Some("fr"),
            ..header.clone()
        };
        let r = resolve(def, "language", Some(&json!("en")), &uselang, &defaults);
        assert_eq!(
            (r.value, r.origin),
            (Some(json!("fr")), PrefOrigin::Request)
        );
        // preferences.defaults names another language than the site's.
        let named = Defaults {
            tenant: Map::from_iter([("language".to_string(), json!("en"))]),
            ..defaults.clone()
        };
        let r = resolve(def, "language", None, &none, &named);
        assert_eq!((r.value, r.origin), (Some(json!("en")), PrefOrigin::Tenant));
    }

    #[test]
    fn defaults_merge_tenant_over_instance() {
        let def = reg().lookup("rc.limit").unwrap();
        let d = Defaults {
            instance: Map::from_iter([
                ("rc.limit".to_string(), json!(100)),
                ("ui.mode".to_string(), json!("dark")),
            ]),
            tenant: Map::from_iter([("rc.limit".to_string(), json!(250))]),
            site_language: None,
        };
        let r = resolve(def, "rc.limit", None, &Request::default(), &d);
        assert_eq!((r.value, r.origin), (Some(json!(250)), PrefOrigin::Tenant));
        let mode = reg().lookup("ui.mode").unwrap();
        let r = resolve(mode, "ui.mode", None, &Request::default(), &d);
        assert_eq!(
            (r.value, r.origin),
            (Some(json!("dark")), PrefOrigin::Instance)
        );
        let r = resolve(
            mode,
            "ui.mode",
            None,
            &Request::default(),
            &Defaults::default(),
        );
        assert_eq!(
            (r.value, r.origin),
            (Some(json!("auto")), PrefOrigin::Registry)
        );
    }

    #[test]
    fn resolve_all_reports_pattern_values_the_account_set() {
        let own = BTreeMap::from([("notifications.mention.email".to_string(), json!(true))]);
        let all = resolve_all(reg(), &own, &Request::default(), &Defaults::default());
        assert_eq!(all["notifications.mention.email"].value, Some(json!(true)));
        assert_eq!(all["autowatch.create"].value, Some(json!(true)));
        assert!(!all.contains_key("notifications.*.email"));
    }
}
