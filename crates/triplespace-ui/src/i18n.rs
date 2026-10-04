//! Interface messages (0034 §9): banana JSON from `i18n/`, the files translatewiki.net
//! reads. `en.json` is embedded and is the fallback for every language.
//!
//! What is implemented is what the site's messages use so far: lookup and `$1`…`$9`.
//! `PLURAL`, `GENDER` and `GRAMMAR` come with the first message that needs them, from
//! the `banana-i18n` crate or the small implementation 0034 §9 allows (0034 Q6).

use std::collections::BTreeMap;
use std::sync::LazyLock;

static EN: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    let v: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../../../i18n/en.json")).expect("i18n/en.json parses");
    v.into_iter()
        .filter(|(k, _)| !k.starts_with('@'))
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
        .collect()
});

/// The messages of one interface language.
#[derive(Debug, Clone, Copy)]
pub struct Messages {
    lang: &'static str,
}

impl Messages {
    /// The messages for a language; every language falls back to English for now.
    #[must_use]
    pub fn for_language(_lang: &str) -> Self {
        Self { lang: "en" }
    }

    /// The language code, for `<html lang>`.
    #[must_use]
    pub fn lang(&self) -> &'static str {
        self.lang
    }

    /// The writing direction, for `<html dir>`.
    #[must_use]
    pub fn dir(&self) -> &'static str {
        "ltr"
    }

    /// A message, or `⧼key⧽` where it is missing, as MediaWiki shows one.
    #[must_use]
    pub fn get(&self, key: &str) -> String {
        EN.get(key).cloned().unwrap_or_else(|| format!("⧼{key}⧽"))
    }

    /// A message with its parameters substituted for `$1`, `$2`, ….
    #[must_use]
    pub fn with(&self, key: &str, params: &[&str]) -> String {
        let mut s = self.get(key);
        for (i, p) in params.iter().enumerate().rev() {
            s = s.replace(&format!("${}", i + 1), p);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_parameters() {
        let m = Messages::for_language("en");
        assert_eq!(m.get("ts-login"), "Log in");
        assert_eq!(
            m.with("ts-account-menu", &["Alice"]),
            "Account menu for Alice"
        );
        assert_eq!(m.get("ts-no-such-message"), "⧼ts-no-such-message⧽");
    }

    #[test]
    fn every_message_is_documented() {
        let qqq: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(include_str!("../../../i18n/qqq.json")).unwrap();
        for k in EN.keys() {
            assert!(qqq.contains_key(k), "{k} has no qqq.json entry");
        }
    }
}
