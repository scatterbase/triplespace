//! Instants: microseconds since the Unix epoch, UTC, the unit of a record header's
//! `appended_at` (0013 §5, header field 3).
//!
//! Records in this crate carry times as integers so that the CBOR of a record is one
//! integer and never a string whose format could drift (0006 §2), and in the header's unit
//! so that "as of the record's `appended_at`" (0023 §6) compares like with like. Nothing
//! here orders records by time; the log's order is the slice order the folds take, and a
//! timestamp only answers "as of when". [`Timestamp`] prints as RFC 3339 for messages and
//! the API.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Microseconds since 1970-01-01T00:00:00Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// From whole seconds since the epoch.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(1_000_000))
    }

    /// Microseconds since the epoch: the header's own unit.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// Whole seconds since the epoch, truncated.
    #[must_use]
    pub const fn as_secs(self) -> u64 {
        self.0 / 1_000_000
    }

    /// The microseconds within the second.
    #[must_use]
    pub const fn subsec_micros(self) -> u32 {
        (self.0 % 1_000_000) as u32
    }

    /// The civil date and time, UTC: (year, month, day, hour, minute, second).
    #[must_use]
    pub fn civil(self) -> (i64, u32, u32, u32, u32, u32) {
        // Howard Hinnant's days-to-civil.
        let secs = i64::try_from(self.as_secs()).unwrap_or(i64::MAX);
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
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"
        )?;
        let micros = self.subsec_micros();
        if micros != 0 {
            write!(f, ".{micros:06}")?;
        }
        f.write_str("Z")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_rfc3339() {
        assert_eq!(Timestamp(0).to_string(), "1970-01-01T00:00:00Z");
        assert_eq!(
            Timestamp::from_secs(951_782_400).to_string(),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(
            Timestamp::from_secs(1_790_000_000).to_string(),
            "2026-09-21T14:13:20Z"
        );
        assert_eq!(
            Timestamp(1_790_000_000_000_042).to_string(),
            "2026-09-21T14:13:20.000042Z"
        );
        assert_eq!(serde_json::to_string(&Timestamp(5)).unwrap(), "5");
        let t = Timestamp(1_790_000_000_000_042);
        assert_eq!(
            (t.as_secs(), t.subsec_micros(), t.as_micros()),
            (1_790_000_000, 42, t.0)
        );
        // Two records in one second keep their order.
        assert!(Timestamp(1_790_000_000_000_001) < Timestamp(1_790_000_000_000_002));
    }
}
