//! Preferences as the API serves them (0027 §6; 0085 §2.2): a caller's resolved values,
//! for `meta=userinfo&uiprop=options`, `GET /account/preferences` and the interface
//! language the site renders in; and the conversion of `action=options`' string values
//! to a key's type.

use std::collections::BTreeMap;

use serde_json::Value;
use triplespace_accounts::AccountsError;
use triplespace_accounts::preferences::{
    self, Defaults, PrefDef, PrefType, PreferenceRegistry, Request, ResolvedPref,
};
use triplespace_accounts::settings;

/// The languages the interface has messages for: the files of `i18n/` but `qqq`. The
/// list is checked against the directory by a test, so a new translation adds itself
/// here or fails the build.
pub const INTERFACE_LANGUAGES: &[&str] = &["en"];

/// A caller's preferences, resolved for one request.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// Every key, resolved.
    pub values: BTreeMap<String, ResolvedPref>,
    /// The account's own values.
    pub own: BTreeMap<String, Value>,
}

/// Resolves a caller's preferences on a tenant: the account's own values (none for a
/// reader who is not logged in), the request's language, and the tenant's defaults.
pub async fn resolve(
    db: &tokio_postgres::Client,
    tenant: &str,
    actor: Option<&str>,
    uselang: Option<&str>,
    accept_language: Option<&str>,
) -> Result<Resolved, AccountsError> {
    let own = match actor {
        Some(a) => preferences::own(db, a).await?,
        None => BTreeMap::new(),
    };
    let (snap, _) = settings::load(db, Some(tenant)).await?;
    let defaults = Defaults::of(&snap);
    let request = Request {
        uselang,
        accept_language,
        interface_languages: INTERFACE_LANGUAGES,
    };
    let values =
        preferences::resolve_all(PreferenceRegistry::embedded(), &own, &request, &defaults);
    Ok(Resolved { values, own })
}

/// An `action=options` value, a string as MediaWiki takes it, read as the key's type:
/// `1`, `true` and `on` are true and `0`, `false`, `off` and the empty string false; a
/// list is JSON or comma-separated; JSON is parsed where it parses.
#[must_use]
pub fn from_mediawiki(def: &PrefDef, s: &str) -> Value {
    match def.ty {
        PrefType::Boolean => match s {
            "1" | "true" | "on" => Value::Bool(true),
            "0" | "false" | "off" | "" => Value::Bool(false),
            other => Value::String(other.to_string()),
        },
        PrefType::Integer => s
            .trim()
            .parse::<i64>()
            .map_or_else(|_| Value::String(s.to_string()), Value::from),
        PrefType::LanguageList => {
            if s.trim_start().starts_with('[') {
                serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
            } else {
                Value::Array(
                    s.split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(|x| Value::String(x.to_string()))
                        .collect(),
                )
            }
        }
        PrefType::Json => serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string())),
        _ => Value::String(s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_languages_match_the_message_files() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../i18n");
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| {
                let name = e.ok()?.file_name().into_string().ok()?;
                let lang = name.strip_suffix(".json")?.to_string();
                (lang != "qqq").then_some(lang)
            })
            .collect();
        found.sort();
        let mut listed: Vec<String> = INTERFACE_LANGUAGES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        listed.sort();
        assert_eq!(found, listed);
    }

    #[test]
    fn mediawiki_values() {
        let r = PreferenceRegistry::embedded();
        assert_eq!(
            from_mediawiki(r.lookup("autowatch.edit").unwrap(), "1"),
            Value::Bool(true)
        );
        assert_eq!(
            from_mediawiki(r.lookup("autowatch.edit").unwrap(), ""),
            Value::Bool(false)
        );
        assert_eq!(
            from_mediawiki(r.lookup("rc.limit").unwrap(), "250"),
            Value::from(250)
        );
        assert_eq!(
            from_mediawiki(r.lookup("label-languages").unwrap(), "en, de"),
            serde_json::json!(["en", "de"])
        );
        assert_eq!(
            from_mediawiki(r.lookup("ui.mode").unwrap(), "dark"),
            Value::String("dark".into())
        );
    }
}
