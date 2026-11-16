//! The provider order (0004 §4; 0009 §8): which member of a link is the highest-ranked,
//! and so the record's key (payloads.md §3.2).
//!
//! A keyed member ranks first, a local member next, then foreign members by the
//! instance's provider order; a provider the order does not name ranks after those it
//! does, in registry order. The same order picks a cluster's canonical ID, which the
//! resolved view computes; here it only names the record.

use std::cmp::Ordering;

use scatter_providers::Registry;
use scatter_wikibase_model::id::{EntityId, IdForm};

/// The instance's provider order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderOrder {
    /// Provider codes, highest-ranked first.
    codes: Vec<String>,
    /// Every provider the registry knows, for the ones the order leaves out.
    registry_codes: Vec<String>,
}

impl Default for ProviderOrder {
    /// The default of 0004 §4: Wikidata, then OpenAlex, then the rest of the registry.
    fn default() -> Self {
        Self::new(&["WD", "OA"], Registry::default_registry())
    }
}

impl ProviderOrder {
    /// An order over the given provider codes, with the registry's others after them.
    #[must_use]
    pub fn new(codes: &[&str], registry: &Registry) -> Self {
        Self {
            codes: codes.iter().map(|c| c.to_ascii_uppercase()).collect(),
            registry_codes: registry
                .providers()
                .iter()
                .map(|p| p.code.clone())
                .collect(),
        }
    }

    /// The rank of an ID: lower is higher-ranked. Keyed, local, then providers.
    #[must_use]
    pub fn rank(&self, id: &EntityId) -> (u8, usize) {
        match id.form() {
            IdForm::Keyed => (0, 0),
            IdForm::Local => (1, 0),
            IdForm::Foreign => {
                let code = &id.as_str()[..2];
                if let Some(i) = self.codes.iter().position(|c| c == code) {
                    (2, i)
                } else {
                    let i = self
                        .registry_codes
                        .iter()
                        .position(|c| c == code)
                        .unwrap_or(self.registry_codes.len());
                    (3, i)
                }
            }
        }
    }

    /// Orders two IDs, highest-ranked first; ties by the ID text, so the result is total.
    #[must_use]
    pub fn compare(&self, a: &EntityId, b: &EntityId) -> Ordering {
        self.rank(a)
            .cmp(&self.rank(b))
            .then_with(|| a.as_str().cmp(b.as_str()))
    }

    /// The highest-ranked of the given IDs.
    #[must_use]
    pub fn highest<'a>(&self, ids: impl IntoIterator<Item = &'a EntityId>) -> Option<&'a EntityId> {
        ids.into_iter().min_by(|a, b| self.compare(a, b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> EntityId {
        EntityId::parse(s).unwrap()
    }

    #[test]
    fn keyed_then_local_then_providers() {
        let order = ProviderOrder::default();
        let ids = [id("OAW1"), id("WDQ1"), id("Q1"), id("domain:example.org")];
        assert_eq!(order.highest(&ids), Some(&ids[3]));
        assert_eq!(order.highest(&ids[..3]), Some(&ids[2]));
        assert_eq!(order.highest(&ids[..2]), Some(&ids[1]));
        assert_eq!(order.highest(std::iter::empty()), None);
    }

    #[test]
    fn an_instance_order_puts_its_providers_first() {
        let order = ProviderOrder::new(&["oa"], Registry::default_registry());
        assert_eq!(
            order.highest(&[id("WDQ1"), id("OAW1")]).unwrap().as_str(),
            "OAW1"
        );
        // Unnamed providers rank after the named ones, in registry order.
        assert!(order.rank(&id("WDQ1")) > order.rank(&id("OAW1")));
        assert!(order.rank(&id("ZZQ1")) >= order.rank(&id("WDQ1")));
    }
}
