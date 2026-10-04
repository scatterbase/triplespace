//! The entity calls the site makes for an entity page: `wbgetentities` for the entity and
//! for the labels of what it refers to, and `GET /entity/{id}/provenance` for where it
//! comes from (0012 §5).
//!
//! Entities come back as the API's JSON, one object per ID: the site parses them with the
//! Wikibase model it renders from, which this crate does not depend on (0005 §2).

use std::collections::BTreeMap;

use axum::http::StatusCode;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::api::{CacheInfo, Fetched, check};
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// What a query value keeps unencoded.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':');

/// What a path segment keeps unencoded.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':');

/// The most IDs one `wbgetentities` call takes.
pub const IDS_PER_CALL: usize = 50;

/// The entities of one or more `wbgetentities` calls, by the ID the API keyed them under.
#[derive(Debug, Clone, Default)]
pub struct Entities {
    /// The entities found, as the API served them.
    pub found: BTreeMap<String, Value>,
    /// The IDs the API reported missing.
    pub missing: Vec<String>,
}

fn entities_of(v: &Value) -> Entities {
    let mut out = Entities::default();
    if let Some(map) = v["entities"].as_object() {
        for (k, e) in map {
            if e.get("missing").is_some() {
                out.missing.push(k.clone());
            } else {
                out.found.insert(k.clone(), e.clone());
            }
        }
    }
    out
}

/// Where an entity comes from, as `GET /entity/{id}/provenance` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Provenance {
    /// The ID asked about, canonical.
    pub id: String,
    /// The canonical ID of its identity cluster.
    #[serde(default)]
    pub canonical: String,
    /// The entity type.
    #[serde(default, rename = "type")]
    pub entity_type: Option<String>,
    /// Who minted the ID.
    #[serde(default)]
    pub minted_by: MintedBy,
    /// The graphs that hold a record for it.
    #[serde(default)]
    pub graphs: Vec<GraphStanding>,
    /// The graph asserting the most statements.
    #[serde(default)]
    pub dominant: Option<String>,
    /// For each statement of the resolved view, the graphs asserting it.
    #[serde(default)]
    pub statements: BTreeMap<String, StatementSources>,
    /// Local corrections, by statement ID.
    #[serde(default)]
    pub corrections: BTreeMap<String, Value>,
    /// When the instance first saw it.
    #[serde(default)]
    pub first_seen: Option<String>,
    /// What happens to local statements if the upstream deletes it.
    #[serde(default)]
    pub retention: Option<String>,
    /// The latest revision.
    #[serde(default)]
    pub lastrevid: Option<u64>,
    /// When it last changed.
    #[serde(default)]
    pub modified: Option<String>,
}

/// Who minted an ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MintedBy {
    /// `tenant`, `provider` or `keyed`.
    #[serde(default)]
    pub kind: String,
    /// The tenant, for a local ID.
    #[serde(default)]
    pub tenant: Option<String>,
    /// The wiki the tenant was adopted from.
    #[serde(default)]
    pub adopted_from: Option<String>,
    /// The provider's slug, for a foreign ID.
    #[serde(default)]
    pub provider: Option<String>,
    /// The provider's code.
    #[serde(default)]
    pub code: Option<String>,
    /// The provider's own ID.
    #[serde(default)]
    pub upstream_id: Option<String>,
    /// The keyed type, for a keyed ID.
    #[serde(default, rename = "type")]
    pub keyed_type: Option<String>,
}

/// One graph's standing for an entity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GraphStanding {
    /// `local`, `mirror/{slug}`, ….
    pub graph: String,
    /// The statements it asserts.
    #[serde(default)]
    pub statements: u64,
    /// When it last synced.
    #[serde(default)]
    pub synced_at: Option<String>,
    /// The upstream's version or revision.
    #[serde(default)]
    pub upstream_version: Option<String>,
    /// The job that wrote it.
    #[serde(default)]
    pub job: Option<u64>,
    /// Its history policy.
    #[serde(default)]
    pub history: Option<String>,
}

/// The graphs asserting one statement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct StatementSources {
    /// The graphs.
    #[serde(default)]
    pub graphs: Vec<String>,
}

impl Client {
    /// `wbgetentities` for some IDs or titles (`Item:Q6`, `Domain:example.org`), in calls of
    /// at most [`IDS_PER_CALL`], with `props` and `languages` as the API takes them (`|`
    /// separated; empty for all). The calls run one after another; their caching facts
    /// are combined: public only if every call was, every `ETag` and tag kept.
    pub async fn entities(
        &self,
        incoming: &Incoming,
        ids: &[String],
        props: &str,
        languages: &str,
    ) -> Result<Fetched<Entities>, ClientError> {
        let mut out = Entities::default();
        let mut caches = Vec::new();
        for chunk in ids.chunks(IDS_PER_CALL) {
            let mut q = format!(
                "/w/api.php?action=wbgetentities&format=json&formatversion=2&ids={}",
                utf8_percent_encode(&chunk.join("|"), QUERY)
            );
            if !props.is_empty() {
                q.push_str("&props=");
                q.push_str(&utf8_percent_encode(props, QUERY).to_string());
            }
            if !languages.is_empty() {
                q.push_str("&languages=");
                q.push_str(&utf8_percent_encode(languages, QUERY).to_string());
            }
            let r = self.get(incoming, &q).await?;
            let v = check(&r)?;
            let e = entities_of(&v);
            out.found.extend(e.found);
            out.missing.extend(e.missing);
            caches.push(CacheInfo::of(&r));
        }
        Ok(Fetched {
            value: out,
            cache: CacheInfo::combine(&caches),
        })
    }

    /// `GET /entity/{id}/provenance`; `None` when the entity does not exist (or is not
    /// the viewer's to see, which the API answers alike).
    pub async fn provenance(
        &self,
        incoming: &Incoming,
        id: &str,
    ) -> Result<Fetched<Option<Provenance>>, ClientError> {
        let path = format!(
            "/w/rest.php/triplespace/v0/entity/{}/provenance",
            utf8_percent_encode(id, SEGMENT)
        );
        let r = self.get(incoming, &path).await?;
        let cache = CacheInfo::of(&r);
        let value = match r.status {
            StatusCode::OK => Some(
                serde_json::from_slice(&r.body)
                    .map_err(|e| ClientError::Unexpected(format!("provenance: {e}")))?,
            ),
            StatusCode::NOT_FOUND => None,
            s => {
                let v: Map<String, Value> = serde_json::from_slice(&r.body).unwrap_or_default();
                return Err(ClientError::Api {
                    code: v
                        .get("code")
                        .and_then(Value::as_str)
                        .map_or_else(|| format!("http-{}", s.as_u16()), str::to_string),
                    info: v
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        };
        Ok(Fetched { value, cache })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_split_found_and_missing() {
        let e = entities_of(&serde_json::json!({"entities": {
            "Q6": {"id": "Q6", "type": "item"},
            "Q7": {"id": "Q7", "missing": ""}
        }}));
        assert!(e.found.contains_key("Q6"));
        assert_eq!(e.missing, vec!["Q7"]);
    }

    #[test]
    fn provenance_parses_the_contract_shape() {
        let p: Provenance = serde_json::from_value(serde_json::json!({
            "id": "WDQ65", "canonical": "WDQ65", "type": "item",
            "minted_by": {"kind": "provider", "provider": "wikidata", "code": "WD", "upstream_id": "Q65"},
            "graphs": [{"graph": "mirror/wikidata", "statements": 40, "synced_at": "2026-10-01T00:00:00Z"}],
            "statements": {"WDQ65$x": {"graphs": ["mirror/wikidata"], "members": []}},
            "corrections": {}, "dominant": "mirror/wikidata"
        }))
        .unwrap();
        assert_eq!(p.minted_by.upstream_id.as_deref(), Some("Q65"));
        assert_eq!(p.graphs[0].statements, 40);
        assert_eq!(p.dominant.as_deref(), Some("mirror/wikidata"));
    }

    #[test]
    fn ids_are_encoded() {
        assert_eq!(
            utf8_percent_encode("Item:Q6|Domain:ex ample.org", QUERY).to_string(),
            "Item:Q6%7CDomain:ex%20ample.org"
        );
    }
}
