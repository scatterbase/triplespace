//! The content-model registry, embedding `docs/registry/content-models.toml` (0041 §3).

use std::sync::OnceLock;

use serde::Deserialize;

/// The registry file, as embedded.
pub const CONTENT_MODELS_TOML: &str = include_str!("../../../docs/registry/content-models.toml");

/// Where a model's content comes from (0041 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    /// Full text stored in each page record; editable with `action=edit`.
    Text,
    /// The fold of an entity's change sets.
    Entity,
    /// The fold of a thread's records.
    Thread,
    /// Other pages; no records of its own.
    Composite,
    /// A page's statement change sets.
    Statements,
}

/// One content model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentModel {
    /// Its ID, as `contentmodel` reports it.
    pub id: String,
    /// Where its content comes from; absent for a reserved model.
    pub source: Option<ModelSource>,
    /// MediaWiki's `contentformat`, where there is one.
    pub format: Option<String>,
    /// Whether `action=edit` can change it.
    pub direct_editing: bool,
    /// Served here (`implemented`), rather than only reserved.
    pub implemented: bool,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    status: String,
    #[serde(default)]
    source: Option<ModelSource>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    direct_editing: bool,
    #[serde(default)]
    retired: bool,
}

#[derive(Deserialize)]
struct File {
    model: Vec<Row>,
}

/// The registry.
#[derive(Debug, Clone)]
pub struct ContentModels {
    models: Vec<ContentModel>,
}

impl ContentModels {
    /// The embedded registry.
    ///
    /// # Panics
    ///
    /// If the embedded file does not parse, which its test rules out.
    #[must_use]
    pub fn embedded() -> &'static ContentModels {
        static R: OnceLock<ContentModels> = OnceLock::new();
        R.get_or_init(|| Self::parse(CONTENT_MODELS_TOML).expect("the embedded registry parses"))
    }

    /// Parses a registry file.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        let f: File = toml::from_str(text)?;
        Ok(Self {
            models: f
                .model
                .into_iter()
                .filter(|r| !r.retired)
                .map(|r| ContentModel {
                    implemented: r.status == "implemented",
                    id: r.id,
                    source: r.source,
                    format: r.format,
                    direct_editing: r.direct_editing,
                })
                .collect(),
        })
    }

    /// A model by ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ContentModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Every model.
    pub fn iter(&self) -> impl Iterator<Item = &ContentModel> {
        self.models.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_registry_parses() {
        let r = ContentModels::embedded();
        let w = r.get("wikitext").expect("wikitext");
        assert_eq!(w.source, Some(ModelSource::Text));
        assert_eq!(w.format.as_deref(), Some("text/x-wiki"));
        assert!(w.direct_editing && w.implemented);
        assert_eq!(
            r.get("wikibase-item").and_then(|m| m.source),
            Some(ModelSource::Entity)
        );
        assert!(!r.get("css").expect("reserved").implemented);
        assert!(r.get("nonsense").is_none());
    }
}
