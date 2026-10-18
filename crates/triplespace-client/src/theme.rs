//! The theme `siprop=triplespace` reports (0086 §5): the palettes and type sets the tenant
//! offers, the wiki's defaults and what readers may choose. The shapes are the records'
//! as `docs/registry/themes.toml` writes them, so the site reads the shipped file into
//! the same types when the API reports no theme.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// A palette (0086 §1–2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Palette {
    /// The name it is chosen by.
    #[serde(default)]
    pub name: String,
    /// What a reader sees.
    #[serde(default)]
    pub label: String,
    /// Whether a reader may always choose it.
    #[serde(default)]
    pub accessibility: bool,
    /// The palette it lays each variant over.
    #[serde(default)]
    pub extends: Option<String>,
    /// `light`.
    #[serde(default)]
    pub light: BTreeMap<String, String>,
    /// `dark`.
    #[serde(default)]
    pub dark: Option<BTreeMap<String, String>>,
    /// `light-more`.
    #[serde(default)]
    pub light_more: Option<BTreeMap<String, String>>,
    /// `dark-more`.
    #[serde(default)]
    pub dark_more: Option<BTreeMap<String, String>>,
    /// The shape tokens, which every variant shares; a value that is not a string is
    /// a field this build does not know, and is not a token.
    #[serde(flatten)]
    pub shape: BTreeMap<String, Value>,
}

/// A typeface a type set serves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Font {
    /// The family.
    pub family: String,
    /// The weights.
    #[serde(default)]
    pub weights: Vec<u16>,
    /// The licence.
    #[serde(default)]
    pub licence: String,
}

/// A type set (0086 §3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Typeset {
    /// The name it is chosen by.
    #[serde(default)]
    pub name: String,
    /// What a reader sees.
    #[serde(default)]
    pub label: String,
    /// Whether a reader may always choose it.
    #[serde(default)]
    pub accessibility: bool,
    /// The typefaces it serves.
    #[serde(default, rename = "font")]
    pub fonts: Vec<Font>,
    /// A stack per language tag.
    #[serde(default)]
    pub languages: BTreeMap<String, String>,
    /// The three roles; as for a palette's shape, only strings are tokens.
    #[serde(flatten)]
    pub roles: BTreeMap<String, Value>,
}

/// The theme of a wiki.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ThemeInfo {
    /// The palettes offered.
    #[serde(default)]
    pub palettes: Vec<Palette>,
    /// The type sets offered.
    #[serde(default)]
    pub typesets: Vec<Typeset>,
    /// `theme.palette`.
    #[serde(default)]
    pub palette: String,
    /// `theme.typeset`.
    #[serde(default)]
    pub typeset: String,
    /// `theme.choice`: `any` or `accessibility`.
    #[serde(default)]
    pub choice: String,
}

impl ThemeInfo {
    /// A palette by name.
    #[must_use]
    pub fn palette(&self, name: &str) -> Option<&Palette> {
        self.palettes.iter().find(|p| p.name == name)
    }

    /// A type set by name.
    #[must_use]
    pub fn typeset(&self, name: &str) -> Option<&Typeset> {
        self.typesets.iter().find(|t| t.name == name)
    }

    /// Whether readers may choose only the accessibility palettes and type sets.
    #[must_use]
    pub fn accessibility_only(&self) -> bool {
        self.choice == "accessibility"
    }
}
