//! Data values as HTML (wikibase-compat §3.3): what a snak says, in the reader's language,
//! with entities by their labels and links to their pages.
//!
//! Numbers are grouped and dates written in English forms for now: the messages carry the
//! word order and month names, and per-language number formats come with ICU4X decimal
//! formatting when a translation needs them.

use std::fmt::Write as _;

use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::{
    DataType, DataValue, EntityIdValue, GlobeCoordinate, MonolingualText, Quantity, Time,
};

use super::Render;
use crate::html::{esc, lang_attrs, link, linkable};

/// The Gregorian and Julian calendar models.
const JULIAN: &str = "Q1985786";

/// The IRI of Earth, the default globe.
const EARTH: &str = "/Q2";

impl Render<'_> {
    /// A snak's value, or what it says instead of one.
    #[must_use]
    pub fn snak(&self, snak: &Snak) -> String {
        match &snak.kind {
            SnakKind::Value(v) => self.value(v, snak.datatype.as_ref()),
            SnakKind::SomeValue => format!(
                "<span class=\"ts-special-value\">{}</span>",
                esc(&self.m.get("ts-value-somevalue"))
            ),
            SnakKind::NoValue => format!(
                "<span class=\"ts-special-value\">{}</span>",
                esc(&self.m.get("ts-value-novalue"))
            ),
        }
    }

    /// A data value, formatted by its data type where that matters (URLs, identifiers,
    /// media) and by its value type otherwise.
    #[must_use]
    pub fn value(&self, value: &DataValue, datatype: Option<&DataType>) -> String {
        match value {
            DataValue::String(s) => match datatype.map(DataType::id) {
                Some("url") if linkable(s) => link(
                    s,
                    &esc(s),
                    " class=\"external\" rel=\"nofollow ugc noopener\" dir=\"ltr\"",
                ),
                Some("external-id") => {
                    format!("<span class=\"ts-mono\" dir=\"ltr\">{}</span>", esc(s))
                }
                Some("commonsMedia") => link(
                    &format!(
                        "https://commons.wikimedia.org/wiki/File:{}",
                        crate::frame::title_url(s)
                    ),
                    &esc(s),
                    " class=\"external\" rel=\"nofollow noopener\"",
                ),
                Some("math" | "musical-notation") => {
                    format!("<code class=\"ts-mono\" dir=\"ltr\">{}</code>", esc(s))
                }
                // A string's direction is its own, whatever the page's (bidi isolation).
                _ => format!("<bdi>{}</bdi>", esc(s)),
            },
            DataValue::EntityId(e) => self.entity_link(e),
            DataValue::MonolingualText(t) => monolingual(t),
            DataValue::Quantity(q) => self.quantity(q),
            DataValue::Time(t) => self.time(t),
            DataValue::GlobeCoordinate(g) => self.coordinate(g),
            DataValue::Unknown { value, .. } => {
                format!("<code class=\"ts-mono\">{}</code>", esc(&value.to_string()))
            }
            // A value type the model adds later.
            #[allow(unreachable_patterns)]
            _ => String::new(),
        }
    }

    /// A link to an entity's page by its label, or its ID where it has no label the
    /// reader can read.
    #[must_use]
    pub fn entity_link(&self, e: &EntityIdValue) -> String {
        self.id_link(e.id.as_str(), &e.entity_type)
    }

    /// A link to an entity by ID and entity type.
    #[must_use]
    pub fn id_link(&self, id: &str, entity_type: &str) -> String {
        let href = super::entity_href(id, entity_type);
        match self.lookup.label(id) {
            Some((text, lang)) => link(
                &href,
                &esc(text),
                &format!(" title=\"{}\"{}", esc(id), lang_attrs(lang, self.m.lang())),
            ),
            None => link(
                &href,
                &format!("<span class=\"ts-mono\">{}</span>", esc(id)),
                "",
            ),
        }
    }

    fn quantity(&self, q: &Quantity) -> String {
        let mut s = format!(
            "<span class=\"ts-number\">{}</span>",
            esc(&group(&q.amount))
        );
        if let (Some(up), Some(low)) = (&q.upper_bound, &q.lower_bound)
            && let (Some(amount), Some(upper), Some(lower)) = (num(&q.amount), num(up), num(low))
        {
            let plus = upper - amount;
            let minus = amount - lower;
            if (plus - minus).abs() < 1e-9 * amount.abs().max(1.0) {
                if plus > 0.0 {
                    let _ = write!(s, "±{}", esc(&group(&trim_float(plus))));
                }
            } else {
                let _ = write!(
                    s,
                    " <span class=\"ts-subtle\">[{}, {}]</span>",
                    esc(&group(low)),
                    esc(&group(up))
                );
            }
        }
        if q.unit != "1" {
            // Underlined: in running text beside the number, a link needs more than its
            // colour to stand out (WCAG 1.4.1).
            let _ = write!(s, " <span class=\"ts-unit\">{}</span>", self.unit(&q.unit));
        }
        s
    }

    fn unit(&self, iri: &str) -> String {
        if let Some(id) = super::unit_id(iri, self.site) {
            return self.id_link(&id, &super::entity_type_of(&id));
        }
        let tail = iri.rsplit('/').next().unwrap_or(iri);
        if linkable(iri) {
            link(
                iri,
                &esc(tail),
                " class=\"external\" rel=\"nofollow noopener\"",
            )
        } else {
            esc(tail)
        }
    }

    fn time(&self, t: &Time) -> String {
        let Some(p) = parse_time(&t.time) else {
            return esc(&t.time);
        };
        // Historical numbering: `-0044` is 44 BCE, and there is no year 0.
        let year = if t.time.starts_with('-') {
            -p.year
        } else {
            p.year
        };
        let year_text = |y: i64| -> String {
            if y < 0 {
                self.m.with("ts-time-bce", &[&y.unsigned_abs().to_string()])
            } else {
                y.to_string()
            }
        };
        let text = match t.precision {
            11.. if p.month > 0 && p.day > 0 => self.m.with(
                "ts-time-day",
                &[
                    &p.day.to_string(),
                    &self.m.get(&format!("ts-month-{}", p.month)),
                    &year_text(year),
                ],
            ),
            10.. if p.month > 0 => self.m.with(
                "ts-time-month",
                &[
                    &self.m.get(&format!("ts-month-{}", p.month)),
                    &year_text(year),
                ],
            ),
            8 => self
                .m
                .with("ts-time-decade", &[&year_text(year - year.rem_euclid(10))]),
            7 => {
                let c = (year.abs() - 1) / 100 + 1;
                let s = self.m.with("ts-time-century", &[&ordinal(c)]);
                if year < 0 {
                    self.m.with("ts-time-bce", &[&s])
                } else {
                    s
                }
            }
            6 => {
                let c = (year.abs() - 1) / 1000 + 1;
                let s = self.m.with("ts-time-millennium", &[&ordinal(c)]);
                if year < 0 {
                    self.m.with("ts-time-bce", &[&s])
                } else {
                    s
                }
            }
            _ => year_text(year),
        };
        let mut s = format!("<span class=\"ts-time\">{}</span>", esc(&text));
        if t.calendarmodel.ends_with(JULIAN) {
            let _ = write!(
                s,
                " <span class=\"ts-subtle\">{}</span>",
                esc(&self.m.get("ts-time-julian"))
            );
        }
        s
    }

    fn coordinate(&self, g: &GlobeCoordinate) -> String {
        let precision = g.precision.unwrap_or(1.0 / 3600.0);
        let text = format!(
            "{}, {}",
            dms(g.latitude, precision, ('N', 'S')),
            dms(g.longitude, precision, ('E', 'W'))
        );
        let mut s = format!("<span class=\"ts-number\">{}</span>", esc(&text));
        if !g.globe.ends_with(EARTH) {
            let globe = super::unit_id(&g.globe, self.site).map_or_else(
                || esc(g.globe.rsplit('/').next().unwrap_or("")),
                |id| self.id_link(&id, "item"),
            );
            let _ = write!(s, " <span class=\"ts-subtle\">({globe})</span>");
        }
        s
    }
}

fn monolingual(t: &MonolingualText) -> String {
    format!(
        "<span lang=\"{lang}\" dir=\"auto\">{text}</span> <span class=\"ts-subtle\">({lang})</span>",
        lang = esc(&t.language),
        text = esc(&t.text)
    )
}

/// The parts of a Wikibase time string.
#[derive(Debug, PartialEq, Eq)]
struct TimeParts {
    year: i64,
    month: u32,
    day: u32,
}

fn parse_time(s: &str) -> Option<TimeParts> {
    let body = s.trim_start_matches(['+', '-']);
    let (date, _) = body.split_once('T')?;
    let mut it = date.split('-');
    let year = it.next()?.parse::<i64>().ok()?;
    let month = it.next()?.parse::<u32>().ok()?;
    let day = it.next()?.parse::<u32>().ok()?;
    Some(TimeParts { year, month, day })
}

/// An English ordinal: 1st, 2nd, 3rd, 4th, 11th, 21st.
fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, r) if r != 11 => "st",
        (2, r) if r != 12 => "nd",
        (3, r) if r != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn num(s: &str) -> Option<f64> {
    s.trim_start_matches('+').parse().ok()
}

fn trim_float(f: f64) -> String {
    let s = format!("{f:.12}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// A decimal string with its sign dropped when positive and its whole part grouped by
/// thousands: `+3898747` → `3,898,747`, `-1234.50` → `-1,234.50`.
#[must_use]
pub fn group(amount: &str) -> String {
    let (sign, rest) = match amount.as_bytes().first() {
        Some(b'+') => ("", &amount[1..]),
        Some(b'-') => ("-", &amount[1..]),
        _ => ("", amount),
    };
    let (whole, frac) = rest
        .split_once('.')
        .map_or((rest, None), |(w, f)| (w, Some(f)));
    if !whole.bytes().all(|b| b.is_ascii_digit()) {
        return amount.to_string();
    }
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// Degrees as degrees, minutes and seconds to the value's precision.
fn dms(value: f64, precision: f64, (pos, neg): (char, char)) -> String {
    let hemi = if value < 0.0 { neg } else { pos };
    let v = value.abs();
    if precision >= 1.0 {
        return format!("{}°{hemi}", v.round());
    }
    if precision >= 1.0 / 60.0 - 1e-12 {
        let total = (v * 60.0).round();
        return format!("{}°{}′{hemi}", (total / 60.0).floor(), total % 60.0);
    }
    let seconds_precision = precision * 3600.0;
    // As many decimals of a second as the precision has, up to six.
    let places: i32 = (0..=6)
        .find(|d| seconds_precision * 10f64.powi(*d) >= 1.0 - 1e-9)
        .unwrap_or(6);
    let decimals = usize::try_from(places).unwrap_or(0);
    let scale = 10f64.powi(places);
    let total = (v * 3600.0 * scale).round() / scale;
    let d = (total / 3600.0).floor();
    let m = ((total - d * 3600.0) / 60.0).floor();
    let s = total - d * 3600.0 - m * 60.0;
    format!("{d}°{m}′{s:.decimals$}″{hemi}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(group("+3898747"), "3,898,747");
        assert_eq!(group("-1234.50"), "-1,234.50");
        assert_eq!(group("+12"), "12");
        assert_eq!(group("+0.5"), "0.5");
        assert_eq!(group("abc"), "abc");
    }

    #[test]
    fn times_and_ordinals() {
        assert_eq!(
            parse_time("+1952-03-11T00:00:00Z"),
            Some(TimeParts {
                year: 1952,
                month: 3,
                day: 11
            })
        );
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(22), "22nd");
        assert_eq!(ordinal(103), "103rd");
    }

    #[test]
    fn coordinates() {
        assert_eq!(dms(34.0522, 1.0 / 60.0, ('N', 'S')), "34°3′N");
        assert_eq!(dms(-118.2437, 1.0 / 60.0, ('E', 'W')), "118°15′W");
        assert_eq!(dms(51.4779, 1.0, ('N', 'S')), "51°N");
        assert_eq!(dms(48.8584, 1.0 / 3600.0, ('N', 'S')), "48°51′30″N");
        assert_eq!(dms(48.858_37, 0.000_01, ('N', 'S')), "48°51′30.13″N");
    }
}
