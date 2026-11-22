//! The wikitext parser alone over arbitrary text (22 §4.9; 0115 §5): any UTF-8 input must
//! parse without a panic inside the step budget `scatter-wikitext` gives it, and every
//! position the parser reports must be consistent, by the checks the crate's own tests use.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use scatter_wikitext_parse::{Configuration, ParseError};

#[path = "../../crates/scatter-wikitext-parse/tests/check/mod.rs"]
mod check;

fn config() -> &'static Configuration {
    static CONFIG: OnceLock<Configuration> = OnceLock::new();
    CONFIG.get_or_init(Configuration::default)
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The budget `scatter-wikitext` gives a text (0033 §9.1, A36).
    let budget = (text.len() as u64).saturating_mul(64).max(1_000_000);
    let output = match config().parse_with_budget(text, budget) {
        Ok(output) => output,
        // Stopped at the budget: the output so far must still be consistent.
        Err(ParseError::BudgetExhausted { output, .. }) => output,
        Err(_) => return,
    };
    if let Err(e) = check::check(text, &output.nodes) {
        panic!("{e}");
    }
});
