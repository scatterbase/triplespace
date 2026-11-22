//! Fork: bold and italic as MediaWiki makes them from runs of apostrophes.
//!
//! The parser reports each run of apostrophes as a toggle (`Node::Italic`, `Node::Bold`,
//! `Node::BoldItalic`) where it meets it. What the runs of a line make depends on the
//! whole line, so this is a pass over a line's runs, following MediaWiki's rules
//! (`Parser::doQuotes`):
//!
//! - Two apostrophes are italic, three bold, five both. Four are one literal apostrophe
//!   and bold; more than five are literals and five. The parser has already counted a
//!   run's toggle; the apostrophes before it are literals.
//! - When the line has an odd number of italic runs and an odd number of bold runs, one
//!   bold run becomes a literal apostrophe and italic: the first after a single-letter
//!   word, else the first after a longer word, else the first after a space.
//! - Five apostrophes that open both open them in the order the next run implies.
//! - Whatever is still open closes at the end of the line.

use crate::Node;

/// What a run of apostrophes was read as by the parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
	/// `''`
	Italic,
	/// `'''`
	Bold,
	/// `'''''`
	BoldItalic,
}

/// An element a run opens or closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quote {
	/// Opens bold (`<b>`).
	OpenBold,
	/// Opens italic (`<i>`).
	OpenItalic,
	/// Closes bold.
	CloseBold,
	/// Closes italic.
	CloseItalic,
}

/// What one run of apostrophes does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunPlan {
	/// Where the run's node starts.
	pub start: usize,
	/// How many apostrophes at its start are written as literal text, before the events.
	pub literal: usize,
	/// The elements it opens and closes, in order.
	pub events: Vec<Quote>,
}

/// What the runs of a line do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuotePlan {
	/// Each run, in the order of the line.
	pub runs: Vec<RunPlan>,
	/// What is still open at the end of the line, closed in this order.
	pub end: Vec<Quote>,
}

impl QuotePlan {
	/// The plan for the run whose node starts at `start`.
	#[must_use]
	pub fn at(&self, start: usize) -> Option<&RunPlan> {
		self.runs.iter().find(|r| r.start == start)
	}
}

/// The run a node is, if it is one.
#[must_use]
pub fn run_of(node: &Node) -> Option<(usize, Run)> {
	match node {
		Node::Italic { start, .. } => Some((*start, Run::Italic)),
		Node::Bold { start, .. } => Some((*start, Run::Bold)),
		Node::BoldItalic { start, .. } => Some((*start, Run::BoldItalic)),
		_ => None,
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
	None,
	B,
	I,
	/// `<b><i>`: bold opened first.
	Bi,
	/// `<i><b>`: italic opened first.
	Ib,
}

/// Plans the runs of one line of `text`. `runs` are the line's apostrophe nodes, by
/// where they start and what the parser read them as, in any order; a run's length is
/// read from `text`.
#[must_use]
pub fn plan_line(text: &str, runs: &[(usize, Run)]) -> QuotePlan {
	let run_length = |start: usize| {
		text.get(start..)
			.map_or(0, |t| t.bytes().take_while(|&b| b == b'\'').count())
	};
	let mut tokens: Vec<(usize, Run, usize)> = runs
		.iter()
		.map(|&(start, run)| {
			let counted = match run {
				Run::Italic => 2,
				Run::Bold => 3,
				Run::BoldItalic => 5,
			};
			// The parser's toggle is the run's last apostrophes; those before are literal.
			(start, run, run_length(start).saturating_sub(counted))
		})
		.collect();
	tokens.sort_by_key(|t| t.0);
	let italics = tokens
		.iter()
		.filter(|t| matches!(t.1, Run::Italic | Run::BoldItalic))
		.count();
	let bolds = tokens
		.iter()
		.filter(|t| matches!(t.1, Run::Bold | Run::BoldItalic))
		.count();
	if italics % 2 == 1 && bolds % 2 == 1 {
		let mut single = None;
		let mut multi = None;
		let mut space = None;
		for (k, &(start, run, literal)) in tokens.iter().enumerate() {
			if run != Run::Bold {
				continue;
			}
			let at = start + literal;
			let mut before = text[..at].chars().rev();
			let x1 = before.next();
			let x2 = before.next();
			if x1 == Some(' ') {
				space.get_or_insert(k);
			} else if x2 == Some(' ') {
				single = Some(k);
				break;
			} else {
				multi.get_or_insert(k);
			}
		}
		if let Some(k) = single.or(multi).or(space) {
			tokens[k].1 = Run::Italic;
			tokens[k].2 += 1;
		}
	}
	let mut plan = QuotePlan::default();
	let mut state = State::None;
	for k in 0..tokens.len() {
		let (start, run, literal) = tokens[k];
		let next = tokens.get(k + 1).map(|t| t.1);
		let (events, next_state) = transition(state, run, next);
		state = next_state;
		plan.runs.push(RunPlan {
			start,
			literal,
			events,
		});
	}
	plan.end = match state {
		State::None => vec![],
		State::B => vec![Quote::CloseBold],
		State::I => vec![Quote::CloseItalic],
		State::Bi => vec![Quote::CloseItalic, Quote::CloseBold],
		State::Ib => vec![Quote::CloseBold, Quote::CloseItalic],
	};
	plan
}

fn transition(
	state: State,
	run: Run,
	next: Option<Run>,
) -> (Vec<Quote>, State) {
	use Quote::{
		CloseBold as CB, CloseItalic as CI, OpenBold as OB, OpenItalic as OI,
	};
	match (run, state) {
		(Run::Italic, State::I) => (vec![CI], State::None),
		(Run::Italic, State::Bi) => (vec![CI], State::B),
		(Run::Italic, State::Ib) => (vec![CB, CI, OB], State::B),
		(Run::Italic, State::None) => (vec![OI], State::I),
		(Run::Italic, State::B) => (vec![OI], State::Bi),
		(Run::Bold, State::B) => (vec![CB], State::None),
		(Run::Bold, State::Bi) => (vec![CI, CB, OI], State::I),
		(Run::Bold, State::Ib) => (vec![CB], State::I),
		(Run::Bold, State::None) => (vec![OB], State::B),
		(Run::Bold, State::I) => (vec![OB], State::Ib),
		(Run::BoldItalic, State::B) => (vec![CB, OI], State::I),
		(Run::BoldItalic, State::I) => (vec![CI, OB], State::B),
		(Run::BoldItalic, State::Bi) => (vec![CI, CB], State::None),
		(Run::BoldItalic, State::Ib) => (vec![CB, CI], State::None),
		(Run::BoldItalic, State::None) => match next {
			Some(Run::Bold | Run::BoldItalic) => (vec![OI, OB], State::Ib),
			Some(Run::Italic) | None => (vec![OB, OI], State::Bi),
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use Quote::*;

	fn plan(text: &str) -> QuotePlan {
		let o = crate::Configuration::default()
			.parse_without_timeout(text)
			.unwrap();
		let runs: Vec<_> = o.nodes.iter().filter_map(run_of).collect();
		plan_line(text, &runs)
	}

	#[test]
	fn mediawiki_rules() {
		let p = plan("''a'' '''b'''");
		assert_eq!(p.runs[0].events, [OpenItalic]);
		assert_eq!(p.runs[1].events, [CloseItalic]);
		assert_eq!(p.runs[2].events, [OpenBold]);
		assert_eq!(p.runs[3].events, [CloseBold]);
		assert!(p.end.is_empty());
		// What is open at the end of the line closes there.
		assert_eq!(plan("'''a").end, [CloseBold]);
		// One bold and one italic: the bold is read as a literal and italic.
		let p = plan("'''a ''b");
		assert_eq!(
			(p.runs[0].literal, &p.runs[0].events[..]),
			(1, &[OpenItalic][..])
		);
		assert!(p.end.is_empty());
		// Four apostrophes: one literal, then bold.
		let p = plan("''''a'''");
		assert_eq!(
			(p.runs[0].literal, &p.runs[0].events[..]),
			(1, &[OpenBold][..])
		);
		// Odd italics and odd bolds: the bold after a single-letter word is a literal and italic.
		let p = plan("''a l'''s b");
		assert_eq!(p.runs[1].literal, 1);
		assert_eq!(p.runs[1].events, [CloseItalic]);
		// Five open in the order the next run implies.
		let p = plan("'''''a''' b''");
		assert_eq!(p.runs[0].events, [OpenItalic, OpenBold]);
	}
}
