//! The delta between two renderings as triples, and its SPARQL 1.1 Update request
//! (02 §6.2, §6.4; 0032 §9).
//!
//! A rendering is compared in the form it is stored and served in: one N-Triples line per
//! triple, sorted and without duplicates. `DELETE DATA` matches a triple only when its
//! lexical form is byte-identical to the one inserted, so the lines of a delta are the
//! lines of the dump and are never parsed and written again.

use oxrdf::Triple;

/// The triples one event removed from a graph and those it added, each as sorted
/// N-Triples lines (`<s> <p> <o> .`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    /// The lines that left the graph.
    pub deleted: Vec<String>,
    /// The lines that entered it.
    pub inserted: Vec<String>,
}

impl Delta {
    /// Whether the event changed nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.deleted.is_empty() && self.inserted.is_empty()
    }
}

/// A rendering in canonical form: one N-Triples line per triple, sorted by bytes, each
/// once. A rendering names a shared value or reference node wherever a statement uses it;
/// a graph holds it once.
#[must_use]
pub fn canonical_lines<'t>(triples: impl IntoIterator<Item = &'t Triple>) -> Vec<String> {
    let mut lines: Vec<String> = triples.into_iter().map(|t| format!("{t} .")).collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// The set difference of two canonical renderings (0032 §9): what `old` holds
/// and `new` does not, and the reverse. Both must be sorted and free of duplicates, as
/// [`canonical_lines`] writes them.
#[must_use]
pub fn diff(old: &[String], new: &[String]) -> Delta {
    let mut delta = Delta::default();
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        match old[i].cmp(&new[j]) {
            std::cmp::Ordering::Less => {
                delta.deleted.push(old[i].clone());
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                delta.inserted.push(new[j].clone());
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    delta.deleted.extend_from_slice(&old[i..]);
    delta.inserted.extend_from_slice(&new[j..]);
    delta
}

/// One graph's part of an event: the delta, and the graph's IRI where the subscription
/// names more than one graph (02 §6.1).
#[derive(Debug, Clone, Copy)]
pub struct GraphDelta<'a> {
    /// The graph IRI, without angle brackets; `None` for a single-graph subscription,
    /// whose triples carry no `GRAPH` clause.
    pub graph: Option<&'a str>,
    /// What changed in it.
    pub delta: &'a Delta,
}

/// One event as a SPARQL 1.1 Update request, `DELETE DATA { … } ;` then
/// `INSERT DATA { … }`, over every delta given; a side with nothing in it is left out, and
/// an event that changed nothing is the empty string. N-Triples lines are SPARQL triples
/// as they stand. A line that names a blank node is left out of both sides: `DELETE DATA`
/// may not name one, and the stream's form has none (02 §4.3).
#[must_use]
pub fn update_request(parts: &[GraphDelta<'_>]) -> String {
    let side = |pick: fn(&Delta) -> &Vec<String>| {
        let mut body = String::new();
        for part in parts {
            let lines: Vec<&String> = pick(part.delta)
                .iter()
                .filter(|l| !names_blank_node(l))
                .collect();
            if lines.is_empty() {
                continue;
            }
            match part.graph {
                Some(graph) => {
                    body.push_str("GRAPH <");
                    body.push_str(graph);
                    body.push_str("> {\n");
                    for l in lines {
                        body.push_str(l);
                        body.push('\n');
                    }
                    body.push_str("}\n");
                }
                None => {
                    for l in lines {
                        body.push_str(l);
                        body.push('\n');
                    }
                }
            }
        }
        body
    };
    let deleted = side(|d| &d.deleted);
    let inserted = side(|d| &d.inserted);
    let mut out = String::new();
    if !deleted.is_empty() {
        out.push_str("DELETE DATA {\n");
        out.push_str(&deleted);
        out.push('}');
    }
    if !inserted.is_empty() {
        if !out.is_empty() {
            out.push_str(" ;\n");
        }
        out.push_str("INSERT DATA {\n");
        out.push_str(&inserted);
        out.push('}');
    }
    out
}

/// Whether an N-Triples line has a blank node as its subject or object. A `_:` inside a
/// literal or an IRI does not count.
fn names_blank_node(line: &str) -> bool {
    if line.starts_with("_:") {
        return true;
    }
    let mut in_iri = false;
    let mut in_literal = false;
    let mut escaped = false;
    let mut prev_space = false;
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if in_literal {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_literal = false;
            }
        } else if in_iri {
            if b == b'>' {
                in_iri = false;
            }
        } else if b == b'<' {
            in_iri = true;
        } else if b == b'"' {
            in_literal = true;
        } else if b == b'_' && prev_space && bytes.get(i + 1) == Some(&b':') {
            return true;
        }
        prev_space = b == b' ';
    }
    false
}

#[cfg(test)]
mod tests {
    use oxrdf::{BlankNode, Literal, NamedNode};

    use super::*;

    fn t(s: &str, p: &str, o: &str) -> Triple {
        Triple::new(
            NamedNode::new_unchecked(s),
            NamedNode::new_unchecked(p),
            Literal::new_simple_literal(o),
        )
    }

    // 0032 §9: a rendering is a set, in one order whatever order it was written in.
    #[test]
    fn canonical_lines_sort_and_deduplicate() {
        let a = t("http://e/Q1", "http://p/a", "x");
        let b = t("http://e/Q1", "http://p/b", "y");
        let lines = canonical_lines([&b, &a, &b]);
        assert_eq!(
            lines,
            vec![
                "<http://e/Q1> <http://p/a> \"x\" .".to_string(),
                "<http://e/Q1> <http://p/b> \"y\" .".to_string(),
            ]
        );
    }

    // 0032 §9; 02 §6.2: a change to one statement of an entity is a few triples, not the
    // entity, and applying the delta to the old rendering gives the new one.
    #[test]
    fn diff_is_the_set_difference_both_ways() {
        let keep = t("http://e/Q1", "http://p/a", "kept");
        let old = canonical_lines([&keep, &t("http://e/Q1", "http://p/b", "old")]);
        let new = canonical_lines([&keep, &t("http://e/Q1", "http://p/b", "new")]);
        let delta = diff(&old, &new);
        assert_eq!(delta.deleted, vec!["<http://e/Q1> <http://p/b> \"old\" ."]);
        assert_eq!(delta.inserted, vec!["<http://e/Q1> <http://p/b> \"new\" ."]);

        let mut applied: Vec<String> = old
            .iter()
            .filter(|l| !delta.deleted.contains(l))
            .cloned()
            .chain(delta.inserted.iter().cloned())
            .collect();
        applied.sort_unstable();
        assert_eq!(applied, new);
        assert!(diff(&new, &new).is_empty());
        assert_eq!(diff(&[], &new).inserted, new);
        assert_eq!(diff(&old, &[]).deleted, old);
    }

    // 0032 §9; 02 §6.1, §6.4: one request per event, deletes before inserts, a `GRAPH`
    // clause only where the subscription names more than one graph.
    #[test]
    fn update_request_writes_delete_then_insert() {
        let delta = Delta {
            deleted: vec!["<http://e/Q1> <http://p/b> \"old\" .".into()],
            inserted: vec!["<http://e/Q1> <http://p/b> \"new\" .".into()],
        };
        assert_eq!(
            update_request(&[GraphDelta {
                graph: None,
                delta: &delta
            }]),
            "DELETE DATA {\n<http://e/Q1> <http://p/b> \"old\" .\n} ;\n\
             INSERT DATA {\n<http://e/Q1> <http://p/b> \"new\" .\n}"
        );
        assert_eq!(
            update_request(&[GraphDelta {
                graph: Some("http://x/graph/local"),
                delta: &Delta {
                    deleted: Vec::new(),
                    ..delta.clone()
                },
            }]),
            "INSERT DATA {\nGRAPH <http://x/graph/local> {\n\
             <http://e/Q1> <http://p/b> \"new\" .\n}\n}"
        );
        assert_eq!(update_request(&[]), "");
    }

    // 02 §4.3: `DELETE DATA` cannot name a blank node, so none reaches a request; text
    // that only looks like one inside a literal or an IRI stays.
    #[test]
    fn update_request_never_names_a_blank_node() {
        let blank = Triple::new(
            NamedNode::new_unchecked("http://e/Q1"),
            NamedNode::new_unchecked("http://p/a"),
            BlankNode::new_unchecked("b0"),
        );
        let subject = Triple::new(
            BlankNode::new_unchecked("b1"),
            NamedNode::new_unchecked("http://p/a"),
            Literal::new_simple_literal("x"),
        );
        let literal = t("http://e/Q1", "http://p/a", "see _:b0 here");
        let delta = Delta {
            deleted: Vec::new(),
            inserted: canonical_lines([&blank, &subject, &literal]),
        };
        assert_eq!(
            update_request(&[GraphDelta {
                graph: None,
                delta: &delta
            }]),
            "INSERT DATA {\n<http://e/Q1> <http://p/a> \"see _:b0 here\" .\n}"
        );
    }
}
