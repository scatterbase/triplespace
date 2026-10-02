//! Actor keys (0007 §1): `{issuer}:{subject}`.
//!
//! An actor key is an identifier and never a name (0006 §3): `wikidatawiki:12345`,
//! `local:42`, `instance:scatter`. It is what log headers, attestations and memberships
//! carry. `local` reads as the current tenant's issuer (0018 §4); a key is written with the
//! tenant's slug wherever it leaves the tenant.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::issuer::{ActorModel, INSTANCE, IssuerRegistry, LOCAL};

/// An actor key.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActorKey {
    issuer: String,
    subject: String,
}

/// Why a string is not an actor key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ActorKeyError {
    /// No colon, or an empty issuer or subject.
    #[error("`{0}` is not an actor key (`{{issuer}}:{{subject}}`)")]
    Shape(String),
    /// The issuer is not lower-case ASCII letters, digits and hyphens.
    #[error("`{0}`: the issuer is not lower-case ASCII letters, digits and hyphens")]
    Issuer(String),
    /// The subject has whitespace or a control character, so it could be a name.
    #[error("`{0}`: the subject is not an identifier")]
    Subject(String),
    /// The issuer is not registered.
    #[error("`{0}`: unknown issuer")]
    UnknownIssuer(String),
    /// A numeric issuer's subject is not a number.
    #[error("`{0}`: the issuer's subjects are numeric user IDs")]
    NotNumeric(String),
}

fn is_issuer(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_subject(s: &str) -> bool {
    !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == ':')
}

impl ActorKey {
    /// Parses `{issuer}:{subject}`, checking the shape only; [`Self::validate`] checks the
    /// issuer against a registry.
    pub fn parse(s: &str) -> Result<Self, ActorKeyError> {
        let Some((issuer, subject)) = s.split_once(':') else {
            return Err(ActorKeyError::Shape(s.to_string()));
        };
        if issuer.is_empty() || subject.is_empty() {
            return Err(ActorKeyError::Shape(s.to_string()));
        }
        if !is_issuer(issuer) {
            return Err(ActorKeyError::Issuer(s.to_string()));
        }
        if !is_subject(subject) {
            return Err(ActorKeyError::Subject(s.to_string()));
        }
        Ok(Self {
            issuer: issuer.to_string(),
            subject: subject.to_string(),
        })
    }

    /// A numeric actor of an issuer.
    #[must_use]
    pub fn numeric(issuer: &str, id: u64) -> Self {
        Self {
            issuer: issuer.to_string(),
            subject: id.to_string(),
        }
    }

    /// `local:{id}`: an account of the current tenant.
    #[must_use]
    pub fn local(id: u64) -> Self {
        Self::numeric(LOCAL, id)
    }

    /// `instance:{farm slug}`: the instance as operator (0040 §2).
    #[must_use]
    pub fn instance(farm_slug: &str) -> Self {
        Self {
            issuer: INSTANCE.to_string(),
            subject: farm_slug.to_string(),
        }
    }

    /// Checks the issuer is registered, or is `local`, or is one of `tenant_slugs`, and
    /// that a numeric issuer's subject is a number.
    pub fn validate(
        &self,
        registry: &IssuerRegistry,
        tenant_slugs: &[&str],
    ) -> Result<(), ActorKeyError> {
        let model = if self.issuer == LOCAL || tenant_slugs.contains(&self.issuer.as_str()) {
            ActorModel::Numeric
        } else {
            registry
                .by_code(&self.issuer)
                .ok_or_else(|| ActorKeyError::UnknownIssuer(self.to_string()))?
                .actor_model
        };
        if model == ActorModel::Numeric && self.numeric_subject().is_none() {
            return Err(ActorKeyError::NotNumeric(self.to_string()));
        }
        Ok(())
    }

    /// The issuer code.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The subject, as written.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The subject as a user ID, for a numeric issuer.
    #[must_use]
    pub fn numeric_subject(&self) -> Option<u64> {
        if self.subject.starts_with('0') && self.subject.len() > 1 {
            return None;
        }
        self.subject.parse().ok()
    }

    /// Whether the key is `local:…`.
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.issuer == LOCAL
    }

    /// Whether the key is the instance operator's.
    #[must_use]
    pub fn is_instance(&self) -> bool {
        self.issuer == INSTANCE
    }

    /// The key as it is written outside the tenant: `local` replaced by the tenant's slug
    /// (0018 §4). Any other key is returned as is.
    #[must_use]
    pub fn qualified(&self, tenant_slug: &str) -> Self {
        if self.is_local() {
            Self {
                issuer: tenant_slug.to_string(),
                subject: self.subject.clone(),
            }
        } else {
            self.clone()
        }
    }

    /// The key as the tenant reads it: its own slug replaced by `local`.
    #[must_use]
    pub fn relative_to(&self, tenant_slug: &str) -> Self {
        if self.issuer == tenant_slug {
            Self {
                issuer: LOCAL.to_string(),
                subject: self.subject.clone(),
            }
        } else {
            self.clone()
        }
    }
}

impl fmt::Display for ActorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.issuer, self.subject)
    }
}

impl fmt::Debug for ActorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ActorKey({self})")
    }
}

impl std::str::FromStr for ActorKey {
    type Err = ActorKeyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for ActorKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ActorKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = std::borrow::Cow::<str>::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Normalizes an identity provider's `sub` claim for a binding (0007 §3): Wikimedia's
/// legacy `7654321` and newer `mw:CentralAuth:7654321` are one subject.
#[must_use]
pub fn normalize_sub(sub: &str) -> &str {
    sub.strip_prefix("mw:CentralAuth:").unwrap_or(sub).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_and_print() {
        let k = ActorKey::parse("wikidatawiki:12345").unwrap();
        assert_eq!((k.issuer(), k.subject()), ("wikidatawiki", "12345"));
        assert_eq!(k.numeric_subject(), Some(12345));
        assert_eq!(k.to_string(), "wikidatawiki:12345");
        let i = ActorKey::instance("scatter");
        assert_eq!(i.to_string(), "instance:scatter");
        assert!(i.is_instance());
        assert_eq!(i.numeric_subject(), None);
        assert_eq!(
            ActorKey::local(42).qualified("librarybase").to_string(),
            "librarybase:42"
        );
        assert_eq!(
            ActorKey::parse("librarybase:42")
                .unwrap()
                .relative_to("librarybase"),
            ActorKey::local(42)
        );
        assert_eq!(ActorKey::numeric("x", 7).numeric_subject(), Some(7));
        assert_eq!(ActorKey::parse("x:007").unwrap().numeric_subject(), None);
        for bad in [
            "",
            "local",
            ":1",
            "local:",
            "Local:1",
            "local:Some Name",
            "a:b:c",
            "a:\u{1}",
        ] {
            assert!(ActorKey::parse(bad).is_err(), "{bad:?}");
        }
        let via_json: ActorKey = serde_json::from_str("\"local:42\"").unwrap();
        assert_eq!(via_json, ActorKey::local(42));
        assert!(serde_json::from_str::<ActorKey>("\"no colon\"").is_err());
    }

    #[test]
    fn validation_against_the_registry() {
        let r = IssuerRegistry::default_registry();
        assert!(ActorKey::local(1).validate(r, &[]).is_ok());
        assert!(
            ActorKey::parse("librarybase:5")
                .unwrap()
                .validate(r, &["librarybase"])
                .is_ok()
        );
        assert!(
            ActorKey::parse("librarybase:5")
                .unwrap()
                .validate(r, &[])
                .is_ok(),
            "registered issuer"
        );
        assert!(ActorKey::instance("scatter").validate(r, &[]).is_ok());
        assert!(matches!(
            ActorKey::parse("nobody:1").unwrap().validate(r, &[]),
            Err(ActorKeyError::UnknownIssuer(_))
        ));
        assert!(matches!(
            ActorKey::parse("wikidatawiki:Jimbo")
                .unwrap()
                .validate(r, &[]),
            Err(ActorKeyError::NotNumeric(_))
        ));
        assert!(matches!(
            ActorKey::parse("local:Jimbo").unwrap().validate(r, &[]),
            Err(ActorKeyError::NotNumeric(_))
        ));
    }

    #[test]
    fn sub_claims_normalize() {
        assert_eq!(normalize_sub("7654321"), "7654321");
        assert_eq!(normalize_sub("mw:CentralAuth:7654321"), "7654321");
        assert_eq!(normalize_sub(" 7654321 "), "7654321");
    }
}
