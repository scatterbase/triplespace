//! What the ID forms need to know about providers (0115 §14): the [`ProviderModel`] trait.
//!
//! A provider is a source whose entity IDs can be referred to directly, under a prefix
//! (`WDQ42` for Wikidata's `Q42`). Triplespace's own model of providers, with its registry,
//! is the `scatter-providers` crate, which implements this trait behind the default
//! `providers` feature; a caller with another model of providers implements it instead, and
//! builds this crate without that feature.

/// The provider facts the ID forms and value IRIs read.
pub trait ProviderModel {
    /// The canonical form of a foreign ID, its prefix and the rest by the entity type's
    /// grammar (`wdq42` as `WDQ42`).
    ///
    /// # Errors
    ///
    /// With the reason, when no provider or entity type of this model mints `id`, or its rest
    /// is not in the type's grammar.
    fn canonical_foreign_id(&self, id: &str) -> Result<String, String>;

    /// For each entity type whose stored IDs are an upstream ID after a prefix, and whose
    /// concept IRI is that upstream ID after an IRI prefix: `(stored prefix, IRI prefix)`,
    /// such as `("WDQ", "http://www.wikidata.org/entity/Q")`. Value IRIs are stored and
    /// expanded through these ([`crate::ValueIris`]).
    fn value_iri_prefixes(&self) -> Vec<(String, String)>;

    /// The code under which the provider named `slug` (`wikidata`) prefixes its IDs (`WD`),
    /// if this model has that provider. `None` by default.
    fn code_for_slug(&self, _slug: &str) -> Option<String> {
        None
    }

    /// For a foreign ID whose concept IRI is its upstream ID after a base: that base and
    /// the upstream ID, as `("http://www.wikidata.org/entity/", "Q42")` for `WDQ42`. Its
    /// concept IRI is the two together, and a property's predicates are named from the base
    /// by Wikibase's rule. `None` by default, which names the entity under the instance's
    /// base, as an ID this model does not know is.
    fn upstream_concept(&self, _id: &str) -> Option<(String, String)> {
        None
    }
}

#[cfg(feature = "providers")]
impl ProviderModel for scatter_providers::Registry {
    fn canonical_foreign_id(&self, id: &str) -> Result<String, String> {
        self.parse_foreign_id(id)
            .map(|f| f.to_string())
            .map_err(|e| e.to_string())
    }

    fn value_iri_prefixes(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for p in self.providers() {
            for t in &p.types {
                if t.key_mapped {
                    continue;
                }
                let Some((base, "")) = t.iri.split_once("{upstream_id}") else {
                    continue;
                };
                let stored = if p.source {
                    format!("{}:{}", p.slug, t.upstream_prefix)
                } else {
                    format!("{}{}", p.code, t.code)
                };
                out.push((stored, format!("{base}{}", t.upstream_prefix)));
            }
        }
        out
    }

    fn code_for_slug(&self, slug: &str) -> Option<String> {
        self.by_slug(slug).map(|p| p.code.clone())
    }

    fn upstream_concept(&self, id: &str) -> Option<(String, String)> {
        let f = self.parse_foreign_id(id).ok()?;
        let (base, "") = f.entity_type().iri.split_once("{upstream_id}")? else {
            return None;
        };
        Some((base.to_string(), f.upstream_id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider model of one provider, `XX`, minting `Q` IDs under `https://x.example/`.
    struct One;

    impl ProviderModel for One {
        fn canonical_foreign_id(&self, id: &str) -> Result<String, String> {
            let up = id.to_ascii_uppercase();
            up.strip_prefix("XXQ")
                .filter(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
                .map(|_| up.clone())
                .ok_or_else(|| format!("`{id}` is not an XX ID"))
        }
        fn value_iri_prefixes(&self) -> Vec<(String, String)> {
            vec![("XXQ".into(), "https://x.example/Q".into())]
        }
    }

    // 0115 §14: the ID forms and value IRIs work over any provider model.
    #[test]
    fn another_provider_model() {
        use crate::{TriplespaceId, ValueIris};
        use scatter_keyed::KeyedRegistry;
        use scatter_wikibase_model::EntityId;
        let keyed = KeyedRegistry::default_registry();
        let id = EntityId::parse("XXQ7").unwrap();
        assert_eq!(id.validate(&One, keyed).unwrap().as_str(), "XXQ7");
        assert!(
            EntityId::parse("WDQ42")
                .unwrap()
                .validate(&One, keyed)
                .is_err()
        );
        let iris = ValueIris::new(&One, None);
        assert_eq!(iris.store("https://x.example/Q7"), "XXQ7");
        assert_eq!(iris.expand("XXQ7"), "https://x.example/Q7");
        assert_eq!(One.code_for_slug("wikidata"), None);
    }

    // 0115 §14: Triplespace's model gives Wikidata's code.
    #[cfg(feature = "providers")]
    #[test]
    fn providers_give_codes_by_slug() {
        let r = scatter_providers::Registry::default_registry();
        assert_eq!(r.code_for_slug("wikidata").as_deref(), Some("WD"));
        assert_eq!(r.code_for_slug("no-such-provider"), None);
    }
}
