// Fork: upstream's tests keep upstream's style (see CHANGELOG.md).
#![allow(clippy::all, clippy::pedantic)]

use std::time::Duration;

use scatter_wikitext_parse::{Configuration, ParseError};

const MAX_EXEC: Duration = Duration::from_millis(200);

/// Upstream's issue 1: nested unclosed openers took time and memory exponential
/// in their number, so 40 `{` timed out. Fork: an opener found unclosed at the
/// end is not parsed again, so this finishes, as text, in quadratic steps.
#[test]
fn issue_1() {
	let s = "{".repeat(40);
	let output = Configuration::default()
		.parse_with_timeout(&s, MAX_EXEC)
		.expect("finishes");
	assert!(matches!(
		output.nodes.as_slice(),
		[scatter_wikitext_parse::Node::Text { value, .. }] if *value == s
	));
	assert!(output.steps < 4 * 40 * 40, "{} steps", output.steps);

	// 24 `{{` used to need gigabytes; 1000 now take at most about n² steps.
	let s = "{{".repeat(1_000);
	let output = Configuration::default().parse_without_timeout(&s).unwrap();
	assert!(output.steps < 3 * 1_000 * 1_000, "{} steps", output.steps);
}

/// Fork: a step budget stops a parse at the same step every time, with the
/// output so far, and reads no clock.
#[test]
fn step_budget() {
	let c = Configuration::default();
	let page = "== A ==\n[[Link]] and {{template|x}}\n* item\n";
	let full = c.parse_without_timeout(page).unwrap();
	assert!(full.steps > 0);
	// A budget of exactly the steps it takes is enough.
	assert!(c.parse_with_budget(page, full.steps).is_ok());

	let s = "{{".repeat(500);
	let stop = |budget| match c.parse_with_budget(&s, budget) {
		Err(ParseError::BudgetExhausted { steps, output }) => {
			assert_eq!(steps, budget);
			format!("{:?}", output.nodes)
		}
		other => panic!("expected the budget to stop it: {:?}", other.is_ok()),
	};
	assert_eq!(
		stop(10_000),
		stop(10_000),
		"the same budget stops at the same place"
	);
}

/// Fork: a link says where its trail starts, an extension tag where its
/// content starts and ends, and a table cell that goes on past its first line
/// keeps its start.
#[test]
fn positions() {
	use scatter_wikitext_parse::Node;
	let c = Configuration::default();

	let s = "[[a]]bc [[d|e]]f [[g]]";
	let o = c.parse_without_timeout(s).unwrap();
	let trails: Vec<_> = o
		.nodes
		.iter()
		.filter_map(|n| match n {
			Node::Link {
				trail_start, end, ..
			} => Some(&s[*trail_start..*end]),
			_ => None,
		})
		.collect();
	assert_eq!(trails, ["bc", "f", ""]);

	let s = "<pre title=\"x\">a\nb</pre><nowiki>c</nowiki><ref name=y/>";
	let o = c.parse_without_timeout(s).unwrap();
	let contents: Vec<_> = o
		.nodes
		.iter()
		.filter_map(|n| match n {
			Node::Tag {
				content_start,
				content_end,
				..
			} => Some(&s[*content_start..*content_end]),
			_ => None,
		})
		.collect();
	assert_eq!(contents, ["a\nb", "c", ""]);

	let s = "{|\n|a||b\nmore\n!c\nd\n|}";
	let o = c.parse_without_timeout(s).unwrap();
	let Node::Table { rows, .. } = &o.nodes[0] else {
		panic!("a table")
	};
	let cells: Vec<_> = rows
		.iter()
		.flat_map(|r| &r.cells)
		.map(|c| &s[c.start..c.end])
		.collect();
	assert_eq!(cells, ["|a", "||b\nmore", "!c\nd"]);
}
