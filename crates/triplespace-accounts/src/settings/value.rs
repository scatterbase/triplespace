//! Whether a value fits a setting's type (0085 §1, §3: `ts-invalid-setting`), and the
//! order ceilings compare values by.
//!
//! These checks are about shape: a reference names something of the right form. Whether
//! the tenant has what it names is the validator's question ([`super::plan()`]), since it
//! needs the tenant's records.

use serde_json::Value;

use super::registry::{RefKind, SettingDef, SettingType};

/// The longest `string` or a list item; `text` may be longer.
const MAX_STRING: usize = 1_000;
/// The longest `text`.
const MAX_TEXT: usize = 100_000;

/// Checks a value against a setting's type and choices; `Err` says why it does not fit.
pub fn check(def: &SettingDef, v: &Value) -> Result<(), String> {
    match def.ty {
        SettingType::Boolean => v
            .as_bool()
            .map(drop)
            .ok_or_else(|| "not true or false".into()),
        SettingType::Integer => v.as_i64().map(drop).ok_or_else(|| "not an integer".into()),
        SettingType::Size => v
            .as_u64()
            .map(drop)
            .ok_or_else(|| "not a size in bytes (a non-negative integer)".into()),
        SettingType::Duration => {
            let s = v.as_str().ok_or("not an ISO 8601 duration string")?;
            duration_seconds(s)
                .map(drop)
                .ok_or_else(|| format!("`{s}` is not an ISO 8601 duration"))
        }
        SettingType::Text => {
            let s = v.as_str().ok_or("not text")?;
            if s.len() > MAX_TEXT {
                return Err(format!("longer than {MAX_TEXT} bytes"));
            }
            Ok(())
        }
        SettingType::String => one_line(v).map(drop),
        SettingType::Url => {
            let s = one_line(v)?;
            if is_url(s) {
                Ok(())
            } else {
                Err(format!("`{s}` is not an absolute http or https URL"))
            }
        }
        SettingType::Language => {
            let s = v.as_str().ok_or("not a language code")?;
            if is_language(s) {
                Ok(())
            } else {
                Err(format!("`{s}` is not a language code"))
            }
        }
        SettingType::LanguageList => {
            let items = list_of_strings(v)?;
            if let Some(bad) = items.iter().find(|s| !is_language(s)) {
                return Err(format!("`{bad}` is not a language code"));
            }
            no_repeats(&items)
        }
        SettingType::Choice => {
            let s = v.as_str().ok_or("not one of the choices")?;
            if def.choices.iter().any(|c| c == s) {
                Ok(())
            } else {
                Err(format!("`{s}` is not one of {}", def.choices.join(", ")))
            }
        }
        SettingType::StringList => list_of_strings(v).map(drop),
        SettingType::IntegerList => match v.as_array() {
            Some(a) if a.iter().all(|x| x.as_i64().is_some()) => Ok(()),
            _ => Err("not a list of integers".into()),
        },
        SettingType::Map => v.as_object().map(drop).ok_or_else(|| "not a map".into()),
        SettingType::Json => Ok(()),
        SettingType::SiteAsset => site_asset(v).map(drop),
        SettingType::Reference(kind) => reference(kind, v),
    }
}

fn one_line(v: &Value) -> Result<&str, String> {
    let s = v.as_str().ok_or("not a string")?;
    if s.contains(['\n', '\r']) {
        return Err("a string is one line".into());
    }
    if s.len() > MAX_STRING {
        return Err(format!("longer than {MAX_STRING} bytes"));
    }
    Ok(s)
}

fn list_of_strings(v: &Value) -> Result<Vec<&str>, String> {
    let a = v.as_array().ok_or("not a list")?;
    a.iter()
        .map(|x| one_line(x).map_err(|_| "not a list of one-line strings".to_string()))
        .collect()
}

fn no_repeats(items: &[&str]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for s in items {
        if !seen.insert(*s) {
            return Err(format!("`{s}` is listed twice"));
        }
    }
    Ok(())
}

fn reference(kind: RefKind, v: &Value) -> Result<(), String> {
    match kind {
        RefKind::NamespaceList => {
            let a = v.as_array().ok_or("not a list of namespaces")?;
            if a.iter()
                .all(|x| x.as_i64().is_some() || x.as_str().is_some_and(|s| !s.is_empty()))
            {
                Ok(())
            } else {
                Err("not a list of namespace numbers or names".into())
            }
        }
        RefKind::PropertyList => {
            let items = list_of_strings(v)?;
            if let Some(bad) = items.iter().find(|s| !is_property_id(s)) {
                return Err(format!("`{bad}` is not a property ID"));
            }
            no_repeats(&items)
        }
        RefKind::Actor => {
            let s = one_line(v)?;
            scatter_actors::key::ActorKey::parse(s)
                .map(drop)
                .map_err(|e| format!("`{s}` is not an actor key: {e}"))
        }
        k if k.is_list() => {
            let items = list_of_strings(v)?;
            if items.iter().any(|s| s.is_empty()) {
                return Err("an empty name in the list".into());
            }
            no_repeats(&items)
        }
        _ => {
            let s = one_line(v)?;
            if s.is_empty() {
                Err("an empty name".into())
            } else {
                Ok(())
            }
        }
    }
}

/// A site asset's value (0085 §8): the `File:` title and the pinned version's SHA-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRef<'a> {
    /// The file's title, `File:Logo.svg`.
    pub title: &'a str,
    /// The version's SHA-1, 40 lower-case hex digits.
    pub sha1: &'a str,
}

/// Reads a site asset's value.
pub fn site_asset(v: &Value) -> Result<AssetRef<'_>, String> {
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .ok_or("a site asset is {title, sha1}")?;
    let sha1 = v
        .get("sha1")
        .and_then(Value::as_str)
        .ok_or("a site asset is {title, sha1}")?;
    if !title.starts_with("File:") || title.len() <= 5 {
        return Err(format!("`{title}` is not a File: title"));
    }
    if sha1.len() != 40
        || !sha1
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!("`{sha1}` is not a SHA-1 in lower-case hex"));
    }
    Ok(AssetRef { title, sha1 })
}

/// A language code: a primary subtag of two or three letters, or `x-`/`qqq`-style
/// private codes, then subtags of one to eight letters or digits.
#[must_use]
pub fn is_language(s: &str) -> bool {
    let mut parts = s.split('-');
    let Some(first) = parts.next() else {
        return false;
    };
    (2..=3).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn is_property_id(s: &str) -> bool {
    s.strip_prefix('P').is_some_and(|n| {
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    })
}

fn is_url(s: &str) -> bool {
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"));
    rest.is_some_and(|r| {
        let host = r.split(['/', '?', '#']).next().unwrap_or("");
        !host.is_empty() && !s.contains(char::is_whitespace)
    })
}

/// An ISO 8601 duration in seconds: `P[nY][nM][nW][nD][T[nH][nM][nS]]`, the last
/// component allowed a fraction. Years are 365 days and months 30, which is all a
/// ceiling comparison needs.
#[must_use]
pub fn duration_seconds(s: &str) -> Option<f64> {
    let rest = s.strip_prefix('P')?;
    if rest.is_empty() {
        return None;
    }
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) if !t.is_empty() => (d, Some(t)),
        Some(_) => return None,
        None => (rest, None),
    };
    let mut total = 0.0;
    let mut any = false;
    for (part, units) in [
        (
            date,
            &[
                ('Y', 365.0 * 86_400.0),
                ('M', 30.0 * 86_400.0),
                ('W', 7.0 * 86_400.0),
                ('D', 86_400.0),
            ][..],
        ),
        (
            time.unwrap_or(""),
            &[('H', 3_600.0), ('M', 60.0), ('S', 1.0)][..],
        ),
    ] {
        let mut number = String::new();
        let mut next = 0;
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' {
                number.push(c);
                continue;
            }
            let i = units[next..].iter().position(|(u, _)| *u == c)? + next;
            if number.is_empty() || number.starts_with('.') || number.ends_with('.') {
                return None;
            }
            let n: f64 = number.parse().ok()?;
            total += n * units[i].1;
            any = true;
            number.clear();
            next = i + 1;
        }
        if !number.is_empty() {
            return None;
        }
    }
    any.then_some(total)
}

/// The number a ceiling compares, for the types that have one.
#[must_use]
pub fn magnitude(def: &SettingDef, v: &Value) -> Option<f64> {
    match def.ty {
        #[allow(clippy::cast_precision_loss)]
        SettingType::Integer | SettingType::Size => v.as_i64().map(|n| n as f64),
        SettingType::Duration => v.as_str().and_then(duration_seconds),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::registry::SettingsRegistry;
    use serde_json::json;

    fn def(key: &str) -> &'static SettingDef {
        SettingsRegistry::embedded().get(key).unwrap()
    }

    #[test]
    fn durations() {
        assert_eq!(duration_seconds("P30D"), Some(30.0 * 86_400.0));
        assert_eq!(duration_seconds("PT0.25S"), Some(0.25));
        assert_eq!(duration_seconds("PT15M"), Some(900.0));
        assert_eq!(duration_seconds("P1DT1H"), Some(90_000.0));
        assert_eq!(duration_seconds("P"), None);
        assert_eq!(duration_seconds("PT"), None);
        assert_eq!(duration_seconds("P1H"), None);
        assert_eq!(duration_seconds("PT1S1M"), None);
        assert_eq!(duration_seconds("30 days"), None);
    }

    #[test]
    fn types() {
        assert!(check(def("account.approval"), &json!(true)).is_ok());
        assert!(check(def("account.approval"), &json!("on")).is_err());
        assert!(check(def("account.request_statement"), &json!("required")).is_ok());
        assert!(check(def("account.request_statement"), &json!("maybe")).is_err());
        assert!(check(def("site.language"), &json!("de-AT")).is_ok());
        assert!(check(def("site.language"), &json!("German")).is_err());
        assert!(check(def("site.name"), &json!("Librarybase")).is_ok());
        assert!(check(def("site.name"), &json!(7)).is_err());
        assert!(check(def("account.request_expiry"), &json!("P14D")).is_ok());
        assert!(check(def("account.request_expiry"), &json!("14d")).is_err());
        let asset =
            json!({"title": "File:Logo.svg", "sha1": "0123456789abcdef0123456789abcdef01234567"});
        assert!(check(def("site.logo"), &asset).is_ok());
        assert!(check(def("site.logo"), &json!({"title": "Logo.svg", "sha1": "x"})).is_err());
        assert!(
            check(
                def("instance.source_url"),
                &json!("https://example.org/src")
            )
            .is_ok()
        );
        assert!(check(def("instance.source_url"), &json!("example.org")).is_err());
        assert!(check(def("instance.owner_of_record"), &json!("librarybase:7")).is_ok());
    }

    #[test]
    fn magnitudes() {
        assert_eq!(magnitude(def("query.timeout"), &json!("PT1M")), Some(60.0));
        assert_eq!(magnitude(def("query.max_results"), &json!(10)), Some(10.0));
        assert_eq!(magnitude(def("site.name"), &json!("x")), None);
    }
}
