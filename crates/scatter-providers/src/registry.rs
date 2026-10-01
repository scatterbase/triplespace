//! Parsing and validating `providers.toml` (ADR 0015 §5).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde::Deserialize;

use crate::DEFAULT_TOML;
use crate::grammar::{IdGrammar, UnknownGrammar};

/// How a provider that publishes a transparency-log checkpoint is read (0022 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Trust {
    /// Every mirrored batch is checked against the provider's checkpoint and key chain.
    /// The default for a Triplespace provider.
    #[default]
    Verified,
    /// Mirrored without proofs, like any other source.
    Stream,
}

/// One entity type a provider mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityType {
    /// The one-letter type code; the entity ID prefix is the provider code followed by it.
    pub code: char,
    /// The Wikibase entity type of the mirrored entity (`item`, `property`, `lexeme`, …).
    pub entity_type: String,
    /// What the provider's own IDs start with, stripped and re-added when rewriting.
    pub upstream_prefix: String,
    /// The canonical concept-IRI template; `{upstream_id}` is the provider's own ID.
    pub iri: String,
    /// The Triplespace namespace whose pages host the type, if one is registered.
    pub namespace: Option<String>,
    /// A display label for the type (`Work`, `Author`, …).
    pub label: Option<String>,
    /// The type's ID grammar: its own, or the provider's, or `digits`.
    pub id_grammar: IdGrammar,
    /// Upstream entities of this type are written under a keyed type's keys (0009 §9),
    /// never under this prefix. The code stays reserved.
    pub key_mapped: bool,
}

impl EntityType {
    /// The concept IRI for an upstream ID of this type, as a string.
    #[must_use]
    pub fn iri_for(&self, upstream_id: &str) -> String {
        self.iri.replace("{upstream_id}", upstream_id)
    }
}

/// A provider registry entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    /// The two-letter code that prefixes entity IDs minted by the provider.
    pub code: String,
    /// The lower-case name used in graph names and the API.
    pub slug: String,
    /// The display name.
    pub name: String,
    /// The provider number for the revision-ID range (0015 §2). Never 0.
    pub number: u32,
    /// The issuer that the provider's revisions attribute actors to (0007 §1, §6).
    pub issuer: String,
    /// Whether the provider publishes revision IDs.
    pub revision_ids: bool,
    /// The article path of the provider's wiki, where it has one.
    pub article_path: Option<String>,
    /// The provider's API endpoint.
    pub api: Option<String>,
    /// The provider's `Special:EntityData/` base, where it has one.
    pub entity_data: Option<String>,
    /// The crate that implements the provider's adapter (0002 §8.4).
    pub adapter: Option<String>,
    /// The provider as a `prov:Organization` (0007 §6).
    pub agent_iri: Option<String>,
    /// The provider-level ID grammar, applied to types that name none.
    pub id_grammar: IdGrammar,
    /// Trust mode for a provider that publishes a checkpoint (0022 §2).
    pub trust: Option<Trust>,
    /// The provider's key-chain URL, required when `trust` is `verified`.
    pub keys: Option<String>,
    /// A retired entry: its code and number are never reused, and it mints nothing new.
    pub retired: bool,
    /// Adapter configuration, opaque to this crate (0009 §9: the identity property of a
    /// key-mapped provider, for example).
    pub adapter_config: Option<toml::Table>,
    /// The entity types the provider mints, in registry order.
    pub types: Vec<EntityType>,
}

impl Provider {
    /// The entity type with the given one-letter code.
    #[must_use]
    pub fn entity_type(&self, code: char) -> Option<&EntityType> {
        let code = code.to_ascii_uppercase();
        self.types.iter().find(|t| t.code == code)
    }

    /// The graph name of the provider's mirror partition, `mirror/{slug}` (0015 §5).
    #[must_use]
    pub fn mirror_graph(&self) -> String {
        format!("mirror/{}", self.slug)
    }
}

/// The parsed and validated provider registry.
#[derive(Debug, Clone, PartialEq)]
pub struct Registry {
    version: u32,
    reserved_codes: BTreeSet<String>,
    providers: Vec<Provider>,
    by_code: BTreeMap<String, usize>,
    by_slug: BTreeMap<String, usize>,
    by_number: BTreeMap<u32, usize>,
}

/// Why a registry file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The TOML did not parse or did not have the expected shape.
    #[error("providers.toml: {0}")]
    Toml(String),
    /// A provider entry is missing a field an active entry needs.
    #[error("provider `{slug}` is missing `{field}`")]
    Missing {
        /// The provider's slug.
        slug: String,
        /// The missing field.
        field: &'static str,
    },
    /// A provider code is not two uppercase ASCII letters.
    #[error("provider code `{0}` is not two uppercase ASCII letters")]
    BadCode(String),
    /// A provider takes a code listed in `reserved_codes` (0044 §2).
    #[error("provider `{slug}` takes the reserved code `{code}`")]
    ReservedCode {
        /// The provider's slug.
        slug: String,
        /// The reserved code.
        code: String,
    },
    /// A slug is not lower-case ASCII letters, digits and hyphens.
    #[error("provider slug `{0}` is not lower-case ASCII letters, digits and hyphens")]
    BadSlug(String),
    /// Provider number 0 is the instance and is never assigned (0015 §2).
    #[error("provider `{0}` has number 0, which is the instance itself")]
    NumberZero(String),
    /// Two entries share a code, slug or number.
    #[error("providers `{a}` and `{b}` share the {what} `{value}`")]
    Duplicate {
        /// The first provider's slug.
        a: String,
        /// The second provider's slug.
        b: String,
        /// `code`, `slug` or `number`.
        what: &'static str,
        /// The shared value.
        value: String,
    },
    /// A type code is not one uppercase ASCII letter.
    #[error("provider `{slug}`: type code `{code}` is not one uppercase ASCII letter")]
    BadTypeCode {
        /// The provider's slug.
        slug: String,
        /// The offending code.
        code: String,
    },
    /// Two types of one provider share a code.
    #[error("provider `{slug}` declares type code `{code}` twice")]
    DuplicateType {
        /// The provider's slug.
        slug: String,
        /// The shared code.
        code: char,
    },
    /// A type's IRI template does not contain `{upstream_id}`.
    #[error("provider `{slug}`, type `{code}`: IRI template `{iri}` has no `{{upstream_id}}`")]
    BadIriTemplate {
        /// The provider's slug.
        slug: String,
        /// The type code.
        code: char,
        /// The template.
        iri: String,
    },
    /// An unknown ID grammar name.
    #[error("provider `{slug}`: {source}")]
    Grammar {
        /// The provider's slug.
        slug: String,
        /// The unknown name.
        #[source]
        source: UnknownGrammar,
    },
    /// `trust = "verified"` without a `keys` URL.
    #[error("provider `{0}` is `trust = \"verified\"` but has no `keys` URL")]
    VerifiedWithoutKeys(String),
    /// The file's `version` is one this crate does not read.
    #[error("providers.toml version {0} is not supported (this crate reads version 1)")]
    Version(u32),
}

// ---------------------------------------------------------------------------------------
// Raw TOML shape
// ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default)]
    reserved_codes: Vec<String>,
    #[serde(default, rename = "provider")]
    providers: Vec<RawProvider>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    code: Option<String>,
    slug: String,
    name: Option<String>,
    number: Option<u32>,
    issuer: Option<String>,
    #[serde(default)]
    revision_ids: bool,
    article_path: Option<String>,
    api: Option<String>,
    entity_data: Option<String>,
    adapter: Option<String>,
    agent_iri: Option<String>,
    id_grammar: Option<String>,
    trust: Option<String>,
    keys: Option<String>,
    #[serde(default)]
    pending: bool,
    #[serde(default)]
    retired: bool,
    adapter_config: Option<toml::Table>,
    #[serde(default, rename = "type")]
    types: Vec<RawType>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawType {
    code: String,
    entity_type: String,
    #[serde(default)]
    upstream_prefix: String,
    iri: String,
    namespace: Option<String>,
    label: Option<String>,
    id_grammar: Option<String>,
    #[serde(default)]
    key_mapped: bool,
}

impl RawProvider {
    /// Validates one raw entry; `None` for a pending entry.
    fn validate(self) -> Result<Option<Provider>, RegistryError> {
        let p = self;
        if p.pending {
            return Ok(None);
        }
        let slug = p.slug;
        if !is_slug(&slug) {
            return Err(RegistryError::BadSlug(slug));
        }
        let missing = |field| RegistryError::Missing {
            slug: slug.clone(),
            field,
        };
        let code = p.code.ok_or_else(|| missing("code"))?;
        if !is_code(&code) {
            return Err(RegistryError::BadCode(code));
        }
        let number = p.number.ok_or_else(|| missing("number"))?;
        if number == 0 {
            return Err(RegistryError::NumberZero(slug));
        }
        let issuer = p.issuer.unwrap_or_else(|| slug.clone());
        let grammar_err = |source| RegistryError::Grammar {
            slug: slug.clone(),
            source,
        };
        let id_grammar = match p.id_grammar {
            Some(g) => g.parse().map_err(grammar_err)?,
            None => IdGrammar::Digits,
        };
        let trust = match p.trust.as_deref() {
            None => None,
            Some("verified") => Some(Trust::Verified),
            Some("stream") => Some(Trust::Stream),
            Some(other) => {
                return Err(RegistryError::Toml(format!(
                    "provider `{slug}`: trust must be \"verified\" or \"stream\", not \"{other}\""
                )));
            }
        };
        if trust == Some(Trust::Verified) && p.keys.is_none() {
            return Err(RegistryError::VerifiedWithoutKeys(slug));
        }

        let mut types = Vec::with_capacity(p.types.len());
        for t in p.types {
            let mut chars = t.code.chars();
            let code_char = match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_uppercase() => c,
                _ => {
                    return Err(RegistryError::BadTypeCode { slug, code: t.code });
                }
            };
            if types.iter().any(|e: &EntityType| e.code == code_char) {
                return Err(RegistryError::DuplicateType {
                    slug,
                    code: code_char,
                });
            }
            if !t.iri.contains("{upstream_id}") {
                return Err(RegistryError::BadIriTemplate {
                    slug,
                    code: code_char,
                    iri: t.iri,
                });
            }
            let type_grammar = match t.id_grammar {
                Some(g) => g.parse().map_err(grammar_err)?,
                None => id_grammar,
            };
            types.push(EntityType {
                code: code_char,
                entity_type: t.entity_type,
                upstream_prefix: t.upstream_prefix,
                iri: t.iri,
                namespace: t.namespace,
                label: t.label,
                id_grammar: type_grammar,
                key_mapped: t.key_mapped,
            });
        }

        Ok(Some(Provider {
            code,
            name: p.name.unwrap_or_else(|| slug.clone()),
            slug,
            number,
            issuer,
            revision_ids: p.revision_ids,
            article_path: p.article_path,
            api: p.api,
            entity_data: p.entity_data,
            adapter: p.adapter,
            agent_iri: p.agent_iri,
            id_grammar,
            trust,
            keys: p.keys,
            retired: p.retired,
            adapter_config: p.adapter_config,
            types,
        }))
    }
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

fn is_code(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
}

impl Registry {
    /// Parses and validates a registry file in the format of `docs/registry/providers.toml`.
    ///
    /// Entries marked `pending = true` have no code or number yet and are left out; the
    /// registry README allocates them by commit. Entries marked `retired = true` are kept,
    /// so that their codes and numbers stay reserved and old data still parses.
    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| RegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(RegistryError::Version(raw.version));
        }

        let mut reserved_codes = BTreeSet::new();
        for code in raw.reserved_codes {
            if !is_code(&code) {
                return Err(RegistryError::BadCode(code));
            }
            reserved_codes.insert(code);
        }

        let mut providers = Vec::with_capacity(raw.providers.len());
        for p in raw.providers {
            if let Some(provider) = p.validate()? {
                // Retired entries too: a reserved code was never allocated.
                if reserved_codes.contains(&provider.code) {
                    return Err(RegistryError::ReservedCode {
                        slug: provider.slug,
                        code: provider.code,
                    });
                }
                providers.push(provider);
            }
        }

        let mut by_code = BTreeMap::new();
        let mut by_slug = BTreeMap::new();
        let mut by_number = BTreeMap::new();
        for (i, p) in providers.iter().enumerate() {
            let dup = |what, value: String, j: usize| RegistryError::Duplicate {
                a: providers[j].slug.clone(),
                b: p.slug.clone(),
                what,
                value,
            };
            if let Some(j) = by_code.insert(p.code.clone(), i) {
                return Err(dup("code", p.code.clone(), j));
            }
            if let Some(j) = by_slug.insert(p.slug.clone(), i) {
                return Err(dup("slug", p.slug.clone(), j));
            }
            if let Some(j) = by_number.insert(p.number, i) {
                return Err(dup("number", p.number.to_string(), j));
            }
        }

        Ok(Self {
            version: raw.version,
            reserved_codes,
            providers,
            by_code,
            by_slug,
            by_number,
        })
    }

    /// The embedded default registry, `docs/registry/providers.toml`, parsed once.
    ///
    /// # Panics
    ///
    /// Never in a build that passed its tests: the default file is validated by a test.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: LazyLock<Registry> = LazyLock::new(|| {
            Registry::parse(DEFAULT_TOML).expect("docs/registry/providers.toml is valid")
        });
        &DEFAULT
    }

    /// The file format version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The provider codes that are never allocated (0044 §2), in sorted order.
    pub fn reserved_codes(&self) -> impl Iterator<Item = &str> {
        self.reserved_codes.iter().map(String::as_str)
    }

    /// Whether this two-letter code is reserved. Case-insensitive.
    #[must_use]
    pub fn is_reserved(&self, code: &str) -> bool {
        self.reserved_codes.contains(&code.to_ascii_uppercase())
    }

    /// Every provider, in registry order, retired ones included.
    #[must_use]
    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }

    /// The provider with this two-letter code. Case-insensitive.
    #[must_use]
    pub fn by_code(&self, code: &str) -> Option<&Provider> {
        let code = code.to_ascii_uppercase();
        self.by_code.get(&code).map(|&i| &self.providers[i])
    }

    /// The provider with this slug.
    #[must_use]
    pub fn by_slug(&self, slug: &str) -> Option<&Provider> {
        self.by_slug.get(slug).map(|&i| &self.providers[i])
    }

    /// The provider with this number. Number 0 is the instance and has no entry.
    #[must_use]
    pub fn by_number(&self, number: u32) -> Option<&Provider> {
        self.by_number.get(&number).map(|&i| &self.providers[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_parses_and_is_consistent() {
        let r = Registry::default_registry();
        assert_eq!(r.version(), 1);
        // The allocations the ADRs record (0000 §3, 0015 §5, 0017 §6, 0036, 0037).
        for (code, slug, number) in [
            ("WD", "wikidata", 1),
            ("LB", "librarybase", 2),
            ("OA", "openalex", 3),
            ("MB", "musicbrainz", 4),
            ("XD", "internetdomains", 5),
            ("OS", "openstreetmap", 6),
            ("OW", "osmwiki", 7),
            ("GD", "gdelt", 8),
        ] {
            let p = r
                .by_code(code)
                .unwrap_or_else(|| panic!("{code} registered"));
            assert_eq!(p.slug, slug);
            assert_eq!(p.number, number);
            assert!(std::ptr::eq(r.by_slug(slug).unwrap(), p));
            assert!(std::ptr::eq(r.by_number(number).unwrap(), p));
            assert!(!p.types.is_empty(), "{code} mints something");
        }
        assert!(r.by_number(0).is_none());
    }

    #[test]
    fn default_registry_reserves_every_doubled_code() {
        // 0044 §2: `QQQ5` is the tenant-relative form, so no doubled letter is a provider code.
        let r = Registry::default_registry();
        let doubled: Vec<String> = ('A'..='Z').map(|c| format!("{c}{c}")).collect();
        assert_eq!(r.reserved_codes().collect::<Vec<_>>(), doubled);
        assert!(r.is_reserved("qq"));
        assert!(!r.is_reserved("WD"));
        assert!(r.providers().iter().all(|p| !r.is_reserved(&p.code)));
    }

    #[test]
    fn grammars_and_key_mapping_from_the_default_file() {
        let r = Registry::default_registry();
        let mb = r.by_code("MB").unwrap();
        assert_eq!(mb.id_grammar, IdGrammar::Uuid);
        assert!(mb.types.iter().all(|t| t.id_grammar == IdGrammar::Uuid));
        let gd = r.by_code("GD").unwrap();
        assert_eq!(gd.entity_type('E').unwrap().id_grammar, IdGrammar::Digits);
        assert_eq!(
            gd.entity_type('D').unwrap().id_grammar,
            IdGrammar::GdeltRecord
        );
        assert_eq!(gd.entity_type('T').unwrap().id_grammar, IdGrammar::Token);
        let oa = r.by_code("OA").unwrap();
        assert!(oa.entity_type('K').unwrap().key_mapped);
        assert!(!oa.entity_type('W').unwrap().key_mapped);
        let xd = r.by_code("XD").unwrap();
        let cfg = xd.adapter_config.as_ref().expect("XD has adapter_config");
        assert_eq!(
            cfg.get("identity_property").and_then(|v| v.as_str()),
            Some("P1")
        );
        assert_eq!(xd.mirror_graph(), "mirror/internetdomains");
    }

    fn one(extra: &str) -> String {
        format!(
            "version = 1\n[[provider]]\ncode = \"ZZ\"\nslug = \"zed\"\nnumber = 99\n{extra}\n[[provider.type]]\ncode = \"Q\"\nentity_type = \"item\"\nupstream_prefix = \"Q\"\niri = \"https://zed.example/entity/{{upstream_id}}\"\n"
        )
    }

    #[test]
    fn rejects_bad_entries() {
        assert!(matches!(
            Registry::parse(&one("").replace("code = \"ZZ\"", "code = \"zz\"")),
            Err(RegistryError::BadCode(_))
        ));
        assert!(matches!(
            Registry::parse(&one("").replace("number = 99", "number = 0")),
            Err(RegistryError::NumberZero(_))
        ));
        assert!(matches!(
            Registry::parse(&one("").replace("{upstream_id}", "{id}")),
            Err(RegistryError::BadIriTemplate { .. })
        ));
        assert!(matches!(
            Registry::parse(&one("id_grammar = \"slug\"")),
            Err(RegistryError::Grammar { .. })
        ));
        assert!(matches!(
            Registry::parse(&one("trust = \"verified\"")),
            Err(RegistryError::VerifiedWithoutKeys(_))
        ));
        assert!(matches!(
            Registry::parse(&one("unknown_field = 1")),
            Err(RegistryError::Toml(_))
        ));
        assert!(matches!(
            Registry::parse("version = 2\n"),
            Err(RegistryError::Version(2))
        ));
    }

    #[test]
    fn rejects_reserved_codes() {
        let reserving = |codes: &str| {
            one("").replace(
                "version = 1\n",
                &format!("version = 1\nreserved_codes = [{codes}]\n"),
            )
        };
        assert!(Registry::parse(&reserving("\"QQ\"")).is_ok());
        assert!(matches!(
            Registry::parse(&reserving("\"QQ\", \"ZZ\"")),
            Err(RegistryError::ReservedCode { ref code, .. }) if code == "ZZ"
        ));
        assert!(matches!(
            Registry::parse(
                &reserving("\"ZZ\"").replace("number = 99", "number = 99\nretired = true")
            ),
            Err(RegistryError::ReservedCode { .. })
        ));
        assert!(matches!(
            Registry::parse(&reserving("\"qq\"")),
            Err(RegistryError::BadCode(_))
        ));
    }

    #[test]
    fn rejects_duplicates() {
        let two = one("")
            + &one("")
                .replace("version = 1\n", "")
                .replace("zed", "zed2")
                .replace("99", "98");
        assert!(matches!(
            Registry::parse(&two),
            Err(RegistryError::Duplicate { what: "code", .. })
        ));
    }

    #[test]
    fn pending_entries_are_skipped_and_defaults_apply() {
        let text = "version = 1\n[[provider]]\nslug = \"later\"\npending = true\n".to_string()
            + &one("").replace("version = 1\n", "");
        let r = Registry::parse(&text).unwrap();
        assert!(r.by_slug("later").is_none());
        let z = r.by_slug("zed").unwrap();
        assert_eq!(z.issuer, "zed", "issuer defaults to the slug");
        assert_eq!(z.name, "zed");
        assert_eq!(z.id_grammar, IdGrammar::Digits);
        assert!(!z.revision_ids);
    }
}
