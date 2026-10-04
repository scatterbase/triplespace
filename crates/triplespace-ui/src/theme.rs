//! Themes (0034 §1): values for Codex design tokens, served as a stylesheet of CSS custom
//! properties on `:root`, named by a hash of its text so that each theme caches as
//! immutable and a changed theme is a new URL.
//!
//! The shipped default is `default` in `docs/registry/themes.toml` (0034 A8). A tenant's
//! or instance's `ui.theme`, which the API reports in `siprop=triplespace`, overrides it
//! token by token. An override is taken only if its name is a token name and its value a
//! plain CSS value; a merged theme whose colours fail the contrast pairs of
//! [`Theme::contrast_problems`] is not served, and the default is served in its place.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

const REGISTRY: &str = include_str!("../../../docs/registry/themes.toml");

#[derive(Debug, Deserialize)]
struct File {
    theme: BTreeMap<String, toml::Table>,
}

/// A typeface a theme serves.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Font {
    /// The family.
    pub family: String,
    /// The weights served.
    pub weights: Vec<u16>,
    /// The licence.
    pub licence: String,
}

/// A theme: token values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    tokens: BTreeMap<String, String>,
    fonts: Vec<Font>,
    /// Provider chips: the class suffix (the code in lower case), text and background.
    chips: Vec<(String, String, String)>,
}

static DEFAULT: LazyLock<Theme> = LazyLock::new(|| {
    let file: File = toml::from_str(REGISTRY).expect("docs/registry/themes.toml parses");
    let table = file
        .theme
        .get("default")
        .expect("themes.toml has [theme.default]");
    let mut tokens = BTreeMap::new();
    let mut fonts = Vec::new();
    for (k, v) in table {
        match (k.as_str(), v) {
            ("label", _) => {}
            ("font", toml::Value::Array(list)) => {
                for f in list {
                    fonts.push(
                        f.clone()
                            .try_into()
                            .expect("a [[theme.default.font]] entry"),
                    );
                }
            }
            (name, toml::Value::String(s)) => {
                assert!(token_name(name) && plain_value(s), "themes.toml: `{name}`");
                tokens.insert(name.to_string(), s.clone());
            }
            (name, _) => panic!("themes.toml: `{name}` is not a string"),
        }
    }
    Theme {
        tokens,
        fonts,
        chips: Vec::new(),
    }
});

/// Whether a key can name a Codex token: lower-case letters, digits and hyphens.
fn token_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && s.as_bytes()[0].is_ascii_lowercase()
}

/// Whether a value is a plain CSS value that cannot escape its declaration or load
/// anything: no `;`, braces, angle brackets, backslashes, comments, `@` or `url(`.
fn plain_value(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && !s.contains(['{', '}', ';', '<', '>', '\\', '@', '\n', '\r'])
        && !s.contains("/*")
        && !s.to_ascii_lowercase().contains("url(")
        && !s.to_ascii_lowercase().contains("expression(")
}

/// The relative luminance of an sRGB colour written `#rgb` or `#rrggbb`.
fn luminance(hex: &str) -> Option<f64> {
    let h = hex.strip_prefix('#')?;
    let full: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let channel = |i: usize| -> Option<f64> {
        let v = f64::from(u8::from_str_radix(&full[i..i + 2], 16).ok()?) / 255.0;
        Some(if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        })
    };
    Some(0.2126 * channel(0)? + 0.7152 * channel(2)? + 0.0722 * channel(4)?)
}

/// The WCAG contrast ratio of two colours, if both are hex colours.
#[must_use]
pub fn contrast(a: &str, b: &str) -> Option<f64> {
    let (la, lb) = (luminance(a)?, luminance(b)?);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    Some((hi + 0.05) / (lo + 0.05))
}

/// Text on surfaces, at 4.5:1 (WCAG 2.1 AA).
const TEXT_PAIRS: &[(&str, &str)] = &[
    ("color-base", "background-color-base"),
    ("color-base", "background-color-neutral-subtle"),
    ("color-neutral", "background-color-base"),
    ("color-subtle", "background-color-base"),
    ("color-subtle", "background-color-neutral-subtle"),
    ("color-progressive", "background-color-base"),
    ("color-progressive", "background-color-neutral-subtle"),
    ("color-visited", "background-color-base"),
    ("color-placeholder", "background-color-base"),
    ("color-warning", "background-color-warning-subtle"),
    ("color-progressive", "background-color-progressive-subtle"),
];

/// The boundaries of controls and the focus outline, at 3:1 (WCAG 1.4.11).
const UI_PAIRS: &[(&str, &str)] = &[
    ("border-color-base", "background-color-base"),
    ("border-color-interactive", "background-color-base"),
    ("outline-color-progressive--focus", "background-color-base"),
];

impl Theme {
    /// The shipped default.
    #[must_use]
    pub fn shipped() -> &'static Self {
        &DEFAULT
    }

    /// The default with a tenant's overrides applied, or the default when the result
    /// would fail a contrast pair.
    #[must_use]
    pub fn with_overrides(overrides: Option<&Map<String, Value>>) -> Self {
        let mut theme = Self::shipped().clone();
        let Some(o) = overrides else { return theme };
        for (k, v) in o {
            if let Some(s) = v.as_str()
                && token_name(k)
                && plain_value(s)
            {
                theme.tokens.insert(k.clone(), s.to_string());
            }
        }
        if theme.contrast_problems().is_empty() {
            theme
        } else {
            Self::shipped().clone()
        }
    }

    /// The theme with the providers' chip colours (0010 §2), each kept only where its text
    /// meets 4.5:1 on its background; a provider without one wears the neutral chip.
    #[must_use]
    pub fn with_chips(mut self, providers: &[triplespace_client::ProviderInfo]) -> Self {
        self.chips = providers
            .iter()
            .filter_map(|p| {
                let c = p.chip.as_ref()?;
                let code: String = p
                    .code
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .map(|c| c.to_ascii_lowercase())
                    .collect();
                let ok = !code.is_empty()
                    && plain_value(&c.color)
                    && plain_value(&c.background)
                    && contrast(&c.color, &c.background).is_some_and(|r| r >= 4.5);
                ok.then(|| (code, c.color.clone(), c.background.clone()))
            })
            .collect();
        self.chips.sort();
        self.chips.dedup();
        self
    }

    /// A token's value.
    #[must_use]
    pub fn token(&self, name: &str) -> Option<&str> {
        self.tokens.get(name).map(String::as_str)
    }

    /// The typefaces the theme serves.
    #[must_use]
    pub fn fonts(&self) -> &[Font] {
        &self.fonts
    }

    /// The pairs that fail their contrast ratio, where both colours are set as hex.
    #[must_use]
    pub fn contrast_problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (pairs, min) in [(TEXT_PAIRS, 4.5), (UI_PAIRS, 3.0)] {
            for (fg, bg) in pairs {
                if let (Some(a), Some(b)) = (self.token(fg), self.token(bg))
                    && let Some(r) = contrast(a, b)
                    && r < min
                {
                    out.push(format!("{fg} on {bg}: {r:.2} < {min}"));
                }
            }
        }
        out
    }

    /// The stylesheet.
    #[must_use]
    pub fn css(&self) -> String {
        let mut s = String::from(":root {\n");
        for (k, v) in &self.tokens {
            s.push_str("\t--");
            s.push_str(k);
            s.push_str(": ");
            s.push_str(v);
            s.push_str(";\n");
        }
        s.push_str("}\n");
        for (code, color, background) in &self.chips {
            let _ = write!(
                s,
                ".ts-chip--p-{code} {{\n\tcolor: {color};\n\tbackground-color: {background};\n}}\n"
            );
        }
        s
    }

    /// The stylesheet's name: 16 hex digits of its SHA-256.
    #[must_use]
    pub fn hash(&self) -> String {
        crate::hex(&Sha256::digest(self.css().as_bytes())[..8])
    }

    /// Where the site serves it.
    #[must_use]
    pub fn href(&self) -> String {
        format!("/ui/theme/{}.css", self.hash())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_shipped_default_meets_its_contrast_pairs() {
        let t = Theme::shipped();
        assert!(
            t.contrast_problems().is_empty(),
            "{:?}",
            t.contrast_problems()
        );
        assert_eq!(
            t.token("font-family-heading-main"),
            Some("'Newsreader', Georgia, 'Times New Roman', serif")
        );
        assert_eq!(t.fonts().len(), 3);
        assert!(t.css().contains("--color-progressive: #2B559E;"));
        assert_eq!(t.hash().len(), 16);
    }

    #[test]
    fn overrides_apply_token_by_token() {
        let o = json!({"color-progressive": "#1F4585", "border-radius-base": "4px"});
        let t = Theme::with_overrides(o.as_object());
        assert_eq!(t.token("color-progressive"), Some("#1F4585"));
        assert_eq!(t.token("color-base"), Theme::shipped().token("color-base"));
        assert_ne!(t.hash(), Theme::shipped().hash());
    }

    #[test]
    fn unsafe_overrides_are_dropped() {
        let o = json!({
            "color-base": "red; } body { display: none",
            "background-color-base": "url(https://evil.example/x.png)",
            "Not-A-Token": "#000",
            "font-family-base": 12
        });
        let t = Theme::with_overrides(o.as_object());
        assert_eq!(&t, Theme::shipped());
    }

    #[test]
    fn a_theme_that_fails_contrast_falls_back_to_the_default() {
        let o = json!({"color-base": "#DDDDDD"});
        assert_eq!(&Theme::with_overrides(o.as_object()), Theme::shipped());
    }

    #[test]
    fn contrast_ratios() {
        assert!((contrast("#000", "#fff").unwrap() - 21.0).abs() < 1e-9);
        assert!((contrast("#fff", "#fff").unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(contrast("red", "#fff"), None);
    }
}
