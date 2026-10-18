//! Palettes and type sets (0086 §1–4): the catalogue a tenant offers and the theme
//! settings that choose from it, as `siprop=triplespace` reports them.
//!
//! The shipped palettes and type sets are `docs/registry/themes.toml`'s. An instance's and
//! a tenant's own, written as `palette` and `typeset` config records, join the catalogue
//! when their routes are built; until then a tenant offers the shipped ones. A setting or
//! preference that names something the tenant does not offer, or does not let readers
//! choose, resolves as unset (0086 §4).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const REGISTRY: &str = include_str!("../../../docs/registry/themes.toml");

/// The default palette and type set, which every wiki offers (0086 §1).
pub const DEFAULT_PALETTE: &str = "parchment";
/// The default type set.
pub const DEFAULT_TYPESET: &str = "default26";

/// The variants a palette may have, in the order their rules are written (0086 §2).
pub const VARIANTS: [&str; 4] = ["light", "dark", "light-more", "dark-more"];

/// A palette: the colour tokens of each variant, and the shape tokens of every variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Palette {
    /// The name it is chosen by.
    #[serde(default)]
    pub name: String,
    /// What a reader sees.
    pub label: String,
    /// Whether a reader may always choose it.
    #[serde(default)]
    pub accessibility: bool,
    /// The palette it lays each variant over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// `light`, required.
    pub light: BTreeMap<String, String>,
    /// `dark`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<BTreeMap<String, String>>,
    /// `light-more`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_more: Option<BTreeMap<String, String>>,
    /// `dark-more`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark_more: Option<BTreeMap<String, String>>,
    /// The shape tokens: radii and spacing.
    #[serde(flatten)]
    pub shape: BTreeMap<String, String>,
}

/// A typeface a type set serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Font {
    /// The family.
    pub family: String,
    /// The weights served.
    pub weights: Vec<u16>,
    /// The licence.
    pub licence: String,
}

/// A type set: the three typeface roles, stacks per language, and the typefaces served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Typeset {
    /// The name it is chosen by.
    #[serde(default)]
    pub name: String,
    /// What a reader sees.
    pub label: String,
    /// Whether a reader may always choose it.
    #[serde(default)]
    pub accessibility: bool,
    /// The typefaces it serves.
    #[serde(default, rename = "font")]
    pub fonts: Vec<Font>,
    /// A stack per language tag, set on the elements of that language (0086 §3).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub languages: BTreeMap<String, String>,
    /// `font-family-base`, `font-family-heading-main` and `font-family-monospace`.
    #[serde(flatten)]
    pub roles: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct File {
    palette: BTreeMap<String, Palette>,
    typeset: BTreeMap<String, Typeset>,
}

/// The palettes and type sets on offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Catalogue {
    /// The palettes, by name.
    pub palettes: Vec<Palette>,
    /// The type sets, by name.
    pub typesets: Vec<Typeset>,
}

static SHIPPED: LazyLock<Catalogue> = LazyLock::new(|| {
    let file: File = toml::from_str(REGISTRY).expect("docs/registry/themes.toml parses");
    Catalogue {
        palettes: file
            .palette
            .into_iter()
            .map(|(name, p)| Palette { name, ..p })
            .collect(),
        typesets: file
            .typeset
            .into_iter()
            .map(|(name, t)| Typeset { name, ..t })
            .collect(),
    }
});

impl Catalogue {
    /// The shipped palettes and type sets.
    #[must_use]
    pub fn shipped() -> &'static Self {
        &SHIPPED
    }

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
}

/// What readers may choose (`theme.choice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    /// Any palette and type set offered.
    Any,
    /// Only the accessibility ones, beside the wiki's default.
    Accessibility,
}

impl Choice {
    /// The setting's value; anything else is `any`, its default.
    #[must_use]
    pub fn parse(s: Option<&str>) -> Self {
        if s == Some("accessibility") {
            Self::Accessibility
        } else {
            Self::Any
        }
    }
}

/// The theme of a wiki: the catalogue, the defaults and what readers may choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Theme {
    /// The palettes offered.
    pub palettes: Vec<Palette>,
    /// The type sets offered.
    pub typesets: Vec<Typeset>,
    /// `theme.palette`, resolved: a palette offered, or the default.
    pub palette: String,
    /// `theme.typeset`, resolved.
    pub typeset: String,
    /// `theme.choice`.
    pub choice: Choice,
}

impl Theme {
    /// The theme from the catalogue and the settings' values, each of which falls back to
    /// the default where it names nothing offered.
    #[must_use]
    pub fn new(
        catalogue: &Catalogue,
        palette: Option<&str>,
        typeset: Option<&str>,
        choice: Option<&str>,
    ) -> Self {
        let palette = palette
            .filter(|n| catalogue.palette(n).is_some())
            .unwrap_or(DEFAULT_PALETTE);
        let typeset = typeset
            .filter(|n| catalogue.typeset(n).is_some())
            .unwrap_or(DEFAULT_TYPESET);
        Self {
            palettes: catalogue.palettes.clone(),
            typesets: catalogue.typesets.clone(),
            palette: palette.into(),
            typeset: typeset.into(),
            choice: Choice::parse(choice),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_catalogue() {
        let c = Catalogue::shipped();
        let names: Vec<&str> = c.palettes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["parchment", "parchment-co", "parchment-ob"]);
        let p = c.palette("parchment").unwrap();
        assert!(!p.accessibility && p.extends.is_none());
        assert_eq!(
            p.shape.get("border-radius-base").map(String::as_str),
            Some("6px")
        );
        assert!(p.dark.is_some() && p.light_more.is_some() && p.dark_more.is_some());
        let ob = c.palette("parchment-ob").unwrap();
        assert!(ob.accessibility);
        assert_eq!(ob.extends.as_deref(), Some("parchment"));
        assert!(ob.shape.is_empty());
        let t = c.typeset("default26").unwrap();
        assert_eq!(t.fonts.len(), 3);
        assert_eq!(t.roles.len(), 3);
        assert!(c.typeset("opendyslexic").unwrap().accessibility);
    }

    #[test]
    fn unknown_names_fall_back_to_the_defaults() {
        let t = Theme::new(
            Catalogue::shipped(),
            Some("nope"),
            Some("hyperlegible"),
            Some("accessibility"),
        );
        assert_eq!(t.palette, "parchment");
        assert_eq!(t.typeset, "hyperlegible");
        assert_eq!(t.choice, Choice::Accessibility);
        let j = serde_json::to_value(&t).unwrap();
        assert_eq!(j["choice"], "accessibility");
        assert_eq!(j["palettes"][0]["light"]["color-base"], "#1D1C1A");
        assert!(j["palettes"][0]["light-more"]["color-base"].is_string());
        assert_eq!(j["typesets"][0]["font"][0]["family"], "Newsreader");
    }
}
