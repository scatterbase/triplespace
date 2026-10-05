//! Logging in and out through the API (0007 §3; 0012 §4; 0057 §4): tokens, `clientlogin`
//! and `logout`, each with the `Set-Cookie` headers the API sent, which the site relays
//! to the browser verbatim (the cookie belongs to the tenant host, which is the site's
//! host too).

use axum::http::{HeaderValue, Method, header};
use serde_json::Value;

use crate::api::{CacheInfo, Fetched, check};
use crate::transport::{ApiRequest, ApiResponse, ClientError};
use crate::{Client, Incoming, forward};

/// A value, with the cookies the API set while producing it.
#[derive(Debug, Clone)]
pub struct WithCookies<T> {
    /// The value.
    pub value: T,
    /// The `Set-Cookie` headers, to relay.
    pub set_cookies: Vec<HeaderValue>,
}

/// How a login went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginResult {
    /// Logged in, as this name.
    Pass {
        /// The account's name.
        username: String,
    },
    /// Refused: the API's message and its code (`wrongpassword`, …).
    Fail {
        /// The message, in English.
        message: String,
        /// The message code.
        code: String,
    },
    /// The login token did not match the session: it expired, or the browser lost its
    /// cookie.
    BadToken,
}

fn cookies(r: &ApiResponse) -> Vec<HeaderValue> {
    r.headers
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect()
}

impl Client {
    /// Sends a `POST` of an `application/x-www-form-urlencoded` form for the browser's
    /// request `incoming`.
    pub async fn post_form(
        &self,
        incoming: &Incoming,
        path_and_query: &str,
        form: &[(&str, &str)],
    ) -> Result<ApiResponse, ClientError> {
        let mut headers = forward::headers(incoming, self.transport());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        self.transport()
            .send(ApiRequest {
                method: Method::POST,
                path_and_query: path_and_query.to_string(),
                headers,
                body: body.into_bytes(),
                peer: incoming.peer,
            })
            .await
    }

    /// `meta=tokens&type={kind}`: a `login` or `csrf` token for the viewer's session. The
    /// API starts an anonymous session where there is none, and its cookie comes back
    /// with the token.
    pub async fn token(
        &self,
        incoming: &Incoming,
        kind: &str,
    ) -> Result<WithCookies<Fetched<String>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&meta=tokens&type={kind}&format=json&formatversion=2"
                ),
            )
            .await?;
        let v = check(&r)?;
        let token = v["query"]["tokens"][format!("{kind}token")]
            .as_str()
            .ok_or_else(|| ClientError::Unexpected(format!("no {kind} token")))?
            .to_string();
        Ok(WithCookies {
            set_cookies: cookies(&r),
            value: Fetched {
                value: token,
                cache: CacheInfo::of(&r),
            },
        })
    }

    /// `action=clientlogin` with a name, a password and the login token.
    pub async fn client_login(
        &self,
        incoming: &Incoming,
        username: &str,
        password: &str,
        token: &str,
        return_url: &str,
    ) -> Result<WithCookies<LoginResult>, ClientError> {
        let r = self
            .post_form(
                incoming,
                "/w/api.php",
                &[
                    ("action", "clientlogin"),
                    ("format", "json"),
                    ("formatversion", "2"),
                    ("username", username),
                    ("password", password),
                    ("logintoken", token),
                    ("loginreturnurl", return_url),
                ],
            )
            .await?;
        let set_cookies = cookies(&r);
        let value = match check(&r) {
            Ok(v) => login_result(&v),
            Err(ClientError::Api { code, .. }) if code == "badtoken" => LoginResult::BadToken,
            Err(e) => return Err(e),
        };
        Ok(WithCookies { value, set_cookies })
    }

    /// `action=logout` with the CSRF token.
    pub async fn logout(
        &self,
        incoming: &Incoming,
        token: &str,
    ) -> Result<WithCookies<()>, ClientError> {
        let r = self
            .post_form(
                incoming,
                "/w/api.php",
                &[
                    ("action", "logout"),
                    ("format", "json"),
                    ("formatversion", "2"),
                    ("token", token),
                ],
            )
            .await?;
        check(&r)?;
        Ok(WithCookies {
            value: (),
            set_cookies: cookies(&r),
        })
    }
}

/// A `clientlogin` answer.
fn login_result(v: &Value) -> LoginResult {
    let c = &v["clientlogin"];
    match c["status"].as_str() {
        Some("PASS") => LoginResult::Pass {
            username: c["username"].as_str().unwrap_or_default().to_string(),
        },
        _ => LoginResult::Fail {
            message: c["message"].as_str().unwrap_or_default().to_string(),
            code: c["messagecode"].as_str().unwrap_or_default().to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_answers() {
        assert_eq!(
            login_result(
                &serde_json::json!({"clientlogin": {"status": "PASS", "username": "Alice"}})
            ),
            LoginResult::Pass {
                username: "Alice".into()
            }
        );
        assert_eq!(
            login_result(
                &serde_json::json!({"clientlogin": {"status": "FAIL", "message": "no", "messagecode": "wrongpassword"}})
            ),
            LoginResult::Fail {
                message: "no".into(),
                code: "wrongpassword".into()
            }
        );
    }
}
