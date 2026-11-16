//! The reason-by-channel preference matrix (0021 §3; 16 §5.3): the
//! `notifications.{reason}.{channel}` keys of an account's preferences, which decide
//! whether a reason is delivered to it on a channel.
//!
//! A cell resolves as any preference does (0085 §2.2): the account's own value, then the
//! tenant's and the instance's `preferences.defaults`, then the registry's default for
//! the key or for its pattern (`notifications.*.web` is on). The addressing projection
//! reads the `web` cells: a reason an account has turned off there is not delivered to
//! its inbox. The other channels are not built, so their cells decide nothing yet.

use serde_json::Value;
use tokio_postgres::GenericClient;
use triplespace_accounts::preferences::{Defaults, PreferenceRegistry, Request, resolve};

/// The reasons the addressing projection delivers (16 §5.2), in the order the matrix
/// lists them.
pub const REASONS: [&str; 9] = [
    "talk",
    "reply",
    "mention",
    "thread-status",
    "watch",
    "rights",
    "rename",
    "group-join",
    "job",
];

/// The channels that deliver (16 §5.4): the web inbox. Email and the fediverse join the
/// matrix as columns when they are built.
pub const CHANNELS: [&str; 1] = ["web"];

/// The web inbox.
pub const WEB: &str = "web";

/// A cell's preference key.
#[must_use]
pub fn key(reason: &str, channel: &str) -> String {
    format!("notifications.{reason}.{channel}")
}

/// The reason and channel a preference key is the cell of, where it is one of the
/// matrix's.
#[must_use]
pub fn cell(key: &str) -> Option<(&str, &str)> {
    let (reason, channel) = key.strip_prefix("notifications.")?.split_once('.')?;
    (REASONS.contains(&reason) && CHANNELS.contains(&channel)).then_some((reason, channel))
}

/// Every cell's key, by reason and then by channel.
#[must_use]
pub fn keys() -> Vec<String> {
    REASONS
        .iter()
        .flat_map(|r| CHANNELS.iter().map(move |c| key(r, c)))
        .collect()
}

/// A cell as it resolves from an account's own value and the defaults: on unless
/// something says otherwise, for a key the registry does not know.
#[must_use]
pub fn resolved(own: Option<&Value>, defaults: &Defaults, reason: &str, channel: &str) -> bool {
    let key = key(reason, channel);
    let Some(def) = PreferenceRegistry::embedded().lookup(&key) else {
        return true;
    };
    resolve(def, &key, own, &Request::default(), defaults)
        .value
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// A tenant's matrix: its defaults, read once, against which each account's cells are
/// resolved.
#[derive(Debug, Clone)]
pub struct Matrix {
    defaults: Defaults,
}

impl Matrix {
    /// Reads the tenant's defaults.
    ///
    /// # Errors
    ///
    /// The store's error, as text.
    pub async fn load<C: GenericClient>(client: &C, tenant: &str) -> Result<Self, String> {
        let (snap, _) = triplespace_accounts::settings::load(client, Some(tenant))
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            defaults: Defaults::of(&snap),
        })
    }

    /// Whether an account takes a reason on a channel.
    ///
    /// # Errors
    ///
    /// The store's error, as text.
    pub async fn wants<C: GenericClient>(
        &self,
        client: &C,
        account: &str,
        reason: &str,
        channel: &str,
    ) -> Result<bool, String> {
        let own: Option<Value> = client
            .query_opt(
                "SELECT value FROM private.preference WHERE actor_key = $1 AND key = $2",
                &[&account, &key(reason, channel)],
            )
            .await
            .map_err(|e| e.to_string())?
            .map(|r| r.get(0));
        Ok(resolved(own.as_ref(), &self.defaults, reason, channel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 16 §5.3: a cell is a preference, resolved as one: the account's own value, the
    // tenant's defaults, then the registry's pattern default, which is on for the web.
    #[test]
    fn a_cell_resolves_as_a_preference() {
        let none = Defaults::default();
        assert!(resolved(None, &none, "mention", WEB));
        assert!(!resolved(Some(&json!(false)), &none, "mention", WEB));
        let mut tenant = Defaults::default();
        tenant
            .tenant
            .insert("notifications.watch.web".into(), json!(false));
        assert!(!resolved(None, &tenant, "watch", WEB));
        assert!(resolved(Some(&json!(true)), &tenant, "watch", WEB));
        assert!(resolved(None, &tenant, "talk", WEB));
        // A value that is not a boolean is no value.
        assert!(resolved(Some(&json!("no")), &none, "talk", WEB));
    }

    #[test]
    fn keys_and_cells() {
        assert_eq!(key("talk", WEB), "notifications.talk.web");
        assert_eq!(
            cell("notifications.thread-status.web"),
            Some(("thread-status", "web"))
        );
        assert_eq!(cell("notifications.talk.email"), None, "email is not built");
        assert_eq!(cell("notifications.email-digest"), None);
        assert_eq!(cell("language"), None);
        assert_eq!(keys().len(), REASONS.len() * CHANNELS.len());
        // Every cell is a registered preference.
        for k in keys() {
            assert!(PreferenceRegistry::embedded().lookup(&k).is_some(), "{k}");
        }
    }
}
