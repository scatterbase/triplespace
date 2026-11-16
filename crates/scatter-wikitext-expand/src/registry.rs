//! The registry of variables, parser functions, extension tags and behaviour switches,
//! embedding `docs/registry/wikitext-functions.toml` (0042 §5; 11 §2.1).

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

/// The registry file, as embedded.
pub const WIKITEXT_FUNCTIONS_TOML: &str =
    include_str!("../../../docs/registry/wikitext-functions.toml");

#[derive(Deserialize)]
struct Entry {
    status: String,
    #[serde(default)]
    synonyms: Vec<String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    variable: BTreeMap<String, Entry>,
    #[serde(default)]
    function: BTreeMap<String, Entry>,
    #[serde(default)]
    tag: BTreeMap<String, Entry>,
}

/// What the registry names, by kind.
#[derive(Debug)]
pub struct Registry {
    /// Variables and parser functions, as written in wikitext, with synonyms.
    magic: HashSet<String>,
    /// Extension tags, lowercase.
    tags: HashSet<String>,
    /// Each variable's and function's status, by its name and each synonym.
    magic_status: BTreeMap<String, String>,
    /// Each spelling's canonical name.
    canonical: BTreeMap<String, String>,
    /// Each tag's status, by its name and each synonym, lowercase.
    tag_status: BTreeMap<String, String>,
}

/// Modifiers a call may start with; this slice leaves them all as written.
const MODIFIERS: &[&str] = &["subst", "safesubst", "msg", "msgnw", "raw", "int"];

impl Registry {
    /// The embedded registry.
    ///
    /// # Panics
    ///
    /// If the embedded file does not parse, which its test rules out.
    #[must_use]
    pub fn embedded() -> &'static Registry {
        static R: OnceLock<Registry> = OnceLock::new();
        R.get_or_init(|| {
            Registry::parse(WIKITEXT_FUNCTIONS_TOML).expect("the embedded registry parses")
        })
    }

    /// Parses a registry file.
    pub fn parse(toml_text: &str) -> Result<Self, toml::de::Error> {
        let f: File = toml::from_str(toml_text)?;
        let mut magic = HashSet::new();
        let mut magic_status = BTreeMap::new();
        let mut canonical = BTreeMap::new();
        for (name, e) in f.variable.iter().chain(f.function.iter()) {
            magic.insert(name.clone());
            magic.extend(e.synonyms.iter().cloned());
            for spelling in std::iter::once(name).chain(e.synonyms.iter()) {
                magic_status.insert(spelling.clone(), e.status.clone());
                canonical.insert(spelling.clone(), name.clone());
            }
        }
        let mut tags: HashSet<String> = HashSet::new();
        let mut tag_status = BTreeMap::new();
        for (name, e) in &f.tag {
            tags.insert(name.to_lowercase());
            tags.extend(e.synonyms.iter().map(|s| s.to_lowercase()));
            for spelling in std::iter::once(name).chain(e.synonyms.iter()) {
                tag_status.insert(spelling.to_lowercase(), e.status.clone());
            }
        }
        Ok(Self {
            magic,
            tags,
            magic_status,
            canonical,
            tag_status,
        })
    }

    /// The status of a variable or parser function by a spelling (`#if`, `PAGENAME`,
    /// `lc`), case-insensitively for lowercase function names; `None` when unregistered.
    #[must_use]
    pub fn magic_status(&self, name: &str) -> Option<&str> {
        let name = name.trim();
        if let Some(s) = self.magic_status.get(name) {
            return Some(s);
        }
        self.magic_status
            .iter()
            .find(|(k, _)| {
                k.eq_ignore_ascii_case(name) && k.chars().all(|c| !c.is_ascii_uppercase())
            })
            .map(|(_, s)| s.as_str())
    }

    /// The registry's own spelling of a variable or function, by any of its spellings.
    #[must_use]
    pub fn canonical_magic(&self, name: &str) -> Option<&str> {
        let name = name.trim();
        if let Some(c) = self.canonical.get(name) {
            return Some(c);
        }
        self.canonical
            .iter()
            .find(|(k, _)| {
                k.eq_ignore_ascii_case(name) && k.chars().all(|c| !c.is_ascii_uppercase())
            })
            .map(|(_, c)| c.as_str())
    }

    /// The status of an extension tag, lowercase; `None` when unregistered.
    #[must_use]
    pub fn tag_status(&self, name: &str) -> Option<&str> {
        self.tag_status
            .get(&name.to_lowercase())
            .map(String::as_str)
    }

    /// Whether a call's name is a variable, a parser function or a modified call, which
    /// this slice does not expand: `#if:…`, `PAGENAME`, `lc:…`, `subst:…`.
    #[must_use]
    pub fn is_magic(&self, name: &str) -> bool {
        if name.starts_with('#') {
            return true;
        }
        let head = name.split_once(':').map_or(name, |(h, _)| h).trim();
        if MODIFIERS.iter().any(|m| m.eq_ignore_ascii_case(head)) && name.contains(':') {
            return true;
        }
        self.magic.contains(name.trim())
            || (name.contains(':')
                && (self.magic.contains(head)
                    || self.magic.iter().any(|m| {
                        m.eq_ignore_ascii_case(head) && m.chars().all(|c| c.is_ascii_lowercase())
                    })))
    }

    /// Whether a tag is an extension tag, whose content expansion leaves alone.
    #[must_use]
    pub fn is_extension_tag(&self, name: &str) -> bool {
        self.tags.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_registry_parses() {
        let r = Registry::embedded();
        assert!(r.is_magic("PAGENAME"));
        assert!(r.is_magic("REVISIONUSER"));
        assert!(r.is_magic("#if: x"));
        assert!(r.is_magic("lc:X"));
        assert!(r.is_magic("LC:X"));
        assert!(r.is_magic("DEFAULTSORT:k"));
        assert!(r.is_magic("subst:Foo"));
        assert!(!r.is_magic("cwbn"));
        assert!(!r.is_magic("User:Example/box"));
        assert!(r.is_extension_tag("nowiki"));
        assert!(r.is_extension_tag("source"));
        assert!(!r.is_extension_tag("div"));
    }
}
