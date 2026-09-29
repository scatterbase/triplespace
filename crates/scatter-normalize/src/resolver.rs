//! Resolver namespaces (ADR 0029 §1): the registry embedded from
//! `docs/registry/resolvers.toml`, and the normalizers of its defaults.
//!
//! A resolver is a namespace whose titles are keys looked up against a property bound to a
//! role, or against sitelinks. Its normalizer **is** the normalizer of every property
//! bound to its role (0004 §7), so a property a resolver casefolds compares its values
//! case-insensitively everywhere, in fusion as in lookup. As with keyed types, the
//! grammar and normalizer are implemented by name; a default whose name has no
//! implementation is rejected at parse time.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::RESOLVERS_TOML;
use crate::url::{DEFAULT_SCHEMES, normalize_url};

/// The case rule of a resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Case {
    /// Case-insensitive: the key is casefolded.
    Fold,
    /// Case-sensitive.
    Keep,
}

/// One resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolver {
    /// The resolver's name (`doi`); one namespace with keyed-type names.
    pub name: String,
    /// The display name ("DOI").
    pub label: String,
    /// The MediaWiki namespace whose titles are keys.
    pub namespace: String,
    /// The role the tenant binds to its properties, or `sitelink`.
    pub binding: String,
    /// The case rule.
    pub case: Case,
    /// Where the key resolves outside the instance; `{key}` is the canonical key.
    pub external: String,
    /// Other titles that resolve here (`Special:ItemByTitle`).
    pub aliases: Vec<String>,
}

/// Why a string is not a key of a resolver.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ResolverError {
    /// The input is not valid under the resolver's grammar after normalization.
    #[error("`{input}` is not a valid {resolver} key: {reason}")]
    Invalid {
        /// The resolver's name.
        resolver: String,
        /// The input.
        input: String,
        /// Why.
        reason: String,
    },
    /// The registry names a resolver this crate has no normalizer for.
    #[error("no normalizer is implemented for resolver `{0}`")]
    NoNormalizer(String),
}

impl Resolver {
    /// Normalizes `input` to a key of this resolver.
    ///
    /// ```
    /// use scatter_normalize::ResolverRegistry;
    /// let doi = ResolverRegistry::default_registry().by_name("doi").unwrap();
    /// assert_eq!(doi.normalize("https://doi.org/10.1000/ABC%2Fx").unwrap(), "10.1000/abc/x");
    /// assert_eq!(doi.normalize(" doi:10.1000/xyz ").unwrap(), "10.1000/xyz");
    /// assert!(doi.normalize("11.1000/xyz").is_err());
    /// ```
    pub fn normalize(&self, input: &str) -> Result<String, ResolverError> {
        let invalid = |reason: String| ResolverError::Invalid {
            resolver: self.name.clone(),
            input: input.to_string(),
            reason,
        };
        let key = match self.name.as_str() {
            "doi" => doi(input).map_err(|e| invalid(e.to_string()))?,
            "url" => {
                normalize_url(input.trim(), DEFAULT_SCHEMES).map_err(|e| invalid(e.to_string()))?
            }
            other => return Err(ResolverError::NoNormalizer(other.to_string())),
        };
        Ok(match self.case {
            Case::Fold => crate::casefold(&key),
            Case::Keep => key,
        })
    }

    /// The external IRI for a canonical key.
    #[must_use]
    pub fn external_iri(&self, key: &str) -> String {
        self.external.replace("{key}", key)
    }
}

/// Why a DOI is not a DOI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DoiError {
    /// Percent-decoding produced invalid UTF-8.
    #[error("percent-decoding is not UTF-8")]
    Utf8,
    /// Not `10.` + 4–9 digits + `/` + something.
    #[error("not of the form 10.NNNN/suffix")]
    Grammar,
}

/// The DOI normalizer of 0029 §1, before the case rule: strip a leading `https://doi.org/`,
/// `http://dx.doi.org/`, `doi.org/` or `doi:`; percent-decode; trim.
pub fn doi(input: &str) -> Result<String, DoiError> {
    const PREFIXES: &[&str] = &[
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi.org/",
        "dx.doi.org/",
        "doi:",
    ];
    let mut s = input.trim();
    for p in PREFIXES {
        if s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p) {
            s = &s[p.len()..];
            break;
        }
    }
    let decoded = percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .map_err(|_| DoiError::Utf8)?;
    let key = decoded.trim().to_string();
    if !is_doi(&key) {
        return Err(DoiError::Grammar);
    }
    Ok(key)
}

/// The DOI grammar `10\.[0-9]{4,9}/.+`.
#[must_use]
pub fn is_doi(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("10.") else {
        return false;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    (4..=9).contains(&digits) && rest[digits..].starts_with('/') && rest.len() > digits + 1
}

/// The parsed and validated resolver registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverRegistry {
    version: u32,
    resolvers: Vec<Resolver>,
    by_name: BTreeMap<String, usize>,
}

/// Why a registry file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ResolverRegistryError {
    /// The TOML did not parse or had the wrong shape.
    #[error("resolvers.toml: {0}")]
    Toml(String),
    /// The file's `version` is one this crate does not read.
    #[error("resolvers.toml version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// Two resolvers share a name.
    #[error("two resolvers are named `{0}`")]
    Duplicate(String),
    /// `case` is neither `fold` nor `keep`.
    #[error("resolver `{name}`: case must be \"fold\" or \"keep\", not \"{case}\"")]
    BadCase {
        /// The resolver.
        name: String,
        /// The value.
        case: String,
    },
    /// The external template has no `{key}`.
    #[error("resolver `{name}`: external `{external}` has no `{{key}}`")]
    BadExternal {
        /// The resolver.
        name: String,
        /// The template.
        external: String,
    },
    /// No normalizer is implemented for the resolver.
    #[error("resolver `{0}` has no normalizer in scatter-normalize")]
    NoNormalizer(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default, rename = "resolver")]
    resolvers: Vec<RawResolver>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResolver {
    name: String,
    label: Option<String>,
    namespace: String,
    binding: String,
    case: String,
    external: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[allow(dead_code)]
    grammar: Option<String>,
    #[allow(dead_code)]
    normalizer: Option<String>,
}

impl ResolverRegistry {
    /// Parses and validates a registry in the format of `docs/registry/resolvers.toml`.
    pub fn parse(text: &str) -> Result<Self, ResolverRegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| ResolverRegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(ResolverRegistryError::Version(raw.version));
        }
        let mut resolvers = Vec::with_capacity(raw.resolvers.len());
        let mut by_name = BTreeMap::new();
        for r in raw.resolvers {
            let name = r.name;
            let case = match r.case.as_str() {
                "fold" => Case::Fold,
                "keep" => Case::Keep,
                _ => {
                    return Err(ResolverRegistryError::BadCase { name, case: r.case });
                }
            };
            if !r.external.contains("{key}") {
                return Err(ResolverRegistryError::BadExternal {
                    name,
                    external: r.external,
                });
            }
            if !matches!(name.as_str(), "doi" | "url") {
                return Err(ResolverRegistryError::NoNormalizer(name));
            }
            if by_name.insert(name.clone(), resolvers.len()).is_some() {
                return Err(ResolverRegistryError::Duplicate(name));
            }
            resolvers.push(Resolver {
                label: r.label.unwrap_or_else(|| name.clone()),
                name,
                namespace: r.namespace,
                binding: r.binding,
                case,
                external: r.external,
                aliases: r.aliases,
            });
        }
        Ok(Self {
            version: raw.version,
            resolvers,
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
        static DEFAULT: LazyLock<ResolverRegistry> = LazyLock::new(|| {
            ResolverRegistry::parse(RESOLVERS_TOML).expect("docs/registry/resolvers.toml is valid")
        });
        &DEFAULT
    }

    /// The file format version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every resolver, in registry order.
    #[must_use]
    pub fn resolvers(&self) -> &[Resolver] {
        &self.resolvers
    }

    /// The resolver with this name.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&Resolver> {
        self.by_name.get(name).map(|&i| &self.resolvers[i])
    }

    /// The resolver bound to this role (`doi`) or to `sitelink`.
    #[must_use]
    pub fn by_binding(&self, binding: &str) -> Option<&Resolver> {
        self.resolvers.iter().find(|r| r.binding == binding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry() {
        let r = ResolverRegistry::default_registry();
        assert_eq!(r.version(), 1);
        let doi = r.by_name("doi").unwrap();
        assert_eq!(doi.case, Case::Fold);
        assert_eq!(doi.binding, "doi");
        assert_eq!(doi.namespace, "DOI");
        assert_eq!(doi.external_iri("10.1000/x"), "https://doi.org/10.1000/x");
        let url = r.by_binding("sitelink").unwrap();
        assert_eq!(url.name, "url");
        assert_eq!(url.case, Case::Keep);
        assert!(url.aliases.contains(&"Special:ItemByTitle".to_string()));
        assert_eq!(
            r.resolvers().len(),
            2,
            "drafted candidates are commented out"
        );
    }

    #[test]
    fn doi_normalizer() {
        let d = ResolverRegistry::default_registry().by_name("doi").unwrap();
        for input in [
            "10.1000/XYZ",
            "https://doi.org/10.1000/xyz",
            "HTTP://DX.DOI.ORG/10.1000/xyz",
            "doi:10.1000/XyZ",
            "  10.1000/xyz\n",
            "10.1000/%58yz", // %58 is X
        ] {
            assert_eq!(d.normalize(input).unwrap(), "10.1000/xyz", "{input}");
        }
        assert!(matches!(
            d.normalize("10.100/xyz"),
            Err(ResolverError::Invalid { .. })
        ));
        assert!(matches!(
            d.normalize("10.1000/"),
            Err(ResolverError::Invalid { .. })
        ));
        assert!(matches!(
            d.normalize("10.1000"),
            Err(ResolverError::Invalid { .. })
        ));
        assert!(matches!(
            d.normalize("10.1000/%ff"),
            Err(ResolverError::Invalid { .. })
        ));
        assert!(is_doi("10.123456789/a"));
        assert!(!is_doi("10.1234567890/a"));
    }

    #[test]
    fn url_resolver_keeps_case() {
        let u = ResolverRegistry::default_registry().by_name("url").unwrap();
        assert_eq!(
            u.normalize(" HTTPS://En.Wikipedia.org/wiki/Douglas_Adams ")
                .unwrap(),
            "https://en.wikipedia.org/wiki/Douglas_Adams"
        );
        assert_eq!(u.external_iri("https://x.example/"), "https://x.example/");
    }

    #[test]
    fn rejects_bad_registries() {
        let one = |case: &str, name: &str| {
            format!(
                "version = 1\n[[resolver]]\nname = \"{name}\"\nnamespace = \"DOI\"\nbinding = \"doi\"\ncase = \"{case}\"\nexternal = \"https://doi.org/{{key}}\"\n"
            )
        };
        assert!(ResolverRegistry::parse(&one("fold", "doi")).is_ok());
        assert!(matches!(
            ResolverRegistry::parse(&one("lower", "doi")),
            Err(ResolverRegistryError::BadCase { .. })
        ));
        assert!(matches!(
            ResolverRegistry::parse(&one("fold", "orcid")),
            Err(ResolverRegistryError::NoNormalizer(_))
        ));
        let two = one("fold", "doi") + &one("fold", "doi").replace("version = 1\n", "");
        assert!(matches!(
            ResolverRegistry::parse(&two),
            Err(ResolverRegistryError::Duplicate(_))
        ));
    }
}
