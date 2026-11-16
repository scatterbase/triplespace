//! The login's email addresses (0100 §7; 18 §3.2): `GET /auth/email`, and the four writes
//! `POST /auth/email`, `…/verify`, `…/primary` and `…/remove`, each a JSON body with the
//! CSRF token. A refusal comes back as its code with the API's body, for the page to
//! show.

use axum::http::{HeaderValue, Method, StatusCode, header};
use serde_json::{Value, json};

use crate::{ApiRequest, ApiResponse, Client, ClientError, Incoming, forward};

const REST: &str = "/w/rest.php/triplespace/v0";

/// One address of the login.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct EmailAddress {
    /// As written.
    pub address: String,
    /// Whether a code was entered for it.
    pub verified: bool,
    /// Whether it is the primary.
    pub primary: bool,
    /// When it was added, seconds since the epoch.
    pub added: u64,
    /// When its code expires, for an unverified address with a live code.
    #[serde(default)]
    pub code_expires: Option<u64>,
    /// `bounce` or `complaint`, after one.
    #[serde(default)]
    pub undeliverable: Option<String>,
}

/// What `GET /auth/email` answers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct EmailInfo {
    /// The addresses, the primary first.
    pub addresses: Vec<EmailAddress>,
    /// Whether the instance sends mail at all.
    pub sends_mail: bool,
    /// `email.max_addresses`.
    pub max_addresses: i64,
    /// `email.required`.
    pub required: bool,
    /// Why the primary cannot be removed now (`ts-email-primary`, `ts-email-last`,
    /// `ts-email-required`), or nothing when it can.
    #[serde(default)]
    pub primary_removal: Option<String>,
}

/// What a write answered: the API's body, or the refusal's code and body.
pub type EmailOutcome = Result<Value, (String, Value)>;

impl Client {
    /// Sends a JSON `POST` for the browser's request `incoming`.
    async fn post_json(
        &self,
        incoming: &Incoming,
        path: &str,
        body: &Value,
    ) -> Result<ApiResponse, ClientError> {
        let mut headers = forward::headers(incoming, self.transport());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        self.transport()
            .send(ApiRequest {
                method: Method::POST,
                path_and_query: path.to_string(),
                headers,
                body: body.to_string().into_bytes(),
                peer: incoming.peer,
            })
            .await
    }

    /// `GET /auth/email`: the viewer's addresses, or `None` when they are not logged in.
    pub async fn email_addresses(
        &self,
        incoming: &Incoming,
    ) -> Result<Option<EmailInfo>, ClientError> {
        let r = self.get(incoming, &format!("{REST}/auth/email")).await?;
        match r.status {
            StatusCode::OK => serde_json::from_slice(&r.body)
                .map(Some)
                .map_err(|e| ClientError::Unexpected(format!("email addresses: {e}"))),
            StatusCode::UNAUTHORIZED => Ok(None),
            s => Err(ClientError::Unexpected(format!(
                "email addresses: HTTP {s}"
            ))),
        }
    }

    /// One of the writes: `action` is `""` to add or resend, `verify`, `primary` or
    /// `remove`; `fields` are the body's fields beside the token.
    pub async fn email_write(
        &self,
        incoming: &Incoming,
        action: &str,
        token: &str,
        mut fields: serde_json::Map<String, Value>,
    ) -> Result<EmailOutcome, ClientError> {
        fields.insert("token".into(), json!(token));
        let path = if action.is_empty() {
            format!("{REST}/auth/email")
        } else {
            format!("{REST}/auth/email/{action}")
        };
        let r = self
            .post_json(incoming, &path, &Value::Object(fields))
            .await?;
        let body: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        if r.status == StatusCode::OK {
            Ok(Ok(body))
        } else {
            let code = body["code"].as_str().unwrap_or("internal").to_string();
            Ok(Err((code, body)))
        }
    }
}
