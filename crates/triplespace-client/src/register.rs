//! Registration through the API (0087 §5, §6, §9): what the account-creation and login
//! forms need, `action=createaccount`, and the review of account requests, each with the
//! `Set-Cookie` headers the API sent, which the site relays.
//!
//! The site never solves a challenge itself: the browser does, and the form carries the
//! solution back. The site only reads the challenge the API issued to the browser's
//! session and passes the answer on.

use axum::http::{HeaderValue, Method, header};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{CacheInfo, check};
use crate::settings::{Saved, saved};
use crate::transport::{ApiResponse, ClientError};
use crate::{Client, Incoming, WithCookies};

const REST: &str = "/w/rest.php/triplespace/v0";

fn cookies(r: &ApiResponse) -> Vec<HeaderValue> {
    r.headers
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect()
}

/// A challenge the API issued to the browser's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeInfo {
    /// The challenge string, `tschallengeid`.
    pub id: String,
    /// The difficulty in bits.
    pub bits: u64,
}

impl ChallengeInfo {
    fn from_fields(fields: &Value) -> Option<Self> {
        let id = fields["tschallengeid"]["value"].as_str()?.to_string();
        // The bits are the fourth field of the challenge string (0087 §3).
        let bits = id.split('.').nth(3)?.parse().ok()?;
        Some(Self { id, bits })
    }

    fn from_answer(c: &Value) -> Option<Self> {
        Some(Self {
            id: c["id"].as_str()?.to_string(),
            bits: c["bits"].as_u64()?,
        })
    }
}

/// What the account-creation form needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateForm {
    /// The `createaccount` token.
    pub token: String,
    /// Whether the viewer may create an account (`createaccount`).
    pub can_create: bool,
    /// The challenge, where the act is guarded for the viewer.
    pub challenge: Option<ChallengeInfo>,
    /// Where the wiki reviews registrations: whether a statement is `optional` or
    /// `required`.
    pub statement: Option<String>,
    /// The caching facts.
    pub cache: CacheInfo,
}

/// What the login form needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginForm {
    /// The login token.
    pub token: String,
    /// The `badlogin` challenge, where the viewer's address has failed too often.
    pub challenge: Option<ChallengeInfo>,
}

/// How a registration went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateResult {
    /// Created, under this name.
    Pass {
        /// The account's name, normalized.
        username: String,
    },
    /// Filed as a request for review (`ts-account-requested`).
    Requested,
    /// The challenge's solution is wanted: a fresh challenge.
    Challenge(ChallengeInfo),
    /// Refused: the message code (`userexists`, `badretype`, …) and the API's message.
    Fail {
        /// The message code.
        code: String,
        /// The message, in English.
        message: String,
    },
    /// The token did not match the session.
    BadToken,
}

/// The fields of a registration.
#[derive(Debug, Clone, Default)]
pub struct CreateFields<'a> {
    /// The name.
    pub username: &'a str,
    /// The password.
    pub password: &'a str,
    /// The password again.
    pub retype: &'a str,
    /// The `createaccount` token.
    pub token: &'a str,
    /// Where to return: required by the API unless continuing.
    pub return_url: &'a str,
    /// The challenge answered, and its solution.
    pub challenge: Option<(&'a str, &'a str)>,
    /// The statement of a request.
    pub statement: Option<&'a str>,
}

/// An open account request, as a reviewer sees it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AccountRequest {
    /// Its ID.
    pub id: String,
    /// The name asked for.
    pub name: String,
    /// The statement.
    #[serde(default)]
    pub statement: Option<String>,
    /// When it was filed, seconds since the epoch.
    pub filed: i64,
    /// When it expires.
    pub expires: i64,
    /// The address key, for holders of `ts-viewip`.
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Deserialize)]
struct RequestList {
    requests: Vec<AccountRequest>,
}

impl Client {
    /// The `createaccount` token and the form's requests, in one call:
    /// `meta=tokens|authmanagerinfo` (0087 §9). The API starts a session where there is
    /// none and binds the challenge to it; its cookie comes back.
    pub async fn create_form(
        &self,
        incoming: &Incoming,
    ) -> Result<WithCookies<CreateForm>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=tokens|authmanagerinfo&type=createaccount&amirequestsfor=create&amimergerequestfields=1&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let info = &v["query"]["authmanagerinfo"];
        let statement = info["requests"].as_array().and_then(|rs| {
            rs.iter()
                .find(|r| r["id"] == "TriplespaceAccountRequestAuthenticationRequest")
                .and_then(|r| r["metadata"]["statement"].as_str().map(str::to_owned))
        });
        let token = v["query"]["tokens"]["createaccounttoken"]
            .as_str()
            .ok_or_else(|| ClientError::Unexpected("no createaccount token".into()))?
            .to_string();
        Ok(WithCookies {
            set_cookies: cookies(&r),
            value: CreateForm {
                token,
                can_create: info["cancreateaccounts"].as_bool().unwrap_or(false),
                challenge: ChallengeInfo::from_fields(&info["fields"]),
                statement,
                cache: CacheInfo::of(&r),
            },
        })
    }

    /// The login token and, where the viewer's address has failed too often, the
    /// `badlogin` challenge.
    pub async fn login_form(
        &self,
        incoming: &Incoming,
    ) -> Result<WithCookies<LoginForm>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=tokens|authmanagerinfo&type=login&amirequestsfor=login&amimergerequestfields=1&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let token = v["query"]["tokens"]["logintoken"]
            .as_str()
            .ok_or_else(|| ClientError::Unexpected("no login token".into()))?
            .to_string();
        Ok(WithCookies {
            set_cookies: cookies(&r),
            value: LoginForm {
                token,
                challenge: ChallengeInfo::from_fields(&v["query"]["authmanagerinfo"]["fields"]),
            },
        })
    }

    /// `action=createaccount`.
    pub async fn create_account(
        &self,
        incoming: &Incoming,
        f: &CreateFields<'_>,
    ) -> Result<WithCookies<CreateResult>, ClientError> {
        let mut form = vec![
            ("action", "createaccount"),
            ("format", "json"),
            ("formatversion", "2"),
            ("username", f.username),
            ("password", f.password),
            ("retype", f.retype),
            ("createtoken", f.token),
            ("createreturnurl", f.return_url),
        ];
        if let Some((id, solution)) = f.challenge {
            form.push(("tschallengeid", id));
            form.push(("tschallengesolution", solution));
        }
        if let Some(s) = f.statement {
            form.push(("tsstatement", s));
        }
        let r = self.post_form(incoming, "/w/api.php", &form).await?;
        let set_cookies = cookies(&r);
        let value = match check(&r) {
            Ok(v) => create_result(&v),
            Err(ClientError::Api { code, .. }) if code == "badtoken" => CreateResult::BadToken,
            Err(e) => return Err(e),
        };
        Ok(WithCookies { value, set_cookies })
    }

    /// `GET /account/requests`: the open requests, for a holder of `confirmaccount`.
    pub async fn account_requests(
        &self,
        incoming: &Incoming,
    ) -> Result<Vec<AccountRequest>, ClientError> {
        let r = self
            .get(incoming, &format!("{REST}/account/requests"))
            .await?;
        match saved::<RequestList>(&r, "account requests")? {
            Saved::Ok(l) => Ok(l.requests),
            Saved::Refused(e) => Err(ClientError::Api {
                code: e.first().map(|x| x.code.clone()).unwrap_or_default(),
                info: e.first().map(|x| x.message.clone()).unwrap_or_default(),
            }),
        }
    }

    /// `POST /account/requests/{id}/approve` or `…/reject` with a reason.
    pub async fn decide_request(
        &self,
        incoming: &Incoming,
        id: &str,
        approve: bool,
        token: &str,
        reason: &str,
    ) -> Result<Saved<Value>, ClientError> {
        let path: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        let (verb, body) = if approve {
            ("approve", json!({"token": token}))
        } else {
            ("reject", json!({"token": token, "reason": reason}))
        };
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{REST}/account/requests/{path}/{verb}"),
                &body,
            )
            .await?;
        saved(&r, "the decision")
    }
}

/// A `createaccount` answer.
fn create_result(v: &Value) -> CreateResult {
    let c = &v["createaccount"];
    let code = c["messagecode"].as_str().unwrap_or_default().to_string();
    match c["status"].as_str() {
        Some("PASS") => CreateResult::Pass {
            username: c["username"].as_str().unwrap_or_default().to_string(),
        },
        Some("UI") => ChallengeInfo::from_answer(&c["challenge"]).map_or_else(
            || CreateResult::Fail {
                code: code.clone(),
                message: c["message"].as_str().unwrap_or_default().to_string(),
            },
            CreateResult::Challenge,
        ),
        _ if code == "ts-account-requested" => CreateResult::Requested,
        _ => CreateResult::Fail {
            code,
            message: c["message"].as_str().unwrap_or_default().to_string(),
        },
    }
}

/// The challenge a `clientlogin` answer of status `UI` asks for.
pub(crate) fn login_challenge(c: &Value) -> Option<ChallengeInfo> {
    ChallengeInfo::from_answer(&c["challenge"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers() {
        assert_eq!(
            create_result(&json!({"createaccount": {"status": "PASS", "username": "Bob"}})),
            CreateResult::Pass {
                username: "Bob".into()
            }
        );
        assert_eq!(
            create_result(
                &json!({"createaccount": {"status": "FAIL", "messagecode": "ts-account-requested"}})
            ),
            CreateResult::Requested
        );
        assert_eq!(
            create_result(
                &json!({"createaccount": {"status": "UI", "messagecode": "ts-challenge-required",
                "challenge": {"id": "1.w.createaccount.8.9.s.m", "bits": 8}}})
            ),
            CreateResult::Challenge(ChallengeInfo {
                id: "1.w.createaccount.8.9.s.m".into(),
                bits: 8
            })
        );
        assert_eq!(
            ChallengeInfo::from_fields(
                &json!({"tschallengeid": {"value": "1.w.createaccount.12.9.s.m"}})
            )
            .map(|c| c.bits),
            Some(12)
        );
    }
}
