//! The trailing signature an editor types out of habit (14 §1.6; 0019 A28).
//!
//! A post is attributed by its attestation, so a signature adds nothing. When an original
//! `create`, `post` or `edit` is written, a signature at the very end of its text, `~~~`,
//! `~~~~` or `~~~~~` with any whitespace and one `--`, `—` or `–` before it, is removed.
//! Tildes anywhere else are kept, and an import's text is never touched.

/// Removes a trailing signature, and the whitespace either side of it.
///
/// Returns the text unchanged, less trailing whitespace, when it does not end in a run of
/// three to five tildes. A run of six or more is not a signature.
#[must_use]
pub fn strip_trailing_signature(text: &str) -> &str {
    let body = text.trim_end();
    let tildes = body.len() - body.trim_end_matches('~').len();
    if !(3..=5).contains(&tildes) {
        return body;
    }
    let mut rest = body[..body.len() - tildes].trim_end();
    for dash in ["--", "\u{2014}", "\u{2013}"] {
        if let Some(r) = rest.strip_suffix(dash) {
            rest = r.trim_end();
            break;
        }
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::strip_trailing_signature as strip;

    #[test]
    fn removes_the_common_forms() {
        assert_eq!(strip("Agreed. ~~~~"), "Agreed.");
        assert_eq!(strip("Agreed.~~~~"), "Agreed.");
        assert_eq!(strip("Agreed. --~~~~"), "Agreed.");
        assert_eq!(strip("Agreed. -- ~~~~\n\n"), "Agreed.");
        assert_eq!(strip("Agreed. \u{2014}~~~~"), "Agreed.");
        assert_eq!(strip("Agreed. \u{2013} ~~~"), "Agreed.");
        assert_eq!(strip("Agreed.\n~~~~~"), "Agreed.");
    }

    #[test]
    fn keeps_what_is_not_a_trailing_signature() {
        assert_eq!(strip("No signature here.  "), "No signature here.");
        assert_eq!(strip("~~~~ then words"), "~~~~ then words");
        assert_eq!(strip("a ~~ b ~~"), "a ~~ b ~~");
        assert_eq!(strip("rule ~~~~~~"), "rule ~~~~~~");
        // Only one dash is the signature's; a second belongs to the text.
        assert_eq!(strip("range 1--2 --~~~~"), "range 1--2");
        assert_eq!(strip("x ---~~~~"), "x -");
    }

    #[test]
    fn a_post_that_is_only_a_signature_is_empty() {
        assert_eq!(strip("~~~~"), "");
        assert_eq!(strip("  -- ~~~~  "), "");
    }
}
