//! The `autoconfirmed` threshold (0016 §3): an account joins `autoconfirmed` once it is
//! old enough and has made enough edits, both from `site` configuration.
//!
//! The starting values are Wikimedia's: four days and ten edits
//! (`$wgAutoConfirmAge = 4 * 86400`, `$wgAutoConfirmCount = 10` on the Wikimedia wikis;
//! MediaWiki's own defaults are zero and zero, which makes every account autoconfirmed at
//! once). An edit filter may withhold autopromotion for a time (0030 §4, the
//! `blockautopromote` action AbuseFilter has), which is the third input.

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// The threshold, as `site` configuration carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoconfirmThreshold {
    /// Seconds since the account was created.
    pub age_secs: u64,
    /// Edits made.
    pub edits: u64,
}

impl Default for AutoconfirmThreshold {
    /// Wikimedia's values: four days, ten edits.
    fn default() -> Self {
        Self {
            age_secs: 4 * 86_400,
            edits: 10,
        }
    }
}

impl AutoconfirmThreshold {
    /// MediaWiki core's defaults: no threshold at all.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            age_secs: 0,
            edits: 0,
        }
    }

    /// Whether an account created at `registered_at` with `edit_count` edits meets the
    /// threshold at `now`, unless a filter withheld autopromotion until a later moment.
    #[must_use]
    pub fn met(
        self,
        registered_at: Timestamp,
        edit_count: u64,
        now: Timestamp,
        autopromote_blocked_until: Option<Timestamp>,
    ) -> bool {
        if autopromote_blocked_until.is_some_and(|until| now < until) {
            return false;
        }
        let age = now.as_micros().saturating_sub(registered_at.as_micros());
        age >= Timestamp::from_secs(self.age_secs).as_micros() && edit_count >= self.edits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikimedia_defaults() {
        let t = AutoconfirmThreshold::default();
        let day = Timestamp::from_secs(86_400).as_micros();
        let created = Timestamp(day);
        assert!(!t.met(created, 10, Timestamp(day * 4), None), "too young");
        assert!(
            !t.met(created, 9, Timestamp(day * 5), None),
            "too few edits"
        );
        assert!(t.met(created, 10, Timestamp(day * 5), None));
        assert!(
            !t.met(created, 10, Timestamp(day * 5), Some(Timestamp(day * 6))),
            "autopromotion withheld by a filter"
        );
        assert!(t.met(created, 10, Timestamp(day * 6), Some(Timestamp(day * 6))));
        assert!(AutoconfirmThreshold::none().met(created, 0, created, None));
        assert_eq!(
            serde_json::to_string(&t).unwrap(),
            r#"{"age_secs":345600,"edits":10}"#
        );
    }
}
