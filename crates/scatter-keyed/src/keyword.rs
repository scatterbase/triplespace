//! The Keyword key (ADR 0017 §5): a short phrase describing what a work is about, as
//! OpenAlex defines its keywords. A keyword is the phrase, not the concept it denotes.
//!
//! Normalizer: Unicode NFKC; casefold; every run of whitespace or punctuation becomes one
//! hyphen; leading and trailing hyphens are removed. Grammar: 1 to 200 characters of
//! letters, digits and single hyphens, at least one letter, no leading or trailing hyphen.
//!
//! "Punctuation" is read as every character that is neither a letter nor a digit, so that
//! symbols (`C++`, `AT&T`) separate words too rather than making the phrase unnormalizable;
//! the grammar admits nothing but letters, digits and hyphens either way.

use unicode_normalization::UnicodeNormalization;

/// The most characters a keyword key may have.
pub const MAX_CHARS: usize = 200;

/// Why a string is not a Keyword key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeywordError {
    /// Nothing but separators, so nothing is left after trimming.
    #[error("keyword is empty after normalization")]
    Empty,
    /// No letter at all (`2024`, `42-1`).
    #[error("keyword `{0}` has no letter")]
    NoLetter(String),
    /// More than [`MAX_CHARS`] characters.
    #[error("keyword `{0}` is longer than {MAX_CHARS} characters")]
    TooLong(String),
}

/// Normalizes `input` to a Keyword key.
///
/// ```
/// use scatter_normalize::keyword::normalize;
/// assert_eq!(normalize("Machine Learning").unwrap(), "machine-learning");
/// assert_eq!(normalize("Type 1 diabetes").unwrap(), "type-1-diabetes");
/// assert_eq!(normalize("  DNA  ").unwrap(), "dna");
/// assert_eq!(normalize("ﬁsh").unwrap(), "fish"); // NFKC unfolds the ligature
/// assert!(normalize("2024").is_err());
/// ```
pub fn normalize(input: &str) -> Result<String, KeywordError> {
    let folded = crate::casefold(&input.nfkc().collect::<String>());
    let mut key = String::with_capacity(folded.len());
    let mut pending_hyphen = false;
    for c in folded.chars() {
        if c.is_alphanumeric() {
            if pending_hyphen && !key.is_empty() {
                key.push('-');
            }
            pending_hyphen = false;
            key.push(c);
        } else {
            pending_hyphen = true;
        }
    }
    if key.is_empty() {
        return Err(KeywordError::Empty);
    }
    if !key.chars().any(char::is_alphabetic) {
        return Err(KeywordError::NoLetter(key));
    }
    if key.chars().count() > MAX_CHARS {
        return Err(KeywordError::TooLong(key));
    }
    Ok(key)
}

/// Whether `s` is already a Keyword key in canonical form.
#[must_use]
pub fn is_key(s: &str) -> bool {
    normalize(s).as_deref() == Ok(s)
}

/// The key shown as a label when no label exists: hyphens as spaces (0017 §5).
#[must_use]
pub fn display(key: &str) -> String {
    key.replace('-', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_collapse_and_trim() {
        assert_eq!(
            normalize("--machine---learning--").unwrap(),
            "machine-learning"
        );
        assert_eq!(normalize("fast_food").unwrap(), "fast-food");
        assert_eq!(normalize("fast:food").unwrap(), "fast-food");
        assert_eq!(normalize("C++").unwrap(), "c");
        assert_eq!(normalize("AT&T").unwrap(), "at-t");
    }

    #[test]
    fn case_and_scripts() {
        assert_eq!(normalize("Straße").unwrap(), "strasse");
        assert_eq!(normalize("ΣΊΣΥΦΟΣ").unwrap(), "σίσυφοσ");
        // A CJK phrase without word boundaries is its own key (0017, open question).
        assert_eq!(normalize("機械学習").unwrap(), "機械学習");
    }

    #[test]
    fn ids_that_look_like_keywords_are_keywords() {
        assert_eq!(normalize("p53").unwrap(), "p53");
        assert_eq!(normalize("H2O").unwrap(), "h2o");
    }

    #[test]
    fn rejections() {
        assert_eq!(normalize(""), Err(KeywordError::Empty));
        assert_eq!(normalize(" - "), Err(KeywordError::Empty));
        assert!(matches!(normalize("2024"), Err(KeywordError::NoLetter(_))));
        assert!(matches!(
            normalize(&"a".repeat(MAX_CHARS + 1)),
            Err(KeywordError::TooLong(_))
        ));
        assert!(normalize(&"a".repeat(MAX_CHARS)).is_ok());
    }

    #[test]
    fn canonical_and_display() {
        assert!(is_key("type-1-diabetes"));
        assert!(!is_key("Type-1-diabetes"));
        assert_eq!(display("type-1-diabetes"), "type 1 diabetes");
    }
}
