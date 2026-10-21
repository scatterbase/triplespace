//! Timestamps: matching the registry's MediaWiki date formats in a line of wikitext, and the
//! minute-resolution UTC times signatures carry (0090 §5).

use crate::registry::Months;

/// A UTC time to the minute, as minutes since 1970-01-01T00:00Z. Signatures carry no seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp(pub i64);

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

impl Stamp {
    /// A civil date and time, read as UTC; `None` when it is not a valid date.
    pub fn from_civil(y: i64, mo: i64, d: i64, h: i64, mi: i64) -> Option<Stamp> {
        if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 {
            return None;
        }
        Some(Stamp(days_from_civil(y, mo, d) * 1440 + h * 60 + mi))
    }

    /// `(year, month, day, hour, minute)`.
    pub fn civil(self) -> (i64, i64, i64, i64, i64) {
        let (y, m, d) = civil_from_days(self.0.div_euclid(1440));
        let rem = self.0.rem_euclid(1440);
        (y, m, d, rem / 60, rem % 60)
    }

    /// `2006-07-10T07:50:00Z`, the form the Action API and DiscussionTools' JSON use.
    pub fn iso(self) -> String {
        let (y, m, d, h, mi) = self.civil();
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:00Z")
    }
}

/// How a wiki's signatures map to UTC (0090 §5): an offset for timestamps that carry no zone,
/// and the offsets of the zone abbreviations its signatures print.
///
/// MediaWiki signatures print their zone (`(UTC)`, `(CET)`, `(CEST)`), so the abbreviation
/// gives the offset exactly, daylight saving included; the default offset is used only for a
/// timestamp written without one.
#[derive(Debug, Clone, Default)]
pub struct Zone {
    /// Minutes east of UTC for a timestamp that carries no zone.
    pub default_offset: i64,
    /// Zone abbreviations and their minutes east of UTC. `UTC`, `GMT` and `Z` are always 0.
    pub abbreviations: Vec<(String, i64)>,
}

impl Zone {
    /// Coordinated Universal Time, as English Wikimedia wikis sign.
    pub fn utc() -> Zone {
        Zone::default()
    }

    fn offset(&self, abbrev: Option<&str>) -> i64 {
        match abbrev {
            Some("UTC" | "GMT" | "Z") => 0,
            Some(a) => self
                .abbreviations
                .iter()
                .find(|(n, _)| n == a)
                .map_or(self.default_offset, |(_, o)| *o),
            None => self.default_offset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Num {
    Hour,
    Minute,
    Second,
    Day,
    Month,
    Year,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Number { field: Num, min: usize, max: usize },
    MonthName,
    Comma,
    Space,
    Literal(char),
}

/// A compiled MediaWiki date-format string (`H:i, j F Y`).
#[derive(Debug, Clone)]
pub struct Format {
    /// The format string as the registry wrote it.
    pub source: String,
    tokens: Vec<Token>,
    zone: bool,
}

/// A timestamp found in a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    /// Byte offset where it starts.
    pub start: usize,
    /// Byte offset just past it, its zone included.
    pub end: usize,
    /// The time, in UTC.
    pub when: Stamp,
}

impl Format {
    /// Compiles a MediaWiki date-format string. `zone`: whether ` (T)` may follow.
    pub fn compile(source: &str, zone: bool) -> Result<Format, String> {
        let mut tokens = Vec::new();
        let mut chars = source.chars();
        while let Some(c) = chars.next() {
            let tok = match c {
                'H' => Token::Number {
                    field: Num::Hour,
                    min: 1,
                    max: 2,
                },
                'i' => Token::Number {
                    field: Num::Minute,
                    min: 2,
                    max: 2,
                },
                's' => Token::Number {
                    field: Num::Second,
                    min: 2,
                    max: 2,
                },
                'j' => Token::Number {
                    field: Num::Day,
                    min: 1,
                    max: 2,
                },
                'd' => Token::Number {
                    field: Num::Day,
                    min: 2,
                    max: 2,
                },
                'n' => Token::Number {
                    field: Num::Month,
                    min: 1,
                    max: 2,
                },
                'm' => Token::Number {
                    field: Num::Month,
                    min: 2,
                    max: 2,
                },
                'Y' => Token::Number {
                    field: Num::Year,
                    min: 4,
                    max: 4,
                },
                'F' | 'M' => Token::MonthName,
                ',' => Token::Comma,
                ' ' => Token::Space,
                '\\' => Token::Literal(chars.next().ok_or("a trailing backslash")?),
                c if c.is_ascii_alphabetic() => {
                    return Err(format!("unsupported format character {c:?} in {source:?}"));
                }
                c => Token::Literal(c),
            };
            tokens.push(tok);
        }
        if !tokens.iter().any(|t| {
            matches!(
                t,
                Token::Number {
                    field: Num::Year,
                    ..
                }
            )
        }) {
            return Err(format!("{source:?} has no year"));
        }
        Ok(Format {
            source: source.to_string(),
            tokens,
            zone,
        })
    }

    /// Whether the format can start at this character.
    fn starts_with_digit(&self) -> bool {
        matches!(self.tokens.first(), Some(Token::Number { .. }))
    }

    /// Matches the format at `start`, returning the end offset, the local fields and any zone.
    fn match_at(
        &self,
        line: &str,
        start: usize,
        months: &Months,
    ) -> Option<(usize, [i64; 5], Option<String>)> {
        let bytes = line.as_bytes();
        let mut pos = start;
        let mut fields = [0i64, 0, 0, 0, 0]; // y, mo, d, h, mi
        for tok in &self.tokens {
            match tok {
                Token::Number { field, min, max } => {
                    let digits = bytes[pos..]
                        .iter()
                        .take(*max)
                        .take_while(|c| c.is_ascii_digit())
                        .count();
                    if digits < *min {
                        return None;
                    }
                    let v: i64 = line[pos..pos + digits].parse().ok()?;
                    match field {
                        Num::Year => fields[0] = v,
                        Num::Month => fields[1] = v,
                        Num::Day => fields[2] = v,
                        Num::Hour => fields[3] = v,
                        Num::Minute => fields[4] = v,
                        Num::Second => {}
                    }
                    pos += digits;
                }
                Token::MonthName => {
                    let (m, len) = month_at(&line[pos..], months)?;
                    fields[1] = m;
                    pos += len;
                }
                Token::Comma => {
                    if bytes.get(pos) == Some(&b',') {
                        pos += 1;
                    }
                }
                Token::Space => {
                    let n = bytes[pos..].iter().take_while(|c| **c == b' ').count();
                    if n == 0 {
                        return None;
                    }
                    pos += n;
                }
                Token::Literal(c) => {
                    let s = line[pos..].strip_prefix(*c)?;
                    pos = line.len() - s.len();
                }
            }
        }
        if bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        let mut zone = None;
        if self.zone {
            let rest = &line[pos..];
            let trimmed = rest.trim_start_matches(' ');
            if let Some(inner) = trimmed.strip_prefix('(') {
                let letters = inner.bytes().take_while(u8::is_ascii_uppercase).count();
                if (1..=6).contains(&letters) && inner.as_bytes().get(letters) == Some(&b')') {
                    zone = Some(inner[..letters].to_string());
                    pos += rest.len() - trimmed.len() + 1 + letters + 1;
                }
            }
        }
        Some((pos, fields, zone))
    }
}

/// A month name or abbreviation at the start of `s`: (month, byte length). Accepts a full
/// name, an abbreviation, or a prefix of a full name of three letters or more, case-insensitively,
/// with an optional full stop.
fn month_at(s: &str, months: &Months) -> Option<(i64, usize)> {
    let word_len: usize = s
        .chars()
        .take_while(|c| c.is_alphabetic())
        .map(char::len_utf8)
        .sum();
    if word_len == 0 {
        return None;
    }
    let word = s[..word_len].to_lowercase();
    let found = months.long.iter().zip(&months.short).position(|(l, sh)| {
        let l = l.to_lowercase();
        word == l
            || word == sh.to_lowercase()
            || (word.chars().count() >= 3 && l.starts_with(&word))
    })?;
    let len = if s[word_len..].starts_with('.') {
        word_len + 1
    } else {
        word_len
    };
    Some((i64::try_from(found).ok()? + 1, len))
}

/// Every timestamp in a line, in any of the formats, leftmost first and longest at each start;
/// overlapping matches are dropped.
pub fn find_timestamps(line: &str, formats: &[Format], months: &Months, zone: &Zone) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if !line.is_char_boundary(i) || (i > 0 && bytes[i - 1].is_ascii_digit()) {
            continue;
        }
        let digit = c.is_ascii_digit();
        let mut best: Option<Found> = None;
        for fmt in formats {
            if fmt.starts_with_digit() != digit {
                continue;
            }
            let Some((end, [y, mo, d, h, mi], abbrev)) = fmt.match_at(line, i, months) else {
                continue;
            };
            if !(2001..=2100).contains(&y) {
                continue;
            }
            let Some(local) = Stamp::from_civil(y, mo, d, h, mi) else {
                continue;
            };
            let when = Stamp(local.0 - zone.offset(abbrev.as_deref()));
            if best.is_none_or(|b| end > b.end) {
                best = Some(Found {
                    start: i,
                    end,
                    when,
                });
            }
        }
        if let Some(b) = best
            && found.last().is_none_or(|l| b.start >= l.end)
        {
            found.push(b);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Markup;

    fn find(line: &str) -> Vec<String> {
        let m = Markup::embedded();
        find_timestamps(line, &m.formats, m.months_for("en").unwrap(), &Zone::utc())
            .iter()
            .map(|f| f.when.iso())
            .collect()
    }

    #[test]
    fn modern_and_old_formats() {
        assert_eq!(
            find("[[User:A|A]] 12:33, 5 June 2025 (UTC)"),
            ["2025-06-05T12:33:00Z"]
        );
        assert_eq!(
            find("-- 04:50, 18 Feb 2005 (UTC)"),
            ["2005-02-18T04:50:00Z"]
        );
        assert_eq!(
            find("Rick 06:31, Feb 18, 2005 (UTC)"),
            ["2005-02-18T06:31:00Z"]
        );
        assert_eq!(find("at 2005-02-18T04:50:00"), ["2005-02-18T04:50:00Z"]);
        assert_eq!(find("12:00, 2005 March 3"), ["2005-03-03T12:00:00Z"]);
        assert!(find("version 1.2.3, 2005").is_empty());
        assert!(find("31:00, 5 June 2025").is_empty());
    }

    #[test]
    fn zone_abbreviations_convert() {
        let m = Markup::embedded();
        let zone = Zone {
            default_offset: 60,
            abbreviations: vec![("CEST".into(), 120)],
        };
        let f = |s: &str| {
            find_timestamps(s, &m.formats, m.months_for("en").unwrap(), &zone)[0]
                .when
                .iso()
        };
        assert_eq!(f("12:00, 5 June 2025 (CEST)"), "2025-06-05T10:00:00Z");
        assert_eq!(f("12:00, 5 June 2025 (UTC)"), "2025-06-05T12:00:00Z");
        assert_eq!(f("12:00, 5 June 2025"), "2025-06-05T11:00:00Z");
    }

    #[test]
    fn civil_round_trip() {
        let s = Stamp::from_civil(2024, 2, 29, 23, 59).unwrap();
        assert_eq!(s.civil(), (2024, 2, 29, 23, 59));
        assert!(Stamp::from_civil(2023, 2, 29, 0, 0).is_none());
    }
}
