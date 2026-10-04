//! `wbsearchentities`: the site's search (0010 §3; 0047 §9).

use std::fmt::Write as _;

use percent_encoding::utf8_percent_encode;
use serde_json::Value;

use crate::api::{CacheInfo, Fetched, check};
use crate::entity::QUERY;
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// One search result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hit {
    /// The entity ID.
    pub id: String,
    /// The page title (`Item:Q6`).
    pub title: String,
    /// The label to show, with its language.
    pub label: Option<(String, String)>,
    /// The description to show, with its language.
    pub description: Option<(String, String)>,
    /// What matched: `label` or `alias`.
    pub match_type: String,
    /// The text that matched, with its language.
    pub match_text: (String, String),
}

/// A page of results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hits {
    /// The results.
    pub hits: Vec<Hit>,
    /// The offset of the next page, where there is one.
    pub next: Option<u32>,
}

fn pair(v: &Value) -> Option<(String, String)> {
    Some((
        v["value"].as_str()?.to_string(),
        v["language"].as_str().unwrap_or_default().to_string(),
    ))
}

/// The results of a `wbsearchentities` response.
fn hits_of(v: &Value) -> Hits {
    let hits = v["search"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|h| {
                    Some(Hit {
                        id: h["id"].as_str()?.to_string(),
                        title: h["title"].as_str().unwrap_or_default().to_string(),
                        label: pair(&h["display"]["label"]),
                        description: pair(&h["display"]["description"]),
                        match_type: h["match"]["type"].as_str().unwrap_or("label").to_string(),
                        match_text: (
                            h["match"]["text"].as_str().unwrap_or_default().to_string(),
                            h["match"]["language"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let next = v["search-continue"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok());
    Hits { hits, next }
}

impl Client {
    /// `wbsearchentities` for `search` among entities of `entity_type` (`item`,
    /// `property`, `domain`), labels and aliases in `language` and its fallbacks, from
    /// `offset`, at most `limit`.
    pub async fn search(
        &self,
        incoming: &Incoming,
        search: &str,
        language: &str,
        entity_type: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Fetched<Hits>, ClientError> {
        let mut q = format!(
            "/w/api.php?action=wbsearchentities&format=json&formatversion=2&search={}&language={}&type={}&limit={limit}",
            utf8_percent_encode(search, QUERY),
            utf8_percent_encode(language, QUERY),
            utf8_percent_encode(entity_type, QUERY),
        );
        if offset > 0 {
            let _ = write!(q, "&continue={offset}");
        }
        let r = self.get(incoming, &q).await?;
        let v = check(&r)?;
        Ok(Fetched {
            value: hits_of(&v),
            cache: CacheInfo::of(&r),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_parse_the_wikibase_shape() {
        let h = hits_of(&serde_json::json!({
            "search": [{
                "id": "Q6", "title": "Item:Q6",
                "display": {"label": {"value": "Six", "language": "en"},
                            "description": {"value": "a number", "language": "en"}},
                "match": {"type": "alias", "language": "en", "text": "half dozen"}
            }, {"id": "Q7", "title": "Item:Q7", "display": {}, "match": {"type": "label", "language": "mul", "text": "Q"}}],
            "search-continue": 7
        }));
        assert_eq!(h.hits.len(), 2);
        assert_eq!(h.hits[0].label, Some(("Six".into(), "en".into())));
        assert_eq!(h.hits[0].match_type, "alias");
        assert_eq!(h.hits[1].label, None);
        assert_eq!(h.next, Some(7));
    }
}
