//! Triplespace's entity ID forms (0017 §1–3, 0044 §1; 0115 §13): an extension trait over the
//! model's [`EntityId`].
//!
//! | Form | Shape | Examples |
//! |---|---|---|
//! | Local | One uppercase letter and digits | `Q5`, `P12`, `L3` |
//! | Foreign | Two-letter provider code, one-letter type code, and the rest in the type's grammar | `WDQ42`, `MBAb10bbbfc-…` |
//! | Keyed | The keyed type's name, a colon, and the normalized key | `domain:en.wikipedia.org` |
//!
//! [`TriplespaceId::form`] tells them apart by shape; [`TriplespaceId::validate`] checks a
//! foreign or keyed ID against the provider and keyed-type registries the caller passes in,
//! and returns it in full canonical form.

use scatter_keyed::KeyedRegistry;
use scatter_providers::Registry as ProviderRegistry;
use scatter_wikibase_model::{EntityId, IdParseError};

/// Which of the three forms an ID has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdForm {
    /// Minted by this instance (`Q5`).
    Local,
    /// Minted by a provider (`WDQ42`).
    Foreign,
    /// The thing itself (`domain:example.org`).
    Keyed,
}

/// Triplespace's reading of an [`EntityId`].
pub trait TriplespaceId {
    /// Which form the ID has: keyed with a colon, foreign when its second character is a
    /// letter and it has at least four, local otherwise.
    fn form(&self) -> IdForm;

    /// For a keyed ID, its type name and key.
    fn keyed_parts(&self) -> Option<(&str, &str)>;

    /// Whether this ID may be a member of an identity cluster. Keyed entities that are
    /// names, keywords and notations never join one (0009 §8): their entry in `keyed` sets
    /// `clusters = false`. Every other ID may, a keyed ID of a type `keyed` does not know
    /// included, since whether the type exists is checked elsewhere.
    fn joins_clusters(&self, keyed: &KeyedRegistry) -> bool;

    /// Checks the ID against the registries and returns it in full canonical form: a
    /// foreign ID's rest canonicalized by its type's grammar, a keyed ID's key normalized.
    /// A local ID has nothing to check beyond its shape.
    ///
    /// # Errors
    ///
    /// When the provider registry rejects a foreign ID, or the keyed-type registry a keyed one.
    fn validate(
        &self,
        providers: &ProviderRegistry,
        keyed: &KeyedRegistry,
    ) -> Result<EntityId, IdParseError>;
}

impl TriplespaceId for EntityId {
    fn form(&self) -> IdForm {
        let s = self.as_str();
        if s.contains(':') {
            IdForm::Keyed
        } else if s.len() >= 4 && s.as_bytes()[1].is_ascii_alphabetic() {
            IdForm::Foreign
        } else {
            IdForm::Local
        }
    }

    fn keyed_parts(&self) -> Option<(&str, &str)> {
        self.as_str().split_once(':')
    }

    fn joins_clusters(&self, keyed: &KeyedRegistry) -> bool {
        self.keyed_parts()
            .and_then(|(name, _)| keyed.by_name(name))
            .is_none_or(|t| t.clusters)
    }

    fn validate(
        &self,
        providers: &ProviderRegistry,
        keyed: &KeyedRegistry,
    ) -> Result<EntityId, IdParseError> {
        let canonical = match self.form() {
            IdForm::Local => return Ok(self.clone()),
            IdForm::Foreign => providers
                .parse_foreign_id(self.as_str())
                .map_err(|e| IdParseError::Foreign(e.to_string()))?
                .to_string(),
            IdForm::Keyed => keyed
                .parse_id(self.as_str())
                .map_err(|e| IdParseError::Keyed(e.to_string()))?
                .to_string(),
        };
        EntityId::parse(&canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Result<EntityId, IdParseError> {
        EntityId::parse(s).unwrap().validate(
            ProviderRegistry::default_registry(),
            KeyedRegistry::default_registry(),
        )
    }

    // 0017 §1: the three forms, by shape.
    #[test]
    fn three_forms_by_shape() {
        for (s, form) in [
            ("Q5", IdForm::Local),
            ("L1-F1", IdForm::Local),
            ("qqq5", IdForm::Local),
            ("WDP31", IdForm::Foreign),
            ("QQP5", IdForm::Foreign),
            ("domain:en.wikipedia.org", IdForm::Keyed),
        ] {
            assert_eq!(EntityId::parse(s).unwrap().form(), form, "{s}");
        }
        let d = EntityId::parse("domain:example.org").unwrap();
        assert_eq!(d.keyed_parts(), Some(("domain", "example.org")));
    }

    // 0017 §2–3: validation against the registries.
    #[test]
    fn validation_against_registries() {
        assert_eq!(v("Q5").unwrap().as_str(), "Q5");
        assert_eq!(
            v("mbab10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D")
                .unwrap()
                .as_str(),
            "MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"
        );
        assert_eq!(
            v("domain:Bücher.example").unwrap().as_str(),
            "domain:xn--bcher-kva.example"
        );
        assert!(matches!(v("ZZQ1"), Err(IdParseError::Foreign(_))));
        assert!(matches!(v("isbn:123"), Err(IdParseError::Keyed(_))));
        assert!(matches!(
            v("OAKmachine-learning"),
            Err(IdParseError::Foreign(_))
        ));
        // 0044 §1: `QQP5` is a reserved, unregistered foreign shape, which the registry rejects.
        assert!(matches!(v("QQP5"), Err(IdParseError::Foreign(_))));
    }

    // 0066 §4: the provider's grammar takes a lexeme part suffix for its lexeme type only.
    #[test]
    fn lexeme_parts_under_a_provider_grammar() {
        assert_eq!(v("WDL1-f1").unwrap().as_str(), "WDL1-F1");
        assert_eq!(v("WDL1-S12").unwrap().as_str(), "WDL1-S12");
        assert!(v("WDQ1-F1").is_err());
        assert!(v("WDL1-X1").is_err());
    }

    // 0009 §8: keyed types whose entry sets `clusters = false` never join a cluster.
    #[test]
    fn joins_clusters_reads_the_registry_passed_in() {
        let k = KeyedRegistry::default_registry();
        assert!(EntityId::parse("Q5").unwrap().joins_clusters(k));
        assert!(
            EntityId::parse("domain:example.org")
                .unwrap()
                .joins_clusters(k)
        );
        assert!(
            !EntityId::parse("keyword:physics")
                .unwrap()
                .joins_clusters(k)
        );
        assert!(EntityId::parse("unknown:x").unwrap().joins_clusters(k));
    }
}
