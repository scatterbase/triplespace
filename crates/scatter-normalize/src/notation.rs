//! Notations (ADR 0048): strings that name something in an outside vocabulary, exactly as
//! that vocabulary's own rule says, and the scheme registry embedded from
//! `docs/registry/notation-schemes.toml`.
//!
//! A notation's key is `{scheme}:{string}` (`osm:amenity=cafe`). A scheme name never
//! contains a colon, so the first colon ends it, and the string may hold colons of its own
//! (`osm:addr:street=Main Street`). The scheme name is matched case-insensitively and
//! written in lowercase; the string is normalized by the scheme's normalizer, one of a
//! closed set, and then checked against the scheme's grammar, which is code here. A scheme
//! naming a normalizer or grammar this crate does not implement is rejected at parse time,
//! so a new scheme is a registry change and a new grammar is a module, never a silent no-op.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::{NOTATION_SCHEMES_TOML, osmtag};

/// The most characters a scheme name may have.
pub const MAX_SCHEME_CHARS: usize = 32;

/// The most characters a string under the `text` grammar may have.
pub const MAX_TEXT_CHARS: usize = 255;

/// How a scheme normalizes its strings (0048 §2). A closed set, so that the rule that
/// decides identity is never free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Normalizer {
    /// The input must already be NFC and is not changed (the `osm` scheme).
    Exact,
    /// Unicode NFC.
    Nfc,
    /// NFC, then full Unicode case folding (0017 §5, 0029 §1), then NFC again, since
    /// folding can leave a string that is not in NFC.
    Casefold,
    /// NFC, then ASCII letters lowercased.
    AsciiLowercase,
}

impl Normalizer {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "exact" => Self::Exact,
            "nfc" => Self::Nfc,
            "casefold" => Self::Casefold,
            "ascii-lowercase" => Self::AsciiLowercase,
            _ => return None,
        })
    }

    /// The name used in the registry.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Nfc => "nfc",
            Self::Casefold => "casefold",
            Self::AsciiLowercase => "ascii-lowercase",
        }
    }

    fn apply(self, input: &str) -> Result<String, String> {
        match self {
            Self::Exact => {
                if crate::is_nfc(input) {
                    Ok(input.to_string())
                } else {
                    Err("not in Unicode NFC".to_string())
                }
            }
            Self::Nfc => Ok(crate::nfc(input)),
            Self::Casefold => Ok(crate::nfc(&crate::casefold(&crate::nfc(input)))),
            Self::AsciiLowercase => Ok(crate::nfc(input).to_ascii_lowercase()),
        }
    }
}

/// Which strings a scheme admits, checked after normalizing (0048 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grammar {
    /// An OSM key or tag ([`crate::osmtag`], 0036 §3).
    OsmTag,
    /// 1 to [`MAX_TEXT_CHARS`] characters, no control characters, no leading or trailing
    /// whitespace.
    Text,
}

impl Grammar {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "osm-tag" => Self::OsmTag,
            "text" => Self::Text,
            _ => return None,
        })
    }

    /// The name used in the registry.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::OsmTag => "osm-tag",
            Self::Text => "text",
        }
    }

    fn check(self, s: &str) -> Result<(), String> {
        match self {
            Self::OsmTag => osmtag::normalize(s).map(drop).map_err(|e| e.to_string()),
            Self::Text => {
                let n = s.chars().count();
                if n == 0 {
                    Err("empty".to_string())
                } else if n > MAX_TEXT_CHARS {
                    Err(format!("longer than {MAX_TEXT_CHARS} characters"))
                } else if s.chars().any(char::is_control) {
                    Err("contains a control character".to_string())
                } else if s.trim() != s {
                    Err("has leading or trailing whitespace".to_string())
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// One notation scheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    /// The scheme name (`osm`); one namespace with keyed-type and resolver names.
    pub name: String,
    /// The vocabulary's name ("OSM tag or key").
    pub label: String,
    /// How strings are normalized.
    pub normalizer: Normalizer,
    /// Which strings are valid.
    pub grammar: Grammar,
    /// URL template for the vocabulary's page about a string; `{string}` is the string.
    pub documentation: Option<String>,
    /// Retired schemes stay in the file so that the name is never reused.
    pub retired: bool,
}

impl Scheme {
    /// Normalizes a string of this scheme (without the scheme prefix).
    pub fn normalize(&self, input: &str) -> Result<String, NotationError> {
        let invalid = |reason: String| NotationError::Invalid {
            scheme: self.name.clone(),
            input: input.to_string(),
            reason,
        };
        let s = self.normalizer.apply(input).map_err(invalid)?;
        self.grammar.check(&s).map_err(invalid)?;
        Ok(s)
    }

    /// The documentation URL for a canonical string, where the scheme has a template.
    #[must_use]
    pub fn documentation_url(&self, string: &str) -> Option<String> {
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
        self.documentation.as_ref().map(|t| {
            let enc = utf8_percent_encode(string, NON_ALPHANUMERIC).to_string();
            // Keep the path separators of strings such as `text/html` readable.
            t.replace("{string}", &enc.replace("%2F", "/"))
        })
    }
}

/// Why a string is not a notation key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NotationError {
    /// No colon, so no scheme.
    #[error("`{0}` has no scheme: a notation key is `{{scheme}}:{{string}}`")]
    NoScheme(String),
    /// The scheme is not registered.
    #[error("`{0}` is not a registered notation scheme")]
    UnknownScheme(String),
    /// The string is not valid under its scheme.
    #[error("`{input}` is not a valid {scheme} notation: {reason}")]
    Invalid {
        /// The scheme name.
        scheme: String,
        /// The string, as given.
        input: String,
        /// The normalizer's or grammar's reason.
        reason: String,
    },
}

/// Why a scheme registry file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SchemeRegistryError {
    /// The TOML did not parse or had the wrong shape.
    #[error("notation-schemes.toml: {0}")]
    Toml(String),
    /// The file's `version` is one this crate does not read.
    #[error("notation-schemes.toml version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// The name breaks the scheme-name grammar.
    #[error(
        "scheme name `{0}` must be lowercase ASCII letters, digits and single hyphens, start with a letter, and have at most {MAX_SCHEME_CHARS} characters"
    )]
    BadName(String),
    /// Two schemes share a name.
    #[error("two notation schemes are named `{0}`")]
    Duplicate(String),
    /// The normalizer is not one of the closed set.
    #[error("scheme `{name}`: normalizer `{normalizer}` is not implemented")]
    UnknownNormalizer {
        /// The scheme.
        name: String,
        /// The value.
        normalizer: String,
    },
    /// The grammar is not implemented in this crate.
    #[error("scheme `{name}`: grammar `{grammar}` is not implemented in scatter-normalize")]
    UnknownGrammar {
        /// The scheme.
        name: String,
        /// The value.
        grammar: String,
    },
    /// The documentation template has no `{string}`.
    #[error("scheme `{name}`: documentation `{documentation}` has no `{{string}}`")]
    BadDocumentation {
        /// The scheme.
        name: String,
        /// The template.
        documentation: String,
    },
}

/// Whether `s` is a valid scheme name: lowercase ASCII letters, digits and single hyphens,
/// starting with a letter, at most [`MAX_SCHEME_CHARS`] characters.
#[must_use]
pub fn is_scheme_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SCHEME_CHARS
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Splits a key into its scheme name, as given, and its string. `None` without a colon.
#[must_use]
pub fn split(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}

/// The string of a canonical key, without the scheme: the derived label (0048 §1).
#[must_use]
pub fn string_of(key: &str) -> &str {
    split(key).map_or(key, |(_, s)| s)
}

/// The parsed and validated scheme registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemeRegistry {
    version: u32,
    schemes: Vec<Scheme>,
    by_name: BTreeMap<String, usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default, rename = "scheme")]
    schemes: Vec<RawScheme>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScheme {
    name: String,
    label: Option<String>,
    normalizer: String,
    grammar: String,
    documentation: Option<String>,
    #[serde(default)]
    retired: bool,
    #[allow(dead_code)]
    defined_in: Option<String>,
}

impl SchemeRegistry {
    /// Parses and validates a registry in the format of `docs/registry/notation-schemes.toml`.
    pub fn parse(text: &str) -> Result<Self, SchemeRegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| SchemeRegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(SchemeRegistryError::Version(raw.version));
        }
        let mut schemes = Vec::with_capacity(raw.schemes.len());
        let mut by_name = BTreeMap::new();
        for s in raw.schemes {
            let name = s.name;
            if !is_scheme_name(&name) {
                return Err(SchemeRegistryError::BadName(name));
            }
            let Some(normalizer) = Normalizer::parse(&s.normalizer) else {
                return Err(SchemeRegistryError::UnknownNormalizer {
                    name,
                    normalizer: s.normalizer,
                });
            };
            let Some(grammar) = Grammar::parse(&s.grammar) else {
                return Err(SchemeRegistryError::UnknownGrammar {
                    name,
                    grammar: s.grammar,
                });
            };
            if let Some(d) = &s.documentation
                && !d.contains("{string}")
            {
                return Err(SchemeRegistryError::BadDocumentation {
                    name,
                    documentation: d.clone(),
                });
            }
            if by_name.insert(name.clone(), schemes.len()).is_some() {
                return Err(SchemeRegistryError::Duplicate(name));
            }
            schemes.push(Scheme {
                label: s.label.unwrap_or_else(|| name.clone()),
                name,
                normalizer,
                grammar,
                documentation: s.documentation,
                retired: s.retired,
            });
        }
        Ok(Self {
            version: raw.version,
            schemes,
            by_name,
        })
    }

    /// The embedded default registry, parsed once.
    ///
    /// # Panics
    ///
    /// Never in a build that passed its tests: the default file is validated by a test.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: LazyLock<SchemeRegistry> = LazyLock::new(|| {
            SchemeRegistry::parse(NOTATION_SCHEMES_TOML)
                .expect("docs/registry/notation-schemes.toml is valid")
        });
        &DEFAULT
    }

    /// The file format version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every scheme, in registry order.
    #[must_use]
    pub fn schemes(&self) -> &[Scheme] {
        &self.schemes
    }

    /// The scheme with this name, matched case-insensitively.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&Scheme> {
        self.by_name
            .get(&name.to_ascii_lowercase())
            .map(|&i| &self.schemes[i])
    }

    /// Normalizes a key, `OSM:amenity=cafe`, to its canonical form, `osm:amenity=cafe`.
    ///
    /// ```
    /// use scatter_normalize::notation::SchemeRegistry;
    /// let r = SchemeRegistry::default_registry();
    /// assert_eq!(r.normalize_key("OSM:amenity=cafe").unwrap(), "osm:amenity=cafe");
    /// assert_eq!(r.normalize_key("osm:addr:street=Main Street").unwrap(), "osm:addr:street=Main Street");
    /// assert!(r.normalize_key("amenity=cafe").is_err(), "a key needs a scheme");
    /// assert!(r.normalize_key("nope:amenity").is_err(), "the scheme must be registered");
    /// ```
    pub fn normalize_key(&self, key: &str) -> Result<String, NotationError> {
        let (scheme, string) =
            split(key).ok_or_else(|| NotationError::NoScheme(key.to_string()))?;
        let s = self
            .by_name(scheme)
            .ok_or_else(|| NotationError::UnknownScheme(scheme.to_string()))?;
        Ok(format!("{}:{}", s.name, s.normalize(string)?))
    }
}

/// Normalizes a key against the default scheme registry ([`SchemeRegistry::normalize_key`]).
pub fn normalize(key: &str) -> Result<String, NotationError> {
    SchemeRegistry::default_registry().normalize_key(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KeyedRegistry, ResolverRegistry};

    fn reg() -> &'static SchemeRegistry {
        SchemeRegistry::default_registry()
    }

    #[test]
    fn default_registry() {
        let r = reg();
        assert_eq!(r.version(), 1);
        let osm = r.by_name("osm").unwrap();
        assert_eq!(osm.normalizer, Normalizer::Exact);
        assert_eq!(osm.grammar, Grammar::OsmTag);
        assert_eq!(osm.label, "OSM tag or key");
        assert!(!osm.retired);
        assert!(std::ptr::eq(r.by_name("OSM").unwrap(), osm));
        assert!(
            r.by_name("hashtag").is_none(),
            "candidates are commented out"
        );
    }

    #[test]
    fn scheme_names_share_one_namespace_with_keyed_types_and_resolvers() {
        // 0029 §2 as extended by 0048 §2: `osm:` in /resolve means one thing.
        let keyed = KeyedRegistry::default_registry();
        let resolvers = ResolverRegistry::default_registry();
        for s in reg().schemes() {
            assert!(
                keyed.by_name(&s.name).is_none(),
                "{} is a keyed type",
                s.name
            );
            assert!(
                resolvers.by_name(&s.name).is_none(),
                "{} is a resolver",
                s.name
            );
        }
    }

    #[test]
    fn osm_scheme_is_exact() {
        let r = reg();
        // Case and punctuation are significant: these are four notations, not one.
        let keys = ["osm:name", "osm:Name", "osm:fast_food", "osm:fast-food"];
        for k in keys {
            assert_eq!(r.normalize_key(k).unwrap(), k);
        }
        assert!(r.normalize_key("osm:amenity=").is_err());
        assert!(r.normalize_key("osm: amenity").is_err());
        assert!(matches!(
            r.normalize_key("osm:Cafe\u{301}"),
            Err(NotationError::Invalid { .. })
        ));
        assert_eq!(
            string_of("osm:addr:street=Main Street"),
            "addr:street=Main Street"
        );
    }

    #[test]
    fn normalizers() {
        assert_eq!(Normalizer::Exact.apply("Ab").unwrap(), "Ab");
        assert!(Normalizer::Exact.apply("e\u{301}").is_err());
        assert_eq!(Normalizer::Nfc.apply("e\u{301}").unwrap(), "\u{e9}");
        assert_eq!(Normalizer::Casefold.apply("Straße").unwrap(), "strasse");
        assert_eq!(
            Normalizer::Casefold.apply("#MachineLearning").unwrap(),
            "#machinelearning"
        );
        assert_eq!(
            Normalizer::AsciiLowercase.apply("Image/SVG+XML").unwrap(),
            "image/svg+xml"
        );
        assert_eq!(Normalizer::AsciiLowercase.apply("Ä").unwrap(), "Ä");
    }

    #[test]
    fn text_grammar() {
        assert!(Grammar::Text.check("a b").is_ok());
        assert!(Grammar::Text.check("").is_err());
        assert!(Grammar::Text.check(" a").is_err());
        assert!(Grammar::Text.check("a\nb").is_err());
        assert!(Grammar::Text.check(&"a".repeat(MAX_TEXT_CHARS)).is_ok());
        assert!(
            Grammar::Text
                .check(&"a".repeat(MAX_TEXT_CHARS + 1))
                .is_err()
        );
    }

    #[test]
    fn scheme_name_grammar() {
        for ok in ["osm", "media-type", "a1", "x"] {
            assert!(is_scheme_name(ok), "{ok}");
        }
        for bad in [
            "",
            "OSM",
            "1a",
            "-a",
            "a-",
            "a--b",
            "a:b",
            "a_b",
            &"a".repeat(33),
        ] {
            assert!(!is_scheme_name(bad), "{bad}");
        }
    }

    #[test]
    fn documentation_urls() {
        let r = SchemeRegistry::parse(
            "version = 1\n[[scheme]]\nname = \"media-type\"\nnormalizer = \"ascii-lowercase\"\ngrammar = \"text\"\ndocumentation = \"https://www.iana.org/assignments/media-types/{string}\"\n",
        )
        .unwrap();
        let s = r.by_name("media-type").unwrap();
        assert_eq!(
            s.documentation_url("image/svg+xml").as_deref(),
            Some("https://www.iana.org/assignments/media-types/image/svg%2Bxml")
        );
        assert_eq!(
            reg().by_name("osm").unwrap().documentation_url("amenity"),
            None
        );
    }

    #[test]
    fn rejects_bad_registries() {
        let one = |extra: &str| {
            format!(
                "version = 1\n[[scheme]]\nname = \"osm\"\nnormalizer = \"exact\"\ngrammar = \"osm-tag\"\n{extra}"
            )
        };
        assert!(SchemeRegistry::parse(&one("")).is_ok());
        assert!(matches!(
            SchemeRegistry::parse(&one("").replace("\"osm\"", "\"OSM\"")),
            Err(SchemeRegistryError::BadName(_))
        ));
        assert!(matches!(
            SchemeRegistry::parse(&one("").replace("exact", "nfkc")),
            Err(SchemeRegistryError::UnknownNormalizer { .. })
        ));
        assert!(matches!(
            SchemeRegistry::parse(&one("").replace("osm-tag", "hashtag")),
            Err(SchemeRegistryError::UnknownGrammar { .. })
        ));
        assert!(matches!(
            SchemeRegistry::parse(&one("documentation = \"https://x/\"")),
            Err(SchemeRegistryError::BadDocumentation { .. })
        ));
        assert!(matches!(
            SchemeRegistry::parse(&one("unknown = 1")),
            Err(SchemeRegistryError::Toml(_))
        ));
        let two = one("") + &one("").replace("version = 1\n", "");
        assert!(matches!(
            SchemeRegistry::parse(&two),
            Err(SchemeRegistryError::Duplicate(_))
        ));
    }
}
