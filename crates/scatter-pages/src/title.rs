//! Title checks: whether a title is a valid title in canonical form. Normalizing one (the
//! namespace's normalizer, prefixes, case) is the title resolver's (10 §2.1); the record
//! only ever holds the canonical form.

/// MediaWiki's limit on a title's length, in bytes, without the namespace.
pub const MAX_TITLE_BYTES: usize = 255;

/// Why a title is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleError {
    /// Empty.
    Empty,
    /// Longer than [`MAX_TITLE_BYTES`].
    TooLong,
    /// Holds a character a title may not: `# < > [ ] | { }`, a control character, or a
    /// percent-encoded byte.
    IllegalCharacter(char),
    /// Not in canonical form: an underscore, leading or trailing whitespace, or a run of
    /// spaces.
    NotCanonical,
}

impl std::fmt::Display for TitleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TitleError::Empty => f.write_str("the title is empty"),
            TitleError::TooLong => write!(f, "the title is longer than {MAX_TITLE_BYTES} bytes"),
            TitleError::IllegalCharacter(c) => write!(f, "the title contains {c:?}"),
            TitleError::NotCanonical => f.write_str("the title is not in canonical form"),
        }
    }
}

/// Checks a title within its namespace.
pub fn check_title(_ns: i32, title: &str) -> Result<(), TitleError> {
    if title.is_empty() {
        return Err(TitleError::Empty);
    }
    if title.len() > MAX_TITLE_BYTES {
        return Err(TitleError::TooLong);
    }
    if let Some(c) = title
        .chars()
        .find(|&c| matches!(c, '#' | '<' | '>' | '[' | ']' | '|' | '{' | '}') || c.is_control())
    {
        return Err(TitleError::IllegalCharacter(c));
    }
    let b = title.as_bytes();
    if b.windows(3)
        .any(|w| w[0] == b'%' && w[1].is_ascii_hexdigit() && w[2].is_ascii_hexdigit())
    {
        return Err(TitleError::IllegalCharacter('%'));
    }
    if title.contains('_')
        || title.starts_with(char::is_whitespace)
        || title.ends_with(char::is_whitespace)
        || title.contains("  ")
        || title.chars().any(|c| c.is_whitespace() && c != ' ')
    {
        return Err(TitleError::NotCanonical);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(check_title(4, "Analytics/WCD"), Ok(()));
        assert_eq!(check_title(4, ""), Err(TitleError::Empty));
        assert_eq!(
            check_title(4, "a#b"),
            Err(TitleError::IllegalCharacter('#'))
        );
        assert_eq!(
            check_title(4, "a%20b"),
            Err(TitleError::IllegalCharacter('%'))
        );
        assert_eq!(check_title(4, "a_b"), Err(TitleError::NotCanonical));
        assert_eq!(check_title(4, " a"), Err(TitleError::NotCanonical));
        assert_eq!(check_title(4, &"x".repeat(256)), Err(TitleError::TooLong));
    }
}
