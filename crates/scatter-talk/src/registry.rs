//! `docs/registry/talk-markup.toml`: the conventions of wikitext talk pages (0090 §5).
//!
//! The registry is universal: every entry applies on every wiki where it matches. What is
//! specific to a wiki (its language, timezone and namespace names) comes from [`crate::Site`].

use std::sync::OnceLock;

use serde::Deserialize;

use crate::time::Format;

/// The embedded registry file.
pub const TALK_MARKUP_TOML: &str = include_str!("../../../docs/registry/talk-markup.toml");

/// The parsed registry: timestamp formats, month names, undated-signature marks and template
/// names by role.
#[derive(Debug, Clone)]
pub struct Markup {
    /// Timestamp formats, compiled from their MediaWiki date-format strings.
    pub formats: Vec<Format>,
    /// Month names and abbreviations, per language.
    pub months: Vec<Months>,
    /// What may precede an undated signature's user link.
    pub undated_marks: Vec<String>,
    /// Whether a user link alone on its line is an undated signature.
    pub undated_alone: bool,
    /// Template names that sign the text before them (`{{unsigned}}`), normalized.
    pub unsigned: Vec<String>,
    /// Template names whose call makes the next comment reply to the previous one, normalized.
    pub outdent: Vec<String>,
    /// Wrapper pairs: the opening and the closing template names, normalized.
    pub wrappers: Vec<Wrapper>,
    /// Literal strings a signing bot or a substituted unsigned template leaves.
    pub autosign: Vec<String>,
}

/// Month names for one language.
#[derive(Debug, Clone, Deserialize)]
pub struct Months {
    /// The language code.
    pub lang: String,
    /// Full names, January first.
    pub long: Vec<String>,
    /// Abbreviations, January first.
    pub short: Vec<String>,
}

/// A wrapper pair (0090 §3).
#[derive(Debug, Clone)]
pub struct Wrapper {
    /// Names of the opening template, normalized.
    pub open: Vec<String>,
    /// Names of the closing template, normalized.
    pub close: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    timestamp: Vec<TimestampEntry>,
    months: Vec<Months>,
    undated: Undated,
    template: Vec<TemplateEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimestampEntry {
    format: String,
    zone: Option<String>,
    #[allow(dead_code)]
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Undated {
    marks: Vec<String>,
    alone: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateEntry {
    role: String,
    #[allow(dead_code)]
    lang: String,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    open: Vec<String>,
    #[serde(default)]
    close: Vec<String>,
    #[serde(default)]
    markers: Vec<String>,
}

/// An error in the registry file.
#[derive(Debug)]
pub struct RegistryError(pub String);

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "talk-markup.toml: {}", self.0)
    }
}

impl std::error::Error for RegistryError {}

/// A template name as MediaWiki compares them: underscores are spaces, runs of spaces are one,
/// and the first letter is case-insensitive.
pub fn normalize_template_name(name: &str) -> String {
    let name = name.trim();
    let name = name
        .strip_prefix("subst:")
        .or_else(|| name.strip_prefix("safesubst:"))
        .unwrap_or(name)
        .trim();
    let mut out = String::with_capacity(name.len());
    let mut space = false;
    for c in name.chars() {
        if c == '_' || c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    let mut chars = out.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => out,
    }
}

impl Markup {
    /// Parses a registry file.
    pub fn from_toml(text: &str) -> Result<Self, RegistryError> {
        let file: File = toml::from_str(text).map_err(|e| RegistryError(e.to_string()))?;
        if file.version != 1 {
            return Err(RegistryError(format!("unknown version {}", file.version)));
        }
        let mut formats = Vec::with_capacity(file.timestamp.len());
        for t in &file.timestamp {
            let zone = match t.zone.as_deref() {
                None => false,
                Some("optional") => true,
                Some(other) => return Err(RegistryError(format!("unknown zone rule {other:?}"))),
            };
            formats.push(Format::compile(&t.format, zone).map_err(RegistryError)?);
        }
        let norm = |v: &[String]| {
            v.iter()
                .map(|n| normalize_template_name(n))
                .collect::<Vec<_>>()
        };
        let mut markup = Markup {
            formats,
            months: file.months,
            undated_marks: file.undated.marks,
            undated_alone: file.undated.alone,
            unsigned: Vec::new(),
            outdent: Vec::new(),
            wrappers: Vec::new(),
            autosign: Vec::new(),
        };
        for t in &file.template {
            match t.role.as_str() {
                "unsigned" => markup.unsigned.extend(norm(&t.names)),
                "outdent" => markup.outdent.extend(norm(&t.names)),
                "wrapper" => {
                    if t.open.is_empty() || t.close.is_empty() {
                        return Err(RegistryError("a wrapper needs `open` and `close`".into()));
                    }
                    markup.wrappers.push(Wrapper {
                        open: norm(&t.open),
                        close: norm(&t.close),
                    });
                }
                "autosign" => markup.autosign.extend(t.markers.iter().cloned()),
                other => return Err(RegistryError(format!("unknown role {other:?}"))),
            }
        }
        Ok(markup)
    }

    /// The embedded registry, parsed once.
    pub fn embedded() -> &'static Markup {
        static MARKUP: OnceLock<Markup> = OnceLock::new();
        MARKUP.get_or_init(|| Markup::from_toml(TALK_MARKUP_TOML).expect("talk-markup.toml parses"))
    }

    /// The month table for a language, falling back to English.
    pub fn months_for(&self, lang: &str) -> Option<&Months> {
        self.months
            .iter()
            .find(|m| m.lang == lang)
            .or_else(|| self.months.iter().find(|m| m.lang == "en"))
    }

    /// Whether a normalized template name is a wrapper's opening template; the wrapper's index.
    pub fn wrapper_opening(&self, name: &str) -> Option<usize> {
        self.wrappers
            .iter()
            .position(|w| w.open.iter().any(|n| n == name))
    }

    /// Whether a normalized template name closes any wrapper.
    pub fn is_wrapper_template(&self, name: &str) -> bool {
        self.wrappers
            .iter()
            .any(|w| w.open.iter().chain(&w.close).any(|n| n == name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_parses() {
        let m = Markup::embedded();
        assert!(m.formats.len() >= 6);
        assert!(m.unsigned.contains(&"Unsigned".to_string()));
        assert!(m.outdent.contains(&"Od".to_string()));
        assert_eq!(m.wrapper_opening("Archive top"), Some(0));
        assert!(m.is_wrapper_template("Hab"));
        assert_eq!(
            m.months_for("en").map(|t| t.long[0].as_str()),
            Some("January")
        );
    }

    #[test]
    fn template_names_normalize() {
        assert_eq!(normalize_template_name("subst:unsigned_IP"), "Unsigned IP");
        assert_eq!(normalize_template_name("  archive   top "), "Archive top");
    }
}
