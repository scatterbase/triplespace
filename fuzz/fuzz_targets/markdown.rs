//! The markdown path over arbitrary text (22 §4.9; 0033 §9.2, §15): comrak with the
//! extensions `scatter-pages` turns on, wiki links resolved by the caller, and the HTML
//! sanitizer `scatter-wikitext` owns, as the thread and talk renderers call it. Any UTF-8
//! input must render without a panic. Nothing else is asserted.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = scatter_pages::markdown::render(
        text,
        &|target| format!("/wiki/{target}"),
        &scatter_wikitext::sanitize,
    );
});
