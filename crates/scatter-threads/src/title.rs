//! Thread subjects and titles: `Thread:2026-09-27/Why is P31 wrong here` (14 §1.4).

/// Why a subject is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectError {
    /// Nothing but spaces.
    Empty,
    /// It holds one of MediaWiki's forbidden title characters.
    Forbidden(char),
    /// The title would be longer than a title may be.
    TooLong,
}

impl std::fmt::Display for SubjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubjectError::Empty => f.write_str("the subject is empty"),
            SubjectError::Forbidden(c) => write!(f, "a subject may not contain `{c}`"),
            SubjectError::TooLong => f.write_str("the subject is too long"),
        }
    }
}

const FORBIDDEN: [char; 8] = ['#', '<', '>', '[', ']', '|', '{', '}'];

/// The room a title keeps for the date and the longest suffix: `YYYY-MM-DD/` and ` (99999)`.
const RESERVED: usize = 11 + 8;

/// Normalizes a subject as a `first-letter` title: underscores read as spaces, runs of spaces
/// made one, the ends trimmed, the first letter upper-cased; MediaWiki's forbidden characters
/// (`# < > [ ] | { }`) and control characters refused.
pub fn normalize_subject(subject: &str) -> Result<String, SubjectError> {
    if let Some(c) = subject
        .chars()
        .find(|c| FORBIDDEN.contains(c) || (c.is_control() && *c != '\t'))
    {
        return Err(SubjectError::Forbidden(c));
    }
    let words: Vec<&str> = subject
        .split(|c: char| c == '_' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    let joined = words.join(" ");
    let mut chars = joined.chars();
    let Some(first) = chars.next() else {
        return Err(SubjectError::Empty);
    };
    let out: String = first.to_uppercase().chain(chars).collect();
    if out.len() + RESERVED > scatter_pages::MAX_TITLE_BYTES {
        return Err(SubjectError::TooLong);
    }
    Ok(out)
}

/// Mints a thread's title within the `Thread` namespace from its date (`YYYY-MM-DD`, UTC) and
/// its normalized subject: `{date}/{subject}`, or with ` (2)`, ` (3)` … when `taken` says the
/// title is already in use.
pub fn mint_title(date: &str, subject: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = format!("{date}/{subject}");
    if !taken(&base) {
        return base;
    }
    (2..=u32::MAX)
        .map(|n| format!("{base} ({n})"))
        .find(|t| !taken(t))
        .unwrap_or(base)
}
