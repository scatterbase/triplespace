//! Time values as `xsd:dateTime`, as Wikibase's `DateTimeValueCleaner` and
//! `JulianDateTimeValueCleaner` write them in XSD 1.1 mode (wikibase-compat.md §5.5).
//!
//! - An unknown month or day (`00`), or one finer than the precision, becomes `01`.
//! - Years are written astronomically: the JSON's historical `-0044` (44 BCE) is `-0043`,
//!   and `-0001` (1 BCE) is `0000`.
//! - A Julian date at day precision or finer is converted to the proleptic Gregorian
//!   calendar; coarser Julian dates are written as they stand, so `+1500-00-00` at year
//!   precision is `1500-01-01`.

/// Wikidata's Julian calendar model.
pub const JULIAN: &str = "http://www.wikidata.org/entity/Q1985786";

/// Day precision (11).
const DAY: u8 = 11;
/// Month precision (10).
const MONTH: u8 = 10;

/// The `xsd:dateTime` lexical form of a Wikibase time string, or `None` when the string is
/// not one (`[+-]Y+-MM-DDThh:mm:ssZ`).
#[must_use]
pub fn xsd_date_time(time: &str, precision: u8, calendar: &str) -> Option<String> {
    let parsed = Parsed::parse(time)?;
    let julian = calendar == JULIAN || calendar.ends_with("/Q1985786");
    let mut p = parsed;
    if julian && precision >= DAY && p.month != 0 && p.day != 0 {
        p = p.julian_to_gregorian()?;
    }
    if precision < DAY || p.day == 0 {
        p.day = 1;
    }
    if precision < MONTH || p.month == 0 {
        p.month = 1;
    }
    Some(p.to_xsd())
}

/// A time split into fields, with the year historical (no year 0) as the JSON writes it.
#[derive(Debug, Clone, Copy)]
struct Parsed {
    negative: bool,
    /// The year's digits as a number; years past `i64` are not representable here.
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl Parsed {
    fn parse(s: &str) -> Option<Self> {
        let (negative, rest) = match s.as_bytes().first()? {
            b'+' => (false, &s[1..]),
            b'-' => (true, &s[1..]),
            _ => (false, s),
        };
        let rest = rest.strip_suffix('Z')?;
        let (date, clock) = rest.split_once('T')?;
        let mut d = date.splitn(3, '-');
        let year_digits = d.next()?;
        if year_digits.is_empty() || !year_digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let year: i64 = year_digits.parse().ok()?;
        let month = d.next()?.parse().ok()?;
        let day = d.next()?.parse().ok()?;
        let mut c = clock.splitn(3, ':');
        let hour = c.next()?.parse().ok()?;
        let minute = c.next()?.parse().ok()?;
        let second = c.next()?.parse().ok()?;
        if month > 12 || day > 31 {
            return None;
        }
        Some(Self {
            negative: negative && year != 0,
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }

    /// The astronomical year: 1 BCE is 0, 2 BCE is −1.
    const fn astronomical(&self) -> i64 {
        if self.negative {
            1 - self.year
        } else {
            self.year
        }
    }

    fn from_astronomical(year: i64, month: u32, day: u32, like: &Self) -> Self {
        let (negative, digits) = if year <= 0 {
            (true, 1 - year)
        } else {
            (false, year)
        };
        Self {
            negative,
            year: digits,
            month,
            day,
            ..*like
        }
    }

    /// The same day in the proleptic Gregorian calendar, by way of the Julian Day Number.
    fn julian_to_gregorian(self) -> Option<Self> {
        let jdn = julian_day_number(
            self.astronomical(),
            i64::from(self.month),
            i64::from(self.day),
        )?;
        let (year, month, day) = gregorian_from_jdn(jdn);
        Some(Self::from_astronomical(
            year,
            u32::try_from(month).ok()?,
            u32::try_from(day).ok()?,
            &self,
        ))
    }

    fn to_xsd(self) -> String {
        let year = self.astronomical();
        let sign = if year < 0 { "-" } else { "" };
        format!(
            "{sign}{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            year.unsigned_abs(),
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second
        )
    }
}

/// The Julian Day Number of a date in the Julian calendar, astronomical year.
fn julian_day_number(year: i64, month: i64, day: i64) -> Option<i64> {
    let shift = (14 - month).div_euclid(12);
    let years = year.checked_add(4800)?.checked_sub(shift)?;
    let months = month + 12 * shift - 3;
    Some(
        day + (153 * months + 2).div_euclid(5) + years.checked_mul(365)? + years.div_euclid(4)
            - 32083,
    )
}

/// The proleptic Gregorian date of a Julian Day Number, astronomical year (Richards).
fn gregorian_from_jdn(jdn: i64) -> (i64, i64, i64) {
    let days = jdn + 32044;
    let centuries = (4 * days + 3).div_euclid(146_097);
    let in_century = days - (146_097 * centuries).div_euclid(4);
    let years = (4 * in_century + 3).div_euclid(1461);
    let in_year = in_century - (1461 * years).div_euclid(4);
    let months = (5 * in_year + 2).div_euclid(153);
    let day = in_year - (153 * months + 2).div_euclid(5) + 1;
    let month = months + 3 - 12 * months.div_euclid(10);
    let year = 100 * centuries + years - 4800 + months.div_euclid(10);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREGORIAN: &str = "http://www.wikidata.org/entity/Q1985727";

    #[test]
    fn as_wikibase_writes_them() {
        assert_eq!(
            xsd_date_time("+1952-03-11T00:00:00Z", 11, GREGORIAN).unwrap(),
            "1952-03-11T00:00:00Z"
        );
        // Year precision, Julian: not converted (wikibase-compat.md §5.5).
        assert_eq!(
            xsd_date_time("+1500-00-00T00:00:00Z", 9, JULIAN).unwrap(),
            "1500-01-01T00:00:00Z"
        );
        // Month precision drops the day.
        assert_eq!(
            xsd_date_time("+2004-06-11T00:00:00Z", 10, GREGORIAN).unwrap(),
            "2004-06-01T00:00:00Z"
        );
        // Historical years become astronomical.
        assert_eq!(
            xsd_date_time("-0044-00-00T00:00:00Z", 9, GREGORIAN).unwrap(),
            "-0043-01-01T00:00:00Z"
        );
        assert_eq!(
            xsd_date_time("-0001-00-00T00:00:00Z", 9, GREGORIAN).unwrap(),
            "0000-01-01T00:00:00Z"
        );
        assert_eq!(
            xsd_date_time("+13798000000-00-00T00:00:00Z", 3, GREGORIAN).unwrap(),
            "13798000000-01-01T00:00:00Z"
        );
    }

    #[test]
    fn julian_days_convert() {
        // Shakespeare's death, 23 April 1616 Julian, was 3 May Gregorian.
        assert_eq!(
            xsd_date_time("+1616-04-23T00:00:00Z", 11, JULIAN).unwrap(),
            "1616-05-03T00:00:00Z"
        );
        // The Ides of March, 44 BCE Julian, is 13 March −43 (astronomical) Gregorian.
        assert_eq!(
            xsd_date_time("-0044-03-15T00:00:00Z", 11, JULIAN).unwrap(),
            "-0043-03-13T00:00:00Z"
        );
    }

    #[test]
    fn rejects_what_is_not_a_time() {
        assert!(xsd_date_time("1952", 9, GREGORIAN).is_none());
        assert!(xsd_date_time("+1952-13-01T00:00:00Z", 11, GREGORIAN).is_none());
    }
}
