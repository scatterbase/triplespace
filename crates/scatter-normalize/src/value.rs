//! Normalized value keys (ADR 0004 §7): the canonical form the resolved view compares
//! literal values by, derived per data type. A property can override its data type's
//! normalizer; a resolver bound to the property's role does so (0029 §1).
//!
//! This crate knows nothing of Wikibase's value structs (0005 §1: the substrate knows
//! nothing about Wikibase), so each key is built from the primitive parts of a value.
//! `scatter-wikibase-model` calls these with the fields of its own types.
//!
//! A key is a string; where it has several parts they are joined with U+001F (unit
//! separator), which no part can contain after normalization. Keys of different data
//! types never compare equal by accident because each is prefixed with its data type.

use std::fmt;

use crate::resolver::{Resolver, ResolverError};
use crate::url::{UrlError, normalize_url};

const SEP: char = '\u{1F}';

/// A normalized value key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ValueKey(String);

impl ValueKey {
    /// The key as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn join(data_type: &str, parts: &[&str]) -> Self {
        let mut s = String::from(data_type);
        for p in parts {
            s.push(SEP);
            s.push_str(&p.replace(SEP, " "));
        }
        Self(s)
    }

    /// The `string` key: Unicode NFC.
    #[must_use]
    pub fn string(value: &str) -> Self {
        Self::join("string", &[&crate::nfc(value)])
    }

    /// The `monolingualtext` key: NFC, together with the language code.
    #[must_use]
    pub fn monolingualtext(text: &str, language: &str) -> Self {
        Self::join(
            "monolingualtext",
            &[&language.to_ascii_lowercase(), &crate::nfc(text)],
        )
    }

    /// The `time` key: the value, precision and calendar model. Values with different
    /// precisions are different values.
    #[must_use]
    pub fn time(time: &str, precision: u8, calendar_model: &str) -> Self {
        Self::join("time", &[time, &precision.to_string(), calendar_model])
    }

    /// The `quantity` key: the amount, unit and bounds, as the strings Wikibase carries
    /// them (0006 §2: amounts stay strings). A missing bound is the empty string.
    #[must_use]
    pub fn quantity(
        amount: &str,
        unit: &str,
        lower_bound: Option<&str>,
        upper_bound: Option<&str>,
    ) -> Self {
        Self::join(
            "quantity",
            &[
                amount,
                unit,
                lower_bound.unwrap_or(""),
                upper_bound.unwrap_or(""),
            ],
        )
    }

    /// The `external-id` key: per property. With a resolver bound to the property's
    /// role, the resolver's normalizer and case rule; otherwise the exact value.
    pub fn external_id(value: &str, resolver: Option<&Resolver>) -> Result<Self, ResolverError> {
        let key = match resolver {
            Some(r) => r.normalize(value)?,
            None => value.to_string(),
        };
        Ok(Self::join("external-id", &[&key]))
    }

    /// The `url` key: the normalized URL of 0026 §1 under the given allowed schemes.
    pub fn url(value: &str, schemes: &[&str]) -> Result<Self, UrlError> {
        Ok(Self::join("url", &[&normalize_url(value, schemes)?]))
    }

    /// Everything else: the exact value, under its data type.
    #[must_use]
    pub fn exact(data_type: &str, value: &str) -> Self {
        Self::join(data_type, &[value])
    }
}

impl fmt::Display for ValueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResolverRegistry;
    use crate::url::DEFAULT_SCHEMES;

    #[test]
    fn strings_compare_by_nfc() {
        assert_eq!(
            ValueKey::string("cafe\u{301}"),
            ValueKey::string("caf\u{e9}")
        );
        assert_ne!(ValueKey::string("Cafe"), ValueKey::string("cafe"));
    }

    #[test]
    fn monolingual_carries_language() {
        assert_ne!(
            ValueKey::monolingualtext("Paris", "en"),
            ValueKey::monolingualtext("Paris", "fr")
        );
        assert_eq!(
            ValueKey::monolingualtext("Paris", "EN"),
            ValueKey::monolingualtext("Paris", "en")
        );
    }

    #[test]
    fn time_precision_matters() {
        let q = "http://www.wikidata.org/entity/Q1985727";
        assert_ne!(
            ValueKey::time("+1952-03-11T00:00:00Z", 11, q),
            ValueKey::time("+1952-03-11T00:00:00Z", 10, q)
        );
    }

    #[test]
    fn quantities_and_bounds() {
        assert_ne!(
            ValueKey::quantity("+1", "1", None, None),
            ValueKey::quantity("+1", "1", Some("+0"), Some("+2"))
        );
        assert_eq!(
            ValueKey::quantity("+1", "1", None, None),
            ValueKey::quantity("+1", "1", None, None)
        );
    }

    #[test]
    fn external_ids_with_and_without_a_resolver() {
        let doi = ResolverRegistry::default_registry().by_name("doi").unwrap();
        assert_eq!(
            ValueKey::external_id("10.1000/ABC", Some(doi)).unwrap(),
            ValueKey::external_id("https://doi.org/10.1000/abc", Some(doi)).unwrap()
        );
        assert_ne!(
            ValueKey::external_id("10.1000/ABC", None).unwrap(),
            ValueKey::external_id("10.1000/abc", None).unwrap()
        );
    }

    #[test]
    fn urls_and_data_types_do_not_collide() {
        let u = ValueKey::url("HTTPS://Example.org", DEFAULT_SCHEMES).unwrap();
        assert_eq!(
            u,
            ValueKey::url("https://example.org/", DEFAULT_SCHEMES).unwrap()
        );
        assert_ne!(
            ValueKey::exact("url", "HTTPS://Example.org"),
            u,
            "exact keys are not normalized"
        );
        assert_ne!(ValueKey::string("x"), ValueKey::exact("external-id", "x"));
        assert!(
            ValueKey::string("a\u{1F}b").as_str().ends_with("a b"),
            "the separator is never in a part"
        );
    }
}
