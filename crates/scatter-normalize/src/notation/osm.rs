//! The grammar of the `osm` notation scheme (ADR 0036 §3, 0048 §6): an OSM key or tag,
//! `amenity` or `amenity=cafe`, the exact string, case-sensitive, in Unicode NFC. The
//! scheme's normalizer is `exact`; input that is not already valid is rejected rather
//! than repaired.
//!
//! An OSM key or tag is a notation with the `osm` scheme ([`crate::notation`]),
//! `notation:osm:amenity=cafe`, not a keyed type of its own. The registry names this
//! grammar `osm-tag` (`docs/registry/notation-schemes.toml`).
//!
//! Grammar: a key of 1 to 255 characters, or a key, `=` and a value of 1 to 255
//! characters; no control characters; no `=` in the key; no leading or trailing
//! whitespace in either part; NFC.

/// The most characters a key or a value may have.
pub const MAX_PART_CHARS: usize = 255;

/// Why a string is not an OSM tag or key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OsmNotationError {
    /// Not in Unicode NFC.
    #[error("`{0}` is not in Unicode NFC")]
    NotNfc(String),
    /// A control character.
    #[error("`{0}` contains a control character")]
    Control(String),
    /// The key or the value is empty.
    #[error("`{0}` has an empty key or value")]
    EmptyPart(String),
    /// The key or the value is longer than [`MAX_PART_CHARS`] characters.
    #[error("`{0}` has a key or value longer than {MAX_PART_CHARS} characters")]
    TooLong(String),
    /// Leading or trailing whitespace in the key or the value.
    #[error("`{0}` has leading or trailing whitespace in its key or value")]
    Whitespace(String),
}

/// Checks that `input` is a valid OSM tag or key and returns it unchanged.
///
/// ```
/// use scatter_normalize::notation::osm::normalize;
/// assert_eq!(normalize("amenity").unwrap(), "amenity");
/// assert_eq!(normalize("amenity=cafe").unwrap(), "amenity=cafe");
/// assert_eq!(normalize("addr:street=Main Street").unwrap(), "addr:street=Main Street");
/// assert!(normalize("Amenity").is_ok(), "case is significant, not an error");
/// assert!(normalize("amenity=").is_err());
/// assert!(normalize(" amenity").is_err());
/// ```
pub fn normalize(input: &str) -> Result<String, OsmNotationError> {
    let err = |f: fn(String) -> OsmNotationError| f(input.to_string());
    if !crate::is_nfc(input) {
        return Err(err(OsmNotationError::NotNfc));
    }
    if input.chars().any(char::is_control) {
        return Err(err(OsmNotationError::Control));
    }
    let (key, value) = match input.split_once('=') {
        Some((k, v)) => (k, Some(v)),
        None => (input, None),
    };
    for part in std::iter::once(key).chain(value) {
        if part.is_empty() {
            return Err(err(OsmNotationError::EmptyPart));
        }
        if part.chars().count() > MAX_PART_CHARS {
            return Err(err(OsmNotationError::TooLong));
        }
        if part.trim() != part {
            return Err(err(OsmNotationError::Whitespace));
        }
    }
    Ok(input.to_string())
}

/// Whether `s` is a valid OSM tag or key. There is no other canonical form.
#[must_use]
pub fn is_key(s: &str) -> bool {
    normalize(s).is_ok()
}

/// Splits a tag into its key and value; a bare key has no value.
#[must_use]
pub fn split(tag: &str) -> (&str, Option<&str>) {
    match tag.split_once('=') {
        Some((k, v)) => (k, Some(v)),
        None => (tag, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_tags() {
        assert!(is_key("amenity"));
        assert!(is_key("amenity=cafe"));
        assert!(is_key("name:en=Café"));
        // A value may itself contain `=`; only the first one splits.
        assert_eq!(split("a=b=c"), ("a", Some("b=c")));
        assert!(is_key("a=b=c"));
    }

    #[test]
    fn distinct_forms_stay_distinct() {
        assert_ne!(normalize("fast_food"), normalize("fast-food"));
        assert_ne!(normalize("Name"), normalize("name"));
    }

    #[test]
    fn rejections() {
        assert!(matches!(
            normalize("=cafe"),
            Err(OsmNotationError::EmptyPart(_))
        ));
        assert!(matches!(
            normalize("amenity="),
            Err(OsmNotationError::EmptyPart(_))
        ));
        assert!(matches!(normalize(""), Err(OsmNotationError::EmptyPart(_))));
        assert!(matches!(
            normalize("amenity =cafe"),
            Err(OsmNotationError::Whitespace(_))
        ));
        assert!(matches!(
            normalize("amenity= cafe"),
            Err(OsmNotationError::Whitespace(_))
        ));
        assert!(matches!(
            normalize("a\tb"),
            Err(OsmNotationError::Control(_))
        ));
        assert!(matches!(
            normalize(&"k".repeat(MAX_PART_CHARS + 1)),
            Err(OsmNotationError::TooLong(_))
        ));
        // "é" as e + combining acute is NFD, not NFC.
        assert!(matches!(
            normalize("cafe\u{301}"),
            Err(OsmNotationError::NotNfc(_))
        ));
        assert!(normalize("caf\u{e9}").is_ok());
    }
}
