//! Adoption mode (0035 §3, A12): a frozen Wikibase's full-history XML dump becomes the
//! tenant's own entities, revision by revision ([`adopt_history`]), or, with
//! `--current-only`, as each entity's newest revision ([`adopt_page`]). IDs stay in home
//! form; an entity-source prefix from a federated install (`wikidata:Q42`,
//! wikibase-compat §5.1) is rewritten to the provider form through the source table.
//! [`Survey`] derives the floors of 0035 §4 and the accounts of §5 from a pass over the
//! same dump, the data type of every property the dump defines, and whether the dump
//! carries history at all.
//!
//! The revision text of an entity page is Wikibase's *stored* JSON, which carries no
//! `datatype` on its snaks: Wikibase adds that at output time from the property. Only
//! property entities carry their `datatype`, at the top level. So the survey collects
//! the property types, and [`adopt_page`] types every snak from them, so that the adopted
//! record says what a Wikibase JSON dump of the same entity would say.

use std::collections::{BTreeMap, BTreeSet};

use scatter_providers::Registry;
use scatter_wikibase_changeset::PropertyTypes;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::{DataType, DataValue};

use crate::DumpError;
use crate::xml_dump::{DumpPage, Revision};

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

/// Who made a revision, as the dump records it (0007 §5; 0035 A12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contributor {
    /// A registered account, by the source's user ID.
    Account(u64),
    /// An anonymous edit, by IP.
    Ip(String),
    /// A name with no user ID on the source: `imported>Name` from an earlier import, or
    /// a software account such as `MediaWiki default`.
    Imported(String),
    /// The dump hid the contributor (`<contributor deleted="deleted"/>`).
    Hidden,
}

impl Contributor {
    /// The contributor of a revision.
    #[must_use]
    pub fn of(r: &Revision) -> Self {
        match (&r.user, &r.ip, r.user_hidden) {
            (_, _, true) | (None, None, false) => Self::Hidden,
            (Some((0, name)), _, _) => Self::Imported(name.clone()),
            (Some((id, _)), _, _) => Self::Account(*id),
            (None, Some(ip), _) => Self::Ip(ip.clone()),
        }
    }
}

/// One revision of an entity as adopted from the dump.
#[derive(Debug, Clone, PartialEq)]
pub struct AdoptedRevision {
    /// The entity's ID, from the page title.
    pub id: EntityId,
    /// The state, in home form; absent for a redirect or a text the dump hid.
    pub entity: Option<Entity>,
    /// The redirect target, for a revision that made the page a redirect.
    pub redirect: Option<EntityId>,
    /// The source's revision ID.
    pub source_revid: u64,
    /// The revision it followed, when that one is adopted too.
    pub source_parentid: Option<u64>,
    /// The source's timestamp.
    pub source_time: String,
    /// The page ID on the source.
    pub source_pageid: u64,
    /// The minor-edit flag.
    pub minor: bool,
    /// The parts the dump hid, as record parts: `content`, `comment`, `attestation`
    /// (0035 A14).
    pub withheld: Vec<String>,
    /// The edit summary.
    pub comment: Option<String>,
    /// Who made it.
    pub contributor: Contributor,
    /// Snaks whose property the dump does not type (a foreign property, or one deleted
    /// on the source), by property; their `datatype` stays absent.
    pub untyped_snaks: BTreeMap<EntityId, u64>,
}

/// An entity page's history as adopted from the dump.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AdoptedPage {
    /// Its revisions, in source-revision order.
    pub revisions: Vec<AdoptedRevision>,
    /// Revisions that could not be read as an entity or a redirect, with the reason.
    pub unreadable: Vec<(u64, String)>,
    /// The oldest revision names a parent the dump lacks: revisions were deleted on the
    /// source (0035 §2, precondition 6).
    pub gap: bool,
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
    /// Revisions by a name with no user ID (`imported>Name`).
    pub imported: u64,
    /// Revisions seen.
    pub revisions: u64,
    /// Pages with more than one revision.
    pub pages_with_history: u64,
    /// Revisions that name a parent revision.
    pub revisions_with_parent: u64,
    /// Pages whose oldest revision names a parent the dump lacks.
    pub history_gaps: u64,
    /// Pages outside the entity namespaces, which the job leaves `deferred` until it can
    /// import pages (0035 A13).
    pub other_pages: u64,
    /// The data type of every property the dump defines, by ID.
    pub property_types: BTreeMap<EntityId, DataType>,
}

impl Survey {
    /// Observes a page.
    pub fn observe(&mut self, page: &DumpPage) {
        self.pages += 1;
        self.max_page_id = self.max_page_id.max(page.id);
        if page.revisions.len() > 1 {
            self.pages_with_history += 1;
        }
        let ids: BTreeSet<u64> = page.revisions.iter().map(|r| r.id).collect();
        if let Some(oldest) = page.revisions.iter().min_by_key(|r| r.id)
            && oldest
                .parent_id
                .is_some_and(|p| p != 0 && !ids.contains(&p))
            && page.revisions.len() > 1
        {
            self.history_gaps += 1;
        }
        for r in &page.revisions {
            self.revisions += 1;
            self.max_revid = self.max_revid.max(r.id);
            if r.parent_id.is_some_and(|p| p != 0) {
                self.revisions_with_parent += 1;
            }
            match Contributor::of(r) {
                Contributor::Account(id) => {
                    if let Some((_, name)) = &r.user {
                        self.accounts.insert(id, name.clone());
                    }
                }
                Contributor::Ip(_) => self.anonymous += 1,
                Contributor::Imported(_) => self.imported += 1,
                Contributor::Hidden => self.hidden_users += 1,
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
            // (Entity pages; every other page is counted below.)
            let id_text = page.title.rsplit(':').next().unwrap_or(&page.title);
            if let Ok(id) = EntityId::parse(id_text)
                && let (Some(t), Some(n)) = (id.implied_entity_type(), id.numeric_id())
            {
                let e = self.max_entity.entry(t.to_string()).or_default();
                *e = (*e).max(n);
            }
        } else {
            self.other_pages += 1;
        }
    }

    /// Whether the dump is a current-state export: one revision per page, while revisions
    /// name parents (0035 §2, precondition 6).
    #[must_use]
    pub fn current_state_dump(&self) -> bool {
        self.pages_with_history == 0 && self.revisions_with_parent > 0
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
        id: entity.id.clone(),
        entity: Some(entity),
        redirect: None,
        source_revid: latest.id,
        source_parentid: None,
        source_time: latest.timestamp.clone(),
        source_pageid: page.id,
        minor: false,
        withheld: Vec::new(),
        comment: None,
        contributor: Contributor::of(latest),
        untyped_snaks,
    }))
}

/// The redirect target of a redirect revision's text, `{"entity":"Q9","redirect":"Q6"}`.
fn redirect_target(text: &str) -> Option<EntityId> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    EntityId::parse(v.get("redirect")?.as_str()?).ok()
}

/// The history of an entity page (0035 A12): every revision the dump holds, in revision
/// order, as an entity in home form, a redirect, or, where the dump hid the text,
/// neither (A14). `None` for a page that is not an entity page (its newest revision's
/// model is not an entity model). A revision that cannot be read is listed in
/// [`AdoptedPage::unreadable`] and left out, and the next one names no parent.
pub fn adopt_history(
    page: &DumpPage,
    sources: &BTreeMap<String, String>,
    registry: &Registry,
    types: &impl PropertyTypes,
) -> Result<Option<AdoptedPage>, DumpError> {
    let Some(latest) = page.latest() else {
        return Ok(None);
    };
    if !latest
        .model
        .as_deref()
        .is_some_and(|m| ENTITY_MODELS.contains(&m))
    {
        return Ok(None);
    }
    let id_text = page.title.rsplit(':').next().unwrap_or(&page.title);
    let id = EntityId::parse(id_text).map_err(|e| DumpError::Entity {
        line: usize::try_from(page.id).unwrap_or(0),
        message: format!("page {} (`{}`): {e}", page.id, page.title),
    })?;
    let mut revisions: Vec<&Revision> = page.revisions.iter().collect();
    revisions.sort_by_key(|r| r.id);
    let ids: BTreeSet<u64> = revisions.iter().map(|r| r.id).collect();
    let mut out = AdoptedPage {
        gap: revisions
            .first()
            .and_then(|r| r.parent_id)
            .is_some_and(|p| p != 0 && !ids.contains(&p))
            && revisions.len() > 1,
        ..AdoptedPage::default()
    };
    let mut adopted: BTreeSet<u64> = BTreeSet::new();
    for r in revisions {
        let mut withheld = Vec::new();
        if r.text_hidden {
            withheld.push("content".to_string());
        }
        if r.comment_hidden {
            withheld.push("comment".to_string());
        }
        if r.user_hidden {
            withheld.push("attestation".to_string());
        }
        let mut untyped_snaks = BTreeMap::new();
        let (entity, redirect) = if r.text_hidden {
            (None, None)
        } else if !r
            .model
            .as_deref()
            .is_some_and(|m| ENTITY_MODELS.contains(&m))
        {
            out.unreadable.push((
                r.id,
                format!("content model `{}`", r.model.as_deref().unwrap_or("?")),
            ));
            continue;
        } else if is_redirect_json(&r.text) {
            let Some(to) = redirect_target(&r.text) else {
                out.unreadable.push((r.id, "an unreadable redirect".into()));
                continue;
            };
            (None, Some(home_id(&to, sources, registry)?))
        } else {
            match Entity::from_json(&r.text) {
                Ok(parsed) => {
                    let mut entity = parsed.entity;
                    home_form(&mut entity, sources, registry)?;
                    untyped_snaks = type_snaks(&mut entity, types);
                    (Some(entity), None)
                }
                Err(e) => {
                    out.unreadable.push((r.id, e.to_string()));
                    continue;
                }
            }
        };
        out.revisions.push(AdoptedRevision {
            id: id.clone(),
            entity,
            redirect,
            source_revid: r.id,
            source_parentid: r.parent_id.filter(|p| adopted.contains(p)),
            source_time: r.timestamp.clone(),
            source_pageid: page.id,
            minor: r.minor,
            withheld,
            comment: if r.comment_hidden {
                None
            } else {
                r.comment.clone()
            },
            contributor: Contributor::of(r),
            untyped_snaks,
        });
        adopted.insert(r.id);
    }
    Ok(Some(out))
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
        assert_eq!(adopted[0].id.as_str(), "Q6");
        assert_eq!(
            adopted[0].entity.as_ref().unwrap().labels["en"],
            "Six & more"
        );
        assert_eq!(
            (adopted[0].source_revid, adopted[0].source_pageid),
            (41_877, 12)
        );
        assert_eq!(adopted[0].source_time, "2026-09-20T14:02:11Z");
        assert_eq!(adopted[1].id.as_str(), "P12");
    }

    const HISTORY: &str = r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/" version="0.11">
  <page>
    <title>Item:Q6</title><ns>120</ns><id>12</id>
    <revision>
      <id>41000</id><timestamp>2026-09-01T00:00:00Z</timestamp>
      <contributor><username>Alice</username><id>7</id></contributor>
      <comment>/* wbeditentity-create-item:0| */</comment>
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"type":"item","id":"Q6","labels":{"en":{"language":"en","value":"Six"}},"claims":{}}</text>
    </revision>
    <revision>
      <id>41500</id><parentid>41000</parentid><timestamp>2026-09-10T00:00:00Z</timestamp>
      <contributor><ip>192.0.2.1</ip></contributor>
      <comment deleted="deleted" />
      <model>wikibase-item</model><format>application/json</format>
      <text bytes="99" deleted="deleted" />
    </revision>
    <revision>
      <id>41877</id><parentid>41500</parentid><timestamp>2026-09-20T14:02:11Z</timestamp>
      <contributor><username>imported&gt;Bob</username><id>0</id></contributor>
      <minor/>
      <comment>/* wbsetlabel-set:1|en */ Six &amp; more</comment>
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"type":"item","id":"Q6","labels":{"en":{"language":"en","value":"Six &amp; more"}},"claims":{}}</text>
    </revision>
  </page>
  <page>
    <title>Item:Q7</title><ns>120</ns><id>13</id>
    <redirect title="Item:Q6" />
    <revision>
      <id>43000</id><parentid>40999</parentid><timestamp>2026-09-02T00:00:00Z</timestamp>
      <contributor deleted="deleted" />
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"type":"item","id":"Q7","labels":{},"claims":{}}</text>
    </revision>
    <revision>
      <id>44000</id><parentid>43000</parentid><timestamp>2026-09-21T00:00:00Z</timestamp>
      <contributor><username>Alice</username><id>7</id></contributor>
      <comment>Merged into [[Q6]]</comment>
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"entity":"Q7","redirect":"Q6"}</text>
    </revision>
  </page>
  <page>
    <title>Main Page</title><ns>0</ns><id>1</id>
    <revision>
      <id>2</id><timestamp>2020-01-01T00:00:00Z</timestamp>
      <contributor><ip>127.0.0.1</ip></contributor>
      <model>wikitext</model><format>text/x-wiki</format>
      <text xml:space="preserve">Hello</text>
    </revision>
  </page>
</mediawiki>
"#;

    #[test]
    fn history_is_adopted_revision_by_revision() {
        let pages: Vec<DumpPage> = XmlDump::new(HISTORY.as_bytes())
            .map(Result::unwrap)
            .collect();
        let mut survey = Survey::default();
        for p in &pages {
            survey.observe(p);
        }
        assert!(!survey.current_state_dump());
        assert_eq!(
            (
                survey.revisions,
                survey.pages_with_history,
                survey.other_pages
            ),
            (6, 2, 1)
        );
        assert_eq!(
            survey.history_gaps, 1,
            "Q7's oldest revision names a parent the dump lacks"
        );
        assert_eq!(survey.accounts().len(), 1, "user 0 is not an account");
        assert_eq!(
            (survey.anonymous, survey.imported, survey.hidden_users),
            (2, 1, 1)
        );

        let registry = Registry::default_registry();
        let none = BTreeMap::new();
        let q6 = adopt_history(&pages[0], &none, registry, &survey.property_types)
            .unwrap()
            .unwrap();
        assert!(q6.unreadable.is_empty() && !q6.gap);
        let revids: Vec<u64> = q6.revisions.iter().map(|r| r.source_revid).collect();
        assert_eq!(revids, [41_000, 41_500, 41_877]);
        let [first, hidden, last] = &q6.revisions[..] else {
            panic!("three revisions")
        };
        assert_eq!(first.source_parentid, None);
        assert_eq!(first.contributor, Contributor::Account(7));
        assert_eq!(
            first.comment.as_deref(),
            Some("/* wbeditentity-create-item:0| */")
        );
        assert_eq!(hidden.withheld, ["content", "comment"]);
        assert!(hidden.entity.is_none() && hidden.redirect.is_none() && hidden.comment.is_none());
        assert_eq!(hidden.contributor, Contributor::Ip("192.0.2.1".into()));
        assert_eq!(last.source_parentid, Some(41_500));
        assert!(last.minor);
        assert_eq!(
            last.contributor,
            Contributor::Imported("imported>Bob".into())
        );
        assert_eq!(last.entity.as_ref().unwrap().labels["en"], "Six & more");

        let q7 = adopt_history(&pages[1], &none, registry, &survey.property_types)
            .unwrap()
            .unwrap();
        assert!(q7.gap);
        assert_eq!(
            q7.revisions[0].source_parentid, None,
            "its parent is not in the dump"
        );
        assert_eq!(q7.revisions[0].withheld, ["attestation"]);
        assert_eq!(q7.revisions[0].contributor, Contributor::Hidden);
        assert_eq!(q7.revisions[1].redirect.as_ref().unwrap().as_str(), "Q6");
        assert!(q7.revisions[1].entity.is_none());
        assert_eq!(
            adopt_history(&pages[2], &none, registry, &survey.property_types).unwrap(),
            None,
            "a wikitext page is not an entity page"
        );
    }

    #[test]
    fn a_current_dump_is_recognised() {
        let pages: Vec<DumpPage> = XmlDump::new(crate::xml_dump::tests::SAMPLE.as_bytes())
            .map(Result::unwrap)
            .collect();
        let mut current = Survey::default();
        for p in &pages {
            let mut p = p.clone();
            let newest = p.latest().cloned().unwrap();
            p.revisions = vec![newest];
            current.observe(&p);
        }
        assert!(current.current_state_dump());
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
