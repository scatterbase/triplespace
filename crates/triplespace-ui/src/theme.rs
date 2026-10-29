//! Themes (0086 §1–5; 19 §5.9): a stylesheet per palette and one per type set, built
//! from the theme the API reports, or from `docs/registry/themes.toml` when it reports
//! none, and named by a hash of their text so that each caches as immutable and a
//! changed palette is a new URL.
//!
//! **A palette's stylesheet** holds its `light` colours and shape tokens on `:root`; its
//! `dark` variant, laid over Codex's dark values, where dark mode is in effect, chosen
//! (`data-mode="dark"`) or the device's (`prefers-color-scheme`) where the reader has not
//! chosen light; and its `light-more` and `dark-more` variants where, besides, more
//! contrast is in effect (`data-contrast="more"` or `prefers-contrast`). A variant the
//! palette lacks is the palette's it extends, or the default palette's. Each variant
//! carries only what it changes: the cascade lays `dark-more` over `dark` and `dark` over
//! `light`. The providers' chips are in it too, their two colours swapped in dark mode.
//!
//! **A type set's stylesheet** holds the three typeface roles, a stack per language
//! tag, set on the elements of that language, the `@font-face` rules of the typefaces it
//! serves, and Codex's text-size modes under `data-text-size`.
//!
//! Every token name is checked to be one and every value to be a plain CSS value before
//! it is written, so a record cannot escape its declaration or load anything.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use triplespace_client::ThemeInfo;
use triplespace_client::theme::{Palette, Typeset};

const REGISTRY: &str = include_str!("../../../docs/registry/themes.toml");

/// The default palette and type set, which every wiki offers (0086 §1).
pub const DEFAULT_PALETTE: &str = "parchment";
/// The default type set.
pub const DEFAULT_TYPESET: &str = "default26";

/// Codex 2.7.0's values that a palette's variants lay over, and its text-size modes
/// (copied from `@wikimedia/codex-design-tokens`; `ui/src/codex-tokens.test.js` holds the
/// copies to the package).
const CODEX_LIGHT: &str = include_str!("../codex/theme-wikimedia-ui-root.css");
const CODEX_DARK: &str = include_str!("../codex/theme-wikimedia-ui-mode-dark.css");
const TEXT_SIZES: [(&str, &str); 3] = [
    (
        "small",
        include_str!("../codex/theme-wikimedia-ui-mode-small.css"),
    ),
    (
        "large",
        include_str!("../codex/theme-wikimedia-ui-mode-large.css"),
    ),
    (
        "x-large",
        include_str!("../codex/theme-wikimedia-ui-mode-x-large.css"),
    ),
];

type Tokens = BTreeMap<String, String>;

/// The custom properties of a Codex token file, without their comments.
fn codex(file: &str) -> Tokens {
    file.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("--")?;
            let (name, rest) = l.split_once(':')?;
            let value = rest.split_once(';')?.0.trim();
            Some((name.trim().to_string(), value.to_string()))
        })
        .collect()
}

static CODEX: LazyLock<(Tokens, Tokens)> =
    LazyLock::new(|| (codex(CODEX_LIGHT), codex(CODEX_DARK)));

#[derive(Deserialize)]
struct File {
    palette: BTreeMap<String, Palette>,
    typeset: BTreeMap<String, Typeset>,
}

static SHIPPED: LazyLock<ThemeInfo> = LazyLock::new(|| {
    let file: File = toml::from_str(REGISTRY).expect("docs/registry/themes.toml parses");
    ThemeInfo {
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
        palette: DEFAULT_PALETTE.into(),
        typeset: DEFAULT_TYPESET.into(),
        choice: "any".into(),
    }
});

/// A typeface of a fallback layer (0093 §1), and the generic stacks it backs up.
#[derive(Debug, Clone, Deserialize)]
struct LayerFont {
    family: String,
    #[serde(default)]
    licence: String,
    #[serde(default)]
    stacks: Vec<String>,
}

/// A fallback layer (0093 §1): typefaces in stack order, and stacks per language tag.
#[derive(Debug, Clone, Default, Deserialize)]
struct Layer {
    #[serde(default, rename = "font")]
    fonts: Vec<LayerFont>,
    #[serde(default)]
    languages: BTreeMap<String, String>,
}

impl Layer {
    /// The families that back up a stack of one generic family, in order.
    fn families(&self, generic: &str) -> Vec<&str> {
        self.fonts
            .iter()
            .filter(|f| f.stacks.iter().any(|s| s == generic))
            .map(|f| f.family.as_str())
            .collect()
    }
}

#[derive(Deserialize)]
struct Layers {
    #[serde(default)]
    fallback: BTreeMap<String, Layer>,
}

/// The shipped fallback layers, by name; a type set's `fallback` may name only these.
static LAYERS: LazyLock<BTreeMap<String, Layer>> = LazyLock::new(|| {
    toml::from_str::<Layers>(REGISTRY)
        .expect("docs/registry/themes.toml parses")
        .fallback
});

/// A family name as a stack compares it: unquoted, in lower case.
fn family_key(f: &str) -> String {
    f.trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .to_ascii_lowercase()
}

/// The generic family of a stack, as a fallback layer files its families: `serif`,
/// `monospace` or, for `sans-serif`, `system-ui`, another or none, `sans-serif`.
fn generic_of(stack: &str) -> &'static str {
    for f in stack.split(',').map(family_key) {
        match f.as_str() {
            "serif" | "ui-serif" => return "serif",
            "monospace" | "ui-monospace" => return "monospace",
            "sans-serif" | "ui-sans-serif" | "system-ui" => return "sans-serif",
            _ => {}
        }
    }
    "sans-serif"
}

/// Whether a family is a generic one, which ends a stack.
fn generic(f: &str) -> bool {
    matches!(
        family_key(f).as_str(),
        "serif"
            | "sans-serif"
            | "monospace"
            | "cursive"
            | "fantasy"
            | "system-ui"
            | "ui-serif"
            | "ui-sans-serif"
            | "ui-monospace"
            | "ui-rounded"
            | "math"
            | "emoji"
            | "fangsong"
    )
}

/// A stack with a layer's families in it (0093 §1): straight after the families the type
/// set serves, or, if it serves none at the head of the stack, before its first generic
/// family. A family the stack already names is not added again.
fn with_layer(stack: &str, served: &[String], layer: &[&str]) -> String {
    let parts: Vec<&str> = stack.split(',').map(str::trim).collect();
    let served: Vec<String> = served.iter().map(|f| family_key(f)).collect();
    let lead = parts
        .iter()
        .take_while(|p| served.contains(&family_key(p)))
        .count();
    let at = if lead > 0 {
        lead
    } else {
        parts.iter().position(|p| generic(p)).unwrap_or(parts.len())
    };
    let named: Vec<String> = parts.iter().map(|p| family_key(p)).collect();
    let mut out: Vec<String> = parts[..at].iter().map(|s| (*s).to_string()).collect();
    out.extend(
        layer
            .iter()
            .filter(|f| !named.contains(&family_key(f)))
            .map(|f| format!("'{f}'")),
    );
    out.extend(parts[at..].iter().map(|s| (*s).to_string()));
    out.join(", ")
}

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

/// Whether a palette or type set name can be part of a URL and a JSON key as it is:
/// lower-case letters, digits, hyphens and the `~` of a keyed name (0086 A1).
fn sheet_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 120
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'~')
}

/// Only the entries that are tokens with plain values.
fn clean<'a>(entries: impl IntoIterator<Item = (&'a String, &'a String)>) -> Tokens {
    entries
        .into_iter()
        .filter(|(k, v)| token_name(k) && plain_value(v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The string entries of a flattened map.
fn strings(m: &BTreeMap<String, Value>) -> Tokens {
    m.iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .filter(|(k, v)| token_name(k) && plain_value(v))
        .collect()
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

/// Text on surfaces (0086 §2): 4.5:1 in `light` and `dark`, 7:1 in the high-contrast
/// variants.
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
    // Button labels, white in every mode.
    ("color-inverted-fixed", "background-color-progressive"),
    (
        "color-inverted-fixed",
        "background-color-progressive--hover",
    ),
    (
        "color-inverted-fixed",
        "background-color-progressive--active",
    ),
    ("color-inverted-fixed", "background-color-destructive"),
    // The colours that carry meaning, on the page and on their subtle backgrounds.
    ("color-error", "background-color-base"),
    ("color-error", "background-color-error-subtle"),
    ("color-destructive", "background-color-base"),
    ("color-destructive", "background-color-destructive-subtle"),
    ("color-success", "background-color-base"),
    ("color-success", "background-color-success-subtle"),
];

/// The boundaries of controls and the focus outline: 3:1 (WCAG 1.4.11), 4.5:1 in the
/// high-contrast variants.
const UI_PAIRS: &[(&str, &str)] = &[
    ("border-color-base", "background-color-base"),
    ("border-color-interactive", "background-color-base"),
    ("outline-color-progressive--focus", "background-color-base"),
];

/// The pairs of a set of token values that fall below their floors.
fn problems(tokens: &Tokens, more: bool) -> Vec<String> {
    let (text, ui) = if more { (7.0, 4.5) } else { (4.5, 3.0) };
    let mut out = Vec::new();
    for (pairs, min) in [(TEXT_PAIRS, text), (UI_PAIRS, ui)] {
        for (fg, bg) in pairs {
            if let (Some(a), Some(b)) = (tokens.get(*fg), tokens.get(*bg))
                && let Some(r) = contrast(a, b)
                && r < min - 0.005
            {
                out.push(format!("{fg} on {bg}: {r:.2} < {min}"));
            }
        }
    }
    out
}

/// A provider's chip: the class suffix (the code in lower case), text and background.
type Chip = (String, String, String);

/// A palette as served: the token blocks of its variants.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Served {
    light: Tokens,
    dark: Tokens,
    light_more: Tokens,
    dark_more: Tokens,
}

/// A wiki's theme: what it offers, its defaults, and the providers' chips.
#[derive(Debug, Clone)]
pub struct Theme {
    info: ThemeInfo,
    chips: Vec<Chip>,
    built: OnceLock<Arc<Built>>,
}

/// The text and URL of every stylesheet a theme serves.
type Built = BTreeMap<(Sheet, String), (String, String)>;

/// Built stylesheets by what they were built from, shared across requests, since a
/// wiki's theme changes rarely and every page names its sheets; emptied when it grows
/// past a bound.
static BUILT: LazyLock<Mutex<BTreeMap<[u8; 32], Arc<Built>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
const BUILT_MAX: usize = 64;

/// One palette or type set a reader may choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choosable {
    /// The name.
    pub name: String,
    /// The label.
    pub label: String,
    /// Whether it is for accessibility.
    pub accessibility: bool,
}

/// Which stylesheet: a palette's or a type set's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sheet {
    /// A palette's.
    Palette,
    /// A type set's.
    Typeset,
}

impl Sheet {
    fn prefix(self) -> &'static str {
        match self {
            Self::Palette => "p",
            Self::Typeset => "t",
        }
    }
}

impl Theme {
    /// The shipped palettes and type sets, with the defaults.
    #[must_use]
    pub fn shipped() -> Self {
        Self {
            info: SHIPPED.clone(),
            chips: Vec::new(),
            built: OnceLock::new(),
        }
    }

    /// The theme the API reports, or the shipped one where it reports none. The default
    /// palette and type set are always offered, and a default that names nothing offered
    /// is the shipped default.
    #[must_use]
    pub fn from_info(info: Option<&ThemeInfo>) -> Self {
        let Some(info) = info.filter(|i| !i.palettes.is_empty()) else {
            return Self::shipped();
        };
        let mut info = info.clone();
        info.palettes.retain(|p| sheet_name(&p.name));
        info.typesets.retain(|t| sheet_name(&t.name));
        for p in &SHIPPED.palettes {
            if p.name == DEFAULT_PALETTE && info.palette(DEFAULT_PALETTE).is_none() {
                info.palettes.push(p.clone());
            }
        }
        for t in &SHIPPED.typesets {
            if t.name == DEFAULT_TYPESET && info.typeset(DEFAULT_TYPESET).is_none() {
                info.typesets.push(t.clone());
            }
        }
        if info.palette(&info.palette).is_none() {
            info.palette = DEFAULT_PALETTE.into();
        }
        if info.typeset(&info.typeset).is_none() {
            info.typeset = DEFAULT_TYPESET.into();
        }
        Self {
            info,
            chips: Vec::new(),
            built: OnceLock::new(),
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
        self.built = OnceLock::new();
        self
    }

    /// Every stylesheet the theme serves, built once per distinct theme.
    fn built(&self) -> Arc<Built> {
        self.built
            .get_or_init(|| {
                let key: [u8; 32] =
                    Sha256::digest(format!("{:?}{:?}", self.info, self.chips).as_bytes()).into();
                if let Some(b) = BUILT.lock().ok().and_then(|m| m.get(&key).cloned()) {
                    return b;
                }
                let mut b = Built::new();
                for (sheet, names) in [
                    (
                        Sheet::Palette,
                        self.info
                            .palettes
                            .iter()
                            .map(|p| p.name.clone())
                            .collect::<Vec<_>>(),
                    ),
                    (
                        Sheet::Typeset,
                        self.info.typesets.iter().map(|t| t.name.clone()).collect(),
                    ),
                ] {
                    for name in names {
                        let css = match sheet {
                            Sheet::Palette => self.palette_css(&name),
                            Sheet::Typeset => self.typeset_css(&name),
                        };
                        if let Some(css) = css {
                            let href = format!(
                                "/ui/theme/{}-{name}-{}.css",
                                sheet.prefix(),
                                crate::hex(&Sha256::digest(css.as_bytes())[..8])
                            );
                            b.insert((sheet, name), (css, href));
                        }
                    }
                }
                let b = Arc::new(b);
                if let Ok(mut m) = BUILT.lock() {
                    if m.len() >= BUILT_MAX {
                        m.clear();
                    }
                    m.insert(key, b.clone());
                }
                b
            })
            .clone()
    }

    /// The wiki's palette.
    #[must_use]
    pub fn default_palette(&self) -> &str {
        &self.info.palette
    }

    /// The wiki's type set.
    #[must_use]
    pub fn default_typeset(&self) -> &str {
        &self.info.typeset
    }

    /// What readers may choose: every palette or type set offered, or under
    /// `theme.choice = accessibility` the accessibility ones, with the wiki's default
    /// first either way (0086 §4).
    #[must_use]
    pub fn choices(&self, sheet: Sheet) -> Vec<Choosable> {
        let only = self.info.accessibility_only();
        let (default, all): (&str, Vec<Choosable>) = match sheet {
            Sheet::Palette => (
                &self.info.palette,
                self.info
                    .palettes
                    .iter()
                    .map(|p| Choosable {
                        name: p.name.clone(),
                        label: p.label.clone(),
                        accessibility: p.accessibility,
                    })
                    .collect(),
            ),
            Sheet::Typeset => (
                &self.info.typeset,
                self.info
                    .typesets
                    .iter()
                    .map(|t| Choosable {
                        name: t.name.clone(),
                        label: t.label.clone(),
                        accessibility: t.accessibility,
                    })
                    .collect(),
            ),
        };
        let mut out: Vec<Choosable> = all.iter().filter(|o| o.name == default).cloned().collect();
        out.extend(
            all.into_iter()
                .filter(|o| o.name != default && (o.accessibility || !only)),
        );
        out
    }

    /// Every palette or type set the wiki offers, by name, which its settings may name.
    #[must_use]
    pub fn offered(&self, sheet: Sheet) -> Vec<String> {
        match sheet {
            Sheet::Palette => self.info.palettes.iter().map(|p| p.name.clone()).collect(),
            Sheet::Typeset => self.info.typesets.iter().map(|t| t.name.clone()).collect(),
        }
    }

    /// The palette or type set a reader's preference picks: theirs where the wiki lets
    /// them choose it, otherwise the wiki's (0086 §4).
    #[must_use]
    pub fn chosen(&self, sheet: Sheet, preference: Option<&str>) -> String {
        let default = match sheet {
            Sheet::Palette => &self.info.palette,
            Sheet::Typeset => &self.info.typeset,
        };
        preference
            .filter(|p| self.choices(sheet).iter().any(|o| o.name == *p))
            .unwrap_or(default)
            .to_string()
    }

    fn palette(&self, name: &str) -> Option<&Palette> {
        self.info.palette(name)
    }

    /// The palettes a palette is laid over, its own last; one level of `extends`, as
    /// 0086 §2 allows, and nothing that loops.
    fn chain<'a>(&'a self, p: &'a Palette) -> Vec<&'a Palette> {
        let mut out = vec![p];
        if let Some(base) = p.extends.as_deref().and_then(|b| self.palette(b))
            && base.name != p.name
            && base.extends.is_none()
        {
            out.insert(0, base);
        }
        out
    }

    /// A variant along a chain: each palette's laid over its base's, or `None` where no
    /// palette in the chain has it.
    fn variant(
        chain: &[&Palette],
        pick: impl Fn(&Palette) -> Option<&BTreeMap<String, String>>,
    ) -> Option<Tokens> {
        let mut out = Tokens::new();
        let mut any = false;
        for p in chain {
            if let Some(v) = pick(p) {
                any = true;
                out.extend(clean(v));
            }
        }
        any.then_some(out)
    }

    /// A palette as served, with the default palette's variants where it lacks them.
    fn served(&self, p: &Palette) -> Served {
        let chain = self.chain(p);
        let fallback = || {
            self.palette(DEFAULT_PALETTE)
                .filter(|d| d.name != p.name)
                .map(|d| self.served(d))
        };
        let light = {
            let mut t = Tokens::new();
            for q in &chain {
                t.extend(strings(&q.shape));
                t.extend(clean(&q.light));
            }
            t
        };
        let dark = Self::variant(&chain, |q| q.dark.as_ref())
            .or_else(|| fallback().map(|s| s.dark))
            .unwrap_or_default();
        let light_more = Self::variant(&chain, |q| q.light_more.as_ref())
            .or_else(|| fallback().map(|s| s.light_more))
            .unwrap_or_default();
        let dark_more = Self::variant(&chain, |q| q.dark_more.as_ref())
            .or_else(|| fallback().map(|s| s.dark_more))
            .unwrap_or_default();
        Served {
            light,
            dark,
            light_more,
            dark_more,
        }
    }

    /// The token values in effect in each variant, merged over Codex's, for the contrast
    /// check: `light`, `dark`, `light-more`, `dark-more`.
    fn effective(&self, p: &Palette) -> [Tokens; 4] {
        let (codex_light, _) = &*CODEX;
        let s = self.served(p);
        let dark_block = Self::dark_block(&s);
        let mut light = codex_light.clone();
        light.extend(s.light.clone());
        let mut dark = light.clone();
        dark.extend(dark_block);
        let mut light_more = light.clone();
        light_more.extend(s.light_more);
        let mut dark_more = dark.clone();
        dark_more.extend(s.dark_more);
        [light, dark, light_more, dark_more]
    }

    /// The pairs that fail their floors in any variant of a palette, as served.
    #[must_use]
    pub fn contrast_problems(&self, name: &str) -> Vec<String> {
        let Some(p) = self.palette(name) else {
            return vec![format!("no palette `{name}`")];
        };
        let mut out = Vec::new();
        for (variant, (tokens, more)) in ["light", "dark", "light-more", "dark-more"]
            .iter()
            .zip(self.effective(p).iter().zip([false, false, true, true]))
        {
            out.extend(
                problems(tokens, more)
                    .into_iter()
                    .map(|e| format!("{name} {variant}: {e}")),
            );
        }
        out
    }

    /// The dark block: Codex's dark values, then the palette's `light` colours that
    /// Codex's dark mode leaves alone set back to Codex's light values, so a light colour
    /// never shows in dark mode, then the palette's `dark`.
    fn dark_block(s: &Served) -> Tokens {
        let (codex_light, codex_dark) = &*CODEX;
        let mut t = codex_dark.clone();
        for k in s.light.keys() {
            if !t.contains_key(k)
                && !s.dark.contains_key(k)
                && let Some(v) = codex_light.get(k)
            {
                t.insert(k.clone(), v.clone());
            }
        }
        t.extend(s.dark.clone());
        t
    }

    /// A palette's stylesheet, if the wiki offers it.
    #[must_use]
    pub fn palette_css(&self, name: &str) -> Option<String> {
        let p = self.palette(name)?;
        let s = self.served(p);
        let dark = Self::dark_block(&s);
        let mut css = String::new();
        block(&mut css, None, ":root", &s.light);
        block(&mut css, None, ":root[data-mode=\"dark\"]", &dark);
        block(
            &mut css,
            Some("(prefers-color-scheme: dark)"),
            ":root:not([data-mode=\"light\"])",
            &dark,
        );
        for (tokens, mode, other) in [
            (&s.light_more, "light", "dark"),
            (&s.dark_more, "dark", "light"),
        ] {
            if tokens.is_empty() {
                continue;
            }
            let device = format!("(prefers-color-scheme: {mode})");
            block(
                &mut css,
                None,
                &format!(":root[data-mode=\"{mode}\"][data-contrast=\"more\"]"),
                tokens,
            );
            block(
                &mut css,
                Some(&device),
                &format!(":root:not([data-mode=\"{other}\"])[data-contrast=\"more\"]"),
                tokens,
            );
            block(
                &mut css,
                Some("(prefers-contrast: more)"),
                &format!(":root[data-mode=\"{mode}\"]"),
                tokens,
            );
            block(
                &mut css,
                Some(&format!("(prefers-contrast: more) and {device}")),
                &format!(":root:not([data-mode=\"{other}\"])"),
                tokens,
            );
        }
        self.chips_css(&mut css);
        Some(css)
    }

    /// The providers' chips: their colours in light mode, swapped in dark mode, which
    /// keeps the pair's contrast exactly (0086 §2).
    fn chips_css(&self, css: &mut String) {
        if self.chips.is_empty() {
            return;
        }
        let rules = |css: &mut String, scope: &str, swap: bool| {
            for (code, color, background) in &self.chips {
                let (fg, bg) = if swap {
                    (background, color)
                } else {
                    (color, background)
                };
                let _ = write!(
                    css,
                    "{scope}.ts-chip--p-{code} {{\n\tcolor: {fg};\n\tbackground-color: {bg};\n}}\n"
                );
            }
        };
        rules(css, "", false);
        rules(css, ":root[data-mode=\"dark\"] ", true);
        css.push_str("@media (prefers-color-scheme: dark) {\n");
        rules(css, ":root:not([data-mode=\"light\"]) ", true);
        css.push_str("}\n");
    }

    /// A type set's stylesheet, if the wiki offers it.
    #[must_use]
    pub fn typeset_css(&self, name: &str) -> Option<String> {
        let t = self.info.typeset(name)?;
        let layer = t
            .fallback
            .as_deref()
            .and_then(|l| LAYERS.get(l).map(|layer| (l, layer)));
        let mut css = String::new();
        let fonts = [
            crate::assets::typeset_fonts(&t.name),
            layer.and_then(|(l, _)| crate::assets::fallback_fonts(l)),
        ];
        for f in fonts.into_iter().flatten() {
            css.push_str(&f);
            if !css.ends_with('\n') {
                css.push('\n');
            }
        }
        let mut roles = strings(&t.roles);
        if let Some((_, layer)) = layer {
            let served: Vec<String> = t.fonts.iter().map(|f| f.family.clone()).collect();
            for (role, stack) in &mut roles {
                if role.starts_with("font-family-") {
                    *stack = with_layer(stack, &served, &layer.families(generic_of(stack)));
                }
            }
        }
        block(&mut css, None, ":root", &roles);
        // The layer's stacks per language tag, where the type set names none of its own:
        // the layer's families, then the text stack for what they lack.
        for (tag, stack) in layer.map(|(_, l)| &l.languages).into_iter().flatten() {
            if language_tag(tag) && plain_value(stack) && !t.languages.contains_key(tag) {
                let _ = writeln!(
                    css,
                    ":lang({tag}):not(code, pre, kbd, samp) {{\n\tfont-family: {stack}, var(--font-family-base);\n}}"
                );
            }
        }
        for (tag, stack) in &t.languages {
            if language_tag(tag) && plain_value(stack) {
                let _ = writeln!(
                    css,
                    ":lang({tag}):not(code, pre, kbd, samp) {{\n\tfont-family: {stack};\n}}"
                );
            }
        }
        for (size, file) in TEXT_SIZES {
            block(
                &mut css,
                None,
                &format!(":root[data-text-size=\"{size}\"]"),
                &codex(file),
            );
        }
        Some(css)
    }

    /// A stylesheet's text.
    #[must_use]
    pub fn css(&self, sheet: Sheet, name: &str) -> Option<String> {
        self.built()
            .get(&(sheet, name.to_string()))
            .map(|(css, _)| css.clone())
    }

    /// Where the site serves a stylesheet: `/ui/theme/{p|t}-{name}-{hash}.css`, the hash
    /// 16 hex digits of the text's SHA-256.
    #[must_use]
    pub fn href(&self, sheet: Sheet, name: &str) -> Option<String> {
        self.built()
            .get(&(sheet, name.to_string()))
            .map(|(_, href)| href.clone())
    }

    /// The stylesheet a `/ui/theme/` file name asks for, and whether the name is of the
    /// text served now, which may then be cached as immutable.
    #[must_use]
    pub fn file(&self, file: &str) -> Option<(String, bool)> {
        let stem = file.strip_suffix(".css")?;
        let (sheet, rest) = if let Some(r) = stem.strip_prefix("p-") {
            (Sheet::Palette, r)
        } else if let Some(r) = stem.strip_prefix("t-") {
            (Sheet::Typeset, r)
        } else {
            // A name from before palettes: the default palette, briefly.
            return self
                .css(Sheet::Palette, &self.info.palette)
                .map(|c| (c, false));
        };
        let (name, _) = rest.rsplit_once('-')?;
        let css = self.css(sheet, name)?;
        let current = self.href(sheet, name).is_some_and(|h| h.ends_with(file));
        Some((css, current))
    }

    /// The JSON block the head script reads (0086 §5): the stylesheet of every palette and
    /// type set a reader may choose, and the wiki's defaults.
    #[must_use]
    pub fn data(&self) -> String {
        let sheets = |sheet: Sheet| -> Map<String, Value> {
            self.choices(sheet)
                .into_iter()
                .filter_map(|o| Some((o.name.clone(), json!(self.href(sheet, &o.name)?))))
                .collect()
        };
        json!({
            "palette": self.info.palette,
            "typeset": self.info.typeset,
            "palettes": sheets(Sheet::Palette),
            "typesets": sheets(Sheet::Typeset),
        })
        .to_string()
        .replace("</", "<\\/")
    }

    /// The typefaces a type set serves, its fallback layer's after its own (0093 §1).
    #[must_use]
    pub fn fonts(&self, name: &str) -> Vec<(String, String)> {
        let Some(t) = self.info.typeset(name) else {
            return Vec::new();
        };
        let layer = t.fallback.as_deref().and_then(|l| LAYERS.get(l));
        t.fonts
            .iter()
            .map(|f| (f.family.clone(), f.licence.clone()))
            .chain(
                layer
                    .into_iter()
                    .flat_map(|l| &l.fonts)
                    .map(|f| (f.family.clone(), f.licence.clone())),
            )
            .collect()
    }

    /// A token's value in a palette's `light` variant as served.
    #[must_use]
    pub fn light_token(&self, palette: &str, token: &str) -> Option<String> {
        let p = self.palette(palette)?;
        self.served(p).light.get(token).cloned()
    }
}

/// Whether a key is a language tag a selector can hold as it is.
fn language_tag(s: &str) -> bool {
    !s.is_empty() && s.len() <= 35 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// One rule of custom properties, inside a media query if one is given.
fn block(css: &mut String, media: Option<&str>, selector: &str, tokens: &Tokens) {
    if tokens.is_empty() {
        return;
    }
    let indent = if media.is_some() { "\t" } else { "" };
    if let Some(m) = media {
        let _ = writeln!(css, "@media {m} {{");
    }
    let _ = writeln!(css, "{indent}{selector} {{");
    for (k, v) in tokens {
        let _ = writeln!(css, "{indent}\t--{k}: {v};");
    }
    let _ = writeln!(css, "{indent}}}");
    if media.is_some() {
        css.push_str("}\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_variant_meets_its_floors() {
        let t = Theme::shipped();
        for p in &SHIPPED.palettes {
            let problems = t.contrast_problems(&p.name);
            assert!(problems.is_empty(), "{problems:#?}");
        }
    }

    #[test]
    fn a_palette_sheet_has_every_variant_in_its_place() {
        let t = Theme::shipped();
        let css = t.palette_css("parchment").unwrap();
        assert!(css.starts_with(":root {\n\t--background-color-base: #FCFBF8;"));
        assert!(css.contains("\t--border-radius-base: 6px;\n"));
        assert!(css.contains(":root[data-mode=\"dark\"] {\n"));
        assert!(css.contains(
            "@media (prefers-color-scheme: dark) {\n\t:root:not([data-mode=\"light\"]) {\n"
        ));
        assert!(css.contains(":root[data-mode=\"dark\"][data-contrast=\"more\"] {"));
        assert!(css.contains(
            "@media (prefers-contrast: more) and (prefers-color-scheme: light) {\n\t:root:not([data-mode=\"dark\"]) {"
        ));
        // Codex's dark values under the palette's own: a token parchment's dark leaves
        // alone is Codex's, one it sets is its own.
        let dark = &css[css.find(":root[data-mode=\"dark\"] {").unwrap()..];
        assert!(dark.contains("--background-color-base: #1E1813;"));
        assert!(dark.contains("--color-destructive: "));
        let href = t.href(Sheet::Palette, "parchment").unwrap();
        assert!(href.starts_with("/ui/theme/p-parchment-") && href.len() == 42);
    }

    #[test]
    fn an_extending_palette_lays_over_its_base() {
        let t = Theme::shipped();
        let ob = t.served(t.palette("parchment-rg").unwrap());
        let base = t.served(t.palette("parchment").unwrap());
        assert_eq!(
            ob.light.get("background-color-base"),
            base.light.get("background-color-base")
        );
        assert_eq!(ob.light["color-destructive"], "#A84300");
        assert_eq!(ob.light["border-radius-base"], "6px");
    }

    #[test]
    fn a_palette_without_variants_takes_the_defaults() {
        let mut info = SHIPPED.clone();
        let mut brand = info.palette("parchment").unwrap().clone();
        brand.name = "brand".into();
        brand.dark = None;
        brand.light_more = None;
        brand.dark_more = None;
        brand
            .light
            .insert("color-progressive".into(), "#1F4585".into());
        info.palettes.push(brand);
        info.palette = "brand".into();
        let t = Theme::from_info(Some(&info));
        assert_eq!(t.default_palette(), "brand");
        let s = t.served(t.palette("brand").unwrap());
        let d = t.served(t.palette("parchment").unwrap());
        assert_eq!(s.dark, d.dark);
        assert_eq!(s.dark_more, d.dark_more);
        assert_eq!(s.light["color-progressive"], "#1F4585");
    }

    #[test]
    fn choices_honour_theme_choice() {
        let mut info = SHIPPED.clone();
        info.choice = "accessibility".into();
        info.typeset = "hyperlegible".into();
        let t = Theme::from_info(Some(&info));
        let names: Vec<String> = t
            .choices(Sheet::Palette)
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(names, ["parchment", "parchment-by", "parchment-rg"]);
        let typesets: Vec<String> = t
            .choices(Sheet::Typeset)
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(typesets, ["hyperlegible", "opendyslexic"]);
        assert_eq!(t.chosen(Sheet::Typeset, Some("default26")), "hyperlegible");
        assert_eq!(
            t.chosen(Sheet::Typeset, Some("opendyslexic")),
            "opendyslexic"
        );
        assert_eq!(t.chosen(Sheet::Palette, Some("nope")), "parchment");
        assert_eq!(t.chosen(Sheet::Palette, None), "parchment");
    }

    #[test]
    fn unsafe_records_write_nothing_unsafe() {
        let mut info = SHIPPED.clone();
        let mut p = info.palette("parchment").unwrap().clone();
        p.name = "x".into();
        p.light = [
            (
                "color-base".to_string(),
                "red; } body { display: none".to_string(),
            ),
            (
                "background-color-base".to_string(),
                "url(https://evil.example/x.png)".to_string(),
            ),
            ("Not-A-Token".to_string(), "#000".to_string()),
        ]
        .into();
        info.palettes.push(p);
        let mut bad = info.palettes[0].clone();
        bad.name = "a\"b".into();
        info.palettes.push(bad);
        let t = Theme::from_info(Some(&info));
        let css = t.palette_css("x").unwrap();
        assert!(!css.contains("evil") && !css.contains("display") && !css.contains("Not-A"));
        assert!(t.palette_css("a\"b").is_none());
    }

    #[test]
    fn a_type_set_sheet() {
        let t = Theme::shipped();
        let css = t.typeset_css("hyperlegible").unwrap();
        assert!(css.contains(
            "\t--font-family-base: 'Atkinson Hyperlegible', 'Noto Sans', 'Noto Sans Arabic', "
        ));
        assert!(css.contains("'Noto Sans Symbols 2', system-ui, sans-serif;\n"));
        assert!(css.contains(":root[data-text-size=\"x-large\"] {\n"));
        if crate::assets::typeset_fonts("hyperlegible").is_some() {
            // Built with the frontend: the typefaces are in the type set's own sheet.
            assert!(css.starts_with("@font-face{font-family:Atkinson Hyperlegible;"));
        }
        assert!(css.contains("--font-size-medium: "));
        let mut info = SHIPPED.clone();
        info.typesets[0]
            .languages
            .insert("ar".into(), "'Noto Naskh Arabic', serif".into());
        info.typesets[0]
            .languages
            .insert("x){}".into(), "serif".into());
        let name = info.typesets[0].name.clone();
        let css = Theme::from_info(Some(&info)).typeset_css(&name).unwrap();
        assert!(css.contains(
            ":lang(ar):not(code, pre, kbd, samp) {\n\tfont-family: 'Noto Naskh Arabic', serif;\n}"
        ));
        assert!(!css.contains("x){}"));
    }

    #[test]
    fn a_layer_goes_after_the_served_families() {
        let served = ["Source Sans 3".to_string()];
        let layer = ["Noto Sans", "Noto Sans Arabic"];
        assert_eq!(
            with_layer("'Source Sans 3', system-ui, sans-serif", &served, &layer),
            "'Source Sans 3', 'Noto Sans', 'Noto Sans Arabic', system-ui, sans-serif"
        );
        // Serving none at the head of the stack: before the generic families, after the
        // device's own named ones.
        assert_eq!(
            with_layer("'Linux Libertine', Georgia, serif", &served, &layer),
            "'Linux Libertine', Georgia, 'Noto Sans', 'Noto Sans Arabic', serif"
        );
        assert_eq!(
            with_layer("Garamond", &[], &layer),
            "Garamond, 'Noto Sans', 'Noto Sans Arabic'"
        );
        // A family the stack names already stays where the stack puts it.
        assert_eq!(
            with_layer(
                "'Source Sans 3', \"Noto Sans\", sans-serif",
                &served,
                &layer
            ),
            "'Source Sans 3', 'Noto Sans Arabic', \"Noto Sans\", sans-serif"
        );
        assert_eq!(generic_of("'Lilex', Menlo, monospace"), "monospace");
        assert_eq!(generic_of("'X', Georgia, serif"), "serif");
        assert_eq!(generic_of("'X', system-ui, sans-serif"), "sans-serif");
        assert_eq!(generic_of("'X'"), "sans-serif");
    }

    #[test]
    fn the_noto_layer() {
        for t in &SHIPPED.typesets {
            if let Some(l) = &t.fallback {
                assert!(LAYERS.contains_key(l), "{}: no layer {l}", t.name);
            }
        }
        let noto = &LAYERS["noto"];
        assert_eq!(noto.fonts.len(), 72);
        let theme = Theme::shipped();
        let css = theme.typeset_css("default26").unwrap();
        let token = |name: &str| {
            css.lines()
                .find_map(|l| l.strip_prefix(&format!("\t--{name}: ")))
                .unwrap()
                .trim_end_matches(';')
                .to_string()
        };
        let base = token("font-family-base");
        assert!(base.starts_with("'Source Sans 3', 'Noto Sans', 'Noto Sans Arabic', "));
        assert!(base.ends_with("'Noto Sans Symbols 2', system-ui, sans-serif"));
        assert!(!base.contains("'Noto Serif'") && !base.contains("Naskh"));
        let heading = token("font-family-heading-main");
        assert!(heading.starts_with("'Source Serif 4', 'Noto Serif', 'Noto Naskh Arabic', "));
        // A script without a Noto serif heads in its sans; Tibetan has only a serif.
        assert!(
            heading.contains("'Noto Sans Tifinagh'") && heading.contains("'Noto Serif Tibetan'")
        );
        assert!(heading.ends_with(", Georgia, 'Times New Roman', serif"));
        let code = token("font-family-monospace");
        assert!(code.starts_with("'Lilex', 'Noto Sans Arabic', "));
        assert!(!code.contains("'Noto Sans',") && code.ends_with(", Menlo, Consolas, monospace"));
        assert!(css.contains(
            ":lang(ur):not(code, pre, kbd, samp) {\n\tfont-family: 'Noto Nastaliq Urdu', var(--font-family-base);\n}"
        ));
        // A sans type set takes the layer's sans families for its headings too.
        let h = theme.typeset_css("hyperlegible").unwrap();
        assert!(h.contains("--font-family-heading-main: 'Atkinson Hyperlegible', 'Noto Sans', "));
        assert!(!h.contains("'Noto Serif'"));
        // `basic` serves no typeface and names no layer.
        assert!(!theme.typeset_css("basic").unwrap().contains("Noto"));
        assert_eq!(theme.fonts("default26").len(), 3 + 72);
        if let Some(layer) = crate::assets::fallback_fonts("noto") {
            // Built with the frontend: the layer's faces follow the type set's own, each
            // for its script's ranges, in WOFF2 alone.
            assert!(css.contains(&layer));
            assert!(layer.contains("font-family:Noto Sans Arabic;"));
            assert!(layer.contains(",U+750-77F,"));
            assert!(!layer.contains(".woff)"));
        }
        // A type set naming a layer that does not exist is served as it is.
        let mut info = SHIPPED.clone();
        info.typesets[0].fallback = Some("nope".into());
        let name = info.typesets[0].name.clone();
        let css = Theme::from_info(Some(&info)).typeset_css(&name).unwrap();
        assert!(!css.contains("Noto Sans Arabic"));
    }

    #[test]
    fn files_name_their_sheets() {
        let t = Theme::shipped();
        let href = t.href(Sheet::Typeset, "default26").unwrap();
        let file = href.strip_prefix("/ui/theme/").unwrap();
        assert_eq!(t.file(file).map(|(_, current)| current), Some(true));
        assert_eq!(
            t.file("t-default26-0000000000000000.css")
                .map(|(_, current)| current),
            Some(false)
        );
        assert!(t.file("t-nope-0000000000000000.css").is_none());
        assert_eq!(t.file("0123456789abcdef.css").map(|(_, c)| c), Some(false));
        let data: Value = serde_json::from_str(&t.data()).unwrap();
        assert_eq!(data["palette"], "parchment");
        assert_eq!(
            data["palettes"]["parchment-rg"],
            json!(t.href(Sheet::Palette, "parchment-rg").unwrap())
        );
    }

    #[test]
    fn chips_swap_in_dark_mode() {
        let p = triplespace_client::ProviderInfo {
            code: "WD".into(),
            chip: Some(triplespace_client::ChipColours {
                color: "#FFFFFF".into(),
                background: "#2B559E".into(),
            }),
            ..Default::default()
        };
        let css = Theme::shipped()
            .with_chips(&[p])
            .palette_css("parchment")
            .unwrap();
        assert!(
            css.contains(".ts-chip--p-wd {\n\tcolor: #FFFFFF;\n\tbackground-color: #2B559E;\n}")
        );
        assert!(css.contains(
            ":root[data-mode=\"dark\"] .ts-chip--p-wd {\n\tcolor: #2B559E;\n\tbackground-color: #FFFFFF;\n}"
        ));
    }

    #[test]
    fn contrast_ratios() {
        assert!((contrast("#000", "#fff").unwrap() - 21.0).abs() < 1e-9);
        assert!((contrast("#fff", "#fff").unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(contrast("red", "#fff"), None);
    }

    #[test]
    fn codex_values_parse() {
        let (light, dark) = &*CODEX;
        assert_eq!(light["color-inverted-fixed"], "#fff");
        assert_eq!(dark["color-base"], "#eaecf0");
        assert!(light.len() > 150 && dark.len() > 90);
    }
}
