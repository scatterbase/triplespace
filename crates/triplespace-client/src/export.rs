//! Exports (0047 §8.1, §8.2; 21 §4.2, §4.4): what `Special:Export` posts to
//! `POST /export`, and the file the API answers with.

use axum::http::{HeaderValue, Method, StatusCode, header};
use serde_json::{Value, json};

use crate::{ApiRequest, Client, ClientError, Incoming, forward};

/// What to export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportAsk {
    /// `xml` or `records`.
    pub format: String,
    /// The titles.
    pub pages: Vec<String>,
    /// Categories whose pages are added (`addcat`).
    pub categories: Vec<String>,
    /// Namespaces whose pages are added (`addns`).
    pub namespaces: Vec<i64>,
    /// Full history rather than the current revision.
    pub history: bool,
    /// `templates`.
    pub templates: bool,
}

/// An export the API answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The file's media type.
    pub content_type: String,
    /// The file name the API suggests.
    pub filename: Option<String>,
    /// The file.
    pub body: Vec<u8>,
    /// How many of the titles asked for the export does not hold.
    pub missing: usize,
    /// What the export says about itself.
    pub notice: Option<String>,
}

impl Client {
    /// `POST /export`: the file, or the API's refusal as its code and message.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn export(
        &self,
        incoming: &Incoming,
        ask: &ExportAsk,
    ) -> Result<Exported, ClientError> {
        let mut headers = forward::headers(incoming, self.transport());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let body = json!({
            "format": ask.format,
            "pages": ask.pages,
            "categories": ask.categories,
            "namespaces": ask.namespaces,
            "history": ask.history,
            "templates": ask.templates,
        });
        let r = self
            .transport()
            .send(ApiRequest {
                method: Method::POST,
                path_and_query: "/w/rest.php/triplespace/v0/export".to_string(),
                headers,
                body: body.to_string().into_bytes(),
                peer: incoming.peer,
            })
            .await?;
        if r.status != StatusCode::OK {
            let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            return Err(ClientError::Api {
                code: v["code"].as_str().unwrap_or(r.status.as_str()).to_string(),
                info: v["message"]
                    .as_str()
                    .or_else(|| v["info"].as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
        let text = |name: &str| {
            r.headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let filename = text("content-disposition").and_then(|d| {
            d.split_once("filename=\"")
                .and_then(|(_, rest)| rest.split('"').next().map(str::to_string))
        });
        Ok(Exported {
            content_type: text("content-type").unwrap_or_else(|| "application/xml".into()),
            filename,
            body: r.body,
            missing: text("x-export-missing")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
            notice: text("x-export-notice"),
        })
    }

    /// `list=categorymembers`: the titles in a category, as the API writes them.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn category_members(
        &self,
        incoming: &Incoming,
        category: &str,
    ) -> Result<crate::api::Fetched<Vec<String>>, ClientError> {
        let q: String = url::form_urlencoded::byte_serialize(category.as_bytes()).collect();
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&list=categorymembers&cmtitle={q}&cmlimit=max&format=json&formatversion=2"
                ),
            )
            .await?;
        let v = crate::api::check(&r)?;
        let titles = v["query"]["categorymembers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["title"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(crate::api::Fetched {
            value: titles,
            cache: crate::api::CacheInfo::of(&r),
        })
    }
}
