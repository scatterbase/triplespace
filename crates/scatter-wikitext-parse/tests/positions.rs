// Fork: the positions the parser reports are consistent (0115 §5). Each node lies
// within the text, on character boundaries, after the node before it and inside its
// parent; a link's trail and a tag's content lie inside the node. Checked over the
// repository's wikitext samples and over random text, and by the fuzz target
// `wikitext_parse`, which carries the same checks.
#![allow(clippy::all, clippy::pedantic)]

mod check;

use check::check;
use scatter_wikitext_parse::{Configuration, ParseError};

fn samples() -> Vec<String> {
	let root = concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../docs/samples/librarybase/parse/pages"
	);
	let mut out = vec![];
	let Ok(dirs) = std::fs::read_dir(root) else {
		return out;
	};
	let mut stack: Vec<std::path::PathBuf> =
		dirs.flatten().map(|d| d.path()).collect();
	while let Some(p) = stack.pop() {
		if p.is_dir() {
			stack.extend(
				std::fs::read_dir(&p).unwrap().flatten().map(|d| d.path()),
			);
		} else if p.extension().is_some_and(|e| e == "wiki") {
			out.push(std::fs::read_to_string(&p).unwrap());
		}
	}
	out
}

#[test]
fn samples_have_consistent_positions() {
	let c = Configuration::default();
	// The samples are in the Triplespace repository, not in the published package.
	let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/samples");
	if !std::path::Path::new(root).is_dir() {
		return;
	}
	let pages = samples();
	assert!(
		!pages.is_empty(),
		"the Librarybase samples are in docs/samples"
	);
	for t in &pages {
		let o = c.parse_without_timeout(t).unwrap();
		if let Err(e) = check(t, &o.nodes) {
			panic!("{e}\nin a page starting {:.80?}", t);
		}
	}
}

#[test]
fn random_text_has_consistent_positions() {
	let alphabet = [
		"{",
		"}",
		"[",
		"]",
		"|",
		"\n",
		"'",
		"<div>",
		"</div>",
		"a",
		" ",
		"=",
		"!",
		"-",
		"*",
		"#",
		":",
		";",
		"<nowiki>",
		"</nowiki>",
		"<!--",
		"-->",
		"{|",
		"|}",
		"|-",
		"|+",
		"http://x ",
		"<ref>",
		"</ref>",
		"<pre>",
		"</pre>",
		"&amp;",
		"é",
		"__TOC__",
		"#REDIRECT",
		"[[File:x|",
		"[[Category:y|",
	];
	let c = Configuration::default();
	let mut seed: u64 = 0x2545F4914F6CDD1D;
	let mut rnd = || {
		seed ^= seed << 13;
		seed ^= seed >> 7;
		seed ^= seed << 17;
		seed
	};
	let mut failures = vec![];
	for _ in 0..20_000 {
		let len = (rnd() as usize) % 24 + 1;
		let s: String = (0..len)
			.map(|_| alphabet[(rnd() as usize) % alphabet.len()])
			.collect();
		let o = c.parse_without_timeout(&s).unwrap();
		if let Err(e) = check(&s, &o.nodes) {
			failures.push(format!("{s:?}: {e}"));
		}
		// A parse stopped at its budget gives consistent output so far.
		if let Err(ParseError::BudgetExhausted { output, .. }) =
			c.parse_with_budget(&s, o.steps / 2)
		{
			if let Err(e) = check(&s, &output.nodes) {
				failures.push(format!("{s:?} stopped at {}: {e}", o.steps / 2));
			}
		}
	}
	assert!(
		failures.is_empty(),
		"{} of 20000 inputs:\n{}",
		failures.len(),
		failures[..failures.len().min(8)].join("\n")
	);
}
