//! Markdown (0008 §5; 0019 §5; 14 §1.6): CommonMark with GitHub's tables, strikethrough,
//! autolinks and task lists, and MediaWiki's `[[Title]]`, `[[Title|text]]` and
//! `[[Title#Section]]` links, rendered to HTML.
//!
//! Two things come from the caller, since both are the tenant's: where a wiki link goes
//! (the title resolver of 0008 §3), and the sanitizer (the allow-list of 0008 §8, which
//! the wikitext renderer owns). Raw HTML in the source passes to the sanitizer, which
//! keeps what the allow-list keeps; CommonMark alone would pass it through.

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, format_html, parse_document};

/// A rendered document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The sanitized HTML, in a `<div class="ts-markdown">`.
    pub html: String,
    /// The targets of its wiki links, as written, in order, each once.
    pub links: Vec<String>,
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    // `[[Title|text]]`: the title before the pipe, as MediaWiki writes it.
    o.extension.wikilinks_title_after_pipe = true;
    // Raw HTML reaches the sanitizer, which decides what stays.
    o.render.unsafe_ = true;
    o
}

/// Renders markdown. `link` turns a wiki link's target into its `href`; `sanitize` cleans
/// the HTML before it is returned.
pub fn render(
    text: &str,
    link: &dyn Fn(&str) -> String,
    sanitize: &dyn Fn(&str) -> String,
) -> Rendered {
    let arena = Arena::new();
    let options = options();
    let root = parse_document(&arena, text, &options);
    let mut links: Vec<String> = Vec::new();
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        if let NodeValue::WikiLink(w) = &mut data.value {
            let target = w.url.trim().to_string();
            if !links.contains(&target) {
                links.push(target.clone());
            }
            w.url = link(&target);
        }
    }
    let mut out = String::new();
    if format_html(root, &options, &mut out).is_err() {
        out.clear();
    }
    Rendered {
        html: format!("<div class=\"ts-markdown\">{}</div>", sanitize(&out)),
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wiki(t: &str) -> String {
        format!("/wiki/{}", t.replace(' ', "_"))
    }

    fn keep(h: &str) -> String {
        h.to_string()
    }

    #[test]
    fn commonmark_and_wiki_links() {
        let r = render(
            "**Yes**, see [[Item:Q42]] and [[Project:Scope|the scope]].",
            &wiki,
            &keep,
        );
        assert!(r.html.contains("<strong>Yes</strong>"), "{}", r.html);
        assert!(
            r.html
                .contains("<a href=\"/wiki/Item:Q42\" data-wikilink=\"true\">Item:Q42</a>"),
            "{}",
            r.html
        );
        assert!(
            r.html
                .contains("<a href=\"/wiki/Project:Scope\" data-wikilink=\"true\">the scope</a>"),
            "{}",
            r.html
        );
        assert_eq!(r.links, ["Item:Q42", "Project:Scope"]);
    }

    #[test]
    fn the_sanitizer_sees_raw_html() {
        let r = render("a <script>x</script> b", &wiki, &|h: &str| {
            h.replace("<script>x</script>", "")
        });
        assert!(!r.html.contains("script"), "{}", r.html);
    }

    #[test]
    fn tildes_are_text() {
        let r = render("a ~~~~ b", &wiki, &keep);
        assert!(r.html.contains("a ~~~~ b"), "{}", r.html);
    }
}
