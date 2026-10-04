//! The response envelope (mediawiki-compat §2.1, §2.4): JSON, HTTP 200 even for an API
//! error, which travels as `{"error": {"code", "info"}}` with a `MediaWiki-API-Error`
//! header; warnings under `warnings`; `Set-Cookie` when a session was opened or ended.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

/// An Action API error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {info}")]
pub struct ApiError {
    /// The code, `badtoken`.
    pub code: String,
    /// Human-readable.
    pub info: String,
    /// Extra fields beside `code` and `info` (an `editconflict` carries the current
    /// revision, 0012 §4).
    pub extra: Map<String, Value>,
}

impl ApiError {
    /// A new error.
    pub fn new(code: impl Into<String>, info: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            info: info.into(),
            extra: Map::new(),
        }
    }

    /// With an extra field.
    #[must_use]
    pub fn with(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    /// `missingparam`.
    #[must_use]
    pub fn missing_param(name: &str) -> Self {
        Self::new(
            "missingparam",
            format!("The \"{name}\" parameter must be set."),
        )
    }

    /// `badvalue`.
    #[must_use]
    pub fn bad_value(name: &str, value: &str) -> Self {
        Self::new(
            "badvalue",
            format!("Unrecognized value for parameter \"{name}\": {value}."),
        )
    }

    /// `permissiondenied`, naming what is missing (0016 §7; 0024 §8).
    #[must_use]
    pub fn permission_denied(what: &str) -> Self {
        Self::new(
            "permissiondenied",
            format!("The action you have requested is limited to users with the \"{what}\" right."),
        )
    }

    /// `internal_api_error_*` for a failure of ours.
    #[must_use]
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::new("internal_api_error", format!("[internal] {e}"))
    }

    /// The error as the response body.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut e = Map::new();
        e.insert("code".into(), Value::String(self.code.clone()));
        e.insert("info".into(), Value::String(self.info.clone()));
        for (k, v) in &self.extra {
            e.insert(k.clone(), v.clone());
        }
        e.insert(
            "*".into(),
            Value::String(
                "See https://www.mediawiki.org/wiki/API:Errors_and_warnings for notice on API errors."
                    .into(),
            ),
        );
        json!({"error": Value::Object(e)})
    }
}

impl From<tokio_postgres::Error> for ApiError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::internal(e)
    }
}

impl From<deadpool_postgres::PoolError> for ApiError {
    fn from(e: deadpool_postgres::PoolError) -> Self {
        Self::internal(e)
    }
}

impl From<triplespace_accounts::AccountsError> for ApiError {
    fn from(e: triplespace_accounts::AccountsError) -> Self {
        Self::internal(e)
    }
}

impl From<scatter_projection::ProjectionError> for ApiError {
    fn from(e: scatter_projection::ProjectionError) -> Self {
        Self::internal(e)
    }
}

/// What a module produced.
#[derive(Debug, Default)]
pub struct ApiResponse {
    /// The body, without `warnings`.
    pub body: Map<String, Value>,
    /// Warnings by module.
    pub warnings: Vec<(String, String)>,
    /// A `Set-Cookie` header to send.
    pub set_cookie: Option<String>,
    /// The error, if the request failed.
    pub error: Option<ApiError>,
}

impl ApiResponse {
    /// A successful response with a body.
    #[must_use]
    pub fn ok(body: Value) -> Self {
        let body = match body {
            Value::Object(m) => m,
            other => {
                let mut m = Map::new();
                m.insert("result".into(), other);
                m
            }
        };
        Self {
            body,
            ..Self::default()
        }
    }

    /// A failed response.
    #[must_use]
    pub fn err(e: ApiError) -> Self {
        Self {
            error: Some(e),
            ..Self::default()
        }
    }

    /// Adds a warning.
    pub fn warn(&mut self, module: &str, text: &str) {
        self.warnings.push((module.to_string(), text.to_string()));
    }

    /// The JSON body.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut out = match &self.error {
            Some(e) => e.to_value(),
            None => Value::Object(self.body.clone()),
        };
        if !self.warnings.is_empty()
            && let Value::Object(m) = &mut out
        {
            let mut w = Map::new();
            for (module, text) in &self.warnings {
                let entry = w.entry(module.clone()).or_insert_with(|| json!({"*": ""}));
                if let Some(Value::String(s)) = entry.get_mut("*") {
                    if !s.is_empty() {
                        s.push('\n');
                    }
                    s.push_str(text);
                }
            }
            m.insert("warnings".into(), Value::Object(w));
        }
        out
    }
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        let body = self.to_value().to_string();
        let mut r = Response::new(body.into());
        *r.status_mut() = StatusCode::OK;
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        r.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-cache"),
        );
        if let Some(e) = &self.error
            && let Ok(v) = HeaderValue::from_str(&e.code)
        {
            r.headers_mut().insert("MediaWiki-API-Error", v);
        }
        if let Some(c) = &self.set_cookie
            && let Ok(v) = HeaderValue::from_str(c)
        {
            r.headers_mut().insert(header::SET_COOKIE, v);
        }
        r
    }
}
