//! Instants, for expiries and validity: whole seconds since the Unix epoch, UTC.
//!
//! Records in this crate carry times as integers so that the CBOR of a record is one
//! integer and never a string whose format could drift (0006 §2). [`Timestamp`] prints as
//! RFC 3339 for messages and the API.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Seconds since 1970-01-01T00:00:00Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Seconds since the epoch.
    #[must_use]
    pub const fn as_secs(self) -> u64 {
        self.0
    }

    /// The civil date and time, UTC: (year, month, day, hour, minute, second).
    #[must_use]
    pub fn civil(self) -> (i64, u32, u32, u32, u32, u32) {
        // Howard Hinnant's days-to-civil.
        let secs = i64::try_from(self.0).unwrap_or(i64::MAX);
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (
            y,
            m as u32,
            d as u32,
            (rem / 3600) as u32,
            (rem % 3600 / 60) as u32,
            (rem % 60) as u32,
        )
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day, hour, minute, second) = self.civil();
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_rfc3339() {
        assert_eq!(Timestamp(0).to_string(), "1970-01-01T00:00:00Z");
        assert_eq!(Timestamp(951_782_400).to_string(), "2000-02-29T00:00:00Z");
        assert_eq!(Timestamp(1_790_000_000).to_string(), "2026-09-21T14:13:20Z");
        assert_eq!(serde_json::to_string(&Timestamp(5)).unwrap(), "5");
    }
}
