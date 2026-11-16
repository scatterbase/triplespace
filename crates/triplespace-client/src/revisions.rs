//! Past revisions and diffs (0012 §4, §7; 18 §2.3, §5.1): `prop=revisions` with content
//! for `oldid=`, `GET /entity/{id}/changes/{partition}/{offset}` for one change's diff,
//! and `GET /entity/{id}/compare` for two revisions'.

use axum::http::StatusCode;
use percent_encoding::utf8_percent_encode;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::Client;
use crate::api::{CacheInfo, Fetched, check};
use crate::entity::{QUERY, SEGMENT};
use crate::forward::Incoming;
use crate::transport::{ApiResponse, ClientError};

/// A local revision's facts, as the diff routes carry them (`RevisionFacts`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct RevisionFacts {
    /// The partition.
    #[serde(default)]
    pub partition: u64,
    /// The offset.
    #[serde(default)]
    pub offset: u64,
    /// The revision ID.
    #[serde(default)]
    pub revid: Option<u64>,
    /// ISO 8601.
    #[serde(default)]
    pub time: String,
    /// The actor key.
    #[serde(default)]
    pub actor: Option<String>,
    /// The comment.
    #[serde(default)]
    pub comment: Option<String>,
    /// Change tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The operation's name.
    #[serde(default)]
    pub op: Option<String>,
    /// A mirror record's upstream version.
    #[serde(default)]
    pub upstream: Option<Value>,
    /// The upstream version it replaced.
    #[serde(default)]
    pub prev_upstream: Option<Value>,
}

/// A keyed value on either side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TermChange {
    /// The language.
    pub language: String,
    /// Before.
    #[serde(default)]
    pub before: Option<String>,
    /// After.
    #[serde(default)]
    pub after: Option<String>,
}

/// A language's aliases on either side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AliasChange {
    /// The language.
    pub language: String,
    /// Added.
    #[serde(default)]
    pub added: Vec<String>,
    /// Removed.
    #[serde(default)]
    pub removed: Vec<String>,
    /// Kept.
    #[serde(default)]
    pub kept: Vec<String>,
}

/// A sitelink on either side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct SitelinkChange {
    /// The site.
    pub site: String,
    /// Before, as the API's JSON.
    #[serde(default)]
    pub before: Option<Value>,
    /// After.
    #[serde(default)]
    pub after: Option<Value>,
}

/// One statement's fate in a group: `added`, `removed`, `changed` or `unchanged`, with
/// the statements as the API's JSON (the storage form).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct StatementChange {
    /// The change.
    #[serde(default)]
    pub change: String,
    /// Before (removed, changed).
    #[serde(default)]
    pub before: Option<Value>,
    /// After (added, changed).
    #[serde(default)]
    pub after: Option<Value>,
    /// The statement (unchanged).
    #[serde(default)]
    pub statement: Option<Value>,
}

/// One property's group in a diff.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GroupDiff {
    /// The property.
    pub property: String,
    /// The statements.
    #[serde(default)]
    pub statements: Vec<StatementChange>,
}

/// The structured diff (`EntityDiff`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct EntityDiff {
    /// Labels.
    #[serde(default)]
    pub labels: Vec<TermChange>,
    /// Descriptions.
    #[serde(default)]
    pub descriptions: Vec<TermChange>,
    /// Aliases.
    #[serde(default)]
    pub aliases: Vec<AliasChange>,
    /// Sitelinks.
    #[serde(default)]
    pub sitelinks: Vec<SitelinkChange>,
    /// Statement groups.
    #[serde(default)]
    pub claims: Vec<GroupDiff>,
}

impl EntityDiff {
    /// Nothing differs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
            && self.descriptions.is_empty()
            && self.aliases.is_empty()
            && self.sitelinks.is_empty()
            && self.claims.is_empty()
    }
}

/// One change (`EntityChange`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Change {
    /// The entity.
    #[serde(default)]
    pub entity: String,
    /// The graph.
    #[serde(default)]
    pub source: String,
    /// `edit`, `sync`, …
    #[serde(default)]
    pub kind: String,
    /// The revision.
    #[serde(default)]
    pub revision: RevisionFacts,
    /// The previous local revision: `{revid, offset}`.
    #[serde(default)]
    pub previous: Option<Value>,
    /// The operation as stored.
    #[serde(default)]
    pub operation: Option<Value>,
    /// The diff, for a local record.
    #[serde(default)]
    pub diff: Option<EntityDiff>,
    /// The state changes beside the entity.
    #[serde(default)]
    pub state: Option<Value>,
    /// A sync's summary.
    #[serde(default)]
    pub changes: Option<Value>,
    /// A mirror record's state after.
    #[serde(default)]
    pub after: Option<Value>,
    /// Size after.
    #[serde(default)]
    pub size: Option<i64>,
    /// Size change.
    #[serde(default)]
    pub delta: Option<i64>,
}

/// A comparison (`EntityComparison`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Comparison {
    /// The entity.
    #[serde(default)]
    pub entity: String,
    /// From.
    #[serde(default)]
    pub from: RevisionFacts,
    /// To.
    #[serde(default)]
    pub to: RevisionFacts,
    /// The diff.
    #[serde(default)]
    pub diff: EntityDiff,
    /// The state changes beside the entity.
    #[serde(default)]
    pub state: Option<Value>,
}

/// A revision from `prop=revisions`, with its content when asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Revision {
    /// The revision ID.
    pub revid: u64,
    /// ISO 8601.
    pub timestamp: String,
    /// The user name.
    pub user: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The content: the entity's JSON at that revision.
    pub content: Option<String>,
    /// The page's title.
    pub title: String,
    /// The record: `(partition, offset)`.
    pub record: Option<(u64, u64)>,
}

fn rest_error(r: &ApiResponse) -> ClientError {
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
    /// `GET /entity/{id}/changes/{partition}/{offset}`; `None` for a record that is not
    /// there, or not the viewer's to see.
    pub async fn change(
        &self,
        incoming: &Incoming,
        id: &str,
        partition: u64,
        offset: u64,
    ) -> Result<Fetched<Option<Change>>, ClientError> {
        let path = format!(
            "/w/rest.php/triplespace/v0/entity/{}/changes/{partition}/{offset}",
            utf8_percent_encode(id, SEGMENT)
        );
        let r = self.get(incoming, &path).await?;
        let cache = CacheInfo::of(&r);
        let value = match r.status {
            StatusCode::OK => Some(
                serde_json::from_slice(&r.body)
                    .map_err(|e| ClientError::Unexpected(format!("change: {e}")))?,
            ),
            StatusCode::NOT_FOUND => None,
            _ => return Err(rest_error(&r)),
        };
        Ok(Fetched { value, cache })
    }

    /// `GET /entity/{id}/compare?from=&to=`; `None` when a revision is not there.
    pub async fn compare(
        &self,
        incoming: &Incoming,
        id: &str,
        from: u64,
        to: u64,
    ) -> Result<Fetched<Option<Comparison>>, ClientError> {
        let path = format!(
            "/w/rest.php/triplespace/v0/entity/{}/compare?source=local&from={from}&to={to}",
            utf8_percent_encode(id, SEGMENT)
        );
        let r = self.get(incoming, &path).await?;
        let cache = CacheInfo::of(&r);
        let value = match r.status {
            StatusCode::OK => Some(
                serde_json::from_slice(&r.body)
                    .map_err(|e| ClientError::Unexpected(format!("compare: {e}")))?,
            ),
            StatusCode::NOT_FOUND => None,
            _ => return Err(rest_error(&r)),
        };
        Ok(Fetched { value, cache })
    }

    /// `prop=revisions&revids={revid}` with the content and the record: the entity as it
    /// was after that revision; `None` when the revision is not a local revision of an
    /// entity.
    pub async fn revision(
        &self,
        incoming: &Incoming,
        revid: u64,
    ) -> Result<Fetched<Option<Revision>>, ClientError> {
        let q = format!(
            "/w/api.php?action=query&prop=revisions&revids={revid}&rvprop=ids%7Ctimestamp%7Cuser%7Ccomment%7Ccontent%7Crecord&rvslots=main&format=json&formatversion=2"
        );
        let r = self.get(incoming, &q).await?;
        let v = check(&r)?;
        let cache = CacheInfo::of(&r);
        let value = v["query"]["pages"]
            .as_array()
            .and_then(|pages| pages.iter().find(|p| p.get("missing").is_none()))
            .and_then(|p| {
                let rev = p["revisions"].as_array()?.first()?;
                Some(Revision {
                    revid: rev["revid"].as_u64()?,
                    timestamp: rev["timestamp"].as_str().unwrap_or_default().to_string(),
                    user: rev["user"].as_str().map(str::to_string),
                    comment: rev["comment"].as_str().map(str::to_string),
                    content: rev["slots"]["main"]["content"].as_str().map(str::to_string),
                    title: p["title"].as_str().unwrap_or_default().to_string(),
                    record: match (
                        rev["record"]["partition"].as_u64(),
                        rev["record"]["offset"].as_u64(),
                    ) {
                        (Some(partition), Some(offset)) => Some((partition, offset)),
                        _ => None,
                    },
                })
            });
        Ok(Fetched { value, cache })
    }

    /// `prop=revisions&titles=…` for the latest revision IDs of a page: the newest first,
    /// `limit` of them, with `rvprop=ids|timestamp|user|comment`.
    pub async fn revisions(
        &self,
        incoming: &Incoming,
        title: &str,
        limit: u32,
    ) -> Result<Fetched<Vec<Revision>>, ClientError> {
        let q = format!(
            "/w/api.php?action=query&prop=revisions&titles={}&rvprop=ids%7Ctimestamp%7Cuser%7Ccomment&rvlimit={limit}&format=json&formatversion=2",
            utf8_percent_encode(title, QUERY)
        );
        let r = self.get(incoming, &q).await?;
        let v = check(&r)?;
        let cache = CacheInfo::of(&r);
        let mut out = Vec::new();
        if let Some(p) = v["query"]["pages"]
            .as_array()
            .and_then(|pages| pages.first())
        {
            for rev in p["revisions"].as_array().into_iter().flatten() {
                if let Some(revid) = rev["revid"].as_u64() {
                    out.push(Revision {
                        revid,
                        timestamp: rev["timestamp"].as_str().unwrap_or_default().to_string(),
                        user: rev["user"].as_str().map(str::to_string),
                        comment: rev["comment"].as_str().map(str::to_string),
                        content: None,
                        title: p["title"].as_str().unwrap_or_default().to_string(),
                        record: None,
                    });
                }
            }
        }
        Ok(Fetched { value: out, cache })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_parses() {
        let c: Change = serde_json::from_value(serde_json::json!({
            "entity": "Q6", "source": "local", "kind": "edit",
            "record": {"partition": 4098, "offset": 9},
            "revision": {"partition": 4098, "offset": 9, "revid": 907, "time": "2026-10-02T14:05:11Z",
                         "actor": "librarybase:10", "comment": null, "tags": [], "op": "add"},
            "previous": {"revid": 906, "offset": 8},
            "operation": {"op": "add", "id": "Q6"},
            "diff": {"labels": [], "descriptions": [], "aliases": [{"language": "en", "added": ["VI"], "removed": [], "kept": []}],
                     "sitelinks": [], "claims": [{"property": "P12", "statements": [{"change": "unchanged", "statement": {}}]}]},
            "state": {"corrections": []}, "changes": null, "size": 412, "delta": 20, "effect": null
        }))
        .unwrap();
        assert_eq!(c.revision.revid, Some(907));
        let d = c.diff.unwrap();
        assert_eq!(d.aliases[0].added, vec!["VI"]);
        assert_eq!(d.claims[0].statements[0].change, "unchanged");
    }
}
