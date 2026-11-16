//! Ordering values, for a Table's rows and the time axis of a Timeline or Series (0003 §3).
//!
//! Values of one type compare by what they mean: times by date (historical numbering:
//! `-0044` is before `+0001`), quantities by amount, entities by provider then number,
//! numeric strings as numbers (0003 A10), other texts by their characters. Values of different types compare by type, and "unknown
//! value" and "no value" sort after every value.

use std::cmp::Ordering;

use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::DataValue;

/// A time string's position on the axis: (year, month, day), with 0 for an unknown
/// month or day, as Wikibase writes them.
#[must_use]
pub fn time_key(time: &str) -> Option<(i64, u32, u32)> {
    let negative = time.starts_with('-');
    let body = time.trim_start_matches(['+', '-']);
    let (date, _) = body.split_once('T')?;
    let mut it = date.split('-');
    let year: i64 = it.next()?.parse().ok()?;
    let month: u32 = it.next()?.parse().ok()?;
    let day: u32 = it.next()?.parse().ok()?;
    Some((if negative { -year } else { year }, month, day))
}

/// A snak's time, where its value is one.
#[must_use]
pub fn snak_time(snak: &Snak) -> Option<(i64, u32, u32)> {
    match &snak.kind {
        SnakKind::Value(DataValue::Time(t)) => time_key(&t.time),
        _ => None,
    }
}

/// A quantity's amount as a number.
#[must_use]
pub fn amount(text: &str) -> Option<f64> {
    text.trim_start_matches('+').parse().ok()
}

/// An entity ID's sort key: its prefix (provider code and type letter, or keyed type),
/// then its number where it has one, then the ID.
fn id_key(id: &str) -> (String, u64, String) {
    let digits = id.trim_start_matches(|c: char| !c.is_ascii_digit());
    let prefix = &id[..id.len() - digits.len()];
    (
        prefix.to_string(),
        digits.parse().unwrap_or(u64::MAX),
        id.to_string(),
    )
}

fn rank_of(v: &DataValue) -> u8 {
    match v {
        DataValue::Time(_) => 0,
        DataValue::Quantity(_) => 1,
        DataValue::EntityId(_) => 2,
        DataValue::MonolingualText(_) => 3,
        DataValue::String(_) => 4,
        DataValue::GlobeCoordinate(_) => 5,
        _ => 6,
    }
}

/// Compares two strings, numerically where both are numbers (series ordinals: "2" before
/// "10"), and by their characters otherwise.
#[must_use]
pub fn cmp_strings(left: &str, right: &str) -> Ordering {
    match (left.trim().parse::<f64>(), right.trim().parse::<f64>()) {
        (Ok(l), Ok(r)) => l.total_cmp(&r).then_with(|| left.cmp(right)),
        _ => left.cmp(right),
    }
}

/// Compares two data values.
#[must_use]
pub fn cmp_values(left: &DataValue, right: &DataValue) -> Ordering {
    match (left, right) {
        (DataValue::Time(l), DataValue::Time(r)) => time_key(&l.time)
            .cmp(&time_key(&r.time))
            .then_with(|| l.time.cmp(&r.time)),
        (DataValue::Quantity(l), DataValue::Quantity(r)) => {
            match (amount(&l.amount), amount(&r.amount)) {
                (Some(la), Some(ra)) => la.total_cmp(&ra),
                _ => l.amount.cmp(&r.amount),
            }
            .then_with(|| l.unit.cmp(&r.unit))
        }
        (DataValue::EntityId(l), DataValue::EntityId(r)) => {
            id_key(l.id.as_str()).cmp(&id_key(r.id.as_str()))
        }
        (DataValue::MonolingualText(l), DataValue::MonolingualText(r)) => l
            .text
            .cmp(&r.text)
            .then_with(|| l.language.cmp(&r.language)),
        (DataValue::String(l), DataValue::String(r)) => cmp_strings(l, r),
        (DataValue::GlobeCoordinate(l), DataValue::GlobeCoordinate(r)) => l
            .latitude
            .total_cmp(&r.latitude)
            .then_with(|| l.longitude.total_cmp(&r.longitude)),
        _ => rank_of(left).cmp(&rank_of(right)),
    }
}

/// Compares two snaks: values by [`cmp_values`], then "unknown value", then "no value".
#[must_use]
pub fn cmp_snaks(a: &Snak, b: &Snak) -> Ordering {
    match (&a.kind, &b.kind) {
        (SnakKind::Value(x), SnakKind::Value(y)) => cmp_values(x, y),
        (SnakKind::Value(_), _) | (SnakKind::SomeValue, SnakKind::NoValue) => Ordering::Less,
        (_, SnakKind::Value(_)) | (SnakKind::NoValue, SnakKind::SomeValue) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// Compares two optional snaks; a missing one sorts last.
#[must_use]
pub fn cmp_cells(a: Option<&Snak>, b: Option<&Snak>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => cmp_snaks(x, y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_use_historical_numbering() {
        assert!(time_key("-0044-03-15T00:00:00Z") < time_key("+0001-00-00T00:00:00Z"));
        assert!(time_key("+1999-12-31T00:00:00Z") < time_key("+2000-00-00T00:00:00Z"));
        assert_eq!(time_key("+2020-00-00T00:00:00Z"), Some((2020, 0, 0)));
        assert_eq!(time_key("nonsense"), None);
    }

    #[test]
    fn numeric_strings_compare_as_numbers() {
        let mut v = vec!["10", "2", "1", "b", "a"];
        v.sort_by(|a, b| cmp_strings(a, b));
        assert_eq!(v, vec!["1", "2", "10", "a", "b"]);
    }

    #[test]
    fn ids_sort_by_number() {
        assert!(id_key("Q9") < id_key("Q10"));
        assert!(id_key("P10") < id_key("Q2"));
        assert!(id_key("WDQ5") > id_key("Q500"));
    }
}
