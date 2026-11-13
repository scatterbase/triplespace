//! Exports (0047 §8.1, §8.2; 21 §4.2, §4.4): what `Special:Export` posts to
//! `POST /export`, and the file the API answers with.

use axum::http::{HeaderValue, Method, StatusCode, header};
use http_body_util::BodyExt as _;
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
    /// The viewer's CSRF token, which the API needs to queue an export too large to
    /// answer in place (0104 A10); empty for a viewer who is not signed in.
    pub token: String,
}

/// An export the API answered.
#[derive(Debug)]
pub struct Exported {
    /// The file's media type.
    pub content_type: String,
    /// The file name the API suggests.
    pub filename: Option<String>,
    /// The file, as it arrives from the API: passed on to the reader without being held
    /// whole, and failing where the API's stream fails, so that a cut-short export is a
    /// failed download and not a short file (21 §4.2).
    pub body: axum::body::Body,
    /// How many of the titles asked for the export does not hold.
    pub missing: usize,
    /// What the export says about itself.
    pub notice: Option<String>,
}

/// What the API made of an export.
#[derive(Debug)]
pub enum ExportAnswer {
    /// The file, made while the caller waited.
    File(Exported),
    /// Too large to make while the caller waits: queued as this job (0104 A9), whose file
    /// [`Client::export_job`] fetches once it is ready.
    Queued(u64),
}

/// An export job, as its submitter sees it.
#[derive(Debug)]
pub enum ExportJob {
    /// Finished: its file.
    File(Exported),
    /// Queued or running still.
    Pending,
    /// Ended without a file: its status, and the error where it failed.
    Ended {
        /// `failed`, `interrupted`, or another final status.
        status: String,
        /// What went wrong.
        error: Option<String>,
    },
}

fn refusal(status: StatusCode, body: &[u8]) -> ClientError {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    ClientError::Api {
        code: v["code"].as_str().unwrap_or(status.as_str()).to_string(),
        info: v["message"]
            .as_str()
            .or_else(|| v["info"].as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

fn file_of(r: crate::StreamedResponse) -> Exported {
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
    Exported {
        content_type: text("content-type").unwrap_or_else(|| "application/xml".into()),
        filename,
        missing: text("x-export-missing")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0),
        notice: text("x-export-notice"),
        body: r.body,
    }
}

impl Client {
    /// `GET /export/{id}`: an export job of the caller's own, and its file once ready.
    ///
    /// # Errors
    ///
    /// The API's refusal (no such job of the caller's, or its file expired), or the API
    /// away or unreadable.
    pub async fn export_job(&self, incoming: &Incoming, id: u64) -> Result<ExportJob, ClientError> {
        let r = self
            .transport()
            .send_streamed(ApiRequest {
                method: Method::GET,
                path_and_query: format!("/w/rest.php/triplespace/v0/export/{id}"),
                headers: forward::headers(incoming, self.transport()),
                body: Vec::new(),
                peer: incoming.peer,
            })
            .await?;
        let json = r
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("application/json"));
        if r.status == StatusCode::OK && !json {
            return Ok(ExportJob::File(file_of(r)));
        }
        let status = r.status;
        let body = r
            .body
            .collect()
            .await
            .map_err(|e| ClientError::Unexpected(e.to_string()))?
            .to_bytes();
        match status {
            StatusCode::ACCEPTED => Ok(ExportJob::Pending),
            StatusCode::OK => {
                let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                Ok(ExportJob::Ended {
                    status: v["status"].as_str().unwrap_or_default().to_string(),
                    error: v["error"].as_str().map(str::to_string),
                })
            }
            _ => Err(refusal(status, &body)),
        }
    }

    /// `POST /export`: the file, the job the export was queued as, or the API's refusal
    /// as its code and message.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn export(
        &self,
        incoming: &Incoming,
        ask: &ExportAsk,
    ) -> Result<ExportAnswer, ClientError> {
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
            "token": ask.token,
        });
        let r = self
            .transport()
            .send_streamed(ApiRequest {
                method: Method::POST,
                path_and_query: "/w/rest.php/triplespace/v0/export".to_string(),
                headers,
                body: body.to_string().into_bytes(),
                peer: incoming.peer,
            })
            .await?;
        if r.status == StatusCode::OK {
            return Ok(ExportAnswer::File(file_of(r)));
        }
        // A refusal, or the job it was queued as: small, read whole.
        let status = r.status;
        let body = r
            .body
            .collect()
            .await
            .map_err(|e| ClientError::Unexpected(e.to_string()))?
            .to_bytes();
        if status == StatusCode::ACCEPTED
            && let Some(job) = serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v["job"].as_u64())
        {
            return Ok(ExportAnswer::Queued(job));
        }
        Err(refusal(status, &body))
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
