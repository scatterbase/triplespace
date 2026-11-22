// Fork: the checks on the positions the parser reports (0115 §5), shared by
// `tests/positions.rs` and the fuzz target `wikitext_parse` (fuzz/), which
// includes this file by path. Each node lies within the text, on character
// boundaries, after the node before it and inside its parent; a link's trail and
// a tag's content lie inside the node. Text inside a table before its first row
// is moved ahead of the table, as MediaWiki moves it, and lies inside the table.

use scatter_wikitext_parse::{Node, Positioned};

/// Checks every node's position, returning the first inconsistency.
pub fn check(text: &str, nodes: &[Node]) -> Result<(), String> {
	walk(text, nodes, 0, text.len())
}

fn span(
	text: &str,
	start: usize,
	end: usize,
	lo: usize,
	hi: usize,
	what: &str,
) -> Result<(), String> {
	if !(lo <= start && start <= end && end <= hi) {
		return Err(format!("{what} {start}..{end} outside {lo}..{hi}"));
	}
	if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
		return Err(format!(
			"{what} {start}..{end} not on a character boundary"
		));
	}
	Ok(())
}

fn walk(
	text: &str,
	nodes: &[Node],
	lo: usize,
	hi: usize,
) -> Result<(), String> {
	let mut after = lo;
	for (i, n) in nodes.iter().enumerate() {
		let (s, e) = (n.start(), n.end());
		// Text inside a table before its first row is moved out ahead of the table,
		// as MediaWiki moves it: it lies inside the table's span, out of order.
		let fostered = nodes[i + 1..]
			.iter()
			.find(|m| m.start() <= s || matches!(m, Node::Table { .. }));
		if let Some(t @ Node::Table { .. }) = fostered {
			if t.start() <= s && e <= t.end() {
				span(
					text,
					s,
					e,
					t.start(),
					t.end(),
					"text moved out of a table",
				)?;
				continue;
			}
		}
		span(text, s, e, after, hi, &format!("{:.40?}", n))?;
		after = e;
		match n {
			Node::Link {
				text: t,
				trail_start,
				..
			} => {
				span(text, *trail_start, *trail_start, s, e, "trail start")?;
				walk(text, t, s, e)?;
			}
			Node::Tag {
				nodes: c,
				content_start,
				content_end,
				..
			} => {
				span(text, *content_start, *content_end, s, e, "tag content")?;
				walk(text, c, *content_start, *content_end)?;
			}
			Node::Category { ordinal: c, .. }
			| Node::ExternalLink { nodes: c, .. }
			| Node::Heading { nodes: c, .. }
			| Node::Image { text: c, .. }
			| Node::Preformatted { nodes: c, .. } => walk(text, c, s, e)?,
			Node::Parameter { name, default, .. } => {
				walk(text, name, s, e)?;
				if let Some(d) = default {
					walk(text, d, s, e)?;
				}
			}
			Node::Template {
				name, parameters, ..
			} => {
				walk(text, name, s, e)?;
				for p in parameters {
					span(text, p.start, p.end, s, e, "parameter")?;
					if let Some(n) = &p.name {
						walk(text, n, p.start, p.end)?;
					}
					walk(text, &p.value, p.start, p.end)?;
				}
			}
			Node::OrderedList { items, .. }
			| Node::UnorderedList { items, .. } => {
				for i in items {
					span(text, i.start, i.end, s, e, "list item")?;
					walk(text, &i.nodes, i.start, i.end)?;
				}
			}
			Node::DefinitionList { items, .. } => {
				for i in items {
					span(text, i.start, i.end, s, e, "definition item")?;
					walk(text, &i.nodes, i.start, i.end)?;
				}
			}
			Node::Table {
				attributes,
				captions,
				rows,
				..
			} => {
				walk(text, attributes, s, e)?;
				for c in captions {
					span(text, c.start, c.end, s, e, "caption")?;
					walk(text, &c.content, c.start, c.end)?;
				}
				for r in rows {
					span(text, r.start, r.end, s, e, "row")?;
					for c in &r.cells {
						span(text, c.start, c.end, s, e, "cell")?;
						walk(text, &c.content, c.start, c.end)?;
					}
				}
			}
			_ => {}
		}
	}
	Ok(())
}
