//! API-key and token grants (0024 §4; 0025 §3): `docs/registry/grants.toml`.
//!
//! A grant names the permissions a credential may exercise. The effective permissions of
//! a request made with a key or token are the subsidiary's own permissions intersected
//! with what its grants cover; a grant never confers a permission the account lacks.
//! `basic` is always included. `editprotected` covers no permission of its own: it lifts
//! the credential-level bar on edits that a protection ACL restricts (see the evaluator).

use std::collections::BTreeSet;

use serde::Deserialize;

/// The grant every credential has.
pub const BASIC: &str = "basic";
/// The grant that lets a credential make edits admitted by protection ACLs (0023 §1).
pub const EDIT_PROTECTED: &str = "editprotected";

/// One grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The name, MediaWiki's where MediaWiki has one.
    pub name: String,
    /// What it is for.
    pub description: Option<String>,
    /// The permissions it covers.
    pub permissions: BTreeSet<String>,
    /// Included in every credential.
    pub always: bool,
}

/// Why `grants.toml` could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GrantRegistryError {
    /// The TOML did not parse or did not have the expected shape.
    #[error("grants.toml: {0}")]
    Toml(String),
    /// A registry version this crate does not know.
    #[error("grants.toml: version {0} is not supported (this crate reads version 1)")]
    Version(u32),
    /// Two grants share a name.
    #[error("grant `{0}` is declared twice")]
    Duplicate(String),
    /// `basic` is missing or not `always`.
    #[error("grants.toml: `basic` must exist and be always included")]
    Basic,
    /// A grant names a permission the group registry does not list.
    #[error("grant `{grant}` names unknown permission `{permission}`")]
    UnknownPermission {
        /// The grant.
        grant: String,
        /// The permission.
        permission: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default, rename = "grant")]
    grants: Vec<RawGrant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGrant {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    always: bool,
}

/// The grant registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRegistry {
    version: u32,
    grants: Vec<Grant>,
}

impl GrantRegistry {
    /// Parses a `grants.toml`.
    pub fn parse(text: &str) -> Result<Self, GrantRegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| GrantRegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(GrantRegistryError::Version(raw.version));
        }
        let mut grants: Vec<Grant> = Vec::with_capacity(raw.grants.len());
        for g in raw.grants {
            if grants.iter().any(|x| x.name == g.name) {
                return Err(GrantRegistryError::Duplicate(g.name));
            }
            grants.push(Grant {
                name: g.name,
                description: g.description,
                permissions: g.permissions.into_iter().collect(),
                always: g.always,
            });
        }
        if !grants.iter().any(|g| g.name == BASIC && g.always) {
            return Err(GrantRegistryError::Basic);
        }
        Ok(Self {
            version: raw.version,
            grants,
        })
    }

    /// The embedded `docs/registry/grants.toml`.
    ///
    /// # Panics
    ///
    /// If the embedded registry is invalid, which the crate's tests rule out.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: std::sync::OnceLock<GrantRegistry> = std::sync::OnceLock::new();
        DEFAULT.get_or_init(|| Self::parse(crate::GRANTS_TOML).expect("embedded grants.toml"))
    }

    /// Checks every grant's permissions against a group registry.
    pub fn check_against(
        &self,
        groups: &crate::group::GroupRegistry,
    ) -> Result<(), GrantRegistryError> {
        for g in &self.grants {
            if let Some(p) = g.permissions.iter().find(|p| !groups.is_permission(p)) {
                return Err(GrantRegistryError::UnknownPermission {
                    grant: g.name.clone(),
                    permission: p.clone(),
                });
            }
        }
        Ok(())
    }

    /// The registry version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every grant, in registry order.
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// A grant by name.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&Grant> {
        self.grants.iter().find(|g| g.name == name)
    }

    /// The permissions a credential with the named grants covers: the union over them
    /// and every `always` grant. Unknown names cover nothing.
    #[must_use]
    pub fn coverage<'a>(&self, grant_names: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
        let names: BTreeSet<&str> = grant_names.into_iter().collect();
        self.grants
            .iter()
            .filter(|g| g.always || names.contains(g.name.as_str()))
            .flat_map(|g| g.permissions.iter().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_parses_and_covers() {
        let r = GrantRegistry::default_registry();
        r.check_against(crate::group::GroupRegistry::default_registry())
            .unwrap();
        assert!(r.by_name("basic").unwrap().always);
        let cov = r.coverage(["editentity"]);
        assert!(cov.contains("edit") && cov.contains("item-term") && cov.contains("read"));
        assert!(!cov.contains("delete"));
        // Creating a property, through Triplespace's entity grant and Wikibase's (0024 A18).
        assert!(cov.contains("property-create"));
        assert!(
            r.coverage(["createeditmovepage"])
                .contains("property-create")
        );
        assert!(!r.coverage(["editpage"]).contains("property-create"));
        let none = r.coverage([]);
        assert_eq!(none, r.by_name("basic").unwrap().permissions);
        assert!(r.by_name(EDIT_PROTECTED).unwrap().permissions.is_empty());
        assert_eq!(r.coverage(["nonsense"]), none);
    }

    #[test]
    fn rejects_bad_registries() {
        assert!(matches!(
            GrantRegistry::parse("version = 1\n[[grant]]\nname = \"x\"\n"),
            Err(GrantRegistryError::Basic)
        ));
        assert!(matches!(
            GrantRegistry::parse(&format!(
                "{}\n[[grant]]\nname = \"basic\"",
                crate::GRANTS_TOML
            )),
            Err(GrantRegistryError::Duplicate(_))
        ));
        let bad = GrantRegistry::parse(&format!(
            "{}\n[[grant]]\nname = \"x\"\npermissions = [\"fly\"]",
            crate::GRANTS_TOML
        ))
        .unwrap();
        assert!(matches!(
            bad.check_against(crate::group::GroupRegistry::default_registry()),
            Err(GrantRegistryError::UnknownPermission { .. })
        ));
    }
}
