//! Request parameters (mediawiki-compat §2.2–2.3): the query string and a form body merged
//! into one map, with MediaWiki's conventions — a boolean is true if present at all, a
//! multi-value list splits on `|` or U+001F, `formatversion` is 1 or 2.

use std::collections::BTreeMap;

use crate::response::ApiError;

/// The parameters of one request.
#[derive(Debug, Clone, Default)]
pub struct Params {
    map: BTreeMap<String, String>,
    /// Whether the request was a POST.
    pub posted: bool,
    /// Whether the body carried the token (a token in the query string is `mustpostparams`).
    in_body: std::collections::BTreeSet<String>,
}

impl Params {
    /// From a query string and an optional `application/x-www-form-urlencoded` body.
    #[must_use]
    pub fn parse(query: Option<&str>, body: Option<&str>, posted: bool) -> Self {
        let mut p = Self {
            posted,
            ..Self::default()
        };
        if let Some(q) = query {
            for (k, v) in url::form_urlencoded::parse(q.as_bytes()) {
                p.map.insert(k.into_owned(), v.into_owned());
            }
        }
        if let Some(b) = body {
            for (k, v) in url::form_urlencoded::parse(b.as_bytes()) {
                p.in_body.insert(k.to_string());
                p.map.insert(k.into_owned(), v.into_owned());
            }
        }
        p
    }

    /// A parameter's value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.map.get(name).map(String::as_str)
    }

    /// A required parameter: `missingparam` when absent.
    pub fn required(&self, name: &str) -> Result<&str, ApiError> {
        self.get(name).ok_or_else(|| ApiError::missing_param(name))
    }

    /// A boolean: present (with any value) is true (compat §2.3).
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// A multi-value parameter, split on `|` or U+001F; empty when absent.
    #[must_use]
    pub fn list(&self, name: &str) -> Vec<String> {
        let Some(v) = self.get(name) else {
            return Vec::new();
        };
        let sep = if v.starts_with('\u{1f}') {
            '\u{1f}'
        } else {
            '|'
        };
        v.split(sep)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// An integer within a range.
    pub fn int(&self, name: &str, default: u64, min: u64, max: u64) -> Result<u64, ApiError> {
        let Some(v) = self.get(name) else {
            return Ok(default);
        };
        let n: u64 = v.parse().map_err(|_| {
            ApiError::new(
                "badinteger",
                format!("Invalid value \"{v}\" for integer parameter \"{name}\"."),
            )
        })?;
        Ok(n.clamp(min, max))
    }

    /// `formatversion`: 1 (default) or 2.
    pub fn formatversion(&self) -> Result<u8, ApiError> {
        match self.get("formatversion").unwrap_or("1") {
            "1" => Ok(1),
            "2" | "latest" => Ok(2),
            v => Err(ApiError::new(
                "badvalue",
                format!("Unrecognized value for parameter \"formatversion\": {v}."),
            )),
        }
    }

    /// Whether the parameter came in the body rather than the query string.
    #[must_use]
    pub fn in_body(&self, name: &str) -> bool {
        self.in_body.contains(name)
    }

    /// Every parameter name, for the `unrecognizedparams` warning.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_and_splits() {
        let p = Params::parse(
            Some("action=query&bot=0"),
            Some("token=abc%2B%5C&ids=Q1|Q2"),
            true,
        );
        assert_eq!(p.get("action"), Some("query"));
        assert!(p.flag("bot"), "present is true, whatever the value");
        assert_eq!(p.get("token"), Some(r"abc+\"));
        assert!(p.in_body("token") && !p.in_body("action"));
        assert_eq!(p.list("ids"), vec!["Q1", "Q2"]);
        assert_eq!(p.formatversion().unwrap(), 1);
        assert!(p.required("missing").is_err());
    }
}
