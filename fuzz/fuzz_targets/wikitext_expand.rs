//! The preprocessor over arbitrary text (22 §4.9; 0042 §4, §18): expansion, the call
//! spans and the census must terminate without a panic, and every call span must lie in
//! the text, on character boundaries, in order. `{{}` once looped for ever.

#![no_main]

use libfuzzer_sys::fuzz_target;
use scatter_wikitext_expand::{ExpandHost, Transcluded};

/// Every template has a short text of its own that calls another, so transclusion,
/// recursion and loop detection run; a name starting with `M` is missing.
struct Host;

impl ExpandHost for Host {
    fn transclude(&self, name: &str) -> Option<Transcluded> {
        let name = name.trim();
        if name.is_empty() {
            None
        } else if name.starts_with('M') {
            Some(Transcluded::Missing(format!("Template:{name}")))
        } else {
            Some(Transcluded::Found(format!(
                "<noinclude>doc</noinclude>[{{{{{{1|x}}}}}}]{{{{{name}x}}}}"
            )))
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = scatter_wikitext_expand::expand_for_view(text, &Host);
    let _ = scatter_wikitext_expand::census::scan(text);
    let mut after = 0;
    for span in scatter_wikitext_expand::call_spans(text) {
        assert!(after <= span.start && span.start < span.end && span.end <= text.len());
        assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
        after = span.end;
    }
});
