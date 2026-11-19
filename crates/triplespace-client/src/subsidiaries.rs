//! The operator's subsidiaries and their keys (0024 §8; 18 §3.2): `GET
//! /account/subsidiaries` and the writes `Special:Account`'s Subsidiaries section posts
//! through: create, rename, retire, reactivate, issue a key and revoke one. Each write
//! carries the session's CSRF token; a refusal comes back as its code with the API's
//! body, for the page to show. And `list=users&usprop=operator|groups`, which a
//! subsidiary's user page reads for its identity line (0024 §9, A24).

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use crate::api::{CacheInfo, Fetched, check};
use crate::{Client, ClientError, Incoming};

const REST: &str = "/w/rest.php/triplespace/v0";

/// One API key, as listed: never a secret.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct KeyListing {
    /// The key ID.
    pub key_id: String,
    /// The operator's label.
    pub label: String,
    /// The grants.
    pub grants: Vec<String>,
    /// When it was made, in seconds since the epoch.
    pub created: u64,
    /// When it stops working, if ever.
    #[serde(default)]
    pub expires: Option<u64>,
    /// When it was last used.
    #[serde(default)]
    pub last_used: Option<u64>,
    /// When it was revoked.
    #[serde(default)]
    pub revoked: Option<u64>,
}

/// One subsidiary with its keys.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct SubsidiaryListing {
    /// Its name.
    pub name: String,
    /// Its actor key.
    pub actor: String,
    /// `active`, `retired` or `pending`.
    pub status: String,
    /// Its explicit groups; `bot` is the bot flag.
    #[serde(default)]
    pub groups: Vec<String>,
    /// Its keys, newest first.
    #[serde(default)]
    pub keys: Vec<KeyListing>,
}

/// `GET /account/subsidiaries`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Subsidiaries {
    /// The subsidiaries, by name.
    pub subsidiaries: Vec<SubsidiaryListing>,
    /// Whether the reader may create one (`createaccount`).
    pub can_create: bool,
    /// `subsidiaries.max_per_account`.
    pub max: i64,
    /// How many unretired subsidiaries count against it.
    pub held: i64,
    /// The name the form suggests, `{Name}Bot`.
    pub suggested: String,
    /// The grants a key may carry besides `basic`.
    #[serde(default)]
    pub grants: Vec<GrantListing>,
}

/// A grant the issue form offers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct GrantListing {
    /// Its name.
    pub name: String,
    /// What it is for.
    #[serde(default)]
    pub description: Option<String>,
}

/// A subsidiary as its user page names it: who operates it, and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotOf {
    /// The operator's name.
    pub operator: String,
    /// `active`, `pending` or `retired`.
    pub status: String,
    /// Whether it holds `bot`, the bot flag.
    pub flagged: bool,
    /// Where **Request bot approval** links, while it is not flagged.
    pub request_page: Option<String>,
}

/// A write's answer: the API's body, or the refusal's code with it.
pub type SubsidiaryOutcome = Result<Value, (String, Value)>;

fn outcome(r: &crate::ApiResponse) -> SubsidiaryOutcome {
    let body: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    if r.status.is_success() {
        Ok(body)
    } else {
        let code = body["code"].as_str().unwrap_or("internal").to_string();
        Err((code, body))
    }
}

/// A name as a path segment: spaces as underscores, as titles spell them, and the rest
/// percent-encoded.
fn segment(name: &str) -> String {
    url::form_urlencoded::byte_serialize(name.replace(' ', "_").as_bytes()).collect()
}

/// A query value, percent-encoded.
fn query(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

impl Client {
    /// `list=users&usprop=operator|groups` for one name: `Some` for a subsidiary whose
    /// operator the reader may see, `None` for any other account or none.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn bot_of(
        &self,
        incoming: &Incoming,
        name: &str,
    ) -> Result<Fetched<Option<BotOf>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&list=users&usprop=operator%7Cgroups&ususers={}&format=json&formatversion=2",
                    query(name)
                ),
            )
            .await?;
        let v = check(&r)?;
        let u = &v["query"]["users"][0];
        let value = u["operator"].as_str().map(|op| BotOf {
            operator: op.to_string(),
            status: u["subsidiarystatus"]
                .as_str()
                .unwrap_or("active")
                .to_string(),
            flagged: u["groups"]
                .as_array()
                .is_some_and(|g| g.iter().any(|x| x == "bot")),
            request_page: u["botrequestpage"].as_str().map(str::to_string),
        });
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `GET /account/subsidiaries`; `None` for a reader who operates none, being logged
    /// out, a subsidiary or a temporary account.
    pub async fn subsidiaries(
        &self,
        incoming: &Incoming,
    ) -> Result<Option<Subsidiaries>, ClientError> {
        let r = self
            .get(incoming, &format!("{REST}/account/subsidiaries"))
            .await?;
        match r.status {
            StatusCode::OK => serde_json::from_slice(&r.body)
                .map(Some)
                .map_err(|e| ClientError::Unexpected(format!("subsidiaries: {e}"))),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Ok(None),
            s => Err(ClientError::Unexpected(format!("subsidiaries: HTTP {s}"))),
        }
    }

    /// `POST /account/subsidiaries` with `name`.
    pub async fn create_subsidiary(
        &self,
        incoming: &Incoming,
        name: &str,
        token: &str,
    ) -> Result<SubsidiaryOutcome, ClientError> {
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{REST}/account/subsidiaries"),
                &json!({"name": name, "token": token}),
            )
            .await?;
        Ok(outcome(&r))
    }

    /// `POST /account/subsidiaries/{name}/{action}`: `rename` (with `to`), `retire` or
    /// `reactivate`.
    pub async fn change_subsidiary(
        &self,
        incoming: &Incoming,
        name: &str,
        action: &str,
        to: Option<&str>,
        token: &str,
    ) -> Result<SubsidiaryOutcome, ClientError> {
        let mut body = json!({"token": token});
        if let Some(to) = to {
            body["to"] = json!(to);
        }
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{REST}/account/subsidiaries/{}/{action}", segment(name)),
                &body,
            )
            .await?;
        Ok(outcome(&r))
    }

    /// `POST /account/subsidiaries/{name}/keys`: the answer holds the secret, once.
    pub async fn issue_key(
        &self,
        incoming: &Incoming,
        name: &str,
        label: &str,
        grants: &[String],
        expires_days: Option<u64>,
        token: &str,
    ) -> Result<SubsidiaryOutcome, ClientError> {
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{REST}/account/subsidiaries/{}/keys", segment(name)),
                &json!({
                    "label": label,
                    "grants": grants,
                    "expires_days": expires_days,
                    "token": token,
                }),
            )
            .await?;
        Ok(outcome(&r))
    }

    /// `DELETE /account/subsidiaries/{name}/keys?key_id=…&token=…`.
    pub async fn revoke_key(
        &self,
        incoming: &Incoming,
        name: &str,
        key_id: &str,
        token: &str,
    ) -> Result<SubsidiaryOutcome, ClientError> {
        let r = self
            .send_json(
                incoming,
                Method::DELETE,
                &format!(
                    "{REST}/account/subsidiaries/{}/keys?key_id={}&token={}",
                    segment(name),
                    query(key_id),
                    query(token)
                ),
                &Value::Null,
            )
            .await?;
        Ok(outcome(&r))
    }
}
