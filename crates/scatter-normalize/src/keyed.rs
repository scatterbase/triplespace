//! Keyed entity types (ADR 0009 §1, 0017 §3 and §5, 0048 §1): the registry embedded from
//! `docs/registry/keyed-types.toml`, and keyed IDs such as `domain:en.wikipedia.org`.
//!
//! A keyed type's entity ID is its normalized natural key with the type's prefix, not a
//! minted identifier. The registry names each type's namespace, data type, IRI template,
//! ID prefix and display rule; the grammar and normalizer are implemented by name in
//! [`crate::domain`], [`crate::keyword`] and [`crate::notation`], because they are code,
//! not data. A notation's normalizer and grammar are chosen by its scheme, from the
//! default scheme registry. A registry entry whose name has no implementation is rejected at parse time,
//! so a new keyed type is a registry change and a module, never a silent no-op.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

use oxrdf::NamedNode;
use serde::Deserialize;

use crate::{KEYED_TYPES_TOML, domain, keyword, notation};

/// How a keyed type's label is produced (0009 §5, 0017 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Display {
    /// The label is computed from the key and cannot be edited.
    Derived,
    /// Labels and descriptions are stored and reconciled like an item's.
    Editable,
}

/// One keyed entity type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedType {
    /// The `type` in Wikibase JSON and the normalizer's name.
    pub name: String,
    /// The display name of the type ("Domain").
    pub label: String,
    /// The namespace whose pages host the type.
    pub namespace: String,
    /// The Wikibase data type used to refer to entities of this type.
    pub data_type: String,
    /// The concept IRI template; `{key}` is the normalized key.
    pub iri: String,
    /// The entity ID is this prefix followed by the key (`domain:`).
    pub id_prefix: String,
    /// How the label is produced.
    pub display: Display,
    /// Whether entities of this type may join identity clusters (0009 §8, 0017 §5).
    pub clusters: bool,
}

impl KeyedType {
    /// Normalizes `input` to a key of this type, by the type's own rules.
    pub fn normalize(&self, input: &str) -> Result<String, KeyError> {
        let err = |e: String| KeyError::Invalid {
            keyed_type: self.name.clone(),
            input: input.to_string(),
            reason: e,
        };
        match self.name.as_str() {
            "domain" => domain::normalize(input).map_err(|e| err(e.to_string())),
            "keyword" => keyword::normalize(input).map_err(|e| err(e.to_string())),
            "notation" => notation::normalize(input).map_err(|e| err(e.to_string())),
            other => Err(KeyError::NoNormalizer(other.to_string())),
        }
    }

    /// Whether `key` is already canonical for this type.
    #[must_use]
    pub fn is_key(&self, key: &str) -> bool {
        self.normalize(key).as_deref() == Ok(key)
    }

    /// The entity ID for a canonical key: `domain:en.wikipedia.org`.
    #[must_use]
    pub fn id_for(&self, key: &str) -> String {
        format!("{}{}", self.id_prefix, key)
    }

    /// The concept IRI for a canonical key, content-derived and the same on every instance.
    ///
    /// # Panics
    ///
    /// If the registry template does not produce an IRI for this key, which cannot happen
    /// for the keys the type's own normalizer produces.
    #[must_use]
    pub fn iri_for(&self, key: &str) -> NamedNode {
        NamedNode::new(self.iri.replace("{key}", &iri_key(key)))
            .expect("registry IRI templates produce IRIs for normalized keys")
    }

    /// The derived label for a key, when the display rule is `Derived`: a Domain's U-label
    /// (0009 §5), a notation's string without its scheme (0048 §1). `None` for editable types, whose
    /// fallback when no label exists is [`keyword::display`].
    #[must_use]
    pub fn derived_label(&self, key: &str) -> Option<String> {
        match (self.display, self.name.as_str()) {
            (Display::Derived, "domain") => Some(domain::to_unicode(key)),
            (Display::Derived, "notation") => Some(notation::string_of(key).to_string()),
            (Display::Derived, _) => Some(key.to_string()),
            (Display::Editable, _) => None,
        }
    }
}

/// Percent-encodes the characters of a key that cannot appear in an IRI path segment
/// (a notation's space or `#`, say). Domain and keyword keys pass through unchanged.
fn iri_key(key: &str) -> String {
    use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
    const SET: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'`')
        .add(b'{')
        .add(b'}')
        .add(b'%');
    utf8_percent_encode(key, SET).to_string()
}

/// A keyed ID: a type and a canonical key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedId<'r> {
    keyed_type: &'r KeyedType,
    key: String,
}

impl<'r> KeyedId<'r> {
    /// The type.
    #[must_use]
    pub fn keyed_type(&self) -> &'r KeyedType {
        self.keyed_type
    }

    /// The canonical key, without the prefix.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The concept IRI.
    #[must_use]
    pub fn iri(&self) -> NamedNode {
        self.keyed_type.iri_for(&self.key)
    }
}

impl fmt::Display for KeyedId<'_> {
    /// The canonical ID, `domain:en.wikipedia.org`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.keyed_type.id_prefix, self.key)
    }
}

/// Why a string is not a key or a keyed ID.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyError {
    /// The input is not valid under the type's grammar and normalizer.
    #[error("`{input}` is not a valid {keyed_type} key: {reason}")]
    Invalid {
        /// The keyed type's name.
        keyed_type: String,
        /// The input.
        input: String,
        /// The normalizer's reason.
        reason: String,
    },
    /// Not a keyed ID: no colon, or a prefix that names no keyed type. Minted IDs land
    /// here (0017 §1: no minted ID contains a colon).
    #[error("`{0}` is not a keyed entity ID")]
    NotKeyed(String),
    /// The registry names a type this crate has no normalizer for.
    #[error("no normalizer is implemented for keyed type `{0}`")]
    NoNormalizer(String),
}

/// Why a registry file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyedTypeError {
    /// The TOML did not parse or had the wrong shape.
    #[error("keyed-types.toml: {0}")]
    Toml(String),
    /// The file's `version` is one this crate does not read.
    #[error("keyed-types.toml version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// Two types share a name or an ID prefix.
    #[error("keyed types `{a}` and `{b}` share the {what} `{value}`")]
    Duplicate {
        /// The first type.
        a: String,
        /// The second type.
        b: String,
        /// `name` or `id_prefix`.
        what: &'static str,
        /// The shared value.
        value: String,
    },
    /// The ID prefix is not the lower-case name followed by a colon, or not lower-case.
    #[error("keyed type `{name}`: id_prefix `{id_prefix}` must be lower-case and end in `:`")]
    BadPrefix {
        /// The type.
        name: String,
        /// The prefix.
        id_prefix: String,
    },
    /// The IRI template has no `{key}`.
    #[error("keyed type `{name}`: IRI template `{iri}` has no `{{key}}`")]
    BadIriTemplate {
        /// The type.
        name: String,
        /// The template.
        iri: String,
    },
    /// The `display` field does not start with `derived` or `editable`.
    #[error("keyed type `{name}`: display `{display}` is neither derived nor editable")]
    BadDisplay {
        /// The type.
        name: String,
        /// The field.
        display: String,
    },
    /// No normalizer is implemented for the type.
    #[error("keyed type `{0}` has no normalizer in scatter-normalize")]
    NoNormalizer(String),
}

/// The parsed and validated keyed-type registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedRegistry {
    version: u32,
    types: Vec<KeyedType>,
    by_name: BTreeMap<String, usize>,
    by_prefix: BTreeMap<String, usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default, rename = "keyed_type")]
    types: Vec<RawType>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawType {
    name: String,
    label: Option<String>,
    namespace: String,
    data_type: String,
    iri: String,
    id_prefix: String,
    display: String,
    #[serde(default = "default_true")]
    clusters: bool,
    // Prose fields, documented in the file and implemented in code.
    #[allow(dead_code)]
    grammar: Option<String>,
    #[allow(dead_code)]
    normalizer: Option<String>,
    #[allow(dead_code)]
    defined_in: Option<String>,
}

fn default_true() -> bool {
    true
}

impl KeyedRegistry {
    /// Parses and validates a registry in the format of `docs/registry/keyed-types.toml`.
    pub fn parse(text: &str) -> Result<Self, KeyedTypeError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| KeyedTypeError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(KeyedTypeError::Version(raw.version));
        }
        let mut types = Vec::with_capacity(raw.types.len());
        for t in raw.types {
            let name = t.name;
            if !t.id_prefix.ends_with(':')
                || t.id_prefix.len() < 2
                || t.id_prefix != t.id_prefix.to_ascii_lowercase()
            {
                return Err(KeyedTypeError::BadPrefix {
                    name,
                    id_prefix: t.id_prefix,
                });
            }
            if !t.iri.contains("{key}") {
                return Err(KeyedTypeError::BadIriTemplate { name, iri: t.iri });
            }
            let display = if t.display.starts_with("derived") {
                Display::Derived
            } else if t.display.starts_with("editable") {
                Display::Editable
            } else {
                return Err(KeyedTypeError::BadDisplay {
                    name,
                    display: t.display,
                });
            };
            if !matches!(name.as_str(), "domain" | "keyword" | "notation") {
                return Err(KeyedTypeError::NoNormalizer(name));
            }
            types.push(KeyedType {
                label: t.label.unwrap_or_else(|| name.clone()),
                name,
                namespace: t.namespace,
                data_type: t.data_type,
                iri: t.iri,
                id_prefix: t.id_prefix,
                display,
                clusters: t.clusters,
            });
        }
        let mut by_name = BTreeMap::new();
        let mut by_prefix = BTreeMap::new();
        for (i, t) in types.iter().enumerate() {
            let dup = |what, value: &str, j: usize| KeyedTypeError::Duplicate {
                a: types[j].name.clone(),
                b: t.name.clone(),
                what,
                value: value.to_string(),
            };
            if let Some(j) = by_name.insert(t.name.clone(), i) {
                return Err(dup("name", &t.name, j));
            }
            if let Some(j) = by_prefix.insert(t.id_prefix.clone(), i) {
                return Err(dup("id_prefix", &t.id_prefix, j));
            }
        }
        Ok(Self {
            version: raw.version,
            types,
            by_name,
            by_prefix,
        })
    }

    /// The embedded default registry, parsed once.
    ///
    /// # Panics
    ///
    /// Never in a build that passed its tests: the default file is validated by a test.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: LazyLock<KeyedRegistry> = LazyLock::new(|| {
            KeyedRegistry::parse(KEYED_TYPES_TOML).expect("docs/registry/keyed-types.toml is valid")
        });
        &DEFAULT
    }

    /// The file format version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every type, in registry order.
    #[must_use]
    pub fn types(&self) -> &[KeyedType] {
        &self.types
    }

    /// The type with this name (`domain`).
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&KeyedType> {
        self.by_name.get(name).map(|&i| &self.types[i])
    }

    /// The type whose Wikibase data type this is (`wikibase-domain`).
    #[must_use]
    pub fn by_data_type(&self, data_type: &str) -> Option<&KeyedType> {
        self.types.iter().find(|t| t.data_type == data_type)
    }

    /// Parses a keyed ID, `domain:En.Wikipedia.org`, into its type and canonical key.
    /// The prefix is matched case-insensitively; the key is normalized by the type.
    ///
    /// ```
    /// use scatter_normalize::KeyedRegistry;
    /// let r = KeyedRegistry::default_registry();
    /// let id = r.parse_id("Domain:En.Wikipedia.org").unwrap();
    /// assert_eq!(id.to_string(), "domain:en.wikipedia.org");
    /// assert_eq!(id.iri().as_str(), "https://scatter.red/domain/en.wikipedia.org");
    /// assert!(r.parse_id("en.wikipedia.org").is_err(), "bare keys are not IDs (0017 §1)");
    /// assert!(r.parse_id("Q5").is_err());
    /// ```
    pub fn parse_id<'r>(&'r self, id: &str) -> Result<KeyedId<'r>, KeyError> {
        let Some((prefix, key)) = id.split_once(':') else {
            return Err(KeyError::NotKeyed(id.to_string()));
        };
        let prefix = format!("{}:", prefix.to_ascii_lowercase());
        let keyed_type = self
            .by_prefix
            .get(&prefix)
            .map(|&i| &self.types[i])
            .ok_or_else(|| KeyError::NotKeyed(id.to_string()))?;
        let key = keyed_type.normalize(key)?;
        Ok(KeyedId { keyed_type, key })
    }

    /// Tries a bare string as a key of every type, for the title resolver's suggestions
    /// (0017 §1: a bare key is not an ID, but `/resolve` still recognises one).
    pub fn suggest<'r>(&'r self, input: &str) -> Vec<KeyedId<'r>> {
        self.types
            .iter()
            .filter_map(|t| {
                t.normalize(input)
                    .ok()
                    .map(|key| KeyedId { keyed_type: t, key })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> &'static KeyedRegistry {
        KeyedRegistry::default_registry()
    }

    #[test]
    fn default_registry() {
        let r = reg();
        assert_eq!(r.version(), 1);
        let d = r.by_name("domain").unwrap();
        assert_eq!(d.id_prefix, "domain:");
        assert_eq!(d.data_type, "wikibase-domain");
        assert_eq!(d.namespace, "Domain");
        assert_eq!(d.display, Display::Derived);
        assert!(d.clusters);
        let k = r.by_name("keyword").unwrap();
        assert_eq!(k.display, Display::Editable);
        assert!(!k.clusters);
        let n = r.by_name("notation").unwrap();
        assert_eq!(n.id_prefix, "notation:");
        assert_eq!(n.data_type, "wikibase-notation");
        assert_eq!(n.display, Display::Derived);
        assert!(!n.clusters);
        assert!(
            r.by_name("osm-tag").is_none(),
            "an osm scheme now (0048 §6)"
        );
        assert!(std::ptr::eq(r.by_data_type("wikibase-keyword").unwrap(), k));
        assert!(r.by_name("doi").is_none(), "DOI is a resolver (0029)");
    }

    #[test]
    fn ids_round_trip() {
        let r = reg();
        for (input, canonical, iri) in [
            (
                "domain:bücher.example.",
                "domain:xn--bcher-kva.example",
                "https://scatter.red/domain/xn--bcher-kva.example",
            ),
            (
                "keyword:Machine Learning",
                "keyword:machine-learning",
                "https://scatter.red/keyword/machine-learning",
            ),
            (
                "NOTATION:OSM:amenity=cafe",
                "notation:osm:amenity=cafe",
                "https://scatter.red/notation/osm:amenity=cafe",
            ),
            (
                "notation:osm:addr:street=Main Street",
                "notation:osm:addr:street=Main Street",
                "https://scatter.red/notation/osm:addr:street=Main%20Street",
            ),
        ] {
            let id = r.parse_id(input).unwrap();
            assert_eq!(id.to_string(), canonical);
            assert_eq!(id.iri().as_str(), iri);
            assert_eq!(
                r.parse_id(&id.to_string()).unwrap(),
                id,
                "canonical is stable"
            );
        }
    }

    #[test]
    fn keyed_ids_and_minted_ids_never_collide() {
        let r = reg();
        // `p53` is a keyword and a property number; the prefix tells them apart (0017 §1).
        assert_eq!(
            r.parse_id("keyword:p53").unwrap().to_string(),
            "keyword:p53"
        );
        assert!(matches!(r.parse_id("P53"), Err(KeyError::NotKeyed(_))));
        assert!(matches!(r.parse_id("WDQ42"), Err(KeyError::NotKeyed(_))));
        assert!(matches!(
            r.parse_id("doi:10.1000/x"),
            Err(KeyError::NotKeyed(_))
        ));
        assert!(matches!(
            r.parse_id("domain:_dmarc.example.org"),
            Err(KeyError::Invalid { .. })
        ));
        // A notation needs a registered scheme, and its string keeps case (0048 §1, §6).
        assert!(matches!(
            r.parse_id("notation:amenity=cafe"),
            Err(KeyError::Invalid { .. })
        ));
        assert_ne!(
            r.parse_id("notation:osm:Name").unwrap(),
            r.parse_id("notation:osm:name").unwrap()
        );
    }

    #[test]
    fn labels() {
        let r = reg();
        let d = r.by_name("domain").unwrap();
        assert_eq!(
            d.derived_label("xn--bcher-kva.example").as_deref(),
            Some("bücher.example")
        );
        assert_eq!(
            r.by_name("notation")
                .unwrap()
                .derived_label("osm:amenity=cafe")
                .as_deref(),
            Some("amenity=cafe")
        );
        assert_eq!(r.by_name("keyword").unwrap().derived_label("dna"), None);
    }

    #[test]
    fn suggestions_for_bare_strings() {
        let r = reg();
        let s = r.suggest("en.wikipedia.org");
        let names: Vec<_> = s.iter().map(|id| id.keyed_type().name.as_str()).collect();
        assert!(names.contains(&"domain"));
        assert!(
            !names.contains(&"notation"),
            "a bare string has no scheme, so it is not a notation key"
        );
        let osm = r.suggest("osm:amenity=cafe");
        assert!(
            osm.iter()
                .any(|id| id.keyed_type().name == "notation" && id.key() == "osm:amenity=cafe"),
            "a scheme-prefixed string is (0048 §2)"
        );
        // Dots are separators to the keyword normalizer; ranking the suggestions is 0014's.
        let kw = s
            .iter()
            .find(|id| id.keyed_type().name == "keyword")
            .unwrap();
        assert_eq!(kw.key(), "en-wikipedia-org");
        assert!(r.suggest("2024").is_empty());
    }

    #[test]
    fn rejects_bad_registries() {
        let one = |extra: &str| {
            format!(
                "version = 1\n[[keyed_type]]\nname = \"domain\"\nnamespace = \"Domain\"\ndata_type = \"wikibase-domain\"\niri = \"https://x/{{key}}\"\nid_prefix = \"domain:\"\ndisplay = \"derived\"\n{extra}"
            )
        };
        assert!(KeyedRegistry::parse(&one("")).is_ok());
        assert!(matches!(
            KeyedRegistry::parse(
                &one("").replace("id_prefix = \"domain:\"", "id_prefix = \"Domain:\"")
            ),
            Err(KeyedTypeError::BadPrefix { .. })
        ));
        assert!(matches!(
            KeyedRegistry::parse(&one("").replace("{key}", "{k}")),
            Err(KeyedTypeError::BadIriTemplate { .. })
        ));
        assert!(matches!(
            KeyedRegistry::parse(&one("").replace("\"domain\"", "\"isbn\"")),
            Err(KeyedTypeError::NoNormalizer(_))
        ));
        assert!(matches!(
            KeyedRegistry::parse(&one("").replace("derived", "computed")),
            Err(KeyedTypeError::BadDisplay { .. })
        ));
        let two = one("")
            + &one("")
                .replace("version = 1\n", "")
                .replace("domain:", "keyword:");
        assert!(matches!(
            KeyedRegistry::parse(&two),
            Err(KeyedTypeError::Duplicate { what: "name", .. })
        ));
    }
}
