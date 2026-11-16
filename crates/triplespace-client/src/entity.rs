//! The entity calls the site makes: for an entity page, `GET /entity/{id}/page`, the page
//! bundle of the entity and where it comes from, and `GET /labels`, the labels of what
//! it refers to, the two calls of the page's budget (0012 §5, A70; 19 §5.1); and, for the
//! pages that need more of other entities than their labels, `wbgetentities` and
//! `GET /entity/{id}/provenance`.
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
pub(crate) const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':');

/// What a path segment keeps unencoded.
pub(crate) const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':');

/// The most IDs one `wbgetentities` call takes.
pub const IDS_PER_CALL: usize = 50;

/// The most IDs one `GET /labels` call takes (0012 A70).
pub const LABELS_PER_CALL: usize = 1000;

/// The most bytes of IDs one `GET /labels` call carries in its URL, which a thousand
/// IDs stay under unless they are long keyed ones: a URL is at most 64 KiB to the
/// servers in between.
pub const LABEL_BYTES_PER_CALL: usize = 48 * 1024;

/// The page bundle, `GET /entity/{id}/page` (0012 A70): an entity and its provenance.
#[derive(Debug, Clone, Default)]
pub struct Bundle {
    /// The entity's ID, canonical.
    pub id: String,
    /// The entity, as `wbgetentities` serves it.
    pub entity: Value,
    /// Where it comes from; `None` only where the API left it out.
    pub provenance: Option<Provenance>,
    /// The data type of each property a snak of the entity names without saying it.
    pub datatypes: BTreeMap<String, String>,
    /// Who may read it, where it is restricted (19 §1.3).
    pub visibility: Option<crate::pages::Visibility>,
    /// Whether the ID asked for is an alias of this entity (05 §4.1).
    pub redirected: bool,
}

/// A label and its language.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Label {
    /// The language of the chain it was found in.
    pub language: String,
    /// The text.
    pub value: String,
}

/// Splits IDs into the calls `GET /labels` takes: at most [`LABELS_PER_CALL`] IDs and
/// [`LABEL_BYTES_PER_CALL`] bytes of them each.
fn label_calls(ids: &[String]) -> Vec<&[String]> {
    let mut out = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    for (i, id) in ids.iter().enumerate() {
        // An ID and its separator, as the URL encodes them at their longest.
        let cost = id.len() * 3 + 3;
        if i > start && (i - start == LABELS_PER_CALL || bytes + cost > LABEL_BYTES_PER_CALL) {
            out.push(&ids[start..i]);
            start = i;
            bytes = 0;
        }
        bytes += cost;
    }
    if start < ids.len() {
        out.push(&ids[start..]);
    }
    out
}

/// The entities of one or more `wbgetentities` calls, by the ID the API keyed them under.
#[derive(Debug, Clone, Default)]
pub struct Entities {
    /// The entities found, as the API served them.
    pub found: BTreeMap<String, Value>,
    /// The IDs the API reported missing.
    pub missing: Vec<String>,
    /// The visibility of each restricted entity served (`ts-visibility`; 19 §1.3).
    pub visibility: BTreeMap<String, crate::pages::Visibility>,
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
    if let Some(map) = v["ts-visibility"].as_object() {
        for (k, vis) in map {
            if let Some(vis) = crate::pages::Visibility::of(vis) {
                out.visibility.insert(k.clone(), vis);
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
    /// `tenant`, `provider`, `source` or `keyed`.
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
    /// The entity source's name, for a source ID (0078 §2).
    #[serde(default)]
    pub source: Option<String>,
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
    /// The page of that upstream revision, where the API gives one.
    #[serde(default)]
    pub upstream_url: Option<String>,
    /// The provider's own ID of the item the graph's record was mapped from, for a
    /// key-mapped provider's graph on a keyed entity (05 §4.2; 0002 A44).
    #[serde(default)]
    pub upstream_id: Option<String>,
    /// That item's concept IRI, where the API gives one.
    #[serde(default)]
    pub upstream_iri: Option<String>,
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

/// The error a REST response other than `200` and `404` stands for.
fn rest_error(r: &crate::ApiResponse) -> ClientError {
    let v: Map<String, Value> = serde_json::from_slice(&r.body).unwrap_or_default();
    ClientError::Api {
        code: v
            .get("code")
            .and_then(Value::as_str)
            .map_or_else(|| format!("http-{}", r.status.as_u16()), str::to_string),
        info: v
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

impl Client {
    /// `GET /entity/{id}/page`, the page bundle (0012 A70), for an ID or a title
    /// (`Item:Q6`); `None` when the entity does not exist (or is not the viewer's to
    /// see, which the API answers alike).
    pub async fn page_bundle(
        &self,
        incoming: &Incoming,
        id: &str,
    ) -> Result<Fetched<Option<Bundle>>, ClientError> {
        let path = format!(
            "/w/rest.php/triplespace/v0/entity/{}/page",
            utf8_percent_encode(id, SEGMENT)
        );
        let r = self.get(incoming, &path).await?;
        let cache = CacheInfo::of(&r);
        let value = match r.status {
            StatusCode::OK => {
                let mut v: Map<String, Value> = serde_json::from_slice(&r.body)
                    .map_err(|e| ClientError::Unexpected(format!("the page bundle: {e}")))?;
                let provenance = match v.remove("provenance") {
                    Some(p) if !p.is_null() => Some(serde_json::from_value(p).map_err(|e| {
                        ClientError::Unexpected(format!("the page bundle's provenance: {e}"))
                    })?),
                    _ => None,
                };
                Some(Bundle {
                    id: v
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    entity: v.remove("entity").unwrap_or_default(),
                    provenance,
                    datatypes: v
                        .remove("datatypes")
                        .and_then(|d| serde_json::from_value(d).ok())
                        .unwrap_or_default(),
                    visibility: v.get("visibility").and_then(crate::pages::Visibility::of),
                    redirected: v.contains_key("redirects"),
                })
            }
            StatusCode::NOT_FOUND => None,
            _ => return Err(rest_error(&r)),
        };
        Ok(Fetched { value, cache })
    }

    /// `GET /labels` (0012 A70): the label of each ID in the language chain, by the ID
    /// as given; an ID with none is absent. One call for up to [`LABELS_PER_CALL`] IDs;
    /// more run one after another, their caching facts combined.
    pub async fn labels(
        &self,
        incoming: &Incoming,
        ids: &[String],
        chain: &[String],
    ) -> Result<Fetched<BTreeMap<String, Label>>, ClientError> {
        #[derive(Deserialize)]
        struct Answer {
            #[serde(default)]
            labels: BTreeMap<String, Label>,
        }
        let mut out = BTreeMap::new();
        let mut caches = Vec::new();
        let language = utf8_percent_encode(&chain.join("|"), QUERY).to_string();
        for call in label_calls(ids) {
            let path = format!(
                "/w/rest.php/triplespace/v0/labels?ids={}&language={language}",
                utf8_percent_encode(&call.join("|"), QUERY)
            );
            let r = self.get(incoming, &path).await?;
            if r.status != StatusCode::OK {
                return Err(rest_error(&r));
            }
            let a: Answer = serde_json::from_slice(&r.body)
                .map_err(|e| ClientError::Unexpected(format!("labels: {e}")))?;
            out.extend(a.labels);
            caches.push(CacheInfo::of(&r));
        }
        Ok(Fetched {
            value: out,
            cache: CacheInfo::combine(&caches),
        })
    }

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
            out.visibility.extend(e.visibility);
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

    // 0012 §5 (A70): a thousand IDs are one call.
    #[test]
    fn label_calls_hold_a_thousand_ids_and_a_bounded_url() {
        let ids: Vec<String> = (1..=2500).map(|i| format!("Q{i}")).collect();
        let calls = label_calls(&ids);
        assert_eq!(
            calls.iter().map(|c| c.len()).collect::<Vec<_>>(),
            [1000, 1000, 500]
        );
        assert_eq!(label_calls(&ids[..700]).len(), 1);
        assert!(label_calls(&[]).is_empty());
        // Long keyed IDs are split by their bytes.
        let long: Vec<String> = (0..900)
            .map(|i| format!("domain:{}.example", "x".repeat(40 + i % 3)))
            .collect();
        let calls = label_calls(&long);
        assert!(calls.len() > 1);
        assert_eq!(calls.iter().map(|c| c.len()).sum::<usize>(), 900);
        for c in calls {
            let bytes: usize = c.iter().map(|id| id.len() * 3 + 3).sum();
            assert!(bytes <= LABEL_BYTES_PER_CALL);
        }
    }

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
