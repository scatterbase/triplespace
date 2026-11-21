//! What a follower mirrors (0070 §2, §5; 05 §5.2, §5.3, §5.7): which entity types it
//! follows whole, and which properties it walks for closure and how far.
//!
//! A registry provider's mirror is the instance's, so its follower reads the instance's
//! settings; a source's follower reads its tenant's, else the instance's. `entities.mirror`
//! is `{code or source name: {entity type: value}}` and `entities.closure` is
//! `{code or source name: [property, …]}`, with Wikidata's four properties when unset for
//! `WD` (0070 §2.3) and none for anything else.

use std::collections::BTreeMap;

use scatter_providers::Provider;
use tokio_postgres::GenericClient;

/// The closure Wikidata gets when `entities.closure` does not name it (0070 §2.3).
pub const WIKIDATA_CLOSURE: [&str; 4] = ["P31", "P279", "P131", "P17"];

/// `entities.closure_depth`'s default (0070 §2.3).
pub const CLOSURE_DEPTH: i16 = 6;

/// What a follower mirrors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// `entities.mirror` for the provider: entity type → value.
    mirror: BTreeMap<String, String>,
    /// What a type with no setting gets: `all` once the mirror has been loaded, by a dump
    /// or a crawl (0070 A11); else `linked`, or `off` for a source without an `api`.
    unset: &'static str,
    /// Closure properties, as upstream writes them (`P31`).
    pub closure: Vec<String>,
    /// How many closure steps are walked.
    pub depth: i16,
}

impl Policy {
    /// The setting that applies to `entity_type`: the one set, else the default; a
    /// property is always at least `on-demand` (0070 §2.1).
    #[must_use]
    pub fn setting(&self, entity_type: &str) -> &str {
        let v = self
            .mirror
            .get(entity_type)
            .map_or(self.unset, String::as_str);
        if v == "off" && entity_type == "property" {
            "on-demand"
        } else {
            v
        }
    }

    /// Whether every recent change of `entity_type` is fetched (`all`), rather than only
    /// those of entities the mirror holds (0070 §5; 05 §5.7).
    #[must_use]
    pub fn whole(&self, entity_type: &str) -> bool {
        self.setting(entity_type) == "all"
    }
}

/// The value of the `site` setting `code` for `tenant`, else the instance's.
async fn site_value<C: GenericClient>(
    client: &C,
    tenant: &str,
    code: &str,
) -> Result<Option<serde_json::Value>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT tenant, config->'value' FROM view.registry
             WHERE tenant IN ('', $1) AND kind = 'site' AND code = $2",
            &[&tenant, &code],
        )
        .await?;
    let mut instance = None;
    let mut own = None;
    for r in &rows {
        let t: String = r.get(0);
        let v: Option<serde_json::Value> = r.get(1);
        if t.is_empty() {
            instance = v;
        } else {
            own = v;
        }
    }
    Ok(own.or(instance))
}

/// The policy of `provider`'s follower under `tenant` (`""` for a registry provider),
/// `loaded` when a load has recorded an end.
///
/// # Errors
///
/// The database's.
pub async fn policy<C: GenericClient>(
    client: &C,
    tenant: &str,
    provider: &Provider,
    loaded: bool,
) -> Result<Policy, tokio_postgres::Error> {
    let key = if provider.source {
        &provider.slug
    } else {
        &provider.code
    };
    let mirror = site_value(client, tenant, "entities.mirror")
        .await?
        .and_then(|v| v.get(key).cloned())
        .and_then(|v| v.as_object().cloned())
        .map(|o| {
            o.into_iter()
                .filter_map(|(t, v)| v.as_str().map(|v| (t, v.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let closure = match site_value(client, tenant, "entities.closure")
        .await?
        .and_then(|v| v.get(key).cloned())
    {
        Some(list) => list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_string))
            .collect(),
        None if !provider.source && provider.code == "WD" => {
            WIKIDATA_CLOSURE.iter().map(ToString::to_string).collect()
        }
        None => Vec::new(),
    };
    let depth = site_value(client, tenant, "entities.closure_depth")
        .await?
        .and_then(|v| v.as_i64())
        .and_then(|d| i16::try_from(d).ok())
        .unwrap_or(CLOSURE_DEPTH);
    let unset = if loaded {
        "all"
    } else if provider.source && provider.api.is_none() {
        "off"
    } else {
        "linked"
    };
    Ok(Policy {
        mirror,
        unset,
        closure,
        depth,
    })
}

#[cfg(test)]
mod tests {
    use super::Policy;

    // 0070 §5, A11: `all` follows a type whole; `linked`, `on-demand` and `off` only the
    // set; a type with no setting follows the load.
    #[test]
    fn a_type_is_followed_whole_under_all_or_after_a_load() {
        let mut p = Policy {
            mirror: [("item".to_string(), "linked".to_string())]
                .into_iter()
                .chain([("property".to_string(), "all".to_string())])
                .chain([("lexeme".to_string(), "off".to_string())])
                .collect(),
            unset: "linked",
            ..Policy::default()
        };
        assert!(!p.whole("item"));
        assert!(p.whole("property"));
        assert!(!p.whole("form"));
        assert_eq!(p.setting("lexeme"), "off");
        p.unset = "all";
        assert!(!p.whole("item"), "a setting wins over the load");
        assert!(p.whole("form"));
        p.mirror.insert("property".into(), "off".into());
        assert_eq!(
            p.setting("property"),
            "on-demand",
            "a property is at least on demand"
        );
    }
}
