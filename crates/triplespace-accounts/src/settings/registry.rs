//! The settings registry (0085 §1): `docs/registry/settings.toml`, the catalogue of
//! record for every `site` setting, a tenant's and the instance's.
//!
//! `instance create` copies every entry into the instance `config` as a record of kind
//! `setting`, keyed `setting:{key}`, and an instance may register settings of its own
//! the same way. [`SettingsRegistry::overlay`] lays those records over the embedded file,
//! so a setting registered at runtime is validated, resolved and shown like a shipped one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The embedded registry file.
pub const SETTINGS_TOML: &str = include_str!("../../../../docs/registry/settings.toml");

/// The config kind of a registry entry copied into the instance `config` (0085 §1).
pub const SETTING_KIND: &str = "setting";

/// Why `settings.toml` or a `setting:` record is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The TOML did not parse or did not have the expected shape.
    #[error("settings.toml: {0}")]
    Toml(String),
    /// A registry version this crate does not know.
    #[error("settings.toml: version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// An entry breaks a rule of the file's header.
    #[error("setting `{key}`: {problem}")]
    Entry {
        /// The key.
        key: String,
        /// What is wrong.
        problem: String,
    },
}

/// Where a setting's record lives (0085 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// A record in a tenant's `config`.
    Tenant,
    /// A record in the instance `config`, written at the farm base.
    Instance,
}

impl Scope {
    /// The registry word.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tenant => "tenant",
            Self::Instance => "instance",
        }
    }
}

/// Who has the last word (23 §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bound {
    /// The tenant's record is the value.
    Tenant,
    /// The tenant's record, up to the instance's ceiling.
    Ceiling,
    /// The instance's record is the value.
    Instance,
}

impl Bound {
    /// The registry word.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tenant => "tenant",
            Self::Ceiling => "ceiling",
            Self::Instance => "instance",
        }
    }
}

/// A setting's value type (0085 §1): a closed vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingType {
    /// `true` or `false`.
    Boolean,
    /// A JSON integer.
    Integer,
    /// An ISO 8601 duration, `P30D`, `PT0.25S`.
    Duration,
    /// A size in bytes, a non-negative integer.
    Size,
    /// Text, which may span lines.
    Text,
    /// One line of text.
    String,
    /// An absolute `http` or `https` URL.
    Url,
    /// A language code.
    Language,
    /// A list of language codes, no repeats.
    LanguageList,
    /// One of the entry's `choices`.
    Choice,
    /// A list of strings.
    StringList,
    /// A list of integers.
    IntegerList,
    /// A JSON object.
    Map,
    /// Any JSON value.
    Json,
    /// A pinned version of a local file the setting uses for a site function (0085 §8).
    SiteAsset,
    /// A name of something of the tenant's.
    Reference(RefKind),
}

/// The reference kinds (0085 §1): a value that names something of the tenant's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    /// A page title.
    Page,
    /// A list of page titles.
    PageList,
    /// A file title.
    File,
    /// A group name.
    Group,
    /// An actor key.
    Actor,
    /// A page repository's name.
    Repository,
    /// A list of page repository names.
    RepositoryList,
    /// A list of property IDs.
    PropertyList,
    /// A list of namespaces, by number or name.
    NamespaceList,
    /// A site's global ID.
    Site,
    /// A licence ID.
    Licence,
    /// A collation name.
    Collation,
    /// A name pattern.
    Pattern,
    /// A schedule (`daily`, `weekly`, a cron line).
    Schedule,
    /// A palette name (0086).
    Palette,
    /// A type set name (0086).
    Typeset,
}

impl RefKind {
    /// Whether the value is a list.
    #[must_use]
    pub fn is_list(self) -> bool {
        matches!(
            self,
            Self::PageList | Self::RepositoryList | Self::PropertyList | Self::NamespaceList
        )
    }
}

impl SettingType {
    /// From the registry word.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "boolean" => Self::Boolean,
            "integer" => Self::Integer,
            "duration" => Self::Duration,
            "size" => Self::Size,
            "text" => Self::Text,
            "string" => Self::String,
            "url" => Self::Url,
            "language" => Self::Language,
            "language-list" => Self::LanguageList,
            "choice" => Self::Choice,
            "string-list" => Self::StringList,
            "integer-list" => Self::IntegerList,
            "map" => Self::Map,
            "json" => Self::Json,
            "site-asset" => Self::SiteAsset,
            "page" => Self::Reference(RefKind::Page),
            "page-list" => Self::Reference(RefKind::PageList),
            "file" => Self::Reference(RefKind::File),
            "group" => Self::Reference(RefKind::Group),
            "actor" => Self::Reference(RefKind::Actor),
            "repository" => Self::Reference(RefKind::Repository),
            "repository-list" => Self::Reference(RefKind::RepositoryList),
            "property-list" => Self::Reference(RefKind::PropertyList),
            "namespace-list" => Self::Reference(RefKind::NamespaceList),
            "site" => Self::Reference(RefKind::Site),
            "licence" => Self::Reference(RefKind::Licence),
            "collation" => Self::Reference(RefKind::Collation),
            "pattern" => Self::Reference(RefKind::Pattern),
            "schedule" => Self::Reference(RefKind::Schedule),
            "palette" => Self::Reference(RefKind::Palette),
            "typeset" => Self::Reference(RefKind::Typeset),
            _ => return None,
        })
    }

    /// The registry word.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Duration => "duration",
            Self::Size => "size",
            Self::Text => "text",
            Self::String => "string",
            Self::Url => "url",
            Self::Language => "language",
            Self::LanguageList => "language-list",
            Self::Choice => "choice",
            Self::StringList => "string-list",
            Self::IntegerList => "integer-list",
            Self::Map => "map",
            Self::Json => "json",
            Self::SiteAsset => "site-asset",
            Self::Reference(r) => match r {
                RefKind::Page => "page",
                RefKind::PageList => "page-list",
                RefKind::File => "file",
                RefKind::Group => "group",
                RefKind::Actor => "actor",
                RefKind::Repository => "repository",
                RefKind::RepositoryList => "repository-list",
                RefKind::PropertyList => "property-list",
                RefKind::NamespaceList => "namespace-list",
                RefKind::Site => "site",
                RefKind::Licence => "licence",
                RefKind::Collation => "collation",
                RefKind::Pattern => "pattern",
                RefKind::Schedule => "schedule",
                RefKind::Palette => "palette",
                RefKind::Typeset => "typeset",
            },
        }
    }
}

/// One registered setting.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingDef {
    /// The setting name.
    pub key: String,
    /// Where its record lives.
    pub scope: Scope,
    /// Who has the last word.
    pub bound: Bound,
    /// The capping switch, where it is not of the setting's own name.
    pub ceiling: Option<String>,
    /// An instance setting a tenant record may narrow but not widen.
    pub tenant_may_narrow: bool,
    /// The tab it is shown on.
    pub group: String,
    /// The value type.
    pub ty: SettingType,
    /// For `choice`: the permitted values.
    pub choices: Vec<String>,
    /// The registry default; `None` for an entry marked `no_default`.
    pub default: Option<Value>,
    /// For `site-asset`: the media types it accepts.
    pub media: Vec<String>,
    /// The permission that writes it where stronger than `ts-config`, or `owner`.
    pub right: Option<String>,
    /// Whether `meta=siteinfo` reports it.
    pub reported: bool,
    /// A retired setting: kept so its name is never reused, never written.
    pub retired: bool,
    /// The catalogue's text, kept for `GET /settings/schema`.
    pub text: BTreeMap<String, String>,
}

impl SettingDef {
    /// The message key of the label, `ts-setting-{key}` with dots turned into hyphens.
    #[must_use]
    pub fn label_message(&self) -> String {
        format!("ts-setting-{}", self.key.replace('.', "-"))
    }

    /// The message key of the help text.
    #[must_use]
    pub fn help_message(&self) -> String {
        format!("{}-help", self.label_message())
    }

    /// The tenancy switch that caps it, for `bound = ceiling`.
    #[must_use]
    pub fn ceiling_switch(&self) -> Option<&str> {
        (self.bound == Bound::Ceiling).then(|| self.ceiling.as_deref().unwrap_or(&self.key))
    }

    /// The right that writes it.
    #[must_use]
    pub fn write_right(&self) -> &str {
        self.right.as_deref().unwrap_or("ts-config")
    }

    /// The entry as JSON, the shape of a `setting:` record's content and of an entry of
    /// `GET /settings/schema`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("kind".into(), Value::String(SETTING_KIND.into()));
        m.insert("key".into(), Value::String(self.key.clone()));
        m.insert("scope".into(), Value::String(self.scope.as_str().into()));
        m.insert("bound".into(), Value::String(self.bound.as_str().into()));
        if let Some(c) = &self.ceiling {
            m.insert("ceiling".into(), Value::String(c.clone()));
        }
        if self.tenant_may_narrow {
            m.insert("tenant_may_narrow".into(), Value::Bool(true));
        }
        m.insert("group".into(), Value::String(self.group.clone()));
        m.insert("type".into(), Value::String(self.ty.as_str().into()));
        if !self.choices.is_empty() {
            m.insert("choices".into(), serde_json::json!(self.choices));
        }
        match &self.default {
            Some(d) => {
                m.insert("default".into(), d.clone());
            }
            None => {
                m.insert("no_default".into(), Value::Bool(true));
            }
        }
        if !self.media.is_empty() {
            m.insert("media".into(), serde_json::json!(self.media));
        }
        if let Some(r) = &self.right {
            m.insert("right".into(), Value::String(r.clone()));
        }
        if self.reported {
            m.insert("reported".into(), Value::Bool(true));
        }
        if self.retired {
            m.insert("retired".into(), Value::Bool(true));
        }
        for (k, v) in &self.text {
            if k == "by_rule" {
                m.insert(k.clone(), Value::Bool(true));
            } else {
                m.insert(k.clone(), Value::String(v.clone()));
            }
        }
        Value::Object(m)
    }

    /// Reads an entry from its JSON form: a `setting:` record's content, or a TOML table
    /// turned into JSON.
    #[allow(clippy::too_many_lines)]
    pub fn from_json(v: &Value) -> Result<Self, RegistryError> {
        let key = v
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| RegistryError::Toml("a setting without `key`".into()))?
            .to_string();
        let bad = |problem: String| RegistryError::Entry {
            key: key.clone(),
            problem,
        };
        let word = |field: &str| -> Result<String, RegistryError> {
            v.get(field)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| bad(format!("`{field}` is missing or not a string")))
        };
        let flag = |field: &str| v.get(field).and_then(Value::as_bool).unwrap_or(false);
        let strings = |field: &str| -> Vec<String> {
            v.get(field)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let scope = match word("scope")?.as_str() {
            "tenant" => Scope::Tenant,
            "instance" => Scope::Instance,
            other => return Err(bad(format!("scope `{other}`"))),
        };
        let bound = match word("bound")?.as_str() {
            "tenant" => Bound::Tenant,
            "ceiling" => Bound::Ceiling,
            "instance" => Bound::Instance,
            other => return Err(bad(format!("bound `{other}`"))),
        };
        let ty_word = word("type")?;
        let ty = SettingType::parse(&ty_word).ok_or_else(|| bad(format!("type `{ty_word}`")))?;
        let choices = strings("choices");
        if (ty == SettingType::Choice) == choices.is_empty() {
            return Err(bad(
                "`choices` goes with type `choice`, and only with it".into()
            ));
        }
        let default = v.get("default").cloned();
        let no_default = flag("no_default");
        if default.is_some() == no_default {
            return Err(bad(
                "an entry has `default` or `no_default`, not both".into()
            ));
        }
        let media = strings("media");
        if (ty == SettingType::SiteAsset) == media.is_empty() {
            return Err(bad(
                "`media` goes with type `site-asset`, and only with it".into()
            ));
        }
        let mut text = BTreeMap::new();
        for field in ["values", "default_text", "meaning", "source", "scope_note"] {
            if let Some(s) = v.get(field).and_then(Value::as_str) {
                text.insert(field.to_string(), s.to_string());
            }
        }
        if flag("by_rule") {
            text.insert("by_rule".into(), "true".into());
        }
        let def = Self {
            ceiling: v.get("ceiling").and_then(Value::as_str).map(str::to_owned),
            tenant_may_narrow: flag("tenant_may_narrow"),
            group: word("group")?,
            right: v.get("right").and_then(Value::as_str).map(str::to_owned),
            reported: flag("reported"),
            retired: flag("retired"),
            key: key.clone(),
            scope,
            bound,
            ty,
            choices,
            default,
            media,
            text,
        };
        if def.ceiling.is_some() && def.bound != Bound::Ceiling {
            return Err(bad("`ceiling` goes with bound `ceiling`".into()));
        }
        if def.tenant_may_narrow && def.scope != Scope::Instance {
            return Err(bad("`tenant_may_narrow` is for instance settings".into()));
        }
        if (def.scope == Scope::Instance) != (def.bound == Bound::Instance) {
            return Err(bad(
                "an instance setting has bound `instance`, and only it".into()
            ));
        }
        if let Some(d) = &def.default
            && let Err(e) = super::value::check(&def, d)
        {
            return Err(bad(format!("the default does not fit: {e}")));
        }
        Ok(def)
    }
}

/// The registry: the groups and the settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsRegistry {
    groups: Vec<String>,
    settings: BTreeMap<String, SettingDef>,
}

#[derive(Deserialize)]
struct RawRegistry {
    version: u32,
    #[serde(default)]
    group: Vec<RawGroup>,
    #[serde(default)]
    setting: Vec<toml::Table>,
}

#[derive(Deserialize)]
struct RawGroup {
    id: String,
}

impl SettingsRegistry {
    /// Parses a `settings.toml`.
    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| RegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(RegistryError::Version(raw.version));
        }
        let groups: Vec<String> = raw.group.into_iter().map(|g| g.id).collect();
        let mut settings = BTreeMap::new();
        for table in raw.setting {
            let json =
                serde_json::to_value(&table).map_err(|e| RegistryError::Toml(e.to_string()))?;
            let def = SettingDef::from_json(&json)?;
            if !groups.contains(&def.group) {
                return Err(RegistryError::Entry {
                    key: def.key,
                    problem: format!("group `{}` is not declared", def.group),
                });
            }
            if settings.contains_key(&def.key) {
                return Err(RegistryError::Entry {
                    key: def.key,
                    problem: "registered twice".into(),
                });
            }
            settings.insert(def.key.clone(), def);
        }
        Ok(Self { groups, settings })
    }

    /// The registry this build ships.
    pub fn embedded() -> &'static Self {
        static EMBEDDED: std::sync::OnceLock<SettingsRegistry> = std::sync::OnceLock::new();
        EMBEDDED.get_or_init(|| Self::parse(SETTINGS_TOML).expect("embedded settings.toml"))
    }

    /// Lays the instance's `setting:` records over this registry: a record replaces the
    /// shipped entry of its key, or registers a setting the instance added. A record that
    /// does not parse is skipped, so one bad entry cannot take the settings pages down;
    /// the writer of `setting:` records validates them.
    #[must_use]
    pub fn overlay<'a>(&self, records: impl IntoIterator<Item = &'a Value>) -> Self {
        let mut out = self.clone();
        for v in records {
            if let Ok(def) = SettingDef::from_json(v) {
                if !out.groups.contains(&def.group) {
                    out.groups.push(def.group.clone());
                }
                out.settings.insert(def.key.clone(), def);
            }
        }
        out
    }

    /// The groups, in the order of the tabs.
    #[must_use]
    pub fn groups(&self) -> &[String] {
        &self.groups
    }

    /// One setting.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&SettingDef> {
        self.settings.get(key)
    }

    /// Every setting, by key.
    pub fn iter(&self) -> impl Iterator<Item = &SettingDef> {
        self.settings.values()
    }

    /// Every setting of a scope.
    pub fn of_scope(&self, scope: Scope) -> impl Iterator<Item = &SettingDef> {
        self.settings.values().filter(move |d| d.scope == scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_registry_parses() {
        let r = SettingsRegistry::embedded();
        assert!(r.iter().count() > 100);
        let name = r.get("site.name").unwrap();
        assert_eq!(name.scope, Scope::Tenant);
        assert_eq!(name.ty, SettingType::String);
        assert_eq!(name.default, None);
        assert_eq!(name.label_message(), "ts-setting-site-name");
        let lua = r.get("wikitext.lua").unwrap();
        assert_eq!(lua.ceiling_switch(), Some("wikitext.ceiling"));
        let budget = r.get("projections.sync_budget").unwrap();
        assert_eq!(budget.ceiling_switch(), Some("projections.sync_budget"));
        assert_eq!(r.get("files.separation").unwrap().write_right(), "owner");
        assert!(r.groups().iter().any(|g| g == "site"));
    }

    #[test]
    fn an_entry_round_trips_through_its_record() {
        let r = SettingsRegistry::embedded();
        for def in r.iter() {
            let back = SettingDef::from_json(&def.to_json()).unwrap();
            assert_eq!(&back, def, "{}", def.key);
        }
    }

    #[test]
    fn records_overlay_the_file() {
        let r = SettingsRegistry::embedded();
        let mut lang = r.get("site.language").unwrap().to_json();
        lang["default"] = Value::String("de".into());
        let added = serde_json::json!({
            "kind": "setting", "key": "local.banner", "scope": "tenant", "bound": "tenant",
            "group": "local", "type": "text", "no_default": true
        });
        let broken = serde_json::json!({"kind": "setting", "key": "local.broken"});
        let o = r.overlay([&lang, &added, &broken]);
        assert_eq!(
            o.get("site.language").unwrap().default,
            Some(Value::String("de".into()))
        );
        assert!(o.get("local.banner").is_some());
        assert!(o.get("local.broken").is_none());
        assert!(o.groups().iter().any(|g| g == "local"));
    }

    #[test]
    fn entries_are_checked() {
        let bad = serde_json::json!({
            "key": "x.y", "scope": "tenant", "bound": "tenant", "group": "site",
            "type": "integer", "default": "seven"
        });
        assert!(SettingDef::from_json(&bad).is_err());
        let both = serde_json::json!({
            "key": "x.y", "scope": "tenant", "bound": "tenant", "group": "site",
            "type": "integer", "default": 7, "no_default": true
        });
        assert!(SettingDef::from_json(&both).is_err());
    }
}
