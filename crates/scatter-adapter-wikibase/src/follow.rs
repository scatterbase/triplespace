//! The entity follower's reading of a Wikibase's API (0092 §1–§2): which namespaces hold
//! entities, which recent-changes rows name one, and what a `wbgetentities` response says
//! about each. Pure: functions over response bodies. The requests are the caller's.
//!
//! A row only says that a title changed. The follower reads the entity's current state
//! and writes what the state says (0092 §2): an entity is a state with its `lastrevid`, a
//! `redirects` entry is a merge, and `missing` is a deletion, which the sync job applies
//! only to an entity it already holds.

use std::collections::BTreeMap;

use scatter_wikibase_model::entity::{Entity, ParsedEntity};
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde::Deserialize;

/// Which namespaces hold entities, from `meta=siteinfo&siprop=namespaces`: those whose
/// default content model is a Wikibase entity model (0092 §1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntityNamespaces {
    /// Namespace number → the entity type its default model holds.
    pub by_ns: BTreeMap<i64, String>,
}

impl EntityNamespaces {
    /// Reads a `siteinfo` response. A wiki that reports no `defaultcontentmodel` (older
    /// MediaWiki) falls back to the canonical names `Item`, `Property`, `Lexeme`.
    ///
    /// # Errors
    ///
    /// When the body is not a `siteinfo` response.
    pub fn from_siteinfo(body: &str) -> Result<Self, FollowError> {
        #[derive(Deserialize)]
        struct Ns {
            id: i64,
            #[serde(default)]
            canonical: Option<String>,
            #[serde(default)]
            defaultcontentmodel: Option<String>,
        }
        #[derive(Deserialize)]
        struct Query {
            #[serde(default)]
            namespaces: BTreeMap<String, Ns>,
        }
        #[derive(Deserialize)]
        struct Resp {
            query: Option<Query>,
        }
        let resp: Resp = serde_json::from_str(body).map_err(|e| FollowError(e.to_string()))?;
        let mut by_ns = BTreeMap::new();
        for ns in resp
            .query
            .map(|q| q.namespaces)
            .unwrap_or_default()
            .into_values()
        {
            let model = ns.defaultcontentmodel.as_deref().unwrap_or("");
            let kind = match model.strip_prefix("wikibase-") {
                Some(k) if !k.is_empty() => Some(k.to_string()),
                _ => match ns.canonical.as_deref() {
                    Some("Item") => Some("item".into()),
                    Some("Property") => Some("property".into()),
                    Some("Lexeme") => Some("lexeme".into()),
                    _ => None,
                },
            };
            if let Some(kind) = kind {
                by_ns.insert(ns.id, kind);
            }
        }
        Ok(EntityNamespaces { by_ns })
    }

    /// The namespace numbers, for `rcnamespace`.
    #[must_use]
    pub fn numbers(&self) -> Vec<i64> {
        self.by_ns.keys().copied().collect()
    }

    /// Whether a namespace holds entities.
    #[must_use]
    pub fn holds(&self, ns: i64) -> bool {
        self.by_ns.contains_key(&ns)
    }
}

/// The upstream entity ID a recent-changes title names (`Item:Q8` → `Q8`), when the
/// namespace holds entities and the title's last segment is an ID.
#[must_use]
pub fn entity_id_of(title: &str, ns: i64, namespaces: &EntityNamespaces) -> Option<EntityId> {
    if !namespaces.holds(ns) {
        return None;
    }
    let name = title.rsplit_once(':').map_or(title, |(_, n)| n);
    let name = name.rsplit_once('/').map_or(name, |(_, n)| n);
    // An upstream ID in its own form: a letter and digits, as a Wikibase mints them.
    EntityId::parse(name)
        .ok()
        .filter(|id| id.form() == IdForm::Local)
}

/// What `wbgetentities` says about one requested ID.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// The entity, with its page info (`lastrevid`, `modified`).
    State(Box<ParsedEntity>),
    /// The ID redirects to another.
    Redirect {
        /// The requested ID.
        from: EntityId,
        /// The target.
        to: EntityId,
    },
    /// The entity does not exist.
    Missing(EntityId),
}

/// Reads a `wbgetentities` response (0092 §2): one [`Fetched`] per entity it answers for,
/// keyed by the ID as the response wrote it.
///
/// # Errors
///
/// When the body is not a `wbgetentities` response, or an entity in it will not parse.
pub fn read_entities(body: &str) -> Result<BTreeMap<String, Fetched>, FollowError> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        entities: BTreeMap<String, serde_json::Value>,
        #[serde(default)]
        error: Option<serde_json::Value>,
    }
    let resp: Resp = serde_json::from_str(body).map_err(|e| FollowError(e.to_string()))?;
    if let Some(e) = resp.error {
        return Err(FollowError(format!("the API answered an error: {e}")));
    }
    let mut out = BTreeMap::new();
    for (key, v) in resp.entities {
        let id = EntityId::parse(&key).map_err(|e| FollowError(format!("`{key}`: {e}")))?;
        if v.get("missing").is_some() {
            out.insert(key, Fetched::Missing(id));
            continue;
        }
        if let Some(r) = v.get("redirects") {
            let to = r
                .get("to")
                .and_then(|t| t.as_str())
                .ok_or_else(|| FollowError(format!("`{key}`: a redirect without a target")))?;
            let to = EntityId::parse(to).map_err(|e| FollowError(format!("`{key}`: {e}")))?;
            out.insert(key, Fetched::Redirect { from: id, to });
            continue;
        }
        let parsed =
            Entity::from_json(&v.to_string()).map_err(|e| FollowError(format!("`{key}`: {e}")))?;
        out.insert(key, Fetched::State(Box::new(parsed)));
    }
    Ok(out)
}

/// A response the follower could not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("follow: {0}")]
pub struct FollowError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_namespaces_come_from_the_default_content_model() {
        let body = r#"{"query":{"namespaces":{"0":{"id":0,"canonical":"","content":""},
          "860":{"id":860,"canonical":"Item","defaultcontentmodel":"wikibase-item"},
          "862":{"id":862,"canonical":"Property","defaultcontentmodel":"wikibase-property"},
          "861":{"id":861,"canonical":"Item talk","defaultcontentmodel":"wikitext"}}}}"#;
        let ns = EntityNamespaces::from_siteinfo(body).unwrap();
        assert_eq!(ns.numbers(), [860, 862]);
        assert_eq!(ns.by_ns[&860], "item");
        assert_eq!(entity_id_of("Item:Q8", 860, &ns).unwrap().as_str(), "Q8");
        assert!(entity_id_of("Item talk:Q8", 861, &ns).is_none());
        assert!(entity_id_of("Item:Not an id", 860, &ns).is_none());
    }

    #[test]
    fn a_wbgetentities_response_yields_states_redirects_and_misses() {
        let body = r#"{"entities":{
          "Q8":{"type":"item","id":"Q8","pageid":20,"ns":860,"title":"Item:Q8","lastrevid":5001,"modified":"2026-10-20T00:00:00Z","labels":{"en":{"language":"en","value":"A"}},"claims":{}},
          "Q101":{"id":"Q101","redirects":{"from":"Q101","to":"Q102"}},
          "Q5095":{"id":"Q5095","missing":""}}}"#;
        let got = read_entities(body).unwrap();
        assert_eq!(got.len(), 3);
        match &got["Q8"] {
            Fetched::State(p) => {
                assert_eq!(p.entity.id.as_str(), "Q8");
                assert_eq!(p.page.as_ref().unwrap().lastrevid, Some(5001));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            got["Q101"],
            Fetched::Redirect {
                from: EntityId::parse("Q101").unwrap(),
                to: EntityId::parse("Q102").unwrap()
            }
        );
        assert_eq!(
            got["Q5095"],
            Fetched::Missing(EntityId::parse("Q5095").unwrap())
        );
        assert!(read_entities(r#"{"error":{"code":"x"}}"#).is_err());
    }
}
