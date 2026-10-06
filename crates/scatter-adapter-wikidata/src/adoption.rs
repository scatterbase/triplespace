//! Adoption mode (0035 §3): a frozen Wikibase's XML dump becomes the tenant's own
//! entities. IDs stay in home form; an entity-source prefix from a federated install
//! (`wikidata:Q42`, wikibase-compat §5.1) is rewritten to the provider form through the
//! source table. [`Survey`] derives the floors of 0035 §4 and the accounts of §5 from a
//! pass over the same dump, and the data type of every property the dump defines.
//!
//! The revision text of an entity page is Wikibase's *stored* JSON, which carries no
//! `datatype` on its snaks: Wikibase adds that at output time from the property. Only
//! property entities carry their `datatype`, at the top level. So the survey collects
//! the property types, and [`adopt_page`] types every snak from them, so that the adopted
//! record says what a Wikibase JSON dump of the same entity would say.

use std::collections::BTreeMap;

use scatter_providers::Registry;
use scatter_wikibase_changeset::PropertyTypes;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::{DataType, DataValue};

use crate::DumpError;
use crate::xml_dump::DumpPage;

/// The content models of entity pages.
pub const ENTITY_MODELS: &[&str] = &["wikibase-item", "wikibase-property", "wikibase-lexeme"];

/// A source account (0035 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The source's user ID, which becomes `{slug}:{id}`.
    pub id: u64,
    /// The current name.
    pub name: String,
}

/// An entity as adopted from its page's newest revision.
#[derive(Debug, Clone, PartialEq)]
pub struct AdoptedRevision {
    /// The state, in home form.
    pub entity: Entity,
    /// The source's revision ID.
    pub source_revid: u64,
    /// The source's timestamp.
    pub source_time: String,
    /// The page ID on the source.
    pub source_pageid: u64,
    /// Snaks whose property the dump does not type (a foreign property, or one deleted
    /// on the source), by property; their `datatype` stays absent.
    pub untyped_snaks: BTreeMap<EntityId, u64>,
}

/// What a pass over the dump learns: the floors and the accounts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Survey {
    /// Pages seen.
    pub pages: u64,
    /// Entity pages seen, redirects excluded.
    pub entities: u64,
    /// Entity redirects seen.
    pub redirects: u64,
    /// The highest number per entity type (`item` → 350000).
    pub max_entity: BTreeMap<String, u64>,
    /// The highest page ID.
    pub max_page_id: u64,
    /// The highest revision ID.
    pub max_revid: u64,
    /// Registered contributors, by user ID.
    pub accounts: BTreeMap<u64, String>,
    /// Revisions whose contributor the dump hid.
    pub hidden_users: u64,
    /// Anonymous revisions.
    pub anonymous: u64,
    /// The data type of every property the dump defines, by ID.
    pub property_types: BTreeMap<EntityId, DataType>,
}

impl Survey {
    /// Observes a page.
    pub fn observe(&mut self, page: &DumpPage) {
        self.pages += 1;
        self.max_page_id = self.max_page_id.max(page.id);
        for r in &page.revisions {
            self.max_revid = self.max_revid.max(r.id);
            match (&r.user, &r.ip, r.user_hidden) {
                (Some((id, name)), _, _) => {
                    self.accounts.insert(*id, name.clone());
                }
                (None, Some(_), _) => self.anonymous += 1,
                (None, None, true) => self.hidden_users += 1,
                _ => {}
            }
        }
        if let Some(latest) = page.latest()
            && latest
                .model
                .as_deref()
                .is_some_and(|m| ENTITY_MODELS.contains(&m))
        {
            if page.redirect.is_some() || is_redirect_json(&latest.text) {
                self.redirects += 1;
            } else {
                self.entities += 1;
                if latest.model.as_deref() == Some("wikibase-property")
                    && let Ok(parsed) = Entity::from_json(&latest.text)
                    && let Some(dt) = parsed.entity.datatype
                {
                    self.property_types.insert(parsed.entity.id, dt);
                }
            }
            // The ID is in the title's last segment: `Item:Q6`, `Q6`.
            let id_text = page.title.rsplit(':').next().unwrap_or(&page.title);
            if let Ok(id) = EntityId::parse(id_text)
                && let (Some(t), Some(n)) = (id.implied_entity_type(), id.numeric_id())
            {
                let e = self.max_entity.entry(t.to_string()).or_default();
                *e = (*e).max(n);
            }
        }
    }

    /// The highest user ID.
    #[must_use]
    pub fn max_user_id(&self) -> u64 {
        self.accounts.keys().last().copied().unwrap_or(0)
    }

    /// The accounts, in ID order.
    #[must_use]
    pub fn accounts(&self) -> Vec<Account> {
        self.accounts
            .iter()
            .map(|(id, name)| Account {
                id: *id,
                name: name.clone(),
            })
            .collect()
    }
}

/// Whether an entity page's content is a redirect (`{"entity": "Q6", "redirect": "Q7"}`),
/// told by its shape without parsing the whole text: Wikibase writes the two keys and
/// nothing else, never a `type`.
#[must_use]
pub fn is_redirect_json(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("{\"entity\"") || (t.contains("\"redirect\"") && !t.contains("\"type\""))
}

/// Rewrites IDs under an entity-source prefix to the provider form (0035 §3); every
/// other ID is kept as the source wrote it.
fn home_id(
    id: &EntityId,
    sources: &BTreeMap<String, String>,
    registry: &Registry,
) -> Result<EntityId, DumpError> {
    let Some((prefix, rest)) = id.as_str().split_once(':') else {
        return Ok(id.clone());
    };
    let Some(code) = sources.get(prefix) else {
        return Ok(id.clone());
    };
    let provider = registry.by_code(code).ok_or_else(|| {
        DumpError::Xml(format!(
            "entity source `{prefix}` names an unknown provider `{code}`"
        ))
    })?;
    let foreign = registry
        .from_upstream(provider, rest)
        .map_err(scatter_wikibase_changeset::AdapterError::from)?;
    EntityId::parse(&foreign.to_string()).map_err(|e| DumpError::Xml(e.to_string()))
}

fn home_snak(
    snak: &mut Snak,
    sources: &BTreeMap<String, String>,
    registry: &Registry,
) -> Result<(), DumpError> {
    snak.property = home_id(&snak.property, sources, registry)?;
    if let SnakKind::Value(DataValue::EntityId(v)) = &mut snak.kind {
        v.id = home_id(&v.id, sources, registry)?;
    }
    Ok(())
}

/// Puts an entity in home form: only entity-source prefixes change.
pub fn home_form(
    entity: &mut Entity,
    sources: &BTreeMap<String, String>,
    registry: &Registry,
) -> Result<(), DumpError> {
    if sources.is_empty() {
        return Ok(());
    }
    let groups = std::mem::take(&mut entity.statements);
    for (property, mut statements) in groups {
        let property = home_id(&property, sources, registry)?;
        for s in &mut statements {
            home_snak(&mut s.mainsnak, sources, registry)?;
            let qualifiers = std::mem::take(&mut s.qualifiers);
            for (p, mut snaks) in qualifiers {
                for q in &mut snaks {
                    home_snak(q, sources, registry)?;
                }
                s.qualifiers
                    .entry(home_id(&p, sources, registry)?)
                    .or_default()
                    .extend(snaks);
            }
            for r in &mut s.references {
                let snaks = std::mem::take(&mut r.snaks);
                for (p, mut group) in snaks {
                    for q in &mut group {
                        home_snak(q, sources, registry)?;
                    }
                    r.snaks
                        .entry(home_id(&p, sources, registry)?)
                        .or_default()
                        .extend(group);
                }
            }
        }
        entity
            .statements
            .entry(property)
            .or_default()
            .extend(statements);
    }
    for link in entity.sitelinks.values_mut() {
        for b in &mut link.badges {
            *b = home_id(b, sources, registry)?;
        }
    }
    Ok(())
}

/// Gives every snak without a `datatype` its property's, from `types`. Returns, per
/// property `types` does not know, how many snaks were left untyped. Run after
/// [`home_form`], so that the property IDs are in the form `types` is keyed by.
pub fn type_snaks(entity: &mut Entity, types: &impl PropertyTypes) -> BTreeMap<EntityId, u64> {
    let mut untyped = BTreeMap::new();
    for statement in entity.statements.values_mut().flatten() {
        for snak in statement.snaks_mut() {
            if snak.datatype.is_some() {
                continue;
            }
            match types.datatype(&snak.property) {
                Some(dt) => snak.datatype = Some(dt.clone()),
                None => *untyped.entry(snak.property.clone()).or_default() += 1,
            }
        }
    }
    untyped
}

/// The adopted entity of a page: its newest revision parsed as an entity in home form,
/// its snaks typed from `types` (the survey's [`Survey::property_types`]). `None` for a
/// page that is not an entity (another namespace, a redirect, an empty text), with the
/// reason counted by the caller's [`Survey`].
pub fn adopt_page(
    page: &DumpPage,
    sources: &BTreeMap<String, String>,
    registry: &Registry,
    types: &impl PropertyTypes,
) -> Result<Option<AdoptedRevision>, DumpError> {
    let Some(latest) = page.latest() else {
        return Ok(None);
    };
    if page.redirect.is_some()
        || !latest
            .model
            .as_deref()
            .is_some_and(|m| ENTITY_MODELS.contains(&m))
        || is_redirect_json(&latest.text)
    {
        return Ok(None);
    }
    let parsed = Entity::from_json(&latest.text).map_err(|e| DumpError::Entity {
        line: usize::try_from(page.id).unwrap_or(0),
        message: format!("page {} (`{}`): {e}", page.id, page.title),
    })?;
    let mut entity = parsed.entity;
    home_form(&mut entity, sources, registry)?;
    let untyped_snaks = type_snaks(&mut entity, types);
    Ok(Some(AdoptedRevision {
        entity,
        source_revid: latest.id,
        source_time: latest.timestamp.clone(),
        source_pageid: page.id,
        untyped_snaks,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml_dump::XmlDump;

    #[test]
    fn survey_and_adoption_over_the_sample() {
        let pages: Vec<DumpPage> = XmlDump::new(crate::xml_dump::tests::SAMPLE.as_bytes())
            .map(Result::unwrap)
            .collect();
        let mut survey = Survey::default();
        for p in &pages {
            survey.observe(p);
        }
        assert_eq!((survey.pages, survey.entities, survey.redirects), (4, 2, 1));
        assert_eq!(survey.max_page_id, 40);
        assert_eq!(survey.max_revid, 44_000);
        assert_eq!(
            survey.max_entity["item"], 7,
            "the redirect's number still counts"
        );
        assert_eq!(survey.max_entity["property"], 12);
        assert_eq!(survey.max_user_id(), 9);
        assert_eq!(survey.accounts().len(), 2);
        assert_eq!((survey.hidden_users, survey.anonymous), (1, 1));
        assert_eq!(
            survey.property_types,
            BTreeMap::from([(EntityId::parse("P12").unwrap(), DataType::WikibaseItem)]),
            "the dump's properties are typed from their own top-level datatype"
        );

        let registry = Registry::default_registry();
        let adopted: Vec<AdoptedRevision> = pages
            .iter()
            .filter_map(|p| {
                adopt_page(p, &BTreeMap::new(), registry, &survey.property_types).unwrap()
            })
            .collect();
        assert_eq!(adopted.len(), 2);
        assert_eq!(adopted[0].entity.id.as_str(), "Q6");
        assert_eq!(adopted[0].entity.labels["en"], "Six & more");
        assert_eq!(
            (adopted[0].source_revid, adopted[0].source_pageid),
            (41_877, 12)
        );
        assert_eq!(adopted[0].source_time, "2026-09-20T14:02:11Z");
        assert_eq!(adopted[1].entity.id.as_str(), "P12");
    }

    #[test]
    fn entity_source_prefixes_become_provider_ids() {
        let mut e = Entity::from_json(r#"{"type":"item","id":"Q6","labels":{},
            "claims":{"P1":[{"mainsnak":{"snaktype":"value","property":"P1",
                "datavalue":{"value":{"entity-type":"item","id":"wikidata:Q42"},"type":"wikibase-entityid"},
                "datatype":"wikibase-item"},"type":"statement","rank":"normal",
                "qualifiers":{"wikidata:P31":[{"snaktype":"somevalue","property":"wikidata:P31","datatype":"wikibase-item"}]},
                "qualifiers-order":["wikidata:P31"]}]}}"#)
        .unwrap()
        .entity;
        let sources = BTreeMap::from([("wikidata".to_string(), "WD".to_string())]);
        home_form(&mut e, &sources, Registry::default_registry()).unwrap();
        let s = e.all_statements().next().unwrap();
        assert_eq!(
            s.mainsnak.property.as_str(),
            "P1",
            "the source's own properties stay"
        );
        assert_eq!(
            s.mainsnak
                .data_value()
                .unwrap()
                .entity_id()
                .unwrap()
                .as_str(),
            "WDQ42"
        );
        assert_eq!(s.qualifiers.keys().next().unwrap().as_str(), "WDP31");
        assert!(is_redirect_json(r#"{"entity":"Q6","redirect":"Q7"}"#));
        assert!(!is_redirect_json(r#"{"type":"item","id":"Q6"}"#));
    }

    #[test]
    fn stored_snaks_are_typed_from_the_properties() {
        // Stored JSON, as a MediaWiki XML dump carries it: no `datatype` on any snak.
        let mut e = Entity::from_json(r#"{"type":"item","id":"Q6","labels":{},
            "claims":{"P1":[{"mainsnak":{"snaktype":"value","property":"P1",
                "datavalue":{"value":"10.1000/xyz","type":"string"}},"type":"statement","rank":"normal",
                "qualifiers":{"P2":[{"snaktype":"somevalue","property":"P2"}]},"qualifiers-order":["P2"],
                "references":[{"snaks":{"P3":[{"snaktype":"value","property":"P3",
                    "datavalue":{"value":"x","type":"string"}}]},"snaks-order":["P3"]}]}]}}"#)
        .unwrap()
        .entity;
        assert!(
            e.all_statements()
                .next()
                .unwrap()
                .mainsnak
                .datatype
                .is_none()
        );
        let types = BTreeMap::from([
            (EntityId::parse("P1").unwrap(), DataType::ExternalId),
            (EntityId::parse("P2").unwrap(), DataType::WikibaseItem),
        ]);
        let p3 = EntityId::parse("P3").unwrap();
        assert_eq!(
            type_snaks(&mut e, &types),
            BTreeMap::from([(p3.clone(), 1)]),
            "P3 is unknown to the dump"
        );
        let s = e.all_statements().next().unwrap();
        assert_eq!(s.mainsnak.datatype, Some(DataType::ExternalId));
        assert_eq!(
            s.qualifiers.values().flatten().next().unwrap().datatype,
            Some(DataType::WikibaseItem)
        );
        assert!(
            s.references[0]
                .snaks
                .values()
                .flatten()
                .next()
                .unwrap()
                .datatype
                .is_none()
        );
        // Typing is idempotent and never overrides a type the source wrote.
        assert_eq!(type_snaks(&mut e, &types), BTreeMap::from([(p3, 1)]));
    }
}
