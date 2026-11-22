// Copyright 2019 Fredrik Portström <https://portstrom.com>
// This is free software distributed under the terms specified in
// the file LICENSE at the top-level directory of this distribution.

pub struct OpenNode<'a> {
	pub nodes: Vec<crate::Node<'a>>,
	pub start: usize,
	pub type_: OpenNodeType<'a>,
}

pub enum OpenNodeType<'a> {
	DefinitionList {
		items: Vec<crate::DefinitionListItem<'a>>,
	},
	ExternalLink,
	Heading {
		level: u8,
	},
	Link {
		namespace: Option<crate::Namespace>,
		target: &'a str,
	},
	OrderedList {
		items: Vec<crate::ListItem<'a>>,
	},
	Parameter {
		default: Option<Vec<crate::Node<'a>>>,
		name: Option<Vec<crate::Node<'a>>>,
	},
	Preformatted,
	Table(Table<'a>),
	Tag {
		name: crate::Cow<'a, str>,
		/// Fork: where the content starts, after the start tag.
		content_start: usize,
	},
	Template {
		name: Option<Vec<crate::Node<'a>>>,
		parameters: Vec<crate::Parameter<'a>>,
	},
	UnorderedList {
		items: Vec<crate::ListItem<'a>>,
	},
}

pub struct State<'a> {
	/// Fork: the braces that are calls, matched before parsing.
	pub braces: crate::braces::Braces,
	/// Fork: the openers that ran to the end of the text unclosed, by
	/// position and kind. Reaching one again after a rewind, the parser treats
	/// it as text at once instead of parsing everything after it again, which
	/// made nested unclosed openers (`{{{{{{…`, tables in tables) take time and
	/// memory exponential in their number.
	pub failed_at_end: std::collections::HashSet<(
		usize,
		std::mem::Discriminant<OpenNodeType<'a>>,
	)>,
	pub flushed_position: usize,
	/// Fork: for each extension tag name, the last search for its end tag: where it
	/// started, and where the end tag it found starts, or `None` when there was none,
	/// so that searches forward go on from the last and the parse stays linear.
	pub next_end_tag: std::collections::HashMap<String, (usize, Option<usize>)>,
	/// Fork: where the last `|}` in the text starts; a table opened after it can
	/// never be closed.
	pub last_table_end: Option<usize>,
	pub nodes: Vec<crate::Node<'a>>,
	pub scan_position: usize,
	pub stack: Vec<OpenNode<'a>>,
	pub warnings: Vec<crate::Warning>,
	pub wiki_text: &'a str,
}

pub struct Table<'a> {
	pub attributes: Vec<crate::Node<'a>>,
	pub before: Vec<crate::Node<'a>>,
	pub captions: Vec<crate::TableCaption<'a>>,
	pub child_element_attributes: Option<Vec<crate::Node<'a>>>,
	pub rows: Vec<crate::TableRow<'a>>,
	pub start: usize,
	pub state: TableState,
}

pub enum TableState {
	Before,
	CaptionFirstLine,
	CaptionRemainder,
	CellFirstLine,
	CellRemainder,
	HeadingFirstLine,
	HeadingRemainder,
	Row,
	TableAttributes,
}

impl<'a> State<'a> {
	pub fn flush(&mut self, end_position: usize) {
		flush(
			&mut self.nodes,
			self.flushed_position,
			end_position,
			self.wiki_text,
		);
	}

	pub fn get_byte(&self, position: usize) -> Option<u8> {
		self.wiki_text.as_bytes().get(position).copied()
	}

	pub fn push_open_node(
		&mut self,
		type_: OpenNodeType<'a>,
		inner_start_position: usize,
	) {
		let scan_position = self.scan_position;
		self.flush(scan_position);
		// Fork: an opener known to run unclosed to the end of the text is
		// rewound at once, exactly as opening it, parsing to the end and
		// rewinding would leave it, without parsing everything after it again.
		if self
			.failed_at_end
			.contains(&(scan_position, std::mem::discriminant(&type_)))
		{
			// The warning the rewind at the end gives, once (`parse` drops repeats).
			self.warnings.push(crate::Warning {
				end: self.wiki_text.len(),
				message: crate::WarningMessage::MissingEndTagRewinding,
				start: scan_position,
			});
			let nodes = std::mem::take(&mut self.nodes);
			self.rewind(nodes, scan_position);
			return;
		}
		self.stack.push(OpenNode {
			nodes: std::mem::replace(&mut self.nodes, vec![]),
			start: scan_position,
			type_,
		});
		self.scan_position = inner_start_position;
		self.flushed_position = inner_start_position;
	}

	/// Fork: where the first end tag of `name` at or after `from` starts.
	pub fn end_tag_after(&mut self, name: &str, from: usize) -> Option<usize> {
		if let Some(&(searched, found)) = self.next_end_tag.get(name) {
			if searched <= from {
				match found {
					None => return None,
					Some(at) if at >= from => return Some(at),
					Some(_) => {}
				}
			}
		}
		let found = find_end_tag(self.wiki_text, from, name);
		self.next_end_tag.insert(name.to_string(), (from, found));
		found
	}

	/// Fork: whether an extension tag of `name` whose start tag ends at `from` has
	/// an end tag, inside the extension tag that holds it if one does, as MediaWiki
	/// reads extension tags: their content runs to the first end tag of their name.
	pub fn has_end_tag(&mut self, name: &str, from: usize) -> bool {
		let Some(end) = self.end_tag_after(name, from) else {
			return false;
		};
		let holder = self.stack.iter().rev().find_map(|n| match &n.type_ {
			OpenNodeType::Tag {
				name,
				content_start,
			} => Some((name.to_string(), *content_start)),
			_ => None,
		});
		match holder {
			Some((outer, content_start)) => self
				.end_tag_after(&outer, content_start)
				.is_some_and(|outer_end| end < outer_end),
			None => true,
		}
	}

	/// Fork: marks an opener of this kind at `position` as one that cannot close, so
	/// that opening it rewinds it at once, as opening it and parsing to the end would.
	pub fn cannot_close(&mut self, position: usize, type_: &OpenNodeType<'a>) {
		self.failed_at_end
			.insert((position, std::mem::discriminant(type_)));
	}

	pub fn rewind(&mut self, nodes: Vec<crate::Node<'a>>, position: usize) {
		self.scan_position = position + 1;
		self.nodes = nodes;

		let last_text_node_start = match self.nodes.last() {
			Some(crate::Node::Text { start, .. }) => Some(*start),
			_ => None,
		};
		if let Some(position_before_text) = last_text_node_start {
			self.nodes.pop();
			self.flushed_position = position_before_text;
		} else {
			self.flushed_position = position;
		}
	}

	pub fn skip_empty_lines(&mut self) {
		match self.stack.last() {
			Some(OpenNode {
				type_: OpenNodeType::Table { .. },
				..
			}) => {
				self.scan_position -= 1;
				crate::table::parse_table_end_of_line(self, false);
			}
			_ => {
				crate::line::parse_beginning_of_line(self, None);
			}
		}
	}

	pub fn skip_whitespace_backwards(&self, position: usize) -> usize {
		skip_whitespace_backwards(self.wiki_text, position)
	}

	pub fn skip_whitespace_forwards(&self, position: usize) -> usize {
		skip_whitespace_forwards(self.wiki_text, position)
	}
}

pub fn flush<'a>(
	nodes: &mut Vec<crate::Node<'a>>,
	flushed_position: usize,
	end_position: usize,
	wiki_text: &'a str,
) {
	if end_position > flushed_position {
		nodes.push(crate::Node::Text {
			end: end_position,
			start: flushed_position,
			value: &wiki_text[flushed_position..end_position],
		});
	}
}

pub fn skip_whitespace_backwards(wiki_text: &str, position: usize) -> usize {
	let slice = wiki_text.as_bytes().get(..position).unwrap_or(&[]);

	let non_whitespace_position = slice
		.iter()
		.rev()
		.position(|b| !matches!(b, b'\t' | b'\n' | b' '))
		.unwrap_or(slice.len());

	position - non_whitespace_position
}

pub fn skip_whitespace_forwards(wiki_text: &str, position: usize) -> usize {
	let slice = wiki_text.as_bytes().get(position..).unwrap_or(&[]);

	let non_whitespace_position = slice
		.iter()
		.position(|b| !matches!(b, b'\t' | b'\n' | b' '))
		.unwrap_or(slice.len());

	position + non_whitespace_position
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_skip_whitespace_backwards() {
		assert_eq!(skip_whitespace_backwards("0\n2", 2), 1);
		assert_eq!(skip_whitespace_backwards("\n\n", 2), 0);
		assert_eq!(skip_whitespace_backwards("\n\n", 1), 0);
		assert_eq!(skip_whitespace_backwards("1\n\n", 2), 1);
		assert_eq!(skip_whitespace_backwards("\n1\n", 2), 2);
	}

	#[test]
	fn test_skip_whitespace_forwards() {
		assert_eq!(skip_whitespace_forwards("\n\n", 0), 2);
		assert_eq!(skip_whitespace_forwards("1\n\n", 0), 0);
		assert_eq!(skip_whitespace_forwards("1 \n1", 1), 3);
		assert_eq!(skip_whitespace_forwards("1 \n1", 6), 6);
		assert_eq!(skip_whitespace_forwards("1 \n1", 0), 0);
	}
}

/// Fork: where the next end tag of `name` (`</name>`, `</name >`, any case) at or after
/// `from` starts.
fn find_end_tag(wiki_text: &str, from: usize, name: &str) -> Option<usize> {
	let bytes = wiki_text.as_bytes();
	let mut position = from;
	while let Some(offset) = wiki_text.get(position..)?.find("</") {
		let at = position + offset;
		let name_end = at + 2 + name.len();
		if bytes
			.get(at + 2..name_end)
			.is_some_and(|n| n.eq_ignore_ascii_case(name.as_bytes()))
		{
			let rest = skip_whitespace_forwards(wiki_text, name_end);
			if bytes.get(rest) == Some(&b'>') {
				return Some(at);
			}
		}
		position = at + 2;
	}
	None
}
