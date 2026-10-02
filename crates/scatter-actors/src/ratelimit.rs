//! Rate-limit policy (0024 §5): `site` configuration by action class and group, in the
//! shape of MediaWiki's `$wgRateLimits`.
//!
//! The most permissive limit among an actor's groups applies; `noratelimit` exempts an
//! actor from every class but `job`. Counting is the server's (Valkey); this module
//! decides which limit a request is under.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::evaluate::Effective;

/// A count per window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limit {
    /// How many.
    pub count: u64,
    /// Per this many seconds; `0` for a concurrency limit (`stream`).
    pub window_secs: u64,
}

impl Limit {
    /// `count` per minute.
    #[must_use]
    pub const fn per_minute(count: u64) -> Self {
        Self {
            count,
            window_secs: 60,
        }
    }

    /// `count` per hour.
    #[must_use]
    pub const fn per_hour(count: u64) -> Self {
        Self {
            count,
            window_secs: 3600,
        }
    }

    /// `count` at once.
    #[must_use]
    pub const fn concurrent(count: u64) -> Self {
        Self {
            count,
            window_secs: 0,
        }
    }

    /// Permissiveness, for choosing among groups: the rate, or the count for a
    /// concurrency limit.
    fn rate(self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        if self.window_secs == 0 {
            self.count as f64
        } else {
            self.count as f64 / self.window_secs as f64
        }
    }
}

/// The action class `noratelimit` does not exempt.
pub const JOB: &str = "job";
/// The right that exempts from every class but `job`.
pub const NORATELIMIT: &str = "noratelimit";

/// The policy: for each class, for each group, a limit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RateLimitPolicy {
    /// `class → group → limit`.
    pub rows: BTreeMap<String, BTreeMap<String, Limit>>,
}

impl RateLimitPolicy {
    /// The starting values of 0024 §5 for `user` and `bot`, `sysop`'s `job` row, and
    /// rows for `universe` and `temp` below `user`'s, as the ADR asks (MediaWiki's
    /// anonymous defaults).
    #[must_use]
    pub fn defaults() -> Self {
        let mut p = Self::default();
        let mut row = |class: &str, user: Limit, bot: Limit, anon: Option<Limit>| {
            let r = p.rows.entry(class.to_string()).or_default();
            r.insert("user".into(), user);
            r.insert("bot".into(), bot);
            if let Some(a) = anon {
                r.insert("universe".into(), a);
                r.insert("temp".into(), a);
            }
        };
        let m = Limit::per_minute;
        let h = Limit::per_hour;
        let c = Limit::concurrent;
        row("edit", m(90), m(3000), Some(m(8)));
        row("create", m(30), m(1000), Some(m(8)));
        row("move", m(8), m(100), Some(m(2)));
        row("link", m(30), m(1000), Some(m(8)));
        row("job", h(0), h(10), Some(h(0)));
        row("read", m(5000), m(50_000), Some(m(1000)));
        row("stream", c(5), c(20), Some(c(2)));
        row("atom", h(60), h(600), Some(h(30)));
        row("upstream", m(30), m(30), Some(m(5)));
        row("notify", h(5), h(5), None);
        row("account", h(10), h(u64::MAX), None);
        row("parse", m(60), m(600), Some(m(10)));
        row("export", h(30), h(300), Some(h(5)));
        row("fork", h(5), h(100), None);
        p.rows
            .entry(JOB.to_string())
            .or_default()
            .insert("sysop".into(), h(10));
        p
    }

    /// Sets one row, as `ts-config` does.
    pub fn set(&mut self, class: &str, group: &str, limit: Limit) {
        self.rows
            .entry(class.to_string())
            .or_default()
            .insert(group.to_string(), limit);
    }

    /// The limit a principal is under for `class`: `None` means unlimited, because the
    /// actor holds `noratelimit` (any class but `job`) or no row names any of its groups.
    #[must_use]
    pub fn limit_for(&self, class: &str, effective: &Effective) -> Option<Limit> {
        if class != JOB && effective.holds(NORATELIMIT) {
            return None;
        }
        let rows = self.rows.get(class)?;
        effective
            .groups
            .iter()
            .filter_map(|g| rows.get(g))
            .copied()
            .max_by(|a, b| a.rate().total_cmp(&b.rate()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluate::{Principal, evaluate};
    use crate::grant::GrantRegistry;
    use crate::group::GroupRegistry;

    fn eff(groups: &[&str]) -> Effective {
        evaluate(
            &Principal::registered(groups.iter().map(|s| (*s).to_string())),
            GroupRegistry::default_registry(),
            GrantRegistry::default_registry(),
        )
    }

    #[test]
    fn most_permissive_row_applies() {
        let p = RateLimitPolicy::defaults();
        assert_eq!(p.limit_for("edit", &eff(&[])), Some(Limit::per_minute(90)));
        assert_eq!(
            p.limit_for("edit", &eff(&["bot"])),
            Some(Limit::per_minute(3000))
        );
        let anon = evaluate(
            &Principal::anonymous(),
            GroupRegistry::default_registry(),
            GrantRegistry::default_registry(),
        );
        assert_eq!(p.limit_for("edit", &anon), Some(Limit::per_minute(8)));
        assert_eq!(
            p.limit_for("job", &eff(&[])),
            Some(Limit::per_hour(0)),
            "no jobs for users"
        );
        assert_eq!(
            p.limit_for("job", &eff(&["sysop"])),
            Some(Limit::per_hour(10))
        );
        // sysop holds noratelimit: exempt from everything but job.
        assert_eq!(p.limit_for("edit", &eff(&["sysop"])), None);
        assert_eq!(
            p.limit_for("job", &eff(&["sysop", "bot"])),
            Some(Limit::per_hour(10))
        );
        assert_eq!(p.limit_for("nonsense", &eff(&[])), None);
        let mut p = p;
        p.set("edit", "user", Limit::per_minute(10));
        assert_eq!(p.limit_for("edit", &eff(&[])), Some(Limit::per_minute(10)));
        assert_eq!(
            p.limit_for("stream", &eff(&["bot"])),
            Some(Limit::concurrent(20))
        );
    }
}
