//! Entities, terms and sitelinks (wikibase-compat §2 and §3.1; 0009 §3–5; 0026 §2).

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::id::EntityId;
use crate::json::Wire;
use crate::statement::Statement;
use crate::value::DataType;

/// The Wikibase entity type. Registered ones are listed; others are kept by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EntityType {
    /// `item`
    Item,
    /// `property`
    Property,
    /// `lexeme` (namespace reserved, not implemented)
    Lexeme,
    /// `mediainfo` (no namespace registered)
    MediaInfo,
    /// `domain` (0009)
    Domain,
    /// `keyword` (0017 §5)
    Keyword,
    /// `notation` (0048 §1)
    Notation,
    /// A type this crate does not know.
    Other(String),
}

impl EntityType {
    /// Parses the `type` field.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "item" => Self::Item,
            "property" => Self::Property,
            "lexeme" => Self::Lexeme,
            "mediainfo" => Self::MediaInfo,
            "domain" => Self::Domain,
            "keyword" => Self::Keyword,
            "notation" => Self::Notation,
            other => Self::Other(other.to_string()),
        }
    }

    /// The `type` field.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Item => "item",
            Self::Property => "property",
            Self::Lexeme => "lexeme",
            Self::MediaInfo => "mediainfo",
            Self::Domain => "domain",
            Self::Keyword => "keyword",
            Self::Notation => "notation",
            Self::Other(s) => s,
        }
    }

    /// Whether the type is a keyed type (0009 §1), whose ID is its key.
    #[must_use]
    pub fn is_keyed(&self) -> bool {
        matches!(self, Self::Domain | Self::Keyword | Self::Notation)
    }
}

/// A label, description or alias: the language it is in and its text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Term {
    /// The language code.
    pub language: String,
    /// The text.
    pub value: String,
}

/// A sitelink as Wikibase writes it (0026 §2): keyed by a site ID or a host, with the
/// title in MediaWiki form, the badges, and the URL where the source gave one. The
/// normalized URL that is the link's identity is a function of this and the site
/// registry ([`crate::sites`]), not a change to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sitelink {
    /// The site ID (`enwiki`) or the host.
    pub site: String,
    /// The page title, in MediaWiki form (spaces, not underscores).
    pub title: String,
    /// Badge item IDs.
    #[serde(default)]
    pub badges: Vec<EntityId>,
    /// The page URL, where the source wrote one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Page metadata that `Special:EntityData` and `wbgetentities&props=info` carry beside
/// an entity. Not model data (wikibase-compat §3.1).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageInfo {
    /// The page ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pageid: Option<u64>,
    /// The namespace number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ns: Option<i64>,
    /// The page title, `Item:Q8`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The latest revision ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lastrevid: Option<u64>,
    /// When it was last modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

/// Statements grouped by property, in the order the source wrote them.
pub type StatementGroups = IndexMap<EntityId, Vec<Statement>>;

/// An entity: what a source asserts about one ID.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    /// The ID, in the form the source wrote it.
    pub id: EntityId,
    /// The entity type.
    pub entity_type: EntityType,
    /// A property's data type. Present on properties, absent elsewhere.
    pub datatype: Option<DataType>,
    /// Labels, one per language.
    pub labels: BTreeMap<String, String>,
    /// Descriptions, one per language.
    pub descriptions: BTreeMap<String, String>,
    /// Aliases, several per language, in order.
    pub aliases: BTreeMap<String, Vec<String>>,
    /// Statements grouped by property, in source order.
    pub statements: StatementGroups,
    /// Sitelinks keyed as the source wrote them. Items only.
    pub sitelinks: BTreeMap<String, Sitelink>,
}

impl Entity {
    /// An empty entity of a type.
    #[must_use]
    pub fn new(id: EntityId, entity_type: EntityType) -> Self {
        Self {
            id,
            entity_type,
            datatype: None,
            labels: BTreeMap::new(),
            descriptions: BTreeMap::new(),
            aliases: BTreeMap::new(),
            statements: StatementGroups::new(),
            sitelinks: BTreeMap::new(),
        }
    }

    /// Every statement, in property order then statement order.
    pub fn all_statements(&self) -> impl Iterator<Item = &Statement> {
        self.statements.values().flatten()
    }

    /// The number of statements.
    #[must_use]
    pub fn statement_count(&self) -> usize {
        self.statements.values().map(Vec::len).sum()
    }
}

/// An entity as parsed from Wikibase JSON, with any page metadata that came with it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEntity {
    /// The entity.
    pub entity: Entity,
    /// Page metadata, if the JSON carried any.
    pub page: Option<PageInfo>,
}

// ---------------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityWire {
    #[serde(rename = "type")]
    entity_type: String,
    id: EntityId,
    #[serde(default)]
    datatype: Option<DataType>,
    #[serde(default)]
    labels: BTreeMap<String, Term>,
    #[serde(default)]
    descriptions: BTreeMap<String, Term>,
    #[serde(default)]
    aliases: BTreeMap<String, Vec<Term>>,
    #[serde(default)]
    claims: StatementGroups,
    /// MediaInfo entities write their statements under `statements`, as Commons does
    /// (0041 §7). Accepted for every type; emitted for `mediainfo`.
    #[serde(default)]
    statements: StatementGroups,
    #[serde(default)]
    sitelinks: BTreeMap<String, Sitelink>,
    // Page metadata (wikibase-compat §3.1).
    #[serde(default)]
    pageid: Option<u64>,
    #[serde(default)]
    ns: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    lastrevid: Option<u64>,
    #[serde(default)]
    modified: Option<String>,
}

fn terms<E: serde::de::Error>(
    kind: &str,
    map: BTreeMap<String, Term>,
) -> Result<BTreeMap<String, String>, E> {
    map.into_iter()
        .map(|(lang, t)| {
            if t.language != lang {
                return Err(E::custom(format!(
                    "{kind} keyed `{lang}` says its language is `{}`",
                    t.language
                )));
            }
            Ok((lang, t.value))
        })
        .collect()
}

impl<'de> Deserialize<'de> for ParsedEntity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut w = EntityWire::deserialize(d)?;
        let entity_type = EntityType::parse(&w.entity_type);
        if entity_type != EntityType::Item && !w.sitelinks.is_empty() {
            return Err(D::Error::custom(format!(
                "a {} has no sitelinks",
                entity_type.name()
            )));
        }
        if entity_type == EntityType::MediaInfo && !w.aliases.is_empty() {
            return Err(D::Error::custom("a mediainfo has no aliases"));
        }
        if !w.statements.is_empty() {
            if !w.claims.is_empty() {
                return Err(D::Error::custom(
                    "an entity has `claims` or `statements`, not both",
                ));
            }
            w.claims = std::mem::take(&mut w.statements);
        }
        if entity_type == EntityType::Property && w.datatype.is_none() {
            return Err(D::Error::custom("a property has a datatype"));
        }
        if entity_type != EntityType::Property && w.datatype.is_some() {
            return Err(D::Error::custom(format!(
                "a {} has no datatype",
                entity_type.name()
            )));
        }
        for (site, link) in &w.sitelinks {
            if &link.site != site {
                return Err(D::Error::custom(format!(
                    "sitelink keyed `{site}` says its site is `{}`",
                    link.site
                )));
            }
        }
        for (property, statements) in &w.claims {
            for s in statements {
                if s.mainsnak.property != *property {
                    return Err(D::Error::custom(format!(
                        "statement under `{property}` has main snak property `{}`",
                        s.mainsnak.property
                    )));
                }
            }
        }
        let aliases = w
            .aliases
            .into_iter()
            .map(|(lang, list)| {
                let values = list
                    .into_iter()
                    .map(|t| {
                        if t.language == lang {
                            Ok(t.value)
                        } else {
                            Err(D::Error::custom(format!(
                                "alias keyed `{lang}` says its language is `{}`",
                                t.language
                            )))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((lang, values))
            })
            .collect::<Result<BTreeMap<_, _>, D::Error>>()?;
        let page = PageInfo {
            pageid: w.pageid,
            ns: w.ns,
            title: w.title,
            lastrevid: w.lastrevid,
            modified: w.modified,
        };
        Ok(Self {
            entity: Entity {
                id: w.id,
                entity_type,
                datatype: w.datatype,
                labels: terms::<D::Error>("label", w.labels)?,
                descriptions: terms::<D::Error>("description", w.descriptions)?,
                aliases,
                statements: w.claims,
                sitelinks: w.sitelinks,
            },
            page: (page != PageInfo::default()).then_some(page),
        })
    }
}

impl Entity {
    pub(crate) fn to_wire(&self, wire: Wire<'_>, page: Option<&PageInfo>) -> serde_json::Value {
        use serde_json::{Map, Value, json};
        let mut m = Map::new();
        if let Some(p) = page {
            for (k, v) in serde_json::to_value(p)
                .expect("serializable")
                .as_object()
                .expect("object")
            {
                m.insert(k.clone(), v.clone());
            }
        }
        m.insert("type".into(), Value::String(self.entity_type.name().into()));
        m.insert("id".into(), Value::String(self.id.as_str().into()));
        if let Some(dt) = &self.datatype {
            m.insert("datatype".into(), Value::String(dt.id().into()));
        }
        let term_map = |terms: &BTreeMap<String, String>| {
            Value::Object(
                terms
                    .iter()
                    .map(|(l, v)| (l.clone(), json!({"language": l, "value": v})))
                    .collect(),
            )
        };
        m.insert("labels".into(), term_map(&self.labels));
        m.insert("descriptions".into(), term_map(&self.descriptions));
        // A MediaInfo entity has no aliases and writes `statements`, as Commons does
        // (0041 §7); everything else writes `claims`.
        let mediainfo = self.entity_type == EntityType::MediaInfo;
        if !mediainfo {
            m.insert(
                "aliases".into(),
                Value::Object(
                    self.aliases
                        .iter()
                        .map(|(l, vs)| {
                            (
                                l.clone(),
                                Value::Array(
                                    vs.iter()
                                        .map(|v| json!({"language": l, "value": v}))
                                        .collect(),
                                ),
                            )
                        })
                        .collect(),
                ),
            );
        }
        m.insert(
            if mediainfo { "statements" } else { "claims" }.into(),
            Value::Object(
                self.statements
                    .iter()
                    .map(|(p, ss)| {
                        (
                            p.as_str().to_string(),
                            Value::Array(ss.iter().map(|s| s.to_wire(wire)).collect()),
                        )
                    })
                    .collect(),
            ),
        );
        if self.entity_type == EntityType::Item {
            m.insert(
                "sitelinks".into(),
                Value::Object(
                    self.sitelinks
                        .iter()
                        .map(|(k, l)| (k.clone(), serde_json::to_value(l).expect("serializable")))
                        .collect(),
                ),
            );
        }
        Value::Object(m)
    }
}

impl Serialize for Entity {
    /// The storage form, without page metadata or `numeric-id`.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_wire(Wire::STORAGE, None).serialize(s)
    }
}

impl Serialize for ParsedEntity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.entity
            .to_wire(Wire::STORAGE, self.page.as_ref())
            .serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invariants_are_checked() {
        let bad = [
            json!({"type": "item", "id": "Q1", "labels": {"en": {"language": "de", "value": "x"}}}),
            json!({"type": "property", "id": "P1"}),
            json!({"type": "item", "id": "Q1", "datatype": "string"}),
            json!({"type": "domain", "id": "domain:x.org", "sitelinks": {"enwiki": {"site": "enwiki", "title": "X"}}}),
            json!({"type": "item", "id": "Q1", "sitelinks": {"enwiki": {"site": "dewiki", "title": "X"}}}),
            json!({"type": "item", "id": "Q1", "claims": {"P1": [{"mainsnak": {"snaktype": "novalue", "property": "P2"}, "rank": "normal"}]}}),
            json!({"type": "item", "id": "Q1", "unexpected": 1}),
        ];
        for b in bad {
            assert!(
                serde_json::from_value::<ParsedEntity>(b.clone()).is_err(),
                "{b}"
            );
        }
    }

    #[test]
    fn page_metadata_is_separated() {
        let p: ParsedEntity = serde_json::from_value(json!({
            "pageid": 20, "ns": 120, "title": "Item:Q8", "lastrevid": 26, "modified": "2026-09-25T01:07:15Z",
            "type": "item", "id": "Q8"
        }))
        .unwrap();
        assert_eq!(p.page.as_ref().unwrap().lastrevid, Some(26));
        assert_eq!(p.entity.entity_type, EntityType::Item);
        let bare: ParsedEntity =
            serde_json::from_value(json!({"type": "item", "id": "Q8"})).unwrap();
        assert!(bare.page.is_none());
        assert_eq!(
            serde_json::to_value(&bare.entity).unwrap(),
            json!({"type": "item", "id": "Q8", "labels": {}, "descriptions": {}, "aliases": {}, "claims": {}, "sitelinks": {}})
        );
    }

    #[test]
    fn keyed_entities_and_properties() {
        let d: ParsedEntity = serde_json::from_value(json!({
            "type": "domain", "id": "Domain:En.Wikipedia.org", "labels": {"mul": {"language": "mul", "value": "en.wikipedia.org"}}
        }))
        .unwrap();
        assert_eq!(
            d.entity.id.as_str(),
            "domain:En.Wikipedia.org",
            "shape-canonical; key normalization is validate()'s"
        );
        assert!(d.entity.entity_type.is_keyed());
        let out = serde_json::to_value(&d.entity).unwrap();
        assert!(
            out.get("sitelinks").is_none(),
            "no sitelinks key on a keyed type"
        );
        let p: ParsedEntity =
            serde_json::from_value(json!({"type": "property", "id": "P1", "datatype": "string"}))
                .unwrap();
        assert_eq!(p.entity.datatype, Some(DataType::String));
        assert!(
            serde_json::to_value(&p.entity)
                .unwrap()
                .get("sitelinks")
                .is_none()
        );
    }

    #[test]
    fn mediainfo_in_the_commons_shape() {
        // 0041 §7: a File page's statements have the derived ID `M{page ID}` and are
        // served in the shape Commons serves, with `statements` and no aliases.
        let text = json!({
            "pageid": 1234, "ns": 6, "title": "File:Example.jpg", "lastrevid": 9, "modified": "2026-09-30T00:00:00Z",
            "type": "mediainfo", "id": "M1234",
            "labels": {"en": {"language": "en", "value": "An example"}},
            "descriptions": {},
            "statements": {"P1": [{
                "mainsnak": {"snaktype": "value", "property": "P1", "datatype": "string",
                             "datavalue": {"value": "x", "type": "string"}},
                "type": "statement", "id": "M1234$C13E7A23-11E7-4C91-A799-3D1806B65444", "rank": "normal"
            }]}
        });
        let m: ParsedEntity = serde_json::from_value(text.clone()).unwrap();
        assert_eq!(m.entity.entity_type, EntityType::MediaInfo);
        assert_eq!(m.entity.id.derived_page_id(), Some(1234));
        assert_eq!(m.entity.statements.len(), 1);
        let out = m
            .entity
            .to_wire(Wire::STORAGE.with_numeric_ids(), m.page.as_ref());
        assert!(out.get("claims").is_none());
        assert!(out.get("aliases").is_none());
        assert!(out.get("sitelinks").is_none());
        assert_eq!(out, text, "Commons shape is a fixed point");
        // `claims` is accepted too, so a Wikibase that wrote it that way still parses.
        let via_claims: ParsedEntity = serde_json::from_value(json!({
            "type": "mediainfo", "id": "M1", "claims": {}
        }))
        .unwrap();
        assert!(via_claims.entity.statements.is_empty());
        for bad in [
            json!({"type": "mediainfo", "id": "M1", "aliases": {"en": [{"language": "en", "value": "x"}]}}),
            json!({"type": "item", "id": "Q1", "claims": {"P1": []}, "statements": {"P2": []}}),
        ] {
            assert!(
                serde_json::from_value::<ParsedEntity>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }
}
