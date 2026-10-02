//! The graph registry (0005 §4.1–4.2; 0015 §3, §5): the reserved graph names of
//! `docs/registry/graphs.toml`, which this crate embeds, and the two `config` record
//! kinds the log cannot start without, `key:` and `graph:` (payloads.md §4).

use serde::{Deserialize, Serialize};

use crate::cbor::{self, SerdeError, Value};

/// The default registry, `docs/registry/graphs.toml`.
pub const GRAPHS_TOML: &str = include_str!("../../../docs/registry/graphs.toml");

/// The payload type of configuration records.
pub const PAYLOAD_CONFIG: &str = "scatter:v0/config";

/// The one hash function this version defines.
pub const HASH_SHA256: &str = "sha-256";

/// What a graph is (0005 §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A log partition.
    Source,
    /// Computed from source graphs.
    Projection,
}

/// Where a graph exists (0018 §2; 0046 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Once per tenant, under the tenant's base.
    Tenant,
    /// Once, under `{farm base}/instance/`.
    Instance,
    /// Both an instance graph and one per tenant.
    Both,
}

/// The history policy (0002 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum History {
    /// Every record is kept.
    Full,
    /// Compaction may drop superseded records.
    Latest,
}

/// The integrity policy (0006 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Integrity {
    /// One tree for the partition, with signed checkpoints.
    Logged,
    /// One tree per sealed segment, with a signed manifest each.
    Hashed,
}

/// The export policy (0005 §4.1; 0007 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Export {
    /// In the public dump.
    Public,
    /// In the full dump only.
    Internal,
    /// Never leaves the instance except in a tenant move.
    Private,
}

/// What a name's placeholder is instantiated with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Per {
    /// One graph per registered provider: `{provider}` is the provider slug.
    Provider,
    /// One graph per mirrored repository: `{repo}` is the repository name.
    Repo,
    /// One graph for the farm as an issuer: `{farm}` is the farm's slug (0028 §2).
    Farm,
}

impl Per {
    /// The placeholder in the registry name.
    #[must_use]
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Provider => "{provider}",
            Self::Repo => "{repo}",
            Self::Farm => "{farm}",
        }
    }
}

/// One row of `graphs.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graph {
    /// The reserved name, with its placeholder when `per` is set.
    pub name: String,
    /// Source or projection.
    pub kind: Kind,
    /// Tenant, instance or both.
    pub scope: Scope,
    /// The fixed partition number, only for `config` (0).
    pub partition: Option<u64>,
    /// The history policy; sources only.
    pub history: Option<History>,
    /// The integrity policy; sources only.
    pub integrity: Option<Integrity>,
    /// The export policy.
    pub export: Export,
    /// The payload types the partition accepts; sources only.
    pub payload_types: Vec<String>,
    /// What the name's placeholder stands for, when it has one.
    pub per: Option<Per>,
    /// A projection's description.
    pub description: Option<String>,
    /// The ADR sections that define it.
    pub defined_in: String,
    /// The Scatterbase graph it corresponds to.
    pub scatterbase: Option<String>,
}

impl Graph {
    /// The instantiated name for a provider slug, repository or farm: `mirror/wikidata`.
    /// Returns the name itself for a graph without a placeholder.
    #[must_use]
    pub fn instantiate(&self, with: &str) -> String {
        match self.per {
            Some(per) => self.name.replace(per.placeholder(), with),
            None => self.name.clone(),
        }
    }

    /// Whether `name` is this graph: the name itself, or an instantiation of it, in
    /// which case the slug is returned.
    #[must_use]
    pub fn matches<'a>(&self, name: &'a str) -> Option<Option<&'a str>> {
        match self.per {
            None => (name == self.name).then_some(None),
            Some(per) => {
                let (prefix, suffix) = self.name.split_once(per.placeholder())?;
                let rest = name.strip_prefix(prefix)?;
                let slug = rest.strip_suffix(suffix)?;
                (!slug.is_empty() && !slug.contains('/')).then_some(Some(slug))
            }
        }
    }
}

/// Why a `graphs.toml` is not a registry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The TOML does not parse or has an unknown field.
    #[error("graphs.toml: {0}")]
    Toml(String),
    /// A version this crate does not read.
    #[error("graphs.toml: version {0} is not supported")]
    Version(u32),
    /// Two rows share a name.
    #[error("graph `{0}` is declared twice")]
    Duplicate(String),
    /// A row is inconsistent.
    #[error("graph `{0}`: {1}")]
    Row(String, &'static str),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default, rename = "graph")]
    graphs: Vec<RawGraph>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGraph {
    name: String,
    kind: Kind,
    scope: Scope,
    #[serde(default)]
    partition: Option<u64>,
    #[serde(default)]
    history: Option<History>,
    #[serde(default)]
    integrity: Option<Integrity>,
    export: Export,
    #[serde(default)]
    payload_types: Vec<String>,
    #[serde(default)]
    per_provider: bool,
    #[serde(default)]
    per_repo: bool,
    #[serde(default)]
    per_farm: bool,
    #[serde(default)]
    description: Option<String>,
    defined_in: String,
    #[serde(default)]
    scatterbase: Option<String>,
}

/// The graph registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRegistry {
    version: u32,
    graphs: Vec<Graph>,
}

impl GraphRegistry {
    /// Parses a `graphs.toml`.
    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| RegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(RegistryError::Version(raw.version));
        }
        let mut graphs: Vec<Graph> = Vec::with_capacity(raw.graphs.len());
        for r in raw.graphs {
            if graphs.iter().any(|g| g.name == r.name) {
                return Err(RegistryError::Duplicate(r.name));
            }
            graphs.push(Self::row(r)?);
        }
        Ok(Self {
            version: raw.version,
            graphs,
        })
    }

    fn row(r: RawGraph) -> Result<Graph, RegistryError> {
        let row = |what| RegistryError::Row(r.name.clone(), what);
        let per = match (r.per_provider, r.per_repo, r.per_farm) {
            (false, false, false) => None,
            (true, false, false) => Some(Per::Provider),
            (false, true, false) => Some(Per::Repo),
            (false, false, true) => Some(Per::Farm),
            _ => return Err(row("more than one of per_provider, per_repo and per_farm")),
        };
        match per {
            Some(p) if !r.name.contains(p.placeholder()) => {
                return Err(row("the name lacks its placeholder"));
            }
            None if r.name.contains('{') => return Err(row("a placeholder without per_*")),
            _ => {}
        }
        match r.kind {
            Kind::Source => {
                if r.history.is_none() || r.integrity.is_none() {
                    return Err(row("a source graph has history and integrity policies"));
                }
                if r.payload_types.is_empty() {
                    return Err(row("a source graph has payload types"));
                }
            }
            Kind::Projection => {
                if r.history.is_some() || r.integrity.is_some() || !r.payload_types.is_empty() {
                    return Err(row("a projection has no log policies"));
                }
                if r.partition.is_some() {
                    return Err(row("a projection has no partition"));
                }
            }
        }
        if r.partition
            .is_some_and(|p| p != crate::header::CONFIG_PARTITION || r.name != "config")
        {
            return Err(row("only `config` has a fixed partition, 0"));
        }
        Ok(Graph {
            name: r.name,
            kind: r.kind,
            scope: r.scope,
            partition: r.partition,
            history: r.history,
            integrity: r.integrity,
            export: r.export,
            payload_types: r.payload_types,
            per,
            description: r.description,
            defined_in: r.defined_in,
            scatterbase: r.scatterbase,
        })
    }

    /// The embedded default registry.
    ///
    /// # Panics
    ///
    /// If the embedded file is invalid, which the tests rule out.
    #[must_use]
    pub fn embedded() -> Self {
        Self::parse(GRAPHS_TOML).expect("docs/registry/graphs.toml is valid")
    }

    /// The file version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every row.
    #[must_use]
    pub fn graphs(&self) -> &[Graph] {
        &self.graphs
    }

    /// The row for a name, which may be an instantiated one (`mirror/wikidata`); the
    /// slug it was instantiated with comes back with it.
    #[must_use]
    pub fn lookup<'a>(&self, name: &'a str) -> Option<(&Graph, Option<&'a str>)> {
        // An exact name wins over a template that would also match it.
        if let Some(g) = self.graphs.iter().find(|g| g.name == name) {
            return Some((g, None));
        }
        self.graphs
            .iter()
            .find_map(|g| g.matches(name).map(|s| (g, s)))
    }
}

/// The header key of a configuration record: `{kind}:{code}`.
#[must_use]
pub fn config_key(kind: &str, code: &str) -> String {
    format!("{kind}:{code}")
}

/// The content of a `graph:` record: a registry row with the instance's values filled
/// in (payloads.md §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEntry {
    /// Always `graph`.
    pub kind: String,
    /// The instantiated name.
    pub name: String,
    /// The TOML `kind`.
    pub kind_of: Kind,
    /// The scope.
    pub scope: Scope,
    /// The history policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<History>,
    /// The integrity policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<Integrity>,
    /// The export policy.
    pub export: Export,
    /// The partition ID; absent for a projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition: Option<u64>,
    /// The segment exponent `k`; absent for a projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_exponent: Option<u8>,
    /// The hash function, `sha-256`; absent for a projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// The payload types; absent for a projection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_types: Vec<String>,
}

impl GraphEntry {
    /// The entry for a source partition of the registry row `graph`.
    #[must_use]
    pub fn source(graph: &Graph, name: String, partition: u64, segment_exponent: u8) -> Self {
        Self {
            kind: "graph".into(),
            name,
            kind_of: graph.kind,
            scope: graph.scope,
            history: graph.history,
            integrity: graph.integrity,
            export: graph.export,
            partition: Some(partition),
            segment_exponent: Some(segment_exponent),
            hash: Some(HASH_SHA256.into()),
            payload_types: graph.payload_types.clone(),
        }
    }

    /// The header key, `graph:{name}`.
    #[must_use]
    pub fn key(&self) -> String {
        config_key("graph", &self.name)
    }

    /// The content part's value.
    pub fn to_value(&self) -> Result<Value, SerdeError> {
        cbor::to_value(self)
    }

    /// From a content part's value.
    pub fn from_value(v: Value) -> Result<Self, SerdeError> {
        let entry: Self = cbor::from_value(v)?;
        if entry.kind != "graph" {
            return Err(SerdeError::message(format!(
                "a graph entry has kind `graph`, not `{}`",
                entry.kind
            )));
        }
        Ok(entry)
    }
}

/// The content of a `key:` record: the instance's public key (payloads.md §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEntry {
    /// The signed-note key ID, z-base-32.
    pub key_id: String,
    /// The algorithm, `ed25519`.
    pub alg: String,
    /// The public key bytes.
    pub public_key: Vec<u8>,
    /// For a tenant move: the tenant's final checkpoint under the old key, as text.
    pub final_checkpoint: Option<String>,
}

impl KeyEntry {
    /// The header key, `key:{key_id}`.
    #[must_use]
    pub fn key(&self) -> String {
        config_key("key", &self.key_id)
    }

    /// The content part's value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut pairs = vec![
            (Value::text("kind"), Value::text("key")),
            (Value::text("key_id"), Value::Text(self.key_id.clone())),
            (Value::text("alg"), Value::Text(self.alg.clone())),
            (
                Value::text("public_key"),
                Value::Bytes(self.public_key.clone()),
            ),
        ];
        if let Some(c) = &self.final_checkpoint {
            pairs.push((Value::text("final_checkpoint"), Value::Text(c.clone())));
        }
        Value::map(pairs)
    }

    /// From a content part's value.
    pub fn from_value(v: &Value) -> Result<Self, SerdeError> {
        let Value::Map(pairs) = v else {
            return Err(SerdeError::message("a key entry is a map"));
        };
        let field = |name: &str| pairs.iter().find(|(k, _)| k.text_eq(name)).map(|(_, v)| v);
        let text = |name: &str| match field(name) {
            Some(Value::Text(t)) => Ok(t.clone()),
            _ => Err(SerdeError::message(format!(
                "key entry: `{name}` is a text field"
            ))),
        };
        if text("kind")? != "key" {
            return Err(SerdeError::message("a key entry has kind `key`"));
        }
        let public_key = match field("public_key") {
            Some(Value::Bytes(b)) => b.clone(),
            _ => {
                return Err(SerdeError::message("key entry: `public_key` is bytes"));
            }
        };
        let final_checkpoint = match field("final_checkpoint") {
            None => None,
            Some(Value::Text(t)) => Some(t.clone()),
            Some(_) => {
                return Err(SerdeError::message("key entry: `final_checkpoint` is text"));
            }
        };
        Ok(Self {
            key_id: text("key_id")?,
            alg: text("alg")?,
            public_key,
            final_checkpoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_registry_is_valid() {
        let r = GraphRegistry::embedded();
        assert_eq!(r.version(), 1);
        let names: Vec<&str> = r.graphs().iter().map(|g| g.name.as_str()).collect();
        for n in [
            "config",
            "local",
            "pages",
            "log",
            "actors",
            "accounts",
            "mirror/{provider}",
            "actors/{provider}",
            "log/{provider}",
            "actors/{farm}",
            "resolved",
            "metadata",
            "files/{repo}",
            "pages/{repo}",
        ] {
            assert!(names.contains(&n), "{n} is registered");
        }
        let config = r.lookup("config").unwrap().0;
        assert_eq!(config.partition, Some(0));
        assert_eq!(config.scope, Scope::Both);
        assert_eq!(config.integrity, Some(Integrity::Logged));
        assert!(config.payload_types.iter().any(|t| t == PAYLOAD_CONFIG));
        let resolved = r.lookup("resolved").unwrap().0;
        assert_eq!(resolved.kind, Kind::Projection);
        assert!(resolved.history.is_none());
    }

    #[test]
    fn templates_instantiate_and_match() {
        let r = GraphRegistry::embedded();
        let (g, slug) = r.lookup("mirror/wikidata").unwrap();
        assert_eq!(g.name, "mirror/{provider}");
        assert_eq!(slug, Some("wikidata"));
        assert_eq!(g.instantiate("wikidata"), "mirror/wikidata");
        let (g, slug) = r.lookup("pages/enwiki").unwrap();
        assert_eq!(g.per, Some(Per::Repo));
        assert_eq!(slug, Some("enwiki"));
        // `pages` itself is the tenant graph, not an instantiation.
        assert_eq!(
            r.lookup("pages").unwrap(),
            (r.lookup("pages").unwrap().0, None)
        );
        assert!(r.lookup("mirror/").is_none());
        assert!(r.lookup("mirror/a/b").is_none());
        assert!(r.lookup("nothing").is_none());
        // `log/x` could be a provider or a farm; the provider row comes first.
        assert_eq!(r.lookup("log/wikidata").unwrap().0.per, Some(Per::Provider));
    }

    #[test]
    fn rows_are_checked() {
        let bad = |body: &str| {
            GraphRegistry::parse(&format!("version = 1\n[[graph]]\n{body}\n")).unwrap_err()
        };
        assert!(matches!(
            bad(
                "name = \"x\"\nkind = \"source\"\nscope = \"tenant\"\nexport = \"public\"\ndefined_in = \"\""
            ),
            RegistryError::Row(_, _)
        ));
        assert!(matches!(
            bad(
                "name = \"x\"\nkind = \"projection\"\nscope = \"tenant\"\nexport = \"public\"\nhistory = \"full\"\ndefined_in = \"\""
            ),
            RegistryError::Row(_, _)
        ));
        assert!(matches!(
            bad(
                "name = \"x/{provider}\"\nkind = \"projection\"\nscope = \"tenant\"\nexport = \"public\"\ndefined_in = \"\""
            ),
            RegistryError::Row(_, _)
        ));
        assert!(matches!(
            bad(
                "name = \"x\"\nkind = \"projection\"\nscope = \"tenant\"\nexport = \"public\"\npartition = 3\ndefined_in = \"\""
            ),
            RegistryError::Row(_, _)
        ));
        assert!(matches!(
            GraphRegistry::parse("version = 2\n").unwrap_err(),
            RegistryError::Version(2)
        ));
        assert!(matches!(
            GraphRegistry::parse("version = 1\n[[graph]]\nname = \"a\"\nkind = \"projection\"\nscope = \"tenant\"\nexport = \"public\"\ndefined_in = \"\"\n[[graph]]\nname = \"a\"\nkind = \"projection\"\nscope = \"tenant\"\nexport = \"public\"\ndefined_in = \"\"\n").unwrap_err(),
            RegistryError::Duplicate(_)
        ));
    }

    #[test]
    fn entries_round_trip() {
        let registry = GraphRegistry::embedded();
        let graph = registry.lookup("mirror/wikidata").unwrap().0;
        let e = GraphEntry::source(
            graph,
            "mirror/wikidata".into(),
            12_345_678_901_234_567_890,
            16,
        );
        assert_eq!(e.key(), "graph:mirror/wikidata");
        let v = e.to_value().unwrap();
        let bytes = v.encode().unwrap();
        let back = GraphEntry::from_value(cbor::decode(&bytes).unwrap()).unwrap();
        assert_eq!(back, e);
        assert_eq!(back.hash.as_deref(), Some("sha-256"));
        assert_eq!(back.history, Some(History::Latest));
        let json = v.to_json().unwrap();
        assert_eq!(json["kind_of"], "source");
        assert_eq!(json["segment_exponent"], 16);
        let mut wrong = v;
        if let Value::Map(p) = &mut wrong {
            p.iter_mut().find(|(k, _)| k.text_eq("kind")).unwrap().1 = Value::text("key");
        }
        assert!(GraphEntry::from_value(wrong).is_err());

        let k = KeyEntry {
            key_id: "ybndrfg8ejkmcpqxot1uwisza345h769".into(),
            alg: "ed25519".into(),
            public_key: vec![9; 32],
            final_checkpoint: None,
        };
        assert_eq!(k.key(), "key:ybndrfg8ejkmcpqxot1uwisza345h769");
        let v = k.to_value();
        assert_eq!(
            KeyEntry::from_value(&cbor::decode(&v.encode().unwrap()).unwrap()).unwrap(),
            k
        );
        assert!(KeyEntry::from_value(&Value::Null).is_err());
    }
}
