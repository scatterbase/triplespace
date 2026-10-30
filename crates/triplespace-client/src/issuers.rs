//! Logging in through external issuers (0088 §5–§7): the pending identity a first login
//! leaves, the account for it, and the holder's sign-in methods. The redirects
//! themselves never pass through the site: its forms post straight to the API's
//! `/auth/{issuer}/start`, and the provider returns to the API (0057 §7).

use axum::http::StatusCode;
use serde::Deserialize;

use crate::api::check;
use crate::register::CreateResult;
use crate::transport::ClientError;
use crate::{Client, Incoming, WithCookies};

const REST: &str = "/w/rest.php/triplespace/v0";

/// The path a login button posts to: the API's start route.
#[must_use]
pub fn start_path(issuer: &str) -> String {
    format!("{REST}/auth/{issuer}/start")
}

/// An issuer the tenant offers, as a button names it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LoginIssuer {
    /// The issuer's code.
    #[serde(alias = "code")]
    pub issuer: String,
    /// What a button names it.
    pub label: String,
    /// The origin of its authorization endpoint, which the page's `form-action` allows.
    #[serde(default)]
    pub origin: Option<String>,
}

/// An identity with no account here, waiting for one (0088 §6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Pending {
    /// The issuer's code.
    pub issuer: String,
    /// The issuer's name.
    pub issuer_name: String,
    /// The provider's username.
    pub username: String,
}

/// One sign-in method.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Binding {
    /// The issuer.
    pub issuer: String,
    /// What it is called.
    pub label: String,
    /// When it was added, seconds since the epoch.
    pub added: u64,
    /// When it was last used.
    #[serde(default)]
    pub last_used: Option<u64>,
    /// Whether the wiki still offers the issuer.
    pub available: bool,
}

/// The holder's account, as `GET /account` answers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AccountInfo {
    /// The name.
    #[serde(default)]
    pub name: Option<String>,
    /// Whether a password is set.
    pub password: bool,
    /// The external bindings.
    pub bindings: Vec<Binding>,
    /// The issuers that can be added.
    pub can_add: Vec<LoginIssuer>,
    /// How many ways to log in there are.
    pub ways: usize,
}

impl Client {
    /// `GET /auth/pending`: the viewer's pending identity, if their session holds one.
    pub async fn pending_identity(
        &self,
        incoming: &Incoming,
    ) -> Result<Option<Pending>, ClientError> {
        let r = self.get(incoming, &format!("{REST}/auth/pending")).await?;
        match r.status {
            StatusCode::OK => serde_json::from_slice(&r.body)
                .map(Some)
                .map_err(|e| ClientError::Unexpected(format!("pending identity: {e}"))),
            StatusCode::NOT_FOUND => Ok(None),
            s => Err(ClientError::Unexpected(format!(
                "pending identity: HTTP {s}"
            ))),
        }
    }

    /// `GET /account`: the holder's sign-in methods; `None` for a reader not logged in.
    pub async fn account(&self, incoming: &Incoming) -> Result<Option<AccountInfo>, ClientError> {
        let r = self.get(incoming, &format!("{REST}/account")).await?;
        match r.status {
            StatusCode::OK => serde_json::from_slice(&r.body)
                .map(Some)
                .map_err(|e| ClientError::Unexpected(format!("account: {e}"))),
            StatusCode::UNAUTHORIZED => Ok(None),
            s => Err(ClientError::Unexpected(format!("account: HTTP {s}"))),
        }
    }

    /// `action=createaccount&createpreservestate=1`: the account for the session's
    /// pending identity, with a name and, under review, a statement.
    pub async fn create_preserved(
        &self,
        incoming: &Incoming,
        username: &str,
        token: &str,
        return_url: &str,
        statement: Option<&str>,
    ) -> Result<WithCookies<CreateResult>, ClientError> {
        let mut form = vec![
            ("action", "createaccount"),
            ("format", "json"),
            ("formatversion", "2"),
            ("username", username),
            ("createpreservestate", "1"),
            ("createtoken", token),
            ("createreturnurl", return_url),
        ];
        if let Some(s) = statement {
            form.push(("tsstatement", s));
        }
        let r = self.post_form(incoming, "/w/api.php", &form).await?;
        let set_cookies = crate::account::cookies(&r);
        let value = match check(&r) {
            Ok(v) => crate::register::create_result(&v),
            Err(ClientError::Api { code, .. }) if code == "badtoken" => CreateResult::BadToken,
            Err(e) => return Err(e),
        };
        Ok(WithCookies { value, set_cookies })
    }

    /// `action=unlinkaccount`: removes a sign-in method. `Ok(Err(code))` when refused
    /// (`ts-last-binding`, `badtoken`).
    pub async fn unlink(
        &self,
        incoming: &Incoming,
        issuer: &str,
        token: &str,
    ) -> Result<Result<(), String>, ClientError> {
        let request = format!("TriplespaceIssuerAuthenticationRequest:{issuer}");
        let r = self
            .post_form(
                incoming,
                "/w/api.php",
                &[
                    ("action", "unlinkaccount"),
                    ("format", "json"),
                    ("formatversion", "2"),
                    ("request", &request),
                    ("token", token),
                ],
            )
            .await?;
        match check(&r) {
            Ok(_) => Ok(Ok(())),
            Err(ClientError::Api { code, .. }) => Ok(Err(code)),
            Err(e) => Err(e),
        }
    }
}
