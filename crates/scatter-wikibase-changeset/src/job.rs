//! The job line (0002 §8.3, §8.7; 0035 §3): the first line of an NDJSON batch, naming
//! the source and its version, the mode, the target graph and whether the batch is
//! atomic. The job *record* the instance appends (0011 §6.3) adds the actor and the job
//! ID; this is only what the submitter says.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An import mode (0002 §8.4; 0035 §3). A local bulk job has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Adds and updates; never deletes.
    Upsert,
    /// The input is complete for one provider and type; what it did not see is
    /// tombstoned after it completes.
    Snapshot,
    /// An adoption job (0035): `adopt` operations only.
    Adopt,
}

/// Which partition a job writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Graph {
    /// The tenant's `local` partition.
    Local,
    /// `mirror/{slug}`: the provider's mirror partition.
    Mirror(String),
    /// The `pages` partition (0038 §1).
    Pages,
}

impl Graph {
    /// Parses a graph name: `local`, `pages` or `mirror/{slug}`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "local" => Some(Self::Local),
            "pages" => Some(Self::Pages),
            _ => name
                .strip_prefix("mirror/")
                .filter(|slug| {
                    !slug.is_empty()
                        && slug
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
                .map(|slug| Self::Mirror(slug.to_string())),
        }
    }

    /// The graph name.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Local => "local".to_string(),
            Self::Pages => "pages".to_string(),
            Self::Mirror(slug) => format!("mirror/{slug}"),
        }
    }

    /// The provider slug of a mirror graph.
    #[must_use]
    pub fn provider_slug(&self) -> Option<&str> {
        match self {
            Self::Mirror(slug) => Some(slug),
            _ => None,
        }
    }
}

/// The job line's object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobHeader {
    /// The source: a provider slug, a URL, or a name the submitter chooses.
    pub source: String,
    /// The source version: a dump date, a snapshot manifest, a file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The snapshot the input was taken from, where that differs from `version`; the
    /// 0002 §8.7 sketch's name for the version of a `snapshot`-mode job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    /// The import mode; absent for a local bulk job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// The graph written: `local`, `pages` or `mirror/{slug}`.
    pub graph: String,
    /// All-or-nothing (0002 §8.5); otherwise each operation is applied on its own and
    /// rejects go to the rejects file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub atomic: bool,
    /// The adapter version that produced the batch (0002 §8.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,
    /// The job's parameters, as the adapter or submitter records them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

impl JobHeader {
    /// A local bulk job.
    #[must_use]
    pub fn local(source: &str) -> Self {
        Self {
            source: source.to_string(),
            version: None,
            snapshot: None,
            mode: None,
            graph: "local".to_string(),
            atomic: false,
            adapter_version: None,
            params: BTreeMap::new(),
        }
    }

    /// A mirror sync job for a provider.
    #[must_use]
    pub fn mirror(slug: &str, version: &str, mode: Mode) -> Self {
        Self {
            source: slug.to_string(),
            version: Some(version.to_string()),
            snapshot: None,
            mode: Some(mode),
            graph: format!("mirror/{slug}"),
            atomic: false,
            adapter_version: None,
            params: BTreeMap::new(),
        }
    }

    /// The parsed graph, when the name is one.
    #[must_use]
    pub fn graph(&self) -> Option<Graph> {
        Graph::parse(&self.graph)
    }

    /// The source version, whichever field names it.
    #[must_use]
    pub fn source_version(&self) -> Option<&str> {
        self.version.as_deref().or(self.snapshot.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphs_parse() {
        assert_eq!(Graph::parse("local"), Some(Graph::Local));
        assert_eq!(Graph::parse("pages"), Some(Graph::Pages));
        assert_eq!(
            Graph::parse("mirror/openalex"),
            Some(Graph::Mirror("openalex".into()))
        );
        assert_eq!(Graph::parse("mirror/"), None);
        assert_eq!(Graph::parse("mirror/Open Alex"), None);
        assert_eq!(Graph::parse("accounts"), None);
        assert_eq!(Graph::Mirror("wikidata".into()).name(), "mirror/wikidata");
    }

    #[test]
    fn the_sketches_parse() {
        let j: JobHeader = serde_json::from_str(
            r#"{"source":"openalex","snapshot":"2026-09-01","mode":"upsert","graph":"mirror/openalex"}"#,
        )
        .unwrap();
        assert_eq!(j.mode, Some(Mode::Upsert));
        assert_eq!(j.source_version(), Some("2026-09-01"));
        assert_eq!(j.graph(), Some(Graph::Mirror("openalex".into())));
        let j: JobHeader =
            serde_json::from_str(r#"{"source":"citation-batch","graph":"local","atomic":true}"#)
                .unwrap();
        assert!(j.atomic && j.mode.is_none());
        assert_eq!(
            serde_json::to_string(&j).unwrap(),
            r#"{"source":"citation-batch","graph":"local","atomic":true}"#
        );
        let j: JobHeader = serde_json::from_str(
            r#"{"source":"https://librarybase.org/","version":"librarybase-20260928.json.gz","mode":"adopt","graph":"local"}"#,
        )
        .unwrap();
        assert_eq!(j.mode, Some(Mode::Adopt));
        assert!(
            serde_json::from_str::<JobHeader>(r#"{"source":"x","graph":"local","bogus":1}"#)
                .is_err()
        );
    }
}
