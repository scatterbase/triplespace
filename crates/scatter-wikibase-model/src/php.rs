//! The pieces of PHP's `serialize()`, `json_encode()` and float-to-string conversion that
//! Wikibase's hashes are computed over (see [`crate::hash`]).
//!
//! Only what the hashes need is here, plus [`empty_array_as_map`] for reading what
//! `json_encode()` wrote. The formats are PHP 7.1+ with the default settings
//! Wikibase runs under: `serialize_precision = -1` (shortest round-trip digits) for
//! `serialize()` and `json_encode()`, `precision = 14` for `(string)$float`, and
//! `json_encode()` with no flags, so `/` is written `\/` and non-ASCII as `\uXXXX`.

use std::fmt::Write as _;
use std::marker::PhantomData;

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// Deserializes a JSON object into a map type, also accepting `[]` as the empty map.
///
/// PHP has one array type, so `json_encode()` of an empty associative array is `[]`:
/// revision text in XML dumps and older API output carry `"claims":[]`, `"labels":[]`,
/// `"sitelinks":[]` and so on wherever a map is empty. Wikibase's own readers accept the
/// same. A non-empty array is still an error.
pub(crate) fn empty_array_as_map<'de, D, M>(d: D) -> Result<M, D::Error>
where
    D: Deserializer<'de>,
    M: Deserialize<'de> + Default,
{
    struct MapOrEmpty<M>(PhantomData<M>);

    impl<'de, M: Deserialize<'de> + Default> Visitor<'de> for MapOrEmpty<M> {
        type Value = M;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map, or an empty array")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<M, A::Error> {
            if seq.next_element::<IgnoredAny>()?.is_some() {
                return Err(de::Error::invalid_type(de::Unexpected::Seq, &self));
            }
            Ok(M::default())
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<M, A::Error> {
            M::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }

    d.deserialize_any(MapOrEmpty(PhantomData))
}

/// `serialize($string)`: `s:<byte length>:"<raw bytes>";`.
pub(crate) fn serialize_str(out: &mut String, s: &str) {
    let _ = write!(out, "s:{}:\"{s}\";", s.len());
}

/// `serialize($int)`.
pub(crate) fn serialize_int(out: &mut String, n: i64) {
    let _ = write!(out, "i:{n};");
}

/// `serialize($float)`: shortest round-trip digits, `E` for the exponent.
pub(crate) fn serialize_float(out: &mut String, f: f64) {
    out.push_str("d:");
    gcvt(out, f, None, 'E');
    out.push(';');
}

/// The `C:<len>:"<class>":<len>:{<data>}` wrapper of the pre-7.4 `Serializable` format,
/// which Wikibase and DataValues reproduce by hand in `getSerializationForHash()` so that
/// hashes did not change when they moved to `__serialize()`.
pub(crate) fn class_wrapper(class: &str, data: &str) -> String {
    format!("C:{}:\"{class}\":{}:{{{data}}}", class.len(), data.len())
}

/// `serialize()` of a value as `json_decode($json, true)` would have produced it: objects
/// become arrays whose canonical-integer keys are integers, integral-looking floats stay
/// floats, and numbers too large for an integer become floats.
pub(crate) fn serialize_json(out: &mut String, v: &serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Null => out.push_str("N;"),
        Value::Bool(b) => out.push_str(if *b { "b:1;" } else { "b:0;" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serialize_int(out, i);
            } else {
                serialize_float(out, n.as_f64().unwrap_or(f64::NAN));
            }
        }
        Value::String(s) => serialize_str(out, s),
        Value::Array(items) => {
            let _ = write!(out, "a:{}:{{", items.len());
            for (i, item) in items.iter().enumerate() {
                serialize_int(out, i64::try_from(i).unwrap_or(i64::MAX));
                serialize_json(out, item);
            }
            out.push('}');
        }
        Value::Object(map) => {
            let _ = write!(out, "a:{}:{{", map.len());
            for (k, item) in map {
                match php_array_key(k) {
                    Some(i) => serialize_int(out, i),
                    None => serialize_str(out, k),
                }
                serialize_json(out, item);
            }
            out.push('}');
        }
    }
}

/// PHP turns a string array key that is a canonical decimal integer (`"7"`, `"-3"`, not
/// `"07"` or `"+3"`) into an integer key.
fn php_array_key(k: &str) -> Option<i64> {
    let digits = k.strip_prefix('-').unwrap_or(k);
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || (k.starts_with('-') && digits == "0")
    {
        return None;
    }
    k.parse().ok()
}

/// `json_encode($string)` with no flags.
pub(crate) fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || !c.is_ascii() => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `json_encode($float)` with no flags: shortest round-trip digits, `e` for the exponent,
/// and no `.0` on an integral value (that needs `JSON_PRESERVE_ZERO_FRACTION`).
pub(crate) fn json_float(out: &mut String, f: f64) {
    gcvt(out, f, None, 'e');
}

/// `(string)$float`, which is `%.14G`: fourteen significant digits, `E` for the exponent.
pub(crate) fn float_to_string(out: &mut String, f: f64) {
    gcvt(out, f, Some(14), 'E');
}

/// PHP's `php_gcvt()`. `ndigit` is the number of significant digits to round to, or
/// `None` for the shortest digits that round-trip (`serialize_precision = -1`), which
/// switches to exponential form past 17 digits. Exponential form is `d.ddde±X` with at
/// least one digit after the point and no zero padding of the exponent: `1.0e-5`,
/// `1.0e+25`.
pub(crate) fn gcvt(out: &mut String, value: f64, ndigit: Option<usize>, exp_char: char) {
    if value.is_nan() {
        out.push_str("NAN");
        return;
    }
    if value.is_infinite() {
        out.push_str(if value < 0.0 { "-INF" } else { "INF" });
        return;
    }
    if value.is_sign_negative() {
        out.push('-');
    }
    let (digits, decpt) = dtoa(value.abs(), ndigit);
    let ndigit = ndigit.unwrap_or(17);
    let digits = digits.as_bytes();
    let exponential = if decpt < 0 {
        decpt < -3
    } else {
        decpt > i64::try_from(ndigit).unwrap_or(i64::MAX)
    };
    if exponential {
        out.push(digits[0] as char);
        out.push('.');
        if digits.len() == 1 {
            out.push('0');
        } else {
            out.push_str(std::str::from_utf8(&digits[1..]).expect("ascii"));
        }
        out.push(exp_char);
        let e = decpt - 1;
        out.push(if e < 0 { '-' } else { '+' });
        let _ = write!(out, "{}", e.abs());
    } else if decpt < 0 {
        out.push_str("0.");
        for _ in decpt..0 {
            out.push('0');
        }
        out.push_str(std::str::from_utf8(digits).expect("ascii"));
    } else {
        let decpt = usize::try_from(decpt).expect("non-negative");
        for i in 0..decpt {
            out.push(digits.get(i).map_or('0', |b| *b as char));
        }
        if digits.len() > decpt {
            if decpt == 0 {
                out.push('0');
            }
            out.push('.');
            out.push_str(std::str::from_utf8(&digits[decpt..]).expect("ascii"));
        }
    }
}

/// `zend_dtoa()` in mode 0 (`ndigit` `None`: shortest round-trip) or mode 2 (`ndigit`
/// significant digits, correctly rounded): the significant digits with trailing zeros
/// removed, and the position of the decimal point relative to their start.
fn dtoa(value: f64, ndigit: Option<usize>) -> (String, i64) {
    let formatted = match ndigit {
        None => format!("{value:e}"),
        Some(n) => format!("{value:.*e}", n.saturating_sub(1)),
    };
    let (mantissa, exp) = formatted.split_once('e').expect("exponent form");
    let exp: i64 = exp.parse().expect("exponent");
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    (digits, exp + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(f: impl Fn(&mut String)) -> String {
        let mut out = String::new();
        f(&mut out);
        out
    }

    #[test]
    fn json_floats_match_php() {
        // php -r 'echo json_encode([...]);' with PHP 8.4 defaults.
        let cases = [
            (51.0, "51"),
            (51.5, "51.5"),
            (0.0001, "0.0001"),
            (0.00001, "1.0e-5"),
            (1e17, "1.0e+17"),
            (1e16, "10000000000000000"),
            (1e25, "1.0e+25"),
            (-0.1275, "-0.1275"),
            (1.5e-7, "1.5e-7"),
            (123_456_789.123_456_79, "123456789.12345679"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-0.0, "-0"),
            (100.0, "100"),
            (0.5, "0.5"),
        ];
        for (f, want) in cases {
            assert_eq!(s(|o| json_float(o, f)), want, "{f:?}");
        }
    }

    #[test]
    fn string_casts_match_php() {
        // php -r 'foreach ([...] as $f) echo "$f|";'
        let cases = [
            (51.0, "51"),
            (51.5, "51.5"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (1e17, "1.0E+17"),
            (1e13, "10000000000000"),
            (1e14, "1.0E+14"),
            (1e15, "1.0E+15"),
            (1e25, "1.0E+25"),
            (-0.1275, "-0.1275"),
            (123_456_789.123_456_79, "123456789.12346"),
            (0.1 + 0.2, "0.3"),
            (-0.0, "-0"),
            (1_234_567_890_123_456.0, "1.2345678901235E+15"),
            (1.0E-10, "1.0E-10"),
            (5e-5, "5.0E-5"),
            (99_999_999_999_999.9, "1.0E+14"),
            (999_999_999_999_999.0, "1.0E+15"),
        ];
        for (f, want) in cases {
            assert_eq!(s(|o| float_to_string(o, f)), want, "{f:?}");
        }
    }

    #[test]
    fn json_strings_match_php() {
        assert_eq!(
            s(|o| json_str(o, "Douglas Noël Adams")),
            "\"Douglas No\\u00ebl Adams\""
        );
        assert_eq!(s(|o| json_str(o, "a/b")), r#""a\/b""#);
        assert_eq!(s(|o| json_str(o, "😀")), "\"\\ud83d\\ude00\"");
        assert_eq!(
            s(|o| json_str(o, "<>&'\"\\\u{1}\u{7f}\t")),
            "\"<>&'\\\"\\\\\\u0001\u{7f}\\t\""
        );
    }

    #[test]
    fn serialize_matches_php() {
        // php -r 'echo serialize([1.5, true, null, "x", 2, ["a"=>1, "2"=>3]]);'
        // (`serde_json::Map` keeps keys sorted, so the object here is given in that order;
        // PHP would keep the JSON's own order, which an unknown value has already lost.)
        let v = serde_json::json!([1.5, true, null, "x", 2, {"2": 3, "a": 1}]);
        assert_eq!(
            s(|o| serialize_json(o, &v)),
            r#"a:6:{i:0;d:1.5;i:1;b:1;i:2;N;i:3;s:1:"x";i:4;i:2;i:5;a:2:{i:2;i:3;s:1:"a";i:1;}}"#
        );
        // php -r 'echo serialize(json_decode("{\"1\":2,\"a\":[1,2.0],\"b\":1e2}", true));'
        let v: serde_json::Value = serde_json::from_str(r#"{"1":2,"a":[1,2.0],"b":1e2}"#).unwrap();
        assert_eq!(
            s(|o| serialize_json(o, &v)),
            r#"a:3:{i:1;i:2;s:1:"a";a:2:{i:0;i:1;i:1;d:2;}s:1:"b";d:100;}"#
        );
        assert_eq!(s(|o| serialize_float(o, 1e25)), "d:1.0E+25;");
        assert_eq!(s(|o| serialize_str(o, "Noël")), "s:5:\"Noël\";");
        for (k, want) in [
            ("7", Some(7)),
            ("-3", Some(-3)),
            ("0", Some(0)),
            ("07", None),
            ("-0", None),
            ("+3", None),
            ("", None),
            ("a", None),
        ] {
            assert_eq!(php_array_key(k), want, "{k:?}");
        }
    }

    #[test]
    fn empty_array_reads_as_empty_map() {
        use std::collections::BTreeMap;

        #[derive(serde::Deserialize)]
        struct W {
            #[serde(default, deserialize_with = "empty_array_as_map")]
            m: BTreeMap<String, u32>,
        }
        let w: W = serde_json::from_str(r#"{"m":[]}"#).unwrap();
        assert!(w.m.is_empty());
        let w: W = serde_json::from_str(r#"{"m":{"a":1}}"#).unwrap();
        assert_eq!(w.m.get("a"), Some(&1));
        let w: W = serde_json::from_str("{}").unwrap();
        assert!(w.m.is_empty());
        assert!(serde_json::from_str::<W>(r#"{"m":[1]}"#).is_err());
        assert!(serde_json::from_str::<W>(r#"{"m":"x"}"#).is_err());
    }

    #[test]
    fn class_wrapper_counts_bytes() {
        assert_eq!(
            class_wrapper("DataValues\\StringValue", "Noël"),
            "C:22:\"DataValues\\StringValue\":5:{Noël}"
        );
    }
}
