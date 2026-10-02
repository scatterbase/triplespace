//! The issuer registry (0007 §1): who assigns actors.
//!
//! An actor is namespaced by its **issuer**, the authority that assigns the account, as an
//! entity is namespaced by its provider. The registry of record is
//! `docs/registry/issuers.toml`; this crate embeds it as [`crate::ISSUERS_TOML`]. Two
//! entries are **templates**: `{tenant}`, which every tenant of the instance follows
//! under its own slug (0018 §4), and `{farm}`, which the farm follows where the tenancy
//! policy gives it identity (0028 §2). [`IssuerRegistry::for_tenant`] and
//! [`IssuerRegistry::for_farm`] instantiate them.

use serde::Deserialize;

/// Which kind of subject an issuer's actor keys carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorModel {
    /// Subjects are the issuer's own numeric user IDs.
    Numeric,
    /// The issuer has no individual actors: changes are attributed to the provider as a
    /// whole (0007 §6), or, for `instance`, to the operator actor (0040 §2).
    ProviderOnly,
}

/// One issuer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issuer {
    /// The code: a tenant slug, `wikidatawiki`, `password`, `instance`, …
    pub code: String,
    /// The display name.
    pub name: String,
    /// The actor model.
    pub actor_model: ActorModel,
    /// The actor IRI template (0007 §2), with `{id}` for the subject, and `{tenant base}`
    /// or `{farm base}` for the templates. Absent where IDs are never published.
    pub iri: Option<String>,
    /// The API that resolves names and central IDs, where there is one.
    pub users_api: Option<String>,
    /// Whether the issuer may serve as an identity provider for login (0010 §10).
    pub login: bool,
    /// Provider slugs whose adapters attribute actors to this issuer.
    pub providers: Vec<String>,
    /// Operated by the instance itself: `password` and `instance`.
    pub builtin: bool,
    /// A template entry, instantiated per tenant or farm; never an issuer by itself.
    pub template: bool,
}

/// Why `issuers.toml` could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IssuerRegistryError {
    /// The TOML did not parse or did not have the expected shape.
    #[error("issuers.toml: {0}")]
    Toml(String),
    /// A registry version this crate does not know.
    #[error("issuers.toml: version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// A code is not lower-case ASCII letters, digits and hyphens.
    #[error("issuer code `{0}` is not lower-case ASCII letters, digits and hyphens")]
    BadCode(String),
    /// Two entries share a code.
    #[error("issuer code `{0}` is declared twice")]
    Duplicate(String),
    /// A numeric issuer's IRI template does not contain `{id}`.
    #[error("issuer `{0}`: IRI template has no `{{id}}`")]
    BadIriTemplate(String),
    /// A template entry is missing, or an entry other than `{tenant}` and `{farm}` claims
    /// to be one.
    #[error("issuers.toml: {0}")]
    Template(String),
    /// A builtin the crate relies on is missing or has the wrong shape.
    #[error("issuers.toml: builtin issuer `{0}` {1}")]
    Builtin(&'static str, &'static str),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default, rename = "issuer")]
    issuers: Vec<RawIssuer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIssuer {
    code: String,
    name: String,
    actor_model: String,
    #[serde(default)]
    iri: Option<String>,
    #[serde(default)]
    users_api: Option<String>,
    #[serde(default)]
    login: bool,
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    builtin: bool,
    #[serde(default)]
    template: bool,
}

/// The template code every tenant issuer is instantiated from.
pub const TENANT_TEMPLATE: &str = "{tenant}";
/// The template code the farm issuer is instantiated from.
pub const FARM_TEMPLATE: &str = "{farm}";
/// The built-in password issuer (0007 §3).
pub const PASSWORD: &str = "password";
/// The instance as operator (0040 §2).
pub const INSTANCE: &str = "instance";
/// Reads as the current tenant's issuer (0018 §4); never a registered code.
pub const LOCAL: &str = "local";

/// Codes no tenant, provider or issuer may take (0018 §1, 0040 §2).
pub const RESERVED_CODES: [&str; 3] = [INSTANCE, PASSWORD, LOCAL];

/// The issuer registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuerRegistry {
    version: u32,
    issuers: Vec<Issuer>,
}

fn is_code(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

impl IssuerRegistry {
    /// Parses an `issuers.toml`.
    pub fn parse(text: &str) -> Result<Self, IssuerRegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| IssuerRegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(IssuerRegistryError::Version(raw.version));
        }
        let mut issuers = Vec::with_capacity(raw.issuers.len());
        for r in raw.issuers {
            let template = r.template;
            if template {
                if r.code != TENANT_TEMPLATE && r.code != FARM_TEMPLATE {
                    return Err(IssuerRegistryError::Template(format!(
                        "`{}` is not a template code",
                        r.code
                    )));
                }
            } else if !is_code(&r.code) {
                return Err(IssuerRegistryError::BadCode(r.code));
            }
            if issuers.iter().any(|i: &Issuer| i.code == r.code) {
                return Err(IssuerRegistryError::Duplicate(r.code));
            }
            let actor_model = match r.actor_model.as_str() {
                "numeric" => ActorModel::Numeric,
                "provider-only" => ActorModel::ProviderOnly,
                other => {
                    return Err(IssuerRegistryError::Toml(format!(
                        "issuer `{}`: actor_model must be \"numeric\" or \"provider-only\", not \"{other}\"",
                        r.code
                    )));
                }
            };
            if actor_model == ActorModel::Numeric
                && let Some(iri) = &r.iri
                && !iri.contains("{id}")
            {
                return Err(IssuerRegistryError::BadIriTemplate(r.code));
            }
            issuers.push(Issuer {
                code: r.code,
                name: r.name,
                actor_model,
                iri: r.iri,
                users_api: r.users_api,
                login: r.login,
                providers: r.providers,
                builtin: r.builtin,
                template,
            });
        }
        let reg = Self {
            version: raw.version,
            issuers,
        };
        for t in [TENANT_TEMPLATE, FARM_TEMPLATE] {
            if reg.find(t).is_none_or(|i| !i.template) {
                return Err(IssuerRegistryError::Template(format!(
                    "the `{t}` template is missing"
                )));
            }
        }
        match reg.find(PASSWORD) {
            None => return Err(IssuerRegistryError::Builtin(PASSWORD, "is missing")),
            Some(p) if !p.builtin || !p.login || p.actor_model != ActorModel::Numeric => {
                return Err(IssuerRegistryError::Builtin(
                    PASSWORD,
                    "must be builtin, numeric and a login provider",
                ));
            }
            Some(_) => {}
        }
        match reg.find(INSTANCE) {
            None => return Err(IssuerRegistryError::Builtin(INSTANCE, "is missing")),
            Some(i) if !i.builtin || i.login || i.actor_model != ActorModel::ProviderOnly => {
                return Err(IssuerRegistryError::Builtin(
                    INSTANCE,
                    "must be builtin, provider-only and never a login provider",
                ));
            }
            Some(_) => {}
        }
        Ok(reg)
    }

    /// The embedded `docs/registry/issuers.toml`.
    ///
    /// # Panics
    ///
    /// If the embedded registry is invalid, which the crate's tests rule out.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: std::sync::OnceLock<IssuerRegistry> = std::sync::OnceLock::new();
        DEFAULT.get_or_init(|| Self::parse(crate::ISSUERS_TOML).expect("embedded issuers.toml"))
    }

    /// The registry version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every entry, templates included, in registry order.
    #[must_use]
    pub fn issuers(&self) -> &[Issuer] {
        &self.issuers
    }

    fn find(&self, code: &str) -> Option<&Issuer> {
        self.issuers.iter().find(|i| i.code == code)
    }

    /// A registered issuer by code. Templates are not issuers; see [`Self::for_tenant`].
    #[must_use]
    pub fn by_code(&self, code: &str) -> Option<&Issuer> {
        self.find(code).filter(|i| !i.template)
    }

    /// Whether a code is reserved and can never be a tenant, provider or issuer code.
    #[must_use]
    pub fn is_reserved(code: &str) -> bool {
        RESERVED_CODES.contains(&code)
    }

    /// The issuer of a tenant: the `{tenant}` template under the tenant's slug, with the
    /// IRI template's `{tenant base}` filled in.
    ///
    /// # Panics
    ///
    /// Never: the constructor checked the template exists.
    #[must_use]
    pub fn for_tenant(&self, slug: &str, tenant_base: &str) -> Issuer {
        self.instantiate(TENANT_TEMPLATE, slug, "{tenant base}", tenant_base)
    }

    /// The farm as an issuer: the `{farm}` template under the farm slug, with `{farm base}`
    /// filled in (0028 §2, 0046 §6–7).
    ///
    /// # Panics
    ///
    /// Never: the constructor checked the template exists.
    #[must_use]
    pub fn for_farm(&self, farm_slug: &str, farm_base: &str) -> Issuer {
        self.instantiate(FARM_TEMPLATE, farm_slug, "{farm base}", farm_base)
    }

    fn instantiate(&self, template: &str, code: &str, placeholder: &str, base: &str) -> Issuer {
        let t = self.find(template).expect("checked at parse");
        Issuer {
            code: code.to_string(),
            name: t.name.clone(),
            actor_model: t.actor_model,
            iri: t
                .iri
                .as_ref()
                .map(|i| i.replace(placeholder, base.trim_end_matches('/'))),
            users_api: t.users_api.clone(),
            login: t.login,
            providers: Vec::new(),
            builtin: false,
            template: false,
        }
    }

    /// The issuers that may serve as identity providers for login, in registry order.
    pub fn login_providers(&self) -> impl Iterator<Item = &Issuer> {
        self.issuers.iter().filter(|i| i.login && !i.template)
    }

    /// The issuer a provider's adapter attributes actors to, if any.
    #[must_use]
    pub fn for_provider(&self, provider_slug: &str) -> Option<&Issuer> {
        self.issuers
            .iter()
            .find(|i| !i.template && i.providers.iter().any(|p| p == provider_slug))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_parses() {
        let r = IssuerRegistry::default_registry();
        assert_eq!(r.version(), 1);
        let wd = r.by_code("wikidatawiki").unwrap();
        assert_eq!(wd.actor_model, ActorModel::Numeric);
        assert_eq!(r.for_provider("wikidata").unwrap().code, "wikidatawiki");
        assert!(r.by_code("{tenant}").is_none(), "templates are not issuers");
        assert!(
            r.by_code("openalex")
                .is_some_and(|i| i.actor_model == ActorModel::ProviderOnly)
        );
        assert!(r.by_code("password").unwrap().builtin);
        assert!(r.by_code("instance").unwrap().builtin);
        assert!(r.by_code("wikimedia-central").unwrap().iri.is_none());
        let logins: Vec<&str> = r.login_providers().map(|i| i.code.as_str()).collect();
        assert!(logins.contains(&"password") && logins.contains(&"wikimedia-central"));
        assert!(!logins.contains(&"instance"));
    }

    #[test]
    fn tenant_and_farm_issuers_come_from_the_templates() {
        let r = IssuerRegistry::default_registry();
        let lb = r.for_tenant("librarybase", "https://librarybase.org/");
        assert_eq!(lb.code, "librarybase");
        assert_eq!(lb.iri.as_deref(), Some("https://librarybase.org/user/{id}"));
        assert!(!lb.login && !lb.template);
        let farm = r.for_farm("scatter", "https://scatter.red");
        assert_eq!(
            farm.iri.as_deref(),
            Some("https://scatter.red/instance/user/{id}")
        );
        assert!(farm.login);
        for c in ["instance", "password", "local"] {
            assert!(IssuerRegistry::is_reserved(c));
        }
        assert!(!IssuerRegistry::is_reserved("librarybase"));
    }

    #[test]
    fn rejects_bad_registries() {
        let base = |extra: &str| format!("{}\n{extra}", crate::ISSUERS_TOML);
        assert!(matches!(
            IssuerRegistry::parse(&base(
                "[[issuer]]\ncode = \"Bad Code\"\nname = \"x\"\nactor_model = \"numeric\""
            )),
            Err(IssuerRegistryError::BadCode(_))
        ));
        assert!(matches!(
            IssuerRegistry::parse(&base(
                "[[issuer]]\ncode = \"wikidatawiki\"\nname = \"x\"\nactor_model = \"numeric\""
            )),
            Err(IssuerRegistryError::Duplicate(_))
        ));
        assert!(matches!(
            IssuerRegistry::parse(&base(
                "[[issuer]]\ncode = \"x\"\nname = \"x\"\nactor_model = \"numeric\"\niri = \"https://x/user\""
            )),
            Err(IssuerRegistryError::BadIriTemplate(_))
        ));
        assert!(matches!(
            IssuerRegistry::parse(&base(
                "[[issuer]]\ncode = \"x\"\nname = \"x\"\nactor_model = \"other\""
            )),
            Err(IssuerRegistryError::Toml(_))
        ));
        assert!(matches!(
            IssuerRegistry::parse(&base(
                "[[issuer]]\ncode = \"{other}\"\nname = \"x\"\nactor_model = \"numeric\"\ntemplate = true"
            )),
            Err(IssuerRegistryError::Template(_))
        ));
        assert!(matches!(
            IssuerRegistry::parse("version = 1\n"),
            Err(IssuerRegistryError::Template(_))
        ));
        assert!(matches!(
            IssuerRegistry::parse("version = 2\n"),
            Err(IssuerRegistryError::Version(2))
        ));
    }
}
