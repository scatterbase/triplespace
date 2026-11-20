//! An edit summary as a feed row, a history row and a diff's header show it (19 §3.4):
//! parsed as far as MediaWiki parses a comment, and no further.
//!
//! - **`/* … */`** is an automatic comment, drawn apart from the editor's own words. On a
//!   page it names a section and links to it, with MediaWiki's arrow. Where it is one of
//!   Wikibase's (`/* wbsetlabel-add:1|en */`, which an adopted revision keeps as its
//!   source wrote it), it is said in words, as Wikibase's own history says it; one this
//!   module has no words for is shown as written.
//! - **`[[Target]]` and `[[Target|text]]`** are links to the page, a leading colon
//!   dropped. Nothing is looked up: a link to a missing page is not marked.
//!
//! Everything else is text. Templates, bold and italics are not expanded in a summary,
//! as MediaWiki does not expand them.

use serde::Serialize;

use crate::frame::title_url;
use crate::i18n::Messages;

/// One part of a summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Part {
    /// The editor's words.
    Text {
        /// The text.
        text: String,
    },
    /// A wiki link.
    Link {
        /// The page.
        href: String,
        /// The text.
        text: String,
    },
    /// An automatic comment.
    Auto {
        /// What it says.
        text: String,
        /// The section it names, on a page.
        href: Option<String>,
        /// Whether more of the summary follows it, so that it ends with a colon.
        more: bool,
    },
}

/// MediaWiki's anchor for a heading: spaces as underscores, the rest percent-encoded by
/// the browser.
fn anchor(section: &str) -> String {
    title_url(section.trim())
}

/// A Wikibase automatic comment in words: `key:count|language|…`. `None` when the text
/// is not one, or the messages have no words for its key.
fn wikibase(m: &Messages, auto: &str) -> Option<String> {
    let (key, args) = auto.split_once(':').unwrap_or((auto, ""));
    let key = key.trim();
    if !(key.starts_with("wb") || key.starts_with("special-create-"))
        || !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return None;
    }
    let msg = format!("ts-summary-{key}");
    if !m.has(&msg) {
        return None;
    }
    // The first argument is a count; the second is the language or the site.
    let mut it = args.split('|');
    let count = it.next().unwrap_or_default().trim();
    let first = it.next().unwrap_or_default().trim();
    Some(m.with(&msg, &[first, count]))
}

/// The links of a run of text.
fn links(text: &str, out: &mut Vec<Part>) {
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else {
            break;
        };
        let inner = &rest[start + 2..start + 2 + len];
        let (target, shown) = inner.split_once('|').unwrap_or((inner, inner));
        let target = target.trim().trim_start_matches(':').trim();
        // Not a link: nothing to go to, or brackets inside the target.
        if target.is_empty() || target.contains(['[', ']', '{', '}', '<', '>']) {
            push_text(out, &rest[..start + 2]);
            rest = &rest[start + 2..];
            continue;
        }
        push_text(out, &rest[..start]);
        let shown = shown.trim();
        out.push(Part::Link {
            href: format!("/wiki/{}", title_url(target)),
            text: if shown.is_empty() { target } else { shown }
                .trim_start_matches(':')
                .to_string(),
        });
        rest = &rest[start + 2 + len + 2..];
    }
    push_text(out, rest);
}

fn push_text(out: &mut Vec<Part>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Part::Text { text: last }) = out.last_mut() {
        last.push_str(text);
    } else {
        out.push(Part::Text {
            text: text.to_string(),
        });
    }
}

/// A summary's parts. `page` is the title of the page the edit was to, where an
/// automatic comment may name one of its sections; an entity has none.
#[must_use]
pub fn parts(m: &Messages, comment: &str, page: Option<&str>) -> Vec<Part> {
    let mut out = Vec::new();
    let mut rest = comment;
    while let Some(start) = rest.find("/*") {
        let Some(len) = rest[start + 2..].find("*/") else {
            break;
        };
        let auto = rest[start + 2..start + 2 + len].trim();
        let after = &rest[start + 2 + len + 2..];
        if auto.is_empty() {
            links(&rest[..start + 2 + len + 2], &mut out);
            rest = after;
            continue;
        }
        links(&rest[..start], &mut out);
        let words = wikibase(m, auto);
        let href = match (&words, page) {
            (None, Some(title)) => Some(format!("/wiki/{}#{}", title_url(title), anchor(auto))),
            _ => None,
        };
        out.push(Part::Auto {
            text: match (words, &href) {
                (Some(w), _) => w,
                (None, Some(_)) => format!("\u{2192}{auto}"),
                (None, None) => auto.to_string(),
            },
            href,
            more: !after.trim().is_empty(),
        });
        rest = after.trim_start();
    }
    links(rest, &mut out);
    // Text that is only space between two parts stays; a summary of nothing is empty.
    if out
        .iter()
        .all(|p| matches!(p, Part::Text { text } if text.trim().is_empty()))
    {
        out.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Part {
        Part::Text { text: t.into() }
    }

    // 19 §3.4: a summary is parsed as MediaWiki parses a comment.
    #[test]
    fn a_section_comment_links_to_the_section() {
        let m = Messages::for_language("en");
        assert_eq!(
            parts(&m, "/* Early life */ fixed a date", Some("Ada Lovelace")),
            vec![
                Part::Auto {
                    text: "→Early life".into(),
                    href: Some("/wiki/Ada_Lovelace#Early_life".into()),
                    more: true,
                },
                text("fixed a date"),
            ]
        );
        // Alone, and on an entity, where there is no section to go to.
        assert_eq!(
            parts(&m, "/* Notes */", None),
            vec![Part::Auto {
                text: "Notes".into(),
                href: None,
                more: false,
            }]
        );
    }

    #[test]
    fn a_wikibase_comment_is_said_in_words() {
        let m = Messages::for_language("en");
        assert_eq!(
            parts(&m, "/* wbsetlabel-add:1|en */ Six", None),
            vec![
                Part::Auto {
                    text: "Added [en] label".into(),
                    href: None,
                    more: true,
                },
                text("Six"),
            ]
        );
        assert_eq!(
            parts(
                &m,
                "/* wbsetclaim-create:2||1 */ [[Property:P3]]: [[Q5]]",
                None
            ),
            vec![
                Part::Auto {
                    text: "Created claim".into(),
                    href: None,
                    more: true,
                },
                Part::Link {
                    href: "/wiki/Property:P3".into(),
                    text: "Property:P3".into(),
                },
                text(": "),
                Part::Link {
                    href: "/wiki/Q5".into(),
                    text: "Q5".into(),
                },
            ]
        );
        // One it has no words for is shown as written.
        assert_eq!(
            parts(&m, "/* wbsomething-new:1| */", None),
            vec![Part::Auto {
                text: "wbsomething-new:1|".into(),
                href: None,
                more: false,
            }]
        );
    }

    #[test]
    fn wiki_links_are_links_and_the_rest_is_text() {
        let m = Messages::for_language("en");
        assert_eq!(
            parts(
                &m,
                "see [[:Category:Books|the books]] and [[Help:A b]], not [[]] or [[x",
                None
            ),
            vec![
                text("see "),
                Part::Link {
                    href: "/wiki/Category:Books".into(),
                    text: "the books".into(),
                },
                text(" and "),
                Part::Link {
                    href: "/wiki/Help:A_b".into(),
                    text: "Help:A b".into(),
                },
                text(", not [[]] or [[x"),
            ]
        );
        assert_eq!(
            parts(&m, "'''bold''' {{tl}} <b>x</b>", None),
            vec![text("'''bold''' {{tl}} <b>x</b>")]
        );
        assert!(parts(&m, "  ", None).is_empty());
        assert_eq!(parts(&m, "a /* b", None), vec![text("a /* b")]);
    }
}
