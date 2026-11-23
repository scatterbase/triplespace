//! The IRIs inside values, stored as entity IDs (0004 A20; 04 §4.7): the `unit` of a
//! quantity, the `globe` of a coordinate and the `calendarmodel` of a time. Wikibase writes
//! them as concept IRIs (`http://www.wikidata.org/entity/Q11573`); the stored form is the
//! entity ID they name, prefixed as every stored ID is (`WDQ11573`, a local `Q7`, a
//! source's `lb:Q7`), and `"1"` stays `"1"` for a dimensionless quantity. They are expanded
//! through the current IRI template on output and before Wikibase's hashes are taken, so
//! that the served JSON and the snak hashes are Wikibase's.
//!
//! [`ValueIris`] is the map both ways, from the provider registry's concept IRI templates
//! and the tenant's entity base. An IRI it cannot name an entity for is kept as it was
//! written, and a stored value that is still an IRI is served as it is: what was stored
//! before 0004 A20 was built, or an IRI under a base no provider registers (0004 A23).

use std::borrow::Cow;

use scatter_providers::Registry;

use scatter_wikibase_model::id::EntityId;

use crate::id::{IdForm, TriplespaceId};

/// The map between value IRIs and stored IDs for one tenant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValueIris {
    /// `(stored prefix, IRI prefix)`: `("WDQ", "http://www.wikidata.org/entity/Q")`,
    /// `("lb:Q", "https://librarybase.org/entity/Q")`. The rest of either form is the
    /// digits of the upstream ID. Longest IRI prefix first.
    types: Vec<(String, String)>,
    /// The tenant's entity base, `https://librarybase.org/entity/`: a local ID's IRI.
    local: Option<String>,
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

impl ValueIris {
    /// The map for `registry` (a tenant's, with its sources) and the tenant's entity base,
    /// `{base}/entity/`, where its local IDs are named.
    #[must_use]
    pub fn new(registry: &Registry, local_entity_base: Option<&str>) -> Self {
        let mut types = Vec::new();
        for p in registry.providers() {
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
                types.push((stored, format!("{base}{}", t.upstream_prefix)));
            }
        }
        types.sort_by_key(|(_, iri)| std::cmp::Reverse(iri.len()));
        Self {
            types,
            local: local_entity_base.map(str::to_string),
        }
    }

    /// The IRI a stored value names: its concept IRI under the current template, or the
    /// value itself where it is `"1"`, an IRI already, or no entity this map knows.
    #[must_use]
    pub fn expand<'a>(&self, stored: &'a str) -> Cow<'a, str> {
        if stored == "1" || stored.contains("://") {
            return Cow::Borrowed(stored);
        }
        for (prefix, iri) in &self.types {
            if let Some(rest) = stored.strip_prefix(prefix.as_str())
                && digits(rest)
            {
                return Cow::Owned(format!("{iri}{rest}"));
            }
        }
        if let Some(base) = &self.local
            && EntityId::parse(stored).is_ok_and(|id| id.form() == IdForm::Local)
        {
            return Cow::Owned(format!("{base}{stored}"));
        }
        Cow::Borrowed(stored)
    }

    /// The stored form of a value IRI: the entity ID it names, or the IRI itself where it
    /// names none this map knows, and `"1"` as it is.
    #[must_use]
    pub fn store<'a>(&self, iri: &'a str) -> Cow<'a, str> {
        if !iri.contains("://") {
            return Cow::Borrowed(iri);
        }
        if let Some(base) = &self.local
            && let Some(rest) = iri.strip_prefix(base.as_str())
            && EntityId::parse(rest).is_ok_and(|id| id.form() == IdForm::Local)
        {
            return Cow::Owned(rest.to_string());
        }
        for (prefix, base) in &self.types {
            if let Some(rest) = iri.strip_prefix(base.as_str())
                && digits(rest)
            {
                return Cow::Owned(format!("{prefix}{rest}"));
            }
        }
        Cow::Borrowed(iri)
    }

    /// Rewrites the value IRIs of `value` with `f`: its unit, globe or calendar model
    /// ([`scatter_wikibase_model::map_value_iris`]).
    pub fn map_value(
        value: &mut scatter_wikibase_model::value::DataValue,
        f: impl Fn(&str) -> Cow<'_, str>,
    ) {
        scatter_wikibase_model::map_value_iris(value, f);
    }

    /// Rewrites every value IRI in `statement` to the stored form: the main snak's, the
    /// qualifiers' and the references'.
    pub fn store_statement(&self, statement: &mut scatter_wikibase_model::statement::Statement) {
        for snak in statement.snaks_mut() {
            if let scatter_wikibase_model::statement::SnakKind::Value(v) = &mut snak.kind {
                Self::map_value(v, |s| self.store(s));
            }
        }
    }

    /// Rewrites every value IRI in `entity`'s statements to the stored form.
    pub fn store_entity(&self, entity: &mut scatter_wikibase_model::entity::Entity) {
        for group in entity.statements.values_mut() {
            for s in group {
                self.store_statement(s);
            }
        }
    }
}

/// The model's hasher expands stored value IRIs through this map (0004 A20).
impl scatter_wikibase_model::ValueIriMap for ValueIris {
    fn expand<'a>(&self, stored: &'a str) -> Cow<'a, str> {
        ValueIris::expand(self, stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iris(base: &str) -> ValueIris {
        ValueIris::new(Registry::default_registry(), Some(base))
    }

    // 0004 A20: a provider's IRI is its prefixed ID, a local one the bare ID, "1" stays.
    #[test]
    fn value_iris_are_stored_as_ids_and_expanded_back() {
        let m = iris("https://reader.example/entity/");
        for (iri, stored) in [
            ("http://www.wikidata.org/entity/Q11573", "WDQ11573"),
            ("http://www.wikidata.org/entity/Q1985727", "WDQ1985727"),
            ("https://reader.example/entity/Q7", "Q7"),
            ("https://librarybase.org/entity/Q7", "LBQ7"),
            ("1", "1"),
        ] {
            assert_eq!(m.store(iri), stored, "{iri}");
            assert_eq!(m.expand(stored), iri, "{stored}");
        }
        // Stored twice is stored once.
        assert_eq!(m.store("WDQ11573"), "WDQ11573");
        // An IRI no provider registers is kept, and served as it is.
        let other = "http://example.org/entity/Q5";
        assert_eq!(m.store(other), other);
        assert_eq!(m.expand(other), other);
        // A tenant whose base is a registered provider's is that provider's own wiki: its
        // IRIs are its local IDs (0018 §5).
        let lb = iris("https://librarybase.org/entity/");
        assert_eq!(lb.store("https://librarybase.org/entity/Q7"), "Q7");
        // Without a local base, a local ID cannot be expanded and is served as stored.
        let bare = ValueIris::new(Registry::default_registry(), None);
        assert_eq!(bare.expand("Q7"), "Q7");
    }
}
