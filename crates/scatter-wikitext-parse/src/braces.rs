// Fork: which braces are calls, matched as MediaWiki's preprocessor matches them.
//
// The preprocessor reads `{{`, `{{{` and `[[` as frames on a stack before any other
// syntax: a run of closing braces closes the innermost frame only when that frame is
// one of braces, taking three braces for an argument and two for a template; a `[[`
// still open stops a `}}` from closing the template around it; and braces left
// unmatched at the end are text, without the text after them being read again.
// Comments are not read. An extension tag's content is the extension's: braces cannot
// match across its edges, and inside a tag whose content is wikitext they match on
// their own. A start tag with no end tag is text.
//
// One difference from the preprocessor viewing a page: the inclusion tags
// (`noinclude`, `onlyinclude`, `includeonly`) are extension tags here, as they are
// to the parser, which keeps them as nodes for a reader of the source; the
// preprocessor removes the first two and drops what the third holds, so braces can
// match across them there. A text that has been expanded holds none of them.
//
// The parser opens a template or argument only where this pass found an opener, and
// closes one only at the closer that matches it, so the two agree by construction and
// unmatched braces cost no rewinding.

use std::collections::{HashMap, HashSet};

/// Whether a matched opener starts a template or an argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	/// `{{…}}`
	Template,
	/// `{{{…}}}`
	Argument,
}

/// The matched braces of a text, by byte position.
#[derive(Default)]
pub struct Braces {
	/// The position of each matched opener's first brace: its kind, and where its
	/// closer starts.
	pub opens: HashMap<usize, (Kind, usize)>,
	/// The position of each matched closer's first brace, and its opener's.
	pub closes: HashMap<usize, usize>,
	/// Where runs of two or more braces that open nothing start: left open at the end,
	/// so the parser warns of a missing end as it did when it rewound them.
	pub unmatched_opens: HashSet<usize>,
	/// Where runs of two or more closing braces that close nothing start.
	pub unmatched_closes: HashSet<usize>,
}

/// Extension tags whose content is not wikitext: braces inside them are never calls.
const PLAIN: &[&str] = &[
	"nowiki",
	"pre",
	"includeonly",
	"math",
	"chem",
	"ce",
	"syntaxhighlight",
	"source",
	"score",
	"timeline",
	"hiero",
	"graph",
	"templatedata",
	"templatestyles",
	"mapframe",
	"maplink",
	"imagemap",
];

struct Frame {
	brace: u8,
	count: usize,
	start: usize,
}

/// Matches the braces of `text`. `is_extension_tag` says which lower-cased tag names
/// are extension tags.
pub fn match_braces(
	text: &str,
	is_extension_tag: &dyn Fn(&str) -> bool,
) -> Braces {
	let mut braces = Braces::default();
	let mut tags = Tags::default();
	scan(
		text,
		0,
		text.len(),
		is_extension_tag,
		&mut tags,
		&mut braces,
	);
	braces
}

/// What is known of extension tags' end tags, so that no search is repeated.
#[derive(Default)]
struct Tags {
	/// Names with no end tag after the last place one was found unclosed.
	unclosed: HashSet<String>,
	/// For each name, the last search: where it started, and what it found.
	found: HashMap<String, (usize, Option<(usize, usize)>)>,
}

impl Tags {
	fn end_tag(
		&mut self,
		text: &str,
		from: usize,
		name: &str,
	) -> Option<(usize, usize)> {
		if let Some(&(searched, found)) = self.found.get(name) {
			if searched <= from && found.map_or(true, |(at, _)| at >= from) {
				return found;
			}
		}
		let found = end_tag(text, from, name);
		self.found.insert(name.to_string(), (from, found));
		found
	}
}

/// A tag at `at`: its lower-cased name, whether it is a closing tag, whether it is
/// self-closing, and where it ends (after `>`).
fn tag_at(text: &str, at: usize) -> Option<(String, bool, bool, usize)> {
	let b = text.as_bytes();
	let mut i = at + 1;
	let closing = b.get(i) == Some(&b'/');
	if closing {
		i += 1;
	}
	let name_start = i;
	while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
		i += 1;
	}
	if i == name_start {
		return None;
	}
	if !matches!(b.get(i), Some(b'>' | b'/' | b' ' | b'\t' | b'\n')) {
		return None;
	}
	let gt = i + text[i..].find('>')?;
	let self_closing = gt > 0 && b[gt - 1] == b'/';
	Some((
		text[name_start..i].to_ascii_lowercase(),
		closing,
		self_closing,
		gt + 1,
	))
}

/// Where the end tag of `name` after `from` starts and ends: `</name>`, with
/// whitespace allowed before the `>` and nothing else, in any case.
fn end_tag(text: &str, from: usize, name: &str) -> Option<(usize, usize)> {
	let b = text.as_bytes();
	let mut position = from;
	while let Some(offset) = text.get(position..)?.find("</") {
		let at = position + offset;
		let name_end = at + 2 + name.len();
		if b.get(at + 2..name_end)
			.is_some_and(|n| n.eq_ignore_ascii_case(name.as_bytes()))
		{
			let mut i = name_end;
			while matches!(b.get(i), Some(b' ' | b'\t' | b'\n')) {
				i += 1;
			}
			if b.get(i) == Some(&b'>') {
				return Some((at, i + 1));
			}
		}
		position = at + 2;
	}
	None
}

fn scan(
	text: &str,
	from: usize,
	to: usize,
	is_extension_tag: &dyn Fn(&str) -> bool,
	tags: &mut Tags,
	braces: &mut Braces,
) {
	let b = &text.as_bytes()[..to];
	let mut stack: Vec<Frame> = Vec::new();
	let mut i = from;
	while i < to {
		match b[i] {
			b'<' if text[i..to].starts_with("<!--") => {
				i = text[i + 4..to].find("-->").map_or(to, |e| i + 4 + e + 3);
			}
			b'<' => {
				let Some((name, closing, self_closing, end)) = tag_at(text, i)
				else {
					i += 1;
					continue;
				};
				if end > to || closing {
					i += 1;
					continue;
				}
				match name.as_str() {
					n if is_extension_tag(n) => {
						if self_closing || tags.unclosed.contains(n) {
							i = end;
						} else {
							match tags.end_tag(text, end, n) {
								Some((close, close_end)) if close_end <= to => {
									if !PLAIN.contains(&n) {
										scan(
											text,
											end,
											close,
											is_extension_tag,
											tags,
											braces,
										);
									}
									i = close_end;
								}
								// Its end tag is past the tag that holds it: the
								// start tag is text here.
								Some(_) => i = end,
								// No end tag at all: the start tag is text, and a
								// name found so is unclosed from then on.
								None => {
									tags.unclosed.insert(n.to_string());
									i = end;
								}
							}
						}
					}
					_ => i += 1,
				}
			}
			c @ (b'{' | b'[') if b.get(i + 1) == Some(&c) => {
				let n = b[i..].iter().take_while(|&&x| x == c).count();
				stack.push(Frame {
					brace: c,
					count: n,
					start: i,
				});
				i += n;
			}
			b']' if b.get(i + 1) == Some(&b']')
				&& stack.last().is_some_and(|f| f.brace == b'[') =>
			{
				stack.pop();
				i += 2;
			}
			b'}' if stack.last().is_some_and(|f| f.brace == b'{') => {
				let mut m = b[i..].iter().take_while(|&&x| x == b'}').count();
				let mut pos = i;
				while m >= 2 {
					let Some(f) = stack.last_mut().filter(|f| f.brace == b'{')
					else {
						break;
					};
					let matching = f.count.min(m).min(3);
					if matching < 2 {
						break;
					}
					f.count -= matching;
					let open = f.start + f.count;
					let kind = if matching == 3 {
						Kind::Argument
					} else {
						Kind::Template
					};
					braces.opens.insert(open, (kind, pos));
					braces.closes.insert(pos, open);
					if f.count < 2 {
						stack.pop();
					}
					m -= matching;
					pos += matching;
				}
				if m >= 2 {
					braces.unmatched_closes.insert(pos);
					pos += m;
				}
				i = if pos == i { i + 1 } else { pos };
			}
			b'}' if b.get(i + 1) == Some(&b'}') => {
				braces.unmatched_closes.insert(i);
				i += b[i..].iter().take_while(|&&x| x == b'}').count();
			}
			_ => i += 1,
		}
	}
	for f in stack {
		if f.brace == b'{' && f.count >= 2 {
			braces.unmatched_opens.insert(f.start);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn calls(text: &str) -> Vec<(usize, usize, Kind)> {
		let ext = |n: &str| matches!(n, "nowiki" | "ref" | "pre");
		let b = match_braces(text, &ext);
		let mut v: Vec<_> = b
			.opens
			.iter()
			.map(|(&o, &(k, c))| {
				let len = if k == Kind::Argument { 3 } else { 2 };
				(o, c + len, k)
			})
			.collect();
		v.sort_by_key(|x| x.0);
		v
	}

	#[test]
	fn as_the_preprocessor_reads_them() {
		use Kind::*;
		assert_eq!(calls("{{a}}"), [(0, 5, Template)]);
		assert_eq!(calls("{{{a}}}"), [(0, 7, Argument)]);
		// The innermost three braces close first.
		assert_eq!(calls("{{{{{a}}}"), [(2, 9, Argument)]);
		assert_eq!(calls("{{{{{a}}}}}"), [(0, 11, Template), (2, 9, Argument)]);
		// A `[[` still open stops `}}` from closing.
		assert_eq!(calls("{{a[[b}}c"), []);
		assert_eq!(calls("{{a[[b]]}}"), [(0, 10, Template)]);
		// Unmatched braces are text; a lone `}` too.
		assert_eq!(calls("{{}"), []);
		assert_eq!(calls("{{a}b}}"), [(0, 7, Template)]);
		// Comments and plain extension tags are not read; a tag's edges are walls.
		assert_eq!(calls("{{a<!--}}-->}}"), [(0, 14, Template)]);
		assert_eq!(calls("{{a<nowiki>}}</nowiki>}}"), [(0, 24, Template)]);
		assert_eq!(calls("<ref>{{a}}</ref>"), [(5, 10, Template)]);
		assert_eq!(calls("{{a<ref>}}</ref>"), []);
		// A start tag with no end tag is text.
		assert_eq!(calls("{{a<nowiki>}}"), [(0, 13, Template)]);
	}
}
