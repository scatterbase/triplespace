//! ID grammars for minted entity types (ADR 0017 §2, extended by 0037 §2).
//!
//! A grammar says what may follow the three-letter prefix of a foreign ID and what its
//! canonical spelling is. Input is case-insensitive in every position (0017 §2); the
//! canonical form is what [`IdGrammar::canonicalize`] returns.

use std::fmt;
use std::str::FromStr;

/// How the part of a foreign ID after its three-letter prefix is formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum IdGrammar {
    /// One or more decimal digits. Canonical as given, so leading zeros are significant
    /// where upstream has them (a CAMEO code `0311`, 0037 §2); providers that never mint
    /// them, such as Wikidata, never produce them. The default.
    #[default]
    Digits,
    /// An RFC 4122 UUID in its 36-character hyphenated text form. Canonical lowercase.
    Uuid,
    /// A GDELT GKG record ID: 14 digits, `-`, an optional `T`, digits (0037 §2).
    /// Canonical as given, with the `T` uppercase.
    GdeltRecord,
    /// Uppercase letters, digits and underscores, starting with a letter, at most 128
    /// characters (0037 §2). Canonical uppercase.
    Token,
}

impl IdGrammar {
    /// The registry name of the grammar, as `providers.toml` spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Digits => "digits",
            Self::Uuid => "uuid",
            Self::GdeltRecord => "gdelt-record",
            Self::Token => "token",
        }
    }

    /// Returns the canonical spelling of `rest` under this grammar, or `None` if `rest`
    /// is not a valid ID under it.
    #[must_use]
    pub fn canonicalize(self, rest: &str) -> Option<String> {
        match self {
            Self::Digits => digits(rest).then(|| rest.to_string()),
            Self::Uuid => uuid(rest).then(|| rest.to_ascii_lowercase()),
            Self::GdeltRecord => {
                let upper = rest.to_ascii_uppercase();
                gdelt_record(&upper).then_some(upper)
            }
            Self::Token => {
                let upper = rest.to_ascii_uppercase();
                token(&upper).then_some(upper)
            }
        }
    }

    /// Whether `rest` is already in canonical form under this grammar.
    #[must_use]
    pub fn is_canonical(self, rest: &str) -> bool {
        self.canonicalize(rest).as_deref() == Some(rest)
    }
}

impl fmt::Display for IdGrammar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The registry named a grammar this crate does not know.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown ID grammar `{0}`; known: digits, uuid, gdelt-record, token")]
pub struct UnknownGrammar(pub String);

impl FromStr for IdGrammar {
    type Err = UnknownGrammar;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "digits" => Ok(Self::Digits),
            "uuid" => Ok(Self::Uuid),
            "gdelt-record" => Ok(Self::GdeltRecord),
            "token" => Ok(Self::Token),
            other => Err(UnknownGrammar(other.to_string())),
        }
    }
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => *c == b'-',
        _ => c.is_ascii_hexdigit(),
    })
}

fn gdelt_record(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 16 || !b[..14].iter().all(u8::is_ascii_digit) || b[14] != b'-' {
        return false;
    }
    let tail = if b[15] == b'T' { &b[16..] } else { &b[15..] };
    !tail.is_empty() && tail.iter().all(u8::is_ascii_digit)
}

fn token(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_uppercase()
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_grammar() {
        assert_eq!(IdGrammar::Digits.canonicalize("42").as_deref(), Some("42"));
        assert_eq!(IdGrammar::Digits.canonicalize("0").as_deref(), Some("0"));
        // Leading zeros are kept as given: a CAMEO code (0037 §2) is `0311`, not `311`.
        assert_eq!(
            IdGrammar::Digits.canonicalize("0311").as_deref(),
            Some("0311")
        );
        assert!(IdGrammar::Digits.is_canonical("0311"));
        assert_eq!(IdGrammar::Digits.canonicalize(""), None);
        assert_eq!(IdGrammar::Digits.canonicalize("4a"), None);
    }

    #[test]
    fn uuid_grammar_lowercases() {
        let u = "B10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D";
        assert_eq!(
            IdGrammar::Uuid.canonicalize(u).as_deref(),
            Some("b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d")
        );
        assert!(!IdGrammar::Uuid.is_canonical(u));
        assert_eq!(
            IdGrammar::Uuid.canonicalize("b10bbbfc-cf9e-42e0-be17"),
            None
        );
        assert_eq!(
            IdGrammar::Uuid.canonicalize("b10bbbfccf9e42e0be17e2c3e1d2600d"),
            None
        );
    }

    #[test]
    fn gdelt_record_grammar() {
        assert!(IdGrammar::GdeltRecord.is_canonical("20150218230000-12"));
        assert!(IdGrammar::GdeltRecord.is_canonical("20150218230000-T12"));
        assert_eq!(
            IdGrammar::GdeltRecord
                .canonicalize("20150218230000-t12")
                .as_deref(),
            Some("20150218230000-T12")
        );
        assert_eq!(IdGrammar::GdeltRecord.canonicalize("20150218230000-"), None);
        assert_eq!(
            IdGrammar::GdeltRecord.canonicalize("2015021823000-12"),
            None
        );
        assert_eq!(
            IdGrammar::GdeltRecord.canonicalize("20150218230000-T"),
            None
        );
    }

    #[test]
    fn token_grammar() {
        assert!(IdGrammar::Token.is_canonical("TAX_FNCACT_MAYOR"));
        assert_eq!(
            IdGrammar::Token.canonicalize("tax_fncact_mayor").as_deref(),
            Some("TAX_FNCACT_MAYOR")
        );
        assert_eq!(IdGrammar::Token.canonicalize("_X"), None);
        assert_eq!(IdGrammar::Token.canonicalize("1X"), None);
        assert_eq!(IdGrammar::Token.canonicalize("A-B"), None);
        assert_eq!(IdGrammar::Token.canonicalize(&"A".repeat(129)), None);
    }

    #[test]
    fn names_round_trip() {
        for g in [
            IdGrammar::Digits,
            IdGrammar::Uuid,
            IdGrammar::GdeltRecord,
            IdGrammar::Token,
        ] {
            assert_eq!(g.name().parse::<IdGrammar>(), Ok(g));
        }
        assert!("slug".parse::<IdGrammar>().is_err());
    }
}
