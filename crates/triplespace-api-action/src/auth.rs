//! Who is asking (0013 §7 step 1; 0024 §4; 0056 §1): the session cookie or the bearer
//! header, resolved through `triplespace-accounts`; the CSRF check every write makes; and
//! the cookie attributes a session is sent with.

use axum::http::HeaderMap;
use scatter_actors::evaluate::Effective;
use tokio_postgres::GenericClient;
use triplespace_accounts::identity::{Auth, Identity, resolve};
use triplespace_accounts::session::COOKIE;
use triplespace_accounts::token;

use crate::app::{App, Mode};
use crate::params::Params;
use crate::response::ApiError;

/// The request's session cookie value, if any.
#[must_use]
pub fn session_cookie(headers: &HeaderMap) -> Option<String> {
    for v in headers.get_all(axum::http::header::COOKIE) {
        let Ok(text) = v.to_str() else { continue };
        for pair in text.split(';') {
            let pair = pair.trim();
            if let Some(value) = pair.strip_prefix(COOKIE).and_then(|r| r.strip_prefix('=')) {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// The bearer credential, if any. A credential in a URL is never accepted (0056 §1).
#[must_use]
pub fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.trim().to_string())
}

/// The resolved caller.
#[derive(Debug, Clone)]
pub struct Caller {
    /// Who.
    pub identity: Identity,
    /// What they may do.
    pub effective: Effective,
}

impl Caller {
    /// Resolves the request's caller on the tenant.
    pub async fn resolve<C: GenericClient>(
        client: &C,
        tenant: &str,
        headers: &HeaderMap,
    ) -> Result<Self, ApiError> {
        let cookie = session_cookie(headers);
        let bearer = bearer(headers);
        let auth = match (&bearer, &cookie) {
            (Some(b), _) => Auth::Bearer(b),
            (None, Some(c)) => Auth::Cookie(c),
            (None, None) => Auth::None,
        };
        let r = resolve(client, tenant, auth).await?;
        Ok(Self {
            identity: r.identity,
            effective: r.effective,
        })
    }

    /// Whether a session is attached (anonymous ones included).
    #[must_use]
    pub fn has_session(&self) -> bool {
        self.identity.session.is_some()
    }

    /// The token of a type for this caller: derived from the session or key; the
    /// anonymous constant without either.
    #[must_use]
    pub fn token(&self, app: &App, token_type: &str) -> String {
        match self.identity.token_binding() {
            Some(b) => app.secret().token(token_type, b),
            None => token::ANONYMOUS.to_string(),
        }
    }

    /// Checks a write's CSRF token (compat §2.5; 0056 §1): required in the body of a POST.
    pub fn check_csrf(&self, app: &App, params: &Params) -> Result<(), ApiError> {
        if !params.posted {
            return Err(ApiError::new(
                "mustbeposted",
                "The module requires a POST request.",
            ));
        }
        let Some(presented) = params.get("token") else {
            return Err(ApiError::missing_param("token"));
        };
        if !params.in_body("token") {
            return Err(ApiError::new(
                "mustpostparams",
                "The following parameter was found in the query string, but must be in the POST body: token.",
            ));
        }
        let ok = match self.identity.token_binding() {
            Some(b) => app.secret().check("csrf", b, presented),
            None => presented == token::ANONYMOUS,
        };
        if ok {
            Ok(())
        } else {
            Err(ApiError::new("badtoken", "Invalid CSRF token."))
        }
    }

    /// `permissiondenied` unless the permission is held.
    pub fn require(&self, permission: &str) -> Result<(), ApiError> {
        if self.effective.holds(permission) {
            Ok(())
        } else {
            Err(ApiError::permission_denied(permission))
        }
    }
}

/// The `Set-Cookie` header for a session (0056 §1): `HttpOnly`, `SameSite=Lax`, `Secure`
/// except in development mode over plain HTTP.
#[must_use]
pub fn set_cookie(app: &App, session_id: &str, max_age_secs: u64) -> String {
    let secure = if app.config().mode == Mode::Development {
        ""
    } else {
        "; Secure"
    };
    format!("{COOKIE}={session_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}")
}

/// The `Set-Cookie` header that ends a session.
#[must_use]
pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}
