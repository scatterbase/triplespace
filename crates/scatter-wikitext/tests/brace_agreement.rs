//! The expander and the parser read a text's calls alike (0042 §4; 0115 §5): every
//! outermost `{{…}}` and `{{{…}}}` the parser finds is a call `scatter-wikitext-expand`'s
//! preprocessor matched, with the same extent, over random text. The parser matches
//! braces as the preprocessor does before it parses, so a call the expander left as
//! written renders as the chip the parser draws for it, and nothing else does.
//!
//! The inclusion tags are left out: the parser keeps them as nodes for a reader of the
//! source, where viewing a page removes them, and an expanded text holds none.

use scatter_wikitext_parse::{Configuration, Node, Positioned as _};

/// The outermost calls, outside extension tags, whose content the preprocessor does
/// not read.
fn outer(nodes: &[Node], out: &mut Vec<(usize, usize)>) {
    for n in nodes {
        match n {
            Node::Template { .. } | Node::Parameter { .. } => out.push((n.start(), n.end())),
            Node::Link { text: c, .. }
            | Node::Image { text: c, .. }
            | Node::Category { ordinal: c, .. }
            | Node::Heading { nodes: c, .. }
            | Node::Preformatted { nodes: c, .. }
            | Node::ExternalLink { nodes: c, .. } => outer(c, out),
            Node::OrderedList { items, .. } | Node::UnorderedList { items, .. } => {
                for i in items {
                    outer(&i.nodes, out);
                }
            }
            Node::DefinitionList { items, .. } => {
                for i in items {
                    outer(&i.nodes, out);
                }
            }
            Node::Table {
                attributes,
                captions,
                rows,
                ..
            } => {
                outer(attributes, out);
                for c in captions {
                    outer(&c.content, out);
                }
                for r in rows {
                    outer(&r.attributes, out);
                    for c in &r.cells {
                        if let Some(a) = &c.attributes {
                            outer(a, out);
                        }
                        outer(&c.content, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn parser_calls(c: &Configuration, s: &str) -> Vec<(usize, usize)> {
    let mut calls = Vec::new();
    outer(
        &c.parse_without_timeout(s).expect("no limit").nodes,
        &mut calls,
    );
    calls.sort_unstable();
    calls
}

fn expander_calls(s: &str) -> Vec<(usize, usize)> {
    scatter_wikitext_expand::call_spans(s)
        .into_iter()
        .map(|r| (r.start, r.end))
        .collect()
}

#[test]
fn examples() {
    let c = Configuration::default();
    for s in [
        "{{a}} {{{b}}} {{c|{{d}}}}",
        "{{{{{a}}}",
        "{{a[[b}}c",
        "{{a[[b]]}}",
        "{{}",
        "{{a<nowiki>}}</nowiki>}}",
        "{{a<!--}}-->}}",
        "<ref>{{a}}</ref>{{b<ref>}}</ref>",
        "{{<ref><pre></ref><pre>}}</pre>}}",
        "[[a<!--x-->]]{{b}}",
        "[[<ref>]]{{}}</ref>",
    ] {
        assert_eq!(parser_calls(&c, s), expander_calls(s), "{s:?}");
    }
}

#[test]
fn random_text() {
    let pieces = [
        "{{",
        "}}",
        "{",
        "}",
        "{{{",
        "}}}",
        "[[",
        "]]",
        "[",
        "]",
        "|",
        "\n",
        "a",
        " ",
        "=",
        "<!--",
        "-->",
        "<nowiki>",
        "</nowiki>",
        "<ref>",
        "</ref>",
        "<ref/>",
        "<pre>",
        "</pre>",
        "{|",
        "|}",
        "|-",
        "''",
        "<div>",
        "</div>",
        "* ",
        "# ",
        "== ",
        "[[File:x|",
        "http://x ",
        "é",
    ];
    let c = Configuration::default();
    let mut seed: u64 = 0x1234_5678_9ABC_DEF1;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % 1_000_003).expect("small")
    };
    let mut differ = Vec::new();
    for _ in 0..30_000 {
        let len = next() % 30 + 1;
        let s: String = (0..len).map(|_| pieces[next() % pieces.len()]).collect();
        if parser_calls(&c, &s) != expander_calls(&s) {
            differ.push(s);
        }
    }
    assert!(
        differ.is_empty(),
        "{} differ, e.g. {:?}",
        differ.len(),
        &differ[..differ.len().min(5)]
    );
}
