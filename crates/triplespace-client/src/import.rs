//! Imports (0047 §8.3; 21 §4.3): what `Special:Import` posts to `action=import`, as a
//! `multipart/form-data` body carrying the file, and what the API answers: the census
//! when only that was asked for, else the job's report.

use axum::http::{HeaderValue, Method, StatusCode, header};
use serde_json::Value;

use crate::api::check;
use crate::{ApiRequest, Client, ClientError, Incoming, forward};

/// What to import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportAsk {
    /// The file (`importupload`), with its name.
    pub xml: Option<(String, Vec<u8>)>,
    /// Another wiki's page: a site alias and the title (`interwikisource`,
    /// `interwikipage`).
    pub from: Option<(String, String)>,
    /// `fullhistory`.
    pub history: bool,
    /// `templates`.
    pub templates: bool,
    /// The job's reason (`summary`).
    pub summary: String,
    /// The imported authors' prefix (`interwikiprefix`); the API's default when empty.
    pub prefix: String,
    /// Only the census (`census`).
    pub census: bool,
    /// The CSRF token.
    pub token: String,
}

/// The multipart body of an ask, and its content type.
#[must_use]
pub fn multipart(ask: &ImportAsk) -> (String, Vec<u8>) {
    const BOUNDARY: &str = "----triplespace-import-5f0e2b7c9a1d";
    let mut body = Vec::new();
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    };
    field("token", &ask.token);
    if !ask.summary.is_empty() {
        field("summary", &ask.summary);
    }
    if !ask.prefix.is_empty() {
        field("interwikiprefix", &ask.prefix);
    }
    if ask.history {
        field("fullhistory", "1");
    }
    if ask.templates {
        field("templates", "1");
    }
    if ask.census {
        field("census", "1");
    }
    if let Some((alias, title)) = &ask.from {
        field("interwikisource", alias);
        field("interwikipage", title);
    }
    if let Some((name, bytes)) = &ask.xml {
        let name = name.replace(['"', '\r', '\n'], "_");
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"xml\"; filename=\"{name}\"\r\nContent-Type: application/xml\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

impl Client {
    /// `action=import`: the API's answer, with `census` or `import` and `job`, or its
    /// refusal as its code and message.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn import(&self, incoming: &Incoming, ask: &ImportAsk) -> Result<Value, ClientError> {
        let mut headers = forward::headers(incoming, self.transport());
        let (content_type, body) = multipart(ask);
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&content_type)
                .map_err(|e| ClientError::Unexpected(e.to_string()))?,
        );
        let r = self
            .transport()
            .send(ApiRequest {
                method: Method::POST,
                path_and_query: "/w/api.php?action=import&format=json&formatversion=2".to_string(),
                headers,
                body,
                peer: incoming.peer,
            })
            .await?;
        if r.status != StatusCode::OK {
            return Err(ClientError::Unexpected(format!("HTTP {}", r.status)));
        }
        check(&r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_carries_the_fields_and_the_file() {
        let (ct, body) = multipart(&ImportAsk {
            xml: Some(("pages.xml".into(), b"<mediawiki/>".to_vec())),
            from: None,
            history: true,
            templates: false,
            summary: "why".into(),
            prefix: "wp".into(),
            census: true,
            token: "t+\\".into(),
        });
        assert!(ct.starts_with("multipart/form-data; boundary="));
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("name=\"token\"\r\n\r\nt+\\\r\n"));
        assert!(text.contains("name=\"fullhistory\"\r\n\r\n1\r\n"));
        assert!(!text.contains("name=\"templates\""));
        assert!(text.contains("name=\"census\""));
        assert!(text.contains("name=\"xml\"; filename=\"pages.xml\"\r\nContent-Type: application/xml\r\n\r\n<mediawiki/>\r\n"));
        assert!(text.ends_with("--\r\n"));
    }
}
