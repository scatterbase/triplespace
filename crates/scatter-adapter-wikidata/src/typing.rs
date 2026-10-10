//! Snak data types for a mirror sync. An XML dump carries Wikibase's *stored* JSON, in
//! which no snak has a `datatype`: Wikibase adds it at output time from the property, and
//! only property entities carry theirs, at the top level. A JSON dump (`dumpJson.php`)
//! carries it on every snak already. [`DumpPropertyTypes`] is the mirror counterpart of
//! [`Survey::property_types`](crate::Survey::property_types): it collects the type of
//! every property a dump defines on a first pass, and types the snaks of each entity
//! from that on the second, so that a mirror state read from an XML dump says what a JSON
//! dump of the same entity would say. On a JSON dump it changes nothing.
//!
//! Typing runs on the entity as the dump has it, before the adapter rewrites IDs, so the
//! types are keyed by the upstream property ID (`P1`) and the adapter can rely on
//! `datatype` (the internetdomains adapter turns a `wikibase-item` value that points at
//! a mapped item into a `wikibase-domain` value).

use std::collections::BTreeMap;

use scatter_wikibase_changeset::PropertyTypes;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::DataType;

use crate::adoption::type_snaks;

/// The data types of the properties a dump defines, and the snaks they could not type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DumpPropertyTypes {
    types: BTreeMap<EntityId, DataType>,
    untyped: BTreeMap<EntityId, u64>,
}

impl DumpPropertyTypes {
    /// Observes an entity on the first pass: a property records its data type.
    pub fn observe(&mut self, entity: &Entity) {
        if let Some(dt) = &entity.datatype {
            self.types.insert(entity.id.clone(), dt.clone());
        }
    }

    /// Gives every snak of `entity` without a `datatype` its property's, on the second
    /// pass. A snak whose property the dump does not define stays untyped and is counted
    /// in [`untyped`](Self::untyped).
    pub fn apply(&mut self, entity: &mut Entity) {
        for (p, n) in type_snaks(entity, &self.types) {
            *self.untyped.entry(p).or_default() += n;
        }
    }

    /// The number of properties whose type is known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// No property type is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Snaks left untyped, by property: a property the dump does not define (deleted
    /// upstream, or outside a slice).
    #[must_use]
    pub fn untyped(&self) -> &BTreeMap<EntityId, u64> {
        &self.untyped
    }

    /// The untyped snaks as one stderr line, `None` when there are none.
    #[must_use]
    pub fn untyped_summary(&self) -> Option<String> {
        if self.untyped.is_empty() {
            return None;
        }
        let total: u64 = self.untyped.values().sum();
        let by_property: Vec<String> = self
            .untyped
            .iter()
            .map(|(p, n)| format!("{p}×{n}"))
            .collect();
        Some(format!(
            "untyped {total} snaks on {} properties the dump does not define: {}",
            self.untyped.len(),
            by_property.join(", ")
        ))
    }
}

impl PropertyTypes for DumpPropertyTypes {
    fn datatype(&self, property: &EntityId) -> Option<&DataType> {
        self.types.get(property)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entities;
    use crate::xml_dump::XmlDump;

    /// Stored JSON, as an XML dump carries it: the item comes before the properties that
    /// type it, and no snak has a `datatype`. P9 is used but not defined.
    const DUMP: &str = r#"<mediawiki xml:lang="en">
  <page>
    <title>Item:Q11</title><ns>120</ns><id>3</id>
    <revision><id>30</id><timestamp>2026-10-01T00:00:00Z</timestamp>
      <contributor><username>A</username><id>1</id></contributor>
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"type":"item","id":"Q11","labels":{},"descriptions":{},"aliases":{},"claims":{"P1":[{"mainsnak":{"snaktype":"value","property":"P1","datavalue":{"value":"wikipedia.org","type":"string"}},"type":"statement","rank":"normal","id":"Q11$00000000-0000-0000-0000-0000000000B1"}],"P8":[{"mainsnak":{"snaktype":"value","property":"P8","datavalue":{"value":{"entity-type":"item","numeric-id":9,"id":"Q9"},"type":"wikibase-entityid"}},"qualifiers":{"P1":[{"snaktype":"value","property":"P1","datavalue":{"value":"q.example","type":"string"}}]},"qualifiers-order":["P1"],"type":"statement","rank":"normal","id":"Q11$00000000-0000-0000-0000-0000000000B2","references":[{"hash":"abc","snaks":{"P9":[{"snaktype":"somevalue","property":"P9"}]},"snaks-order":["P9"]}]}]},"sitelinks":[]}</text>
    </revision>
  </page>
  <page>
    <title>Property:P1</title><ns>122</ns><id>1</id>
    <revision><id>10</id><timestamp>2026-10-01T00:00:00Z</timestamp>
      <contributor><username>A</username><id>1</id></contributor>
      <model>wikibase-property</model><format>application/json</format>
      <text xml:space="preserve">{"type":"property","id":"P1","datatype":"string","labels":{},"descriptions":{},"aliases":{},"claims":[]}</text>
    </revision>
  </page>
  <page>
    <title>Property:P8</title><ns>122</ns><id>2</id>
    <revision><id>20</id><timestamp>2026-10-01T00:00:00Z</timestamp>
      <contributor><username>A</username><id>1</id></contributor>
      <model>wikibase-property</model><format>application/json</format>
      <text xml:space="preserve">{"type":"property","id":"P8","datatype":"wikibase-item","labels":{},"descriptions":{},"aliases":{},"claims":[]}</text>
    </revision>
  </page>
</mediawiki>"#;

    fn entities() -> Vec<Entity> {
        Entities::Xml(XmlDump::new(DUMP.as_bytes()))
            .map(|p| p.unwrap().entity)
            .collect()
    }

    #[test]
    fn two_passes_type_every_snak_the_dump_defines() {
        let mut types = DumpPropertyTypes::default();
        for e in &entities() {
            types.observe(e);
        }
        assert_eq!(types.len(), 2);
        let mut item = entities().remove(0);
        types.apply(&mut item);
        let p1 = EntityId::parse("P1").unwrap();
        let p8 = EntityId::parse("P8").unwrap();
        let p9 = EntityId::parse("P9").unwrap();
        let main = &item.statements[&p1][0].mainsnak;
        assert_eq!(main.datatype, Some(DataType::String));
        let s = &item.statements[&p8][0];
        assert_eq!(s.mainsnak.datatype, Some(DataType::WikibaseItem));
        assert_eq!(s.qualifiers[&p1][0].datatype, Some(DataType::String));
        // The reference's P9 is not defined by the dump: left untyped, and counted.
        assert_eq!(s.references[0].snaks[&p9][0].datatype, None);
        assert_eq!(types.untyped().get(&p9), Some(&1));
        assert_eq!(
            types.untyped_summary().as_deref(),
            Some("untyped 1 snaks on 1 properties the dump does not define: P9×1")
        );
    }

    #[test]
    fn a_typed_snak_is_left_alone() {
        let mut types = DumpPropertyTypes::default();
        let mut entities = entities();
        for e in &entities {
            types.observe(e);
        }
        let p8 = EntityId::parse("P8").unwrap();
        let item = &mut entities[0];
        item.statements.get_mut(&p8).unwrap()[0].mainsnak.datatype = Some(DataType::ExternalId);
        types.apply(item);
        assert_eq!(
            item.statements[&p8][0].mainsnak.datatype,
            Some(DataType::ExternalId)
        );
    }
}
