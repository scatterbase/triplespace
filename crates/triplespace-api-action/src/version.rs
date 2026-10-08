//! What `Special:Version` and `meta=siteinfo` report about this build and the services it
//! runs on (ADR 0077 §4, §10, §13).
//!
//! - [`BuildInfo`]: the binary's `version.json`, which its build script writes from the
//!   build and the component manifest (0077 §15) and the binary passes in
//!   [`crate::Config::version`]. Without one (tests), a stub naming this crate's version.
//! - [`Probes`]: the services' states and versions, read at startup and then on a timer by
//!   the server, never while answering a request, so that no request can make the instance
//!   contact its services (0077 §4). Only PostgreSQL is configured on every instance today;
//!   a service the instance is not configured to use is not reported at all (0077 A2).
//! - [`Disclosure`]: the instance setting `version.services` (0077 §13).

use std::sync::RwLock;

use serde_json::{Value, json};

/// The binary's build and its part of the component manifest.
#[derive(Debug, Clone)]
pub struct BuildInfo {
    json: Value,
}

impl BuildInfo {
    /// Parses a binary's `version.json`; a stub when there is none or it does not parse.
    #[must_use]
    pub fn parse(text: Option<&str>) -> Self {
        let json = text
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .filter(|v| v["build"].is_object())
            .unwrap_or_else(|| {
                json!({
                    "format": 1,
                    "binary": "triplespace-server",
                    "build": {"version": env!("CARGO_PKG_VERSION"), "commit": "", "modified": false},
                    "listed": false,
                })
            });
        Self { json }
    }

    /// The whole document.
    #[must_use]
    pub fn json(&self) -> &Value {
        &self.json
    }

    /// `build`: version, commit, `modified`, commit date, rustc, target, profile, features.
    #[must_use]
    pub fn build(&self) -> &Value {
        &self.json["build"]
    }

    /// Whether the components are listed: false for a build made without a manifest.
    #[must_use]
    pub fn listed(&self) -> bool {
        self.json["listed"].as_bool().unwrap_or(false)
    }

    /// The third-party crates and frontend packages, as `{name, version}`, for
    /// `siprop=libraries` (0077 §10).
    #[must_use]
    pub fn libraries(&self) -> Vec<Value> {
        ["crates", "packages"]
            .iter()
            .flat_map(|k| self.json[*k].as_array().cloned().unwrap_or_default())
            .map(|c| json!({"name": c["name"], "version": c["version"]}))
            .collect()
    }

    /// A component (`crates`, `packages` or `vendored`) by its `name@version` ID, with its
    /// licence texts filled in: what `Special:Version/License/{component}` serves.
    #[must_use]
    pub fn component(&self, id: &str) -> Option<Value> {
        let c = ["crates", "packages", "vendored"]
            .iter()
            .flat_map(|k| self.json[*k].as_array().into_iter().flatten())
            .find(|c| c["id"].as_str() == Some(id))?;
        let texts: Vec<Value> = c["texts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                json!({
                    "file": t["file"],
                    "text": t["hash"].as_str().and_then(|h| self.json["texts"].get(h)).cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(json!({
            "id": c["id"],
            "name": c["name"],
            "version": c["version"],
            "license": c["license"],
            "repository": c["repository"],
            "texts": texts,
        }))
    }
}

/// A configured service's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Whether the last probe reached it.
    pub connected: bool,
    /// The version it reported, if it did.
    pub version: Option<String>,
}

/// The services' last known states.
#[derive(Debug, Default)]
pub struct Probes {
    postgres: RwLock<Option<Probe>>,
}

impl Probes {
    /// PostgreSQL's last known state; `None` before the first probe.
    #[must_use]
    pub fn postgres(&self) -> Option<Probe> {
        self.postgres.read().ok().and_then(|g| g.clone())
    }

    /// Records PostgreSQL's state.
    pub fn set_postgres(&self, probe: Probe) {
        if let Ok(mut g) = self.postgres.write() {
            *g = Some(probe);
        }
    }

    /// The configured services, as `{name, state, version}`: PostgreSQL, which every
    /// instance has (0033 §1). A service is added here when the instance can be configured
    /// to use it.
    #[must_use]
    pub fn services(&self) -> Vec<(&'static str, Probe)> {
        let pg = self.postgres().unwrap_or(Probe {
            connected: false,
            version: None,
        });
        vec![("PostgreSQL", pg)]
    }
}

/// How much of the services `Special:Version` and `siteinfo` show (`version.services`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Disclosure {
    /// Names, roles, states and versions (the default).
    #[default]
    Full,
    /// Names, roles and states, without versions.
    Names,
    /// No services: the build only.
    Off,
}

impl Disclosure {
    /// The setting's value; `full` for anything else.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("names") => Self::Names,
            Some("off") => Self::Off,
            _ => Self::Full,
        }
    }

    /// Whether versions are shown.
    #[must_use]
    pub fn versions(self) -> bool {
        self == Self::Full
    }

    /// Whether services are listed at all.
    #[must_use]
    pub fn services(self) -> bool {
        self != Self::Off
    }
}

/// A `site` setting's value, the tenant's own or else the instance's (`tenant = ''`).
pub async fn setting(
    db: &tokio_postgres::Client,
    tenant: &str,
    key: &str,
) -> Result<Option<Value>, tokio_postgres::Error> {
    Ok(db
        .query_opt(
            "SELECT config->'value' FROM view.registry
             WHERE kind = 'site' AND code = $2 AND tenant IN ($1, '')
             ORDER BY tenant DESC LIMIT 1",
            &[&tenant, &key],
        )
        .await?
        .and_then(|r| r.get::<_, Option<Value>>(0)))
}

/// An instance `site` setting's value (`tenant = ''`).
pub async fn instance_setting(
    db: &tokio_postgres::Client,
    key: &str,
) -> Result<Option<Value>, tokio_postgres::Error> {
    Ok(db
        .query_opt(
            "SELECT config->'value' FROM view.registry
             WHERE kind = 'site' AND code = $1 AND tenant = ''",
            &[&key],
        )
        .await?
        .and_then(|r| r.get::<_, Option<Value>>(0)))
}

/// The instance's `version.services`.
pub async fn disclosure(db: &tokio_postgres::Client) -> Result<Disclosure, tokio_postgres::Error> {
    let v = instance_setting(db, "version.services").await?;
    Ok(Disclosure::parse(v.as_ref().and_then(Value::as_str)))
}

/// Reads PostgreSQL's version over the pool and records it. Called by the server at
/// startup and on a timer, never from a request handler.
pub async fn refresh(app: &crate::App) {
    let probe = match app.pool().get().await {
        Ok(client) => match client.query_one("SHOW server_version", &[]).await {
            Ok(row) => Probe {
                connected: true,
                version: row.get::<_, Option<String>>(0),
            },
            Err(_) => Probe {
                connected: false,
                version: None,
            },
        },
        Err(_) => Probe {
            connected: false,
            version: None,
        },
    };
    app.probes().set_postgres(probe);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stub_without_a_manifest() {
        let b = BuildInfo::parse(None);
        assert!(!b.listed());
        assert_eq!(b.build()["version"], env!("CARGO_PKG_VERSION"));
        assert!(b.libraries().is_empty());
        assert!(BuildInfo::parse(Some("not json")).build().is_object());
    }

    #[test]
    fn components_and_their_texts() {
        let b = BuildInfo::parse(Some(
            &json!({
                "build": {"version": "0.0.1", "commit": "abc", "modified": false},
                "listed": true,
                "crates": [{"id": "url@2.5.0", "name": "url", "version": "2.5.0", "license": "MIT OR Apache-2.0",
                            "texts": [{"file": "LICENSE-MIT", "hash": "h1"}]}],
                "packages": [{"id": "vue@3.5.43", "name": "vue", "version": "3.5.43", "license": "MIT", "texts": []}],
                "texts": {"h1": "MIT text"},
            })
            .to_string(),
        ));
        assert!(b.listed());
        assert_eq!(
            b.libraries(),
            vec![
                json!({"name": "url", "version": "2.5.0"}),
                json!({"name": "vue", "version": "3.5.43"})
            ]
        );
        let c = b.component("url@2.5.0").unwrap();
        assert_eq!(c["texts"][0]["text"], "MIT text");
        assert!(b.component("url@9").is_none());
    }

    #[test]
    fn disclosure() {
        assert_eq!(Disclosure::parse(None), Disclosure::Full);
        assert_eq!(Disclosure::parse(Some("names")), Disclosure::Names);
        assert!(!Disclosure::Names.versions() && Disclosure::Names.services());
        assert!(!Disclosure::Off.services());
    }
}
