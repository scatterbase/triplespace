//! `GET /version` and `GET /version/licenses/{id}` (ADR 0077 §10): what
//! `Special:Version` shows, and one component's licence texts.

use axum::http::StatusCode;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::api::{CacheInfo, Fetched};
use crate::{Client, ClientError, Incoming};

/// What a path segment escapes: an ID such as `@wikimedia/codex@2.7.0` keeps its `@`.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'/')
    .add(b'?')
    .add(b'#')
    .add(b'%')
    .add(b'"');

/// The developer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Developer {
    /// `Scatter LLC`.
    #[serde(default)]
    pub name: String,
    /// `Portland, Oregon`.
    #[serde(default)]
    pub place: String,
    /// The repository.
    #[serde(default)]
    pub source: String,
}

/// A contributor or a funder.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Credit {
    /// As credited.
    pub name: String,
    /// The first contribution, or when the support began.
    #[serde(default)]
    pub since: Option<String>,
    /// When the support ended (funders).
    #[serde(default)]
    pub until: Option<String>,
    /// What the support was for (funders).
    #[serde(default)]
    pub funded: Option<String>,
    /// A link.
    #[serde(default)]
    pub url: Option<String>,
}

/// The licence and the source.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct License {
    /// `GPL-3.0-or-later`.
    #[serde(default)]
    pub spdx: String,
    /// Where the corresponding source is; `None` for a modified build with none configured.
    #[serde(default)]
    pub source: Option<String>,
    /// Whether the build is from a modified tree.
    #[serde(default)]
    pub modified: bool,
}

/// A build.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Build {
    /// The version.
    #[serde(default)]
    pub version: String,
    /// The commit.
    #[serde(default)]
    pub commit: String,
    /// Whether the tree was modified.
    #[serde(default)]
    pub modified: bool,
    /// The commit's date.
    #[serde(default)]
    pub commit_date: String,
    /// `rustc --version`.
    #[serde(default)]
    pub rustc: String,
    /// The target triple.
    #[serde(default)]
    pub target: String,
    /// The cargo features compiled in.
    #[serde(default)]
    pub features: Vec<String>,
}

/// A configured service.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Service {
    /// The name.
    pub name: String,
    /// What the instance uses it for.
    #[serde(default)]
    pub role: Option<String>,
    /// `connected` or `unreachable`.
    #[serde(default)]
    pub state: String,
    /// Its version, where disclosed.
    #[serde(default)]
    pub version: Option<String>,
}

/// A workspace crate.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkspaceCrate {
    /// The name.
    pub name: String,
    /// The version.
    #[serde(default)]
    pub version: String,
    /// The licence.
    #[serde(default)]
    pub license: String,
    /// Also available under a commercial licence (the `scatter-*` crates).
    #[serde(default)]
    pub commercial: bool,
}

/// A third-party component: a crate, a frontend package or a vendored tree.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Component {
    /// `name@version`.
    pub id: String,
    /// The name.
    pub name: String,
    /// The version.
    #[serde(default)]
    pub version: String,
    /// The licence expression.
    #[serde(default)]
    pub license: String,
    /// Upstream.
    #[serde(default)]
    pub repository: Option<String>,
}

/// A feature switch and its value.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FeatureSwitch {
    /// The setting.
    pub key: String,
    /// Whether the tenant or the instance sets it.
    #[serde(default)]
    pub set: bool,
    /// Its value, or the default; `None` when neither.
    #[serde(default)]
    pub value: Option<Value>,
}

/// The features of the forms the request is at.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Features {
    /// The tenant's, at a tenant base.
    #[serde(default)]
    pub tenant: Option<Vec<FeatureSwitch>>,
    /// The instance's, at the farm base.
    #[serde(default)]
    pub instance: Option<Vec<FeatureSwitch>>,
    /// The providers the tenant serves.
    #[serde(default)]
    pub providers: Vec<String>,
}

/// An entry point.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EntryPoint {
    /// Its name.
    pub name: String,
    /// Its URL.
    pub url: String,
}

/// A wikitext entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Wikitext {
    /// `tag`, `function`, `variable` or `switch`.
    pub kind: String,
    /// Its name in the registry.
    pub name: String,
    /// As written on a page: `{{#if:…}}`, `<ref>`, `__NOTOC__`.
    #[serde(default)]
    pub written: String,
    /// Where it comes from.
    #[serde(default)]
    pub origin: Option<String>,
    /// `implemented`, `chip` or `ignored`.
    #[serde(default)]
    pub status: String,
    /// Whether the tenant's settings turn it on.
    #[serde(default)]
    pub active: bool,
}

/// What came from one extension, or from MediaWiki.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Lineage {
    /// The extension, or `MediaWiki`.
    pub name: String,
    /// Its page on mediawiki.org.
    #[serde(default)]
    pub url: Option<String>,
    /// Special pages.
    #[serde(default)]
    pub special_pages: Vec<String>,
    /// Content models.
    #[serde(default)]
    pub content_models: Vec<String>,
    /// Wikitext, implemented and only recognized.
    #[serde(default)]
    pub wikitext: LineageWikitext,
    /// Anything else taken from it.
    #[serde(default)]
    pub inspired_by: Vec<String>,
}

/// Wikitext in a lineage row.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LineageWikitext {
    /// Implemented.
    #[serde(default)]
    pub implemented: Vec<String>,
    /// Recognized only.
    #[serde(default)]
    pub recognized: Vec<String>,
}

/// An AI agent.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Agent {
    /// `Claude Opus`.
    pub name: String,
    /// `Anthropic`.
    #[serde(default)]
    pub maker: String,
    /// The model versions recorded.
    #[serde(default)]
    pub versions: Vec<String>,
}

/// The instance.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Instance {
    /// Its name.
    #[serde(default)]
    pub name: String,
    /// Its owner of record, where set.
    #[serde(default)]
    pub owner_of_record: Option<Value>,
}

/// `GET /version`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Version {
    /// `Triplespace`.
    #[serde(default)]
    pub product: String,
    /// The developer.
    #[serde(default)]
    pub developer: Developer,
    /// The contributors, in order of first contribution.
    #[serde(default)]
    pub contributors: Vec<Credit>,
    /// The funders.
    #[serde(default)]
    pub funders: Vec<Credit>,
    /// The licence and the source.
    #[serde(default)]
    pub license: License,
    /// The API server's build.
    #[serde(default)]
    pub build: Build,
    /// The configured services.
    #[serde(default)]
    pub services: Vec<Service>,
    /// Whether the components are listed.
    #[serde(default)]
    pub components_listed: bool,
    /// The Triplespace crates.
    #[serde(default)]
    pub workspace: Vec<WorkspaceCrate>,
    /// The third-party crates.
    #[serde(default)]
    pub crates: Vec<Component>,
    /// The frontend packages.
    #[serde(default)]
    pub packages: Vec<Component>,
    /// Vendored code.
    #[serde(default)]
    pub vendored: Vec<Component>,
    /// The features.
    #[serde(default)]
    pub features: Features,
    /// The entry points.
    #[serde(default)]
    pub entry_points: Vec<EntryPoint>,
    /// The wikitext.
    #[serde(default)]
    pub wikitext: Vec<Wikitext>,
    /// The lineage.
    #[serde(default)]
    pub inspired_by: Vec<Lineage>,
    /// The AI agents.
    #[serde(default)]
    pub agents: Vec<Agent>,
    /// The instance.
    #[serde(default)]
    pub instance: Instance,
}

/// One file a component ships.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LicenseText {
    /// The file name.
    pub file: String,
    /// Its text.
    #[serde(default)]
    pub text: Option<String>,
}

/// `GET /version/licenses/{id}`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ComponentLicense {
    /// `name@version`.
    pub id: String,
    /// The name.
    pub name: String,
    /// The version.
    #[serde(default)]
    pub version: String,
    /// The licence expression.
    #[serde(default)]
    pub license: String,
    /// Upstream.
    #[serde(default)]
    pub repository: Option<String>,
    /// The files.
    #[serde(default)]
    pub texts: Vec<LicenseText>,
}

fn api_error(status: StatusCode, body: &[u8]) -> ClientError {
    let v: Map<String, Value> = serde_json::from_slice(body).unwrap_or_default();
    ClientError::Api {
        code: v
            .get("code")
            .and_then(Value::as_str)
            .map_or_else(|| format!("http-{}", status.as_u16()), str::to_string),
        info: v
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

impl Client {
    /// `GET /version`.
    pub async fn version(&self, incoming: &Incoming) -> Result<Fetched<Version>, ClientError> {
        let r = self
            .get(incoming, "/w/rest.php/triplespace/v0/version")
            .await?;
        let cache = CacheInfo::of(&r);
        if r.status != StatusCode::OK {
            return Err(api_error(r.status, &r.body));
        }
        let value = serde_json::from_slice(&r.body)
            .map_err(|e| ClientError::Unexpected(format!("version: {e}")))?;
        Ok(Fetched { value, cache })
    }

    /// `GET /version/licenses/{id}`; `None` for a component this build does not have.
    pub async fn component_license(
        &self,
        incoming: &Incoming,
        id: &str,
    ) -> Result<Fetched<Option<ComponentLicense>>, ClientError> {
        let path = format!(
            "/w/rest.php/triplespace/v0/version/licenses/{}",
            utf8_percent_encode(id, SEGMENT)
        );
        let r = self.get(incoming, &path).await?;
        let cache = CacheInfo::of(&r);
        let value = match r.status {
            StatusCode::OK => Some(
                serde_json::from_slice(&r.body)
                    .map_err(|e| ClientError::Unexpected(format!("component licence: {e}")))?,
            ),
            StatusCode::NOT_FOUND => None,
            s => return Err(api_error(s, &r.body)),
        };
        Ok(Fetched { value, cache })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_contract_shape() {
        let v: Version = serde_json::from_value(serde_json::json!({
            "product": "Triplespace",
            "developer": {"name": "Scatter LLC", "place": "Portland, Oregon", "source": "https://github.com/scatterbase/triplespace"},
            "contributors": [], "funders": [],
            "license": {"spdx": "GPL-3.0-or-later", "source": null, "modified": true},
            "build": {"version": "0.0.1", "commit": "abc", "modified": true},
            "services": [{"name": "PostgreSQL", "role": "Log store", "state": "connected", "version": "16.4"}],
            "components_listed": false, "workspace": [], "crates": [], "packages": [], "vendored": [],
            "features": {"tenant": [{"key": "query.enabled", "set": false, "value": false}], "providers": ["wikidata"]},
            "entry_points": [{"name": "api.php", "url": "https://x/w/api.php"}],
            "wikitext": [], "inspired_by": [{"name": "MediaWiki", "url": null, "special_pages": ["Search"], "content_models": [], "wikitext": {"implemented": [], "recognized": []}, "inspired_by": []}],
            "agents": [{"name": "Claude Opus", "maker": "Anthropic", "versions": ["5.5"]}],
            "instance": {"name": "scatter"}
        }))
        .unwrap();
        assert_eq!(v.services[0].version.as_deref(), Some("16.4"));
        assert!(v.license.source.is_none());
        assert_eq!(v.features.tenant.unwrap()[0].key, "query.enabled");
        assert!(v.features.instance.is_none());
    }

    #[test]
    fn ids_keep_their_at_signs() {
        assert_eq!(
            utf8_percent_encode("@wikimedia/codex@2.7.0", SEGMENT).to_string(),
            "@wikimedia%2Fcodex@2.7.0"
        );
    }
}
