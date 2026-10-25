//! `GET /activity` (0020 §4–5; 16 §3.1): a change feed, as the site's recent changes,
//! histories and contributions read it.

use std::fmt::Write as _;

use percent_encoding::utf8_percent_encode;
use serde::Deserialize;
use serde_json::{Map, Value};

use axum::http::StatusCode;

use crate::Client;
use crate::api::{CacheInfo, Fetched};
use crate::entity::{QUERY, SEGMENT};
use crate::forward::Incoming;
use crate::transport::ClientError;

/// A row's target.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ActivityTarget {
    /// `entity`, `page`, `actor`, `job` or `record`.
    #[serde(default)]
    pub kind: String,
    /// The ID.
    #[serde(default)]
    pub id: String,
    /// An entity's type.
    #[serde(default)]
    pub entity_type: Option<String>,
    /// The namespace.
    #[serde(default)]
    pub ns: Option<i64>,
    /// The title.
    #[serde(default)]
    pub title: Option<String>,
    /// The label in the language asked for: `{value, language}`.
    #[serde(default)]
    pub label: Option<Term>,
}

/// A label with its language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Term {
    /// The text.
    pub value: String,
    /// Its language.
    pub language: String,
}

/// A row's actor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ActivityActor {
    /// The key.
    #[serde(default)]
    pub key: Option<String>,
    /// The name.
    #[serde(default)]
    pub name: Option<String>,
    /// The kind: `registered`, `bot`, `anonymous`, …
    #[serde(default)]
    pub kind: Option<String>,
    /// Hidden from the viewer.
    #[serde(default)]
    pub hidden: bool,
}

/// A row's summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Summary {
    /// The operation.
    #[serde(default)]
    pub op: Option<String>,
    /// Its arguments.
    #[serde(default)]
    pub args: Option<String>,
    /// MediaWiki's comment form.
    #[serde(default)]
    pub text: Option<String>,
    /// Hidden from the viewer.
    #[serde(default)]
    pub hidden: bool,
}

/// The flags.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Flags {
    /// Minor.
    #[serde(default)]
    pub minor: bool,
    /// By a bot.
    #[serde(default)]
    pub bot: bool,
    /// A creation.
    #[serde(default)]
    pub new: bool,
}

/// A log event's type and action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LogInfo {
    /// The type.
    #[serde(default)]
    pub r#type: String,
    /// The action.
    #[serde(default)]
    pub action: String,
    /// The parameters.
    #[serde(default)]
    pub params: Option<Value>,
}

/// The record a row is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct RecordRef {
    /// The partition.
    pub partition: u64,
    /// The offset.
    pub offset: u64,
}

/// One activity row (16 §3.1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ActivityRow {
    /// `edit`, `sync`, `log`, `job` or `erased`.
    pub kind: String,
    /// ISO 8601.
    pub time: String,
    /// The graph.
    pub source: String,
    /// The record.
    pub record: RecordRef,
    /// The revision ID.
    #[serde(default)]
    pub revid: Option<u64>,
    /// The log ID.
    #[serde(default)]
    pub logid: Option<u64>,
    /// The target.
    #[serde(default)]
    pub target: Option<ActivityTarget>,
    /// The actor.
    #[serde(default)]
    pub actor: ActivityActor,
    /// The job.
    #[serde(default)]
    pub job: Option<u64>,
    /// The summary.
    #[serde(default)]
    pub summary: Summary,
    /// The user's comment.
    #[serde(default)]
    pub comment: Option<String>,
    /// Change tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The flags.
    #[serde(default)]
    pub flags: Flags,
    /// Patrolled.
    #[serde(default)]
    pub patrolled: bool,
    /// The size after.
    #[serde(default)]
    pub size: Option<i64>,
    /// The size change.
    #[serde(default)]
    pub delta: Option<i64>,
    /// A sync's change summary.
    #[serde(default)]
    pub changes: Option<Value>,
    /// A log event.
    #[serde(default)]
    pub log: Option<LogInfo>,
    /// The hidden parts.
    #[serde(default)]
    pub visibility: Vec<String>,
    /// Links: `diff`, `history`, `job`.
    #[serde(default)]
    pub links: Map<String, Value>,
}

/// One page of a feed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ActivityPage {
    /// The rows.
    #[serde(default)]
    pub rows: Vec<ActivityRow>,
    /// The next page's token.
    #[serde(default)]
    pub next: Option<String>,
}

/// What to ask for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityQuery {
    /// `{kind}:{id}`: one target's history.
    pub target: Option<String>,
    /// An actor's contributions, by name or key.
    pub actor: Option<String>,
    /// Row kinds; the API's default when empty.
    pub kinds: Vec<String>,
    /// Graphs (`local`, `mirror/wikidata`); every graph when empty.
    pub sources: Vec<String>,
    /// Entity namespaces.
    pub namespaces: Vec<String>,
    /// `show` values: `bot`, `!bot`, …
    pub show: Vec<String>,
    /// A tag.
    pub tag: Option<String>,
    /// A log type (`delete`, `protect`, `rights`, `block`, …), for `Special:Log`.
    pub log_type: Option<String>,
    /// From this time, ISO 8601.
    pub since: Option<String>,
    /// Newest first unless set.
    pub newer_first: bool,
    /// Rows per page.
    pub limit: u32,
    /// The previous page's `next`.
    pub cursor: Option<String>,
    /// The language of the labels.
    pub lang: String,
}

impl ActivityQuery {
    /// The path and query of `GET /activity`.
    #[must_use]
    pub fn path(&self) -> String {
        format!(
            "/w/rest.php/triplespace/v0/activity?{}",
            self.query_string()
        )
    }

    /// The path and query of `GET /entity/{id}/history`; `target` and `actor` are not
    /// sent, since the path names the target.
    #[must_use]
    pub fn history_path(&self, id: &str) -> String {
        let q = Self {
            target: None,
            actor: None,
            ..self.clone()
        };
        format!(
            "/w/rest.php/triplespace/v0/entity/{}/history?{}",
            utf8_percent_encode(id, SEGMENT),
            q.query_string()
        )
    }

    /// The path and query of `GET /page/{id}/history`: a page's history, composite for a
    /// talk page or board (16 §6.1).
    #[must_use]
    pub fn page_history_path(&self, page_id: u64) -> String {
        let q = Self {
            target: None,
            actor: None,
            ..self.clone()
        };
        format!(
            "/w/rest.php/triplespace/v0/page/{page_id}/history?{}",
            q.query_string()
        )
    }

    /// The query string, without its `?`.
    fn query_string(&self) -> String {
        let mut q = String::new();
        let mut push = |k: &str, v: &str| {
            if !q.is_empty() {
                q.push('&');
            }
            let _ = write!(q, "{k}={}", utf8_percent_encode(v, QUERY));
        };
        if let Some(t) = &self.target {
            push("target", t);
        }
        if let Some(a) = &self.actor {
            push("actor", a);
        }
        if !self.kinds.is_empty() {
            push("kind", &self.kinds.join("|"));
        }
        if !self.sources.is_empty() {
            push("source", &self.sources.join("|"));
        }
        if !self.namespaces.is_empty() {
            push("namespace", &self.namespaces.join("|"));
        }
        if !self.show.is_empty() {
            push("show", &self.show.join("|"));
        }
        if let Some(t) = &self.tag {
            push("tag", t);
        }
        if let Some(t) = &self.log_type {
            push("log_type", t);
        }
        if let Some(s) = &self.since {
            push("since", s);
        }
        if self.newer_first {
            push("dir", "newer");
        }
        if self.limit > 0 {
            push("limit", &self.limit.to_string());
        }
        if let Some(c) = &self.cursor {
            push("continue", c);
        }
        if !self.lang.is_empty() {
            push("lang", &self.lang);
        }
        q
    }
}

impl Client {
    /// `GET /activity`.
    pub async fn activity(
        &self,
        incoming: &Incoming,
        query: &ActivityQuery,
    ) -> Result<Fetched<ActivityPage>, ClientError> {
        self.feed(incoming, &query.path()).await
    }

    /// `GET /page/{id}/history`: a page's rows, and for a talk page or board those of
    /// every thread attached to it (16 §6.1); `None` for a page that does not exist or is
    /// not the viewer's to see.
    pub async fn page_history(
        &self,
        incoming: &Incoming,
        page_id: u64,
        query: &ActivityQuery,
    ) -> Result<Fetched<Option<ActivityPage>>, ClientError> {
        let r = self
            .get(incoming, &query.page_history_path(page_id))
            .await?;
        if r.status == StatusCode::NOT_FOUND || r.status == StatusCode::GONE {
            return Ok(Fetched {
                value: None,
                cache: CacheInfo::of(&r),
            });
        }
        let page = page_of(&r)?;
        Ok(Fetched {
            value: Some(page.value),
            cache: page.cache,
        })
    }

    /// `GET /entity/{id}/history`; `None` when the entity does not exist (or is not the
    /// viewer's to see, which the API answers alike).
    pub async fn history(
        &self,
        incoming: &Incoming,
        id: &str,
        query: &ActivityQuery,
    ) -> Result<Fetched<Option<ActivityPage>>, ClientError> {
        let r = self.get(incoming, &query.history_path(id)).await?;
        if r.status == StatusCode::NOT_FOUND {
            return Ok(Fetched {
                value: None,
                cache: CacheInfo::of(&r),
            });
        }
        let page = page_of(&r)?;
        Ok(Fetched {
            value: Some(page.value),
            cache: page.cache,
        })
    }

    async fn feed(
        &self,
        incoming: &Incoming,
        path: &str,
    ) -> Result<Fetched<ActivityPage>, ClientError> {
        let r = self.get(incoming, path).await?;
        page_of(&r)
    }
}

fn page_of(r: &crate::transport::ApiResponse) -> Result<Fetched<ActivityPage>, ClientError> {
    {
        let cache = CacheInfo::of(r);
        if r.status != StatusCode::OK {
            let v: Map<String, Value> = serde_json::from_slice(&r.body).unwrap_or_default();
            return Err(ClientError::Api {
                code: v
                    .get("code")
                    .and_then(Value::as_str)
                    .map_or_else(|| format!("http-{}", r.status.as_u16()), str::to_string),
                info: v
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
        let value = serde_json::from_slice(&r.body)
            .map_err(|e| ClientError::Unexpected(format!("activity: {e}")))?;
        Ok(Fetched { value, cache })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_encodes_its_parameters() {
        let q = ActivityQuery {
            actor: Some("Alice bot".into()),
            kinds: vec!["edit".into(), "log".into()],
            show: vec!["!bot".into()],
            limit: 50,
            cursor: Some("1|2|3".into()),
            lang: "fr".into(),
            ..ActivityQuery::default()
        };
        assert_eq!(
            q.path(),
            "/w/rest.php/triplespace/v0/activity?actor=Alice%20bot&kind=edit%7Clog&show=%21bot&limit=50&continue=1%7C2%7C3&lang=fr"
        );
        assert_eq!(
            ActivityQuery::default().path(),
            "/w/rest.php/triplespace/v0/activity?"
        );
        let h = ActivityQuery {
            target: Some("entity:Q6".into()),
            sources: vec!["local".into()],
            limit: 20,
            ..ActivityQuery::default()
        };
        assert_eq!(
            h.history_path("domain:wikipedia.org"),
            "/w/rest.php/triplespace/v0/entity/domain:wikipedia.org/history?source=local&limit=20",
            "the target is the path, not a parameter"
        );
    }

    #[test]
    fn rows_parse() {
        let p: ActivityPage = serde_json::from_value(serde_json::json!({
            "rows": [{
                "kind": "edit", "time": "2026-09-25T00:43:32Z", "source": "local",
                "record": {"partition": 4098, "offset": 9}, "revid": 901,
                "target": {"kind": "entity", "id": "Q6", "entity_type": "item", "ns": 120,
                           "title": "Item:Q6", "label": {"value": "Six", "language": "en"}},
                "actor": {"key": "librarybase:7", "name": "Alice", "kind": "registered"},
                "summary": {"op": "add", "args": "labels:fr", "text": "/* add:labels:fr */ x"},
                "comment": "x", "tags": [], "flags": {"minor": false, "bot": false, "new": false},
                "patrolled": false, "size": 400, "delta": 20, "visibility": [],
                "links": {"diff": "https://x/d"}
            }],
            "next": "1|2|3"
        }))
        .unwrap();
        assert_eq!(
            p.rows[0].target.as_ref().unwrap().title.as_deref(),
            Some("Item:Q6")
        );
        assert_eq!(p.rows[0].actor.name.as_deref(), Some("Alice"));
        assert_eq!(p.rows[0].delta, Some(20));
        assert_eq!(p.next.as_deref(), Some("1|2|3"));
    }
}
