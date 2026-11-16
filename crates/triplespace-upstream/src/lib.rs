//! The live upstream client (22 §2.1). What is here so far is what a follower needs
//! (0092 §1, §10): one MediaWiki API client per followed wiki that sends one request at a
//! time, identifies itself, asks for `maxlag`, honours `Retry-After`, and optionally logs
//! in with a bot password for the higher limits. The cache, the rate limits and the error
//! mapping of 0012 §6, and the other requests the crate map lists, come with their features.
//!
//! The client is an HTTP client and nothing more: which requests to send, and what their
//! answers mean, are the adapters' (`scatter-adapter-mediawiki`, `scatter-adapter-wikibase`).

#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

/// The `maxlag` a follower asks for (0092 §1): a replica this far behind answers "try
/// later", and the client waits.
pub const MAXLAG: u32 = 5;
/// How long one request may take.
const TIMEOUT: Duration = Duration::from_mins(1);
/// How many times a lagged or rate-limited request is retried before it is an error.
const RETRIES: u32 = 5;
/// The longest the client will wait on one `Retry-After`.
const LONGEST_WAIT: u64 = 120;

/// A client for one wiki's `api.php`.
#[derive(Debug, Clone)]
pub struct Wiki {
    api: String,
    http: reqwest::Client,
    /// One request in flight at a time (0092 §1): a follower is one client of the wiki.
    gate: Arc<Mutex<()>>,
    high_limits: bool,
    user: Option<String>,
}

/// What a login did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedIn {
    /// The account name the wiki reports.
    pub user: String,
    /// Whether the account has `apihighlimits`.
    pub high_limits: bool,
}

/// A request that did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpstreamError {
    /// The client could not be set up, or the request failed to send.
    #[error("upstream: {0}")]
    Transport(String),
    /// The wiki answered with an HTTP status other than 200, after any retries.
    #[error("upstream answered {0}")]
    Status(u16),
    /// The wiki answered an API error, after any retries for `maxlag`.
    #[error("upstream API error `{code}`: {info}")]
    Api {
        /// The error code.
        code: String,
        /// Its message.
        info: String,
    },
    /// A login was refused.
    #[error("login refused: {0}")]
    Login(String),
}

impl Wiki {
    /// A client for the wiki at `api` (`https://…/w/api.php`), identifying as `user_agent`.
    ///
    /// # Errors
    ///
    /// If the TLS backend cannot be set up.
    pub fn new(api: &str, user_agent: &str) -> Result<Self, UpstreamError> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(user_agent)
            .cookie_store(true)
            .build()
            .map_err(|e| UpstreamError::Transport(e.to_string()))?;
        Ok(Wiki {
            api: api.to_string(),
            http,
            gate: Arc::new(Mutex::new(())),
            high_limits: false,
            user: None,
        })
    }

    /// The API endpoint.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// The account the client is logged in as, if any.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    /// Whether the client may ask for the higher limits (`apihighlimits`).
    #[must_use]
    pub fn high_limits(&self) -> bool {
        self.high_limits
    }

    /// The most rows one list request may ask for.
    #[must_use]
    pub fn list_limit(&self) -> u32 {
        if self.high_limits { 5000 } else { 500 }
    }

    /// The most IDs or titles one request may name.
    #[must_use]
    pub fn ids_limit(&self) -> usize {
        if self.high_limits { 500 } else { 50 }
    }

    /// A `GET` with `params`, `format=json`, `formatversion=2` and `maxlag`, answered as the
    /// body text. A lagged replica (`maxlag`), a `429` or a `503` is retried after the wait
    /// the wiki asks for.
    ///
    /// # Errors
    ///
    /// [`UpstreamError`] after the retries, or at once for any other failure.
    pub async fn get(&self, params: &[(&str, String)]) -> Result<String, UpstreamError> {
        self.send(params, false).await
    }

    /// A `POST`, otherwise as [`Wiki::get`].
    ///
    /// # Errors
    ///
    /// As [`Wiki::get`].
    pub async fn post(&self, params: &[(&str, String)]) -> Result<String, UpstreamError> {
        self.send(params, true).await
    }

    async fn send(&self, params: &[(&str, String)], post: bool) -> Result<String, UpstreamError> {
        let _gate = self.gate.lock().await;
        let mut params: Vec<(&str, String)> = params.to_vec();
        for (k, v) in [
            ("format", "json"),
            ("formatversion", "2"),
            ("maxlag", &MAXLAG.to_string()),
        ] {
            if !params.iter().any(|(key, _)| *key == k) {
                params.push((k, v.to_string()));
            }
        }
        let mut tries = 0;
        loop {
            tries += 1;
            let req = if post {
                self.http.post(&self.api).form(&params)
            } else {
                self.http.get(&self.api).query(&params)
            };
            let resp = req
                .send()
                .await
                .map_err(|e| UpstreamError::Transport(e.to_string()))?;
            let status = resp.status().as_u16();
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let body = resp
                .text()
                .await
                .map_err(|e| UpstreamError::Transport(e.to_string()))?;
            let wait = match status {
                200 => match api_error(&body) {
                    Some((code, info)) if code == "maxlag" && tries <= RETRIES => {
                        let _ = info;
                        retry_after.unwrap_or(u64::from(MAXLAG))
                    }
                    Some((code, info)) => return Err(UpstreamError::Api { code, info }),
                    None => return Ok(body),
                },
                429 | 503 if tries <= RETRIES => retry_after.unwrap_or(u64::from(MAXLAG)),
                other => return Err(UpstreamError::Status(other)),
            };
            tokio::time::sleep(Duration::from_secs(wait.clamp(1, LONGEST_WAIT))).await;
        }
    }

    /// Logs in with a bot password (0092 §10) and learns whether the account has the
    /// higher limits. On refusal the client stays anonymous.
    ///
    /// # Errors
    ///
    /// [`UpstreamError::Login`] when the wiki refuses the password; otherwise as
    /// [`Wiki::get`].
    pub async fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<LoggedIn, UpstreamError> {
        let body = self
            .get(&[
                ("action", "query".into()),
                ("meta", "tokens".into()),
                ("type", "login".into()),
            ])
            .await?;
        let token = field(&body, &["query", "tokens", "logintoken"])
            .ok_or_else(|| UpstreamError::Login("no login token".into()))?;
        let body = self
            .post(&[
                ("action", "login".into()),
                ("lgname", username.into()),
                ("lgpassword", password.into()),
                ("lgtoken", token),
            ])
            .await?;
        let result = field(&body, &["login", "result"]).unwrap_or_default();
        if result != "Success" {
            let reason = field(&body, &["login", "reason"]).unwrap_or(result);
            return Err(UpstreamError::Login(reason));
        }
        let user = field(&body, &["login", "lgusername"]).unwrap_or_else(|| username.into());
        let body = self
            .get(&[
                ("action", "query".into()),
                ("meta", "userinfo".into()),
                ("uiprop", "rights".into()),
            ])
            .await?;
        let rights: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let high_limits = rights
            .pointer("/query/userinfo/rights")
            .and_then(|r| r.as_array())
            .is_some_and(|r| r.iter().any(|v| v.as_str() == Some("apihighlimits")));
        self.user = Some(user.clone());
        self.high_limits = high_limits;
        Ok(LoggedIn { user, high_limits })
    }
}

/// The `error.code` and `error.info` of an API error body, if it is one.
fn api_error(body: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let e = v.get("error")?;
    Some((
        e.get("code")?.as_str()?.to_string(),
        e.get("info")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string(),
    ))
}

fn field(body: &str, path: &[&str]) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(body).ok()?;
    for key in path {
        v = v.get(key)?.clone();
    }
    v.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_error_body_is_recognised() {
        assert_eq!(
            api_error(r#"{"error":{"code":"maxlag","info":"Waiting for a replica"}}"#),
            Some(("maxlag".into(), "Waiting for a replica".into()))
        );
        assert_eq!(api_error(r#"{"query":{}}"#), None);
        assert_eq!(api_error("not json"), None);
    }

    #[test]
    fn limits_follow_the_login() {
        let w = Wiki::new("https://example.org/w/api.php", "test").unwrap();
        assert_eq!((w.list_limit(), w.ids_limit()), (500, 50));
        assert!(w.user().is_none());
    }
}
