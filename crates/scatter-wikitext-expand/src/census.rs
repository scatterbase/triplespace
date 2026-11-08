//! The template census of one text (0042 §5; 10 §6; 11 §2.1): which templates, modules,
//! variables, parser functions and extension tags a page's wikitext calls, each with the
//! registry's status, so that an import can say before it starts which of them the
//! export carries and which fall outside the registry. A pure scan over the
//! preprocessor's tree: nothing is expanded.

use std::collections::{BTreeMap, BTreeSet};

use crate::registry::Registry;
use crate::{Mode, PNode, preprocess};

/// What the registry says of a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// Expanded or rendered as MediaWiki does.
    Implemented,
    /// Recognised and rendered as a chip.
    Chip,
    /// Recognised and without effect.
    Ignored,
    /// Not in the registry.
    Unregistered,
}

impl Status {
    fn of(status: Option<&str>) -> Self {
        match status {
            Some("implemented") => Self::Implemented,
            Some("chip") => Self::Chip,
            Some("ignored") => Self::Ignored,
            _ => Self::Unregistered,
        }
    }

    /// The status's name, as the registry spells it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Chip => "chip",
            Self::Ignored => "ignored",
            Self::Unregistered => "unregistered",
        }
    }
}

/// What a text uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Uses {
    /// Template names as the Template namespace titles them (`Cite web`), normalised:
    /// underscores to spaces, the first letter upper-cased, a `Template:` prefix dropped.
    /// A transcluded page of another namespace keeps its prefix (`:Main page`,
    /// `User:Example/box`).
    pub templates: BTreeSet<String>,
    /// Module names, from `#invoke:` (`String`).
    pub modules: BTreeSet<String>,
    /// Variables and parser functions, by the name as the registry spells it (`#if`,
    /// `PAGENAME`, `lc`), with their status.
    pub functions: BTreeMap<String, Status>,
    /// Extension tags found, lower-cased, with their status; an unknown tag that is not
    /// HTML is `Unregistered`.
    pub tags: BTreeMap<String, Status>,
}

/// Tags HTML allows in wikitext (0008 §8's allow-list, abridged to what MediaWiki's
/// sanitizer passes), which the census does not report.
const HTML_TAGS: &[&str] = &[
    "a",
    "abbr",
    "b",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "data",
    "dd",
    "del",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "ins",
    "kbd",
    "li",
    "link",
    "mark",
    "meta",
    "ol",
    "p",
    "q",
    "rb",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "tfoot",
    "time",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
    "img",
    "includeonly",
    "noinclude",
    "onlyinclude",
];

/// Extension tags whose content is wikitext, scanned as the page's own text is.
const WIKITEXT_INSIDE: &[&str] = &["ref", "references", "poem", "indicator", "gallery"];

/// Modifiers a call may start with.
const MODIFIERS: &[&str] = &["subst", "safesubst", "msg", "msgnw", "raw"];

/// Scans a text against the embedded registry.
#[must_use]
pub fn scan(text: &str) -> Uses {
    scan_with(text, Registry::embedded())
}

/// Scans a text against a registry.
#[must_use]
pub fn scan_with(text: &str, registry: &Registry) -> Uses {
    let mut uses = Uses::default();
    let mut census = Census {
        registry,
        uses: &mut uses,
        depth: 0,
    };
    census.text(text);
    uses
}

struct Census<'a> {
    registry: &'a Registry,
    uses: &'a mut Uses,
    depth: usize,
}

impl Census<'_> {
    fn text(&mut self, text: &str) {
        if self.depth > 8 {
            return;
        }
        self.depth += 1;
        self.tags(text);
        let nodes = preprocess(text, Mode::View, self.registry);
        self.nodes(&nodes);
        self.depth -= 1;
    }

    fn nodes(&mut self, nodes: &[PNode]) {
        for n in nodes {
            match n {
                PNode::Text(_) => {}
                PNode::Argument { parts } => {
                    for p in parts {
                        self.nodes(p);
                    }
                }
                PNode::Template { parts, .. } => {
                    if let Some(first) = parts.first() {
                        let name = flat_text(first);
                        self.call(name.trim());
                    }
                    for p in parts {
                        self.nodes(p);
                    }
                }
            }
        }
    }

    /// One `{{…}}` by its name part.
    fn call(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        if let Some(rest) = name.strip_prefix('#') {
            let (head, arg) = rest.split_once(':').unwrap_or((rest, ""));
            let key = format!("#{}", head.trim().to_ascii_lowercase());
            if key == "#invoke" {
                let module = normalise(arg.trim());
                if !module.is_empty() {
                    self.uses.modules.insert(module);
                }
            }
            let status = Status::of(self.registry.magic_status(&key));
            self.uses.functions.insert(key, status);
            return;
        }
        let (head, rest) = match name.split_once(':') {
            Some((h, r)) => (h.trim(), Some(r)),
            None => (name, None),
        };
        if let Some(rest) = rest {
            if MODIFIERS.iter().any(|m| m.eq_ignore_ascii_case(head)) {
                let lower = head.to_ascii_lowercase();
                if lower == "msgnw" || lower == "raw" {
                    self.template(rest.trim());
                } else {
                    self.call(rest.trim());
                }
                return;
            }
            if head.eq_ignore_ascii_case("int") {
                self.uses
                    .functions
                    .insert("int".into(), Status::of(self.registry.magic_status("int")));
                return;
            }
        }
        if self.registry.is_magic(name) {
            let key = self
                .registry
                .canonical_magic(head)
                .unwrap_or(head)
                .to_string();
            let status = Status::of(self.registry.magic_status(&key));
            self.uses.functions.insert(key, status);
            return;
        }
        self.template(name);
    }

    /// A transcluded page.
    fn template(&mut self, name: &str) {
        let name = normalise(name);
        if name.is_empty() || name.contains("{{") {
            return;
        }
        let name = match name.split_once(':') {
            Some((prefix, rest)) if prefix.eq_ignore_ascii_case("template") => normalise(rest),
            _ => name,
        };
        if !name.is_empty() {
            self.uses.templates.insert(name);
        }
    }

    /// The tags of a text, and the wikitext inside those that hold some.
    fn tags(&mut self, text: &str) {
        let mut i = 0;
        while let Some(p) = text[i..].find('<') {
            let at = i + p;
            if text[at..].starts_with("<!--") {
                i = text[at + 4..]
                    .find("-->")
                    .map_or(text.len(), |e| at + 4 + e + 3);
                continue;
            }
            let Some((name, closing, self_closing, end)) = crate::tag_at(text, at) else {
                i = at + 1;
                continue;
            };
            i = end;
            if closing {
                continue;
            }
            if self.registry.is_extension_tag(&name) {
                let status = Status::of(self.registry.tag_status(&name));
                self.uses.tags.insert(name.clone(), status);
                if !self_closing {
                    let close = crate::end_of_element(text, end, &name);
                    let inner_end = text[..close]
                        .rfind("</")
                        .filter(|&c| c >= end)
                        .unwrap_or(close);
                    if WIKITEXT_INSIDE.contains(&name.as_str()) {
                        self.text(&text[end..inner_end]);
                    }
                    i = close;
                }
            } else if !HTML_TAGS.contains(&name.as_str()) {
                self.uses.tags.insert(name, Status::Unregistered);
            }
        }
    }
}

/// The text of a part, with any call inside it left as `{{…}}`.
fn flat_text(nodes: &[PNode]) -> String {
    let mut out = String::new();
    for n in nodes {
        match n {
            PNode::Text(t) => out.push_str(t),
            PNode::Template { .. } | PNode::Argument { .. } => out.push_str("{{…}}"),
        }
    }
    out
}

/// A title as the Template namespace spells it: underscores to spaces, runs of space
/// collapsed, trimmed, the first letter upper-cased.
fn normalise(name: &str) -> String {
    let name = name
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_functions_tags_and_modules() {
        let text = "{{Cite web|url=x}} {{cite_web }} {{#if:{{{1|}}}|{{Yes}}|no}} {{PAGENAME}} \
                    {{lc:ABC}} {{#invoke:String|rep|x|3}} {{#foo:bar}} <ref>{{Harvnb|Smith}}</ref> \
                    <math>x</math> <nowiki>{{Not a call}}</nowiki> <smwdoc/> {{subst:Welcome}} \
                    {{:Main page}} {{Template:Infobox}} <!-- {{Hidden}} -->";
        let u = scan(text);
        assert_eq!(
            u.templates.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                ":Main page",
                "Cite web",
                "Harvnb",
                "Infobox",
                "Welcome",
                "Yes"
            ]
        );
        assert_eq!(
            u.modules.iter().map(String::as_str).collect::<Vec<_>>(),
            ["String"]
        );
        assert_eq!(u.functions["#if"], Status::Implemented);
        assert_eq!(u.functions["PAGENAME"], Status::Implemented);
        assert_eq!(u.functions["lc"], Status::Implemented);
        assert_eq!(u.functions["#foo"], Status::Unregistered);
        assert!(u.functions.contains_key("#invoke"));
        assert_eq!(u.tags["ref"], Status::Implemented);
        assert_eq!(u.tags["math"], Status::Chip);
        assert_eq!(u.tags["nowiki"], Status::Implemented);
        assert_eq!(u.tags["smwdoc"], Status::Unregistered);
        assert!(!u.templates.contains("Not a call"));
        assert!(!u.templates.contains("Hidden"));
    }

    #[test]
    fn nothing_from_plain_text() {
        let u = scan("Just <b>text</b> with a < b and [[links]].");
        assert_eq!(u, Uses::default());
    }
}
