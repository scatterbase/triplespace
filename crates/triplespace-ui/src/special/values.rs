//! A typed cell from text (19 §3.1; 0104 §11): the `datavalue` a property of a given data
//! type gets from what a person typed into a form, as `Special:NewItem`'s `statement=`
//! and `Special:CreateManyEntities`'s cells take it. Strings and IDs are taken as written; a
//! quantity is a number (`12`, `-3.5`); a time is a year, a month or a day (`1969`,
//! `1969-07`, `1969-07-20`, a leading `-` for BCE), at that precision in the proleptic
//! Gregorian calendar; monolingual text is `text@lang`; a coordinate is `lat, lon` in
//! decimal degrees on Earth. Anything else waits for `wbparsevalue`, and is refused with
//! the type named.

use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::{DataType, ValueType};
use serde_json::{Value, json};

use crate::i18n::Messages;
use crate::special::new_entity::datatype_name;

/// The entity type an ID's form implies, where it is an ID and implies one.
#[must_use]
pub fn implied_type(id: &str) -> Option<String> {
    EntityId::parse(id)
        .ok()?
        .implied_entity_type()
        .map(str::to_string)
}

/// The `datavalue` for `value` of a property of type `datatype`, or why there is none,
/// as a message. `raw` is how the person wrote the whole statement, for the message.
pub fn datavalue(m: &Messages, datatype: &str, value: &str, raw: &str) -> Result<Value, String> {
    let dt = DataType::parse(datatype);
    let bad = || {
        m.with(
            "ts-new-statement-bad-value",
            &[value, &datatype_name(m, datatype)],
        )
    };
    match dt.value_type() {
        Some(ValueType::String) => Ok(json!({"type": "string", "value": value})),
        Some(ValueType::EntityId) => {
            let want = dt.entity_type().unwrap_or("item");
            // A keyed type's value may be given without its prefix: `example.org` for
            // `domain:example.org` (0009 §2).
            let keyed = matches!(want, "domain" | "keyword" | "notation")
                .then(|| format!("{want}:{value}"));
            let id = std::iter::once(value)
                .chain(keyed.as_deref())
                .find(|c| implied_type(c).as_deref() == Some(want))
                .ok_or_else(bad)?;
            Ok(json!({"type": "wikibase-entityid", "value": {"entity-type": want, "id": id}}))
        }
        Some(ValueType::Quantity) => {
            let amount = quantity(value).ok_or_else(bad)?;
            Ok(json!({"type": "quantity", "value": {"amount": amount, "unit": "1"}}))
        }
        Some(ValueType::Time) => {
            let (time, precision) = time(value).ok_or_else(bad)?;
            Ok(json!({"type": "time", "value": {
                "time": time, "timezone": 0, "before": 0, "after": 0,
                "precision": precision,
                "calendarmodel": "http://www.wikidata.org/entity/Q1985727",
            }}))
        }
        Some(ValueType::MonolingualText) => {
            let (text, lang) = value.rsplit_once('@').ok_or_else(bad)?;
            let lang = crate::i18n::valid_code(lang).ok_or_else(bad)?;
            let text = text.trim();
            if text.is_empty() {
                return Err(bad());
            }
            Ok(json!({"type": "monolingualtext", "value": {"text": text, "language": lang}}))
        }
        Some(ValueType::GlobeCoordinate) => {
            let (lat, lon) = coordinate(value).ok_or_else(bad)?;
            Ok(json!({"type": "globecoordinate", "value": {
                "latitude": lat, "longitude": lon, "precision": 0.0001,
                "globe": "http://www.wikidata.org/entity/Q2",
            }}))
        }
        _ => Err(m.with(
            "ts-new-statement-unsupported",
            &[&datatype_name(m, datatype), raw],
        )),
    }
}

/// A `datavalue` in the plain form [`datavalue`] reads (19 §3.1), where it has one: what
/// a table's text and a Table page's cells show. `None` for a value with no plain form
/// (a time finer than a day or coarser than a year, a quantity with bounds or a unit).
#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn plain(dv: &Value) -> Option<String> {
    let v = &dv["value"];
    match dv["type"].as_str()? {
        "string" => v.as_str().map(str::to_string),
        "wikibase-entityid" => v["id"].as_str().map(str::to_string),
        "quantity" => {
            if v["unit"].as_str().is_some_and(|u| u != "1")
                || v.get("upperBound").is_some_and(|b| !b.is_null())
            {
                return None;
            }
            let amount = v["amount"].as_str()?;
            Some(amount.strip_prefix('+').unwrap_or(amount).to_string())
        }
        "time" => {
            let t = v["time"].as_str()?;
            let (sign, rest) = match t.strip_prefix('-') {
                Some(r) => ("-", r),
                None => ("", t.strip_prefix('+').unwrap_or(t)),
            };
            let date = rest.split('T').next()?;
            let mut parts = date.split('-');
            let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
            let y = y.trim_start_matches('0');
            let y = if y.is_empty() { "0" } else { y };
            match v["precision"].as_u64()? {
                9 => Some(format!("{sign}{y}")),
                10 => Some(format!("{sign}{y}-{m}")),
                11 => Some(format!("{sign}{y}-{m}-{d}")),
                _ => None,
            }
        }
        "monolingualtext" => Some(format!(
            "{}@{}",
            v["text"].as_str()?,
            v["language"].as_str()?
        )),
        "globecoordinate" => Some(format!(
            "{}, {}",
            v["latitude"].as_f64()?,
            v["longitude"].as_f64()?
        )),
        _ => None,
    }
}

/// A quantity's `amount`: a decimal number with its sign made explicit.
fn quantity(value: &str) -> Option<String> {
    let v = value.trim().replace(',', "");
    let (sign, digits) = match v.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("+", v.strip_prefix('+').unwrap_or(&v)),
    };
    let mut parts = digits.splitn(2, '.');
    let whole = parts.next()?;
    let frac = parts.next();
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(f) = frac
        && (f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(format!("{sign}{digits}"))
}

/// A time as Wikibase writes it, with its precision: 9 for a year, 10 for a month, 11
/// for a day.
fn time(value: &str) -> Option<(String, u8)> {
    let v = value.trim();
    let (sign, rest) = match v.strip_prefix('-') {
        Some(r) => ('-', r),
        None => ('+', v.strip_prefix('+').unwrap_or(v)),
    };
    let parts: Vec<&str> = rest.split('-').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let year: u32 = parts[0].parse().ok()?;
    let month: u32 = parts.get(1).map_or(Ok(0), |p| p.parse()).ok()?;
    let day: u32 = parts.get(2).map_or(Ok(0), |p| p.parse()).ok()?;
    if parts.len() >= 2 && !(1..=12).contains(&month) {
        return None;
    }
    if parts.len() == 3 && !(1..=31).contains(&day) {
        return None;
    }
    let precision = match parts.len() {
        1 => 9,
        2 => 10,
        _ => 11,
    };
    Some((
        format!("{sign}{year:04}-{month:02}-{day:02}T00:00:00Z"),
        precision,
    ))
}

/// A coordinate from `lat, lon` in decimal degrees.
fn coordinate(value: &str) -> Option<(f64, f64)> {
    let (a, b) = value.split_once(',')?;
    let lat: f64 = a.trim().parse().ok()?;
    let lon: f64 = b.trim().parse().ok()?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0106 §1: what the text can say reads back as it was written.
    #[test]
    fn plain_forms_read_back() {
        let m = Messages::for_language("en");
        for (dt, text) in [
            ("string", "a|b"),
            ("url", "https://example.org/a"),
            ("wikibase-item", "Q5"),
            ("quantity", "12"),
            ("quantity", "-3.5"),
            ("time", "1969"),
            ("time", "1969-07"),
            ("time", "1969-07-20"),
            ("time", "-44"),
            ("monolingualtext", "Hallo@de"),
            ("globe-coordinate", "45.5, -122.6"),
        ] {
            let dv = datavalue(&m, dt, text, text).unwrap();
            assert_eq!(plain(&dv).as_deref(), Some(text), "{dt}");
        }
        assert_eq!(
            plain(
                &json!({"type": "time", "value": {"time": "+1969-00-00T00:00:00Z", "precision": 7}})
            ),
            None
        );
    }

    #[test]
    fn typed_values_parse() {
        let m = Messages::for_language("en");
        assert_eq!(
            datavalue(&m, "quantity", "12.5", "P1:12.5").unwrap()["value"]["amount"],
            "+12.5"
        );
        assert_eq!(
            datavalue(&m, "quantity", "-3", "P1:-3").unwrap()["value"]["amount"],
            "-3"
        );
        assert!(datavalue(&m, "quantity", "twelve", "P1:twelve").is_err());
        let t = datavalue(&m, "time", "1969-07-20", "P1:1969-07-20").unwrap();
        assert_eq!(t["value"]["time"], "+1969-07-20T00:00:00Z");
        assert_eq!(t["value"]["precision"], 11);
        assert_eq!(
            datavalue(&m, "time", "1969", "P1:1969").unwrap()["value"]["precision"],
            9
        );
        assert_eq!(
            datavalue(&m, "time", "-44-03", "P1:-44-03").unwrap()["value"]["time"],
            "-0044-03-00T00:00:00Z"
        );
        assert!(datavalue(&m, "time", "1969-13", "x").is_err());
        let mt = datavalue(&m, "monolingualtext", "Bonjour@fr", "x").unwrap();
        assert_eq!(mt["value"]["language"], "fr");
        assert_eq!(mt["value"]["text"], "Bonjour");
        let c = datavalue(&m, "globe-coordinate", "45.5, -122.6", "x").unwrap();
        assert_eq!(c["value"]["latitude"], 45.5);
        assert!(datavalue(&m, "globe-coordinate", "95, 0", "x").is_err());
        assert_eq!(
            datavalue(&m, "wikibase-item", "Q6", "P1:Q6").unwrap()["value"]["id"],
            "Q6"
        );
        assert!(datavalue(&m, "wikibase-item", "P6", "P1:P6").is_err());
        assert_eq!(
            datavalue(&m, "commonsMedia", "x.jpg", "P1:x.jpg").unwrap()["type"],
            "string"
        );
        assert!(datavalue(&m, "geo-shape", "Data:x.map", "P1:Data:x.map").is_ok());
    }
}
