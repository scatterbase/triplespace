//! The Domain key: a domain name in lowercase A-label form (ADR 0009 §2).
//!
//! The normalizer:
//!
//! 1. strips one trailing dot;
//! 2. applies UTS #46 processing, non-transitional, with `CheckHyphens`, `CheckBidi`,
//!    `CheckJoiners` and `UseSTD3ASCIIRules` on;
//! 3. converts every label to its A-label;
//! 4. lowercases the result.
//!
//! A key is valid when the whole name is at most 253 octets and each label at most 63,
//! and it has at least two labels, or is a single label that is alphabetic ASCII or begins
//! with `xn--`. `www.` is not stripped; names with underscores are not keys.

use std::fmt;

use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};

/// Why a string is not a Domain key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DomainError {
    /// Empty after stripping the trailing dot.
    #[error("empty domain name")]
    Empty,
    /// UTS #46 processing rejected the name: a disallowed character (underscores among
    /// them), a bad hyphen placement, a bidi or joiner violation, or a length limit.
    #[error("`{0}` is not a valid domain name under UTS #46 with STD3 rules")]
    Uts46(String),
    /// A single label that is neither alphabetic ASCII nor an A-label. A bare top-level
    /// domain is always shaped like one of those.
    #[error("`{0}` is a single label that is neither alphabetic nor an A-label")]
    SingleLabel(String),
}

/// Normalizes `input` to a Domain key, or says why it is not one.
///
/// ```
/// use scatter_normalize::domain::normalize;
/// assert_eq!(normalize("En.Wikipedia.org.").unwrap(), "en.wikipedia.org");
/// assert_eq!(normalize("bücher.example").unwrap(), "xn--bcher-kva.example");
/// assert_eq!(normalize("XN--BCHER-KVA.example").unwrap(), "xn--bcher-kva.example");
/// assert_eq!(normalize("org").unwrap(), "org");
/// assert!(normalize("_dmarc.example.org").is_err());
/// assert!(normalize("12345").is_err());
/// ```
pub fn normalize(input: &str) -> Result<String, DomainError> {
    let stripped = input.strip_suffix('.').unwrap_or(input);
    if stripped.is_empty() {
        return Err(DomainError::Empty);
    }
    let ascii = Uts46::new()
        .to_ascii(
            stripped.as_bytes(),
            AsciiDenyList::STD3,
            Hyphens::Check,
            DnsLength::Verify,
        )
        .map_err(|_| DomainError::Uts46(input.to_string()))?;
    let key = ascii.to_ascii_lowercase();
    if key.is_empty() {
        return Err(DomainError::Empty);
    }
    if !key.contains('.') && !is_tld_shaped(&key) {
        return Err(DomainError::SingleLabel(key));
    }
    Ok(key)
}

/// Whether `s` is already a Domain key in canonical form.
#[must_use]
pub fn is_key(s: &str) -> bool {
    normalize(s).as_deref() == Ok(s)
}

fn is_tld_shaped(label: &str) -> bool {
    label.starts_with("xn--") || label.bytes().all(|b| b.is_ascii_lowercase())
}

/// The U-label form of a key, for display as the derived `mul` label (0009 §5).
///
/// ```
/// use scatter_normalize::domain::to_unicode;
/// assert_eq!(to_unicode("xn--bcher-kva.example"), "bücher.example");
/// assert_eq!(to_unicode("en.wikipedia.org"), "en.wikipedia.org");
/// ```
#[must_use]
pub fn to_unicode(key: &str) -> String {
    let (unicode, _) = Uts46::new().to_unicode(key.as_bytes(), AsciiDenyList::STD3, Hyphens::Check);
    unicode.into_owned()
}

/// A Domain key's labels, most specific first: `en.wikipedia.org` → `en`, `wikipedia`, `org`.
pub fn labels(key: &str) -> impl DoubleEndedIterator<Item = &str> {
    key.split('.')
}

/// The parents of a key, by removing labels from the left: `en.wikipedia.org` →
/// `wikipedia.org`, `org`. This is the DNS hierarchy the page header shows (0009 §5);
/// it is navigation, not data.
pub fn parents(key: &str) -> impl Iterator<Item = &str> {
    key.match_indices('.').map(move |(i, _)| &key[i + 1..])
}

/// A wrapper that formats a key in its U-label form.
#[derive(Debug, Clone, Copy)]
pub struct Unicode<'a>(pub &'a str);

impl fmt::Display for Unicode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_unicode(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_form_is_accepted_and_one_key_comes_out() {
        for input in [
            "bücher.example",
            "Bücher.Example",
            "XN--BCHER-KVA.example",
            "xn--bcher-kva.example.",
        ] {
            assert_eq!(
                normalize(input).unwrap(),
                "xn--bcher-kva.example",
                "{input}"
            );
        }
    }

    #[test]
    fn www_is_not_stripped() {
        assert_eq!(normalize("www.example.org").unwrap(), "www.example.org");
        assert_ne!(normalize("www.example.org"), normalize("example.org"));
    }

    #[test]
    fn underscores_and_bad_hyphens_are_rejected() {
        assert!(matches!(
            normalize("_dmarc.example.org"),
            Err(DomainError::Uts46(_))
        ));
        assert!(matches!(
            normalize("-bad.example"),
            Err(DomainError::Uts46(_))
        ));
        assert!(matches!(
            normalize("bad-.example"),
            Err(DomainError::Uts46(_))
        ));
        assert!(matches!(
            normalize("a b.example"),
            Err(DomainError::Uts46(_))
        ));
    }

    #[test]
    fn single_labels() {
        assert_eq!(normalize("ORG").unwrap(), "org");
        assert_eq!(normalize("xn--p1ai").unwrap(), "xn--p1ai");
        assert!(matches!(
            normalize("example1"),
            Err(DomainError::SingleLabel(_))
        ));
        assert!(matches!(normalize("42"), Err(DomainError::SingleLabel(_))));
    }

    #[test]
    fn length_limits() {
        let label64 = "a".repeat(64);
        assert!(normalize(&format!("{label64}.example")).is_err());
        let label63 = "a".repeat(63);
        assert!(normalize(&format!("{label63}.example")).is_ok());
        let long = (0..5).map(|_| "a".repeat(50)).collect::<Vec<_>>().join(".");
        assert!(long.len() > 253);
        assert!(normalize(&long).is_err());
    }

    #[test]
    fn empty_and_dots() {
        assert_eq!(normalize(""), Err(DomainError::Empty));
        assert_eq!(normalize("."), Err(DomainError::Empty));
        assert!(normalize("..").is_err());
        assert!(normalize("example..org").is_err());
    }

    #[test]
    fn hierarchy() {
        assert_eq!(
            parents("en.wikipedia.org").collect::<Vec<_>>(),
            ["wikipedia.org", "org"]
        );
        assert_eq!(parents("org").count(), 0);
        assert_eq!(labels("en.wikipedia.org").next_back(), Some("org"));
    }

    #[test]
    fn is_key_is_idempotence() {
        assert!(is_key("en.wikipedia.org"));
        assert!(!is_key("En.wikipedia.org"));
        assert!(!is_key("en.wikipedia.org."));
        assert!(!is_key("bücher.example"));
    }
}
