//! Login through external issuers (0088 §1–§6; 07 §2.7): the instance as an OAuth 2.0
//! confidential client, with PKCE, registered with each provider for identity only.
//!
//! - **Client credentials** are rows of `private.oauth_client`, one per tenant and issuer,
//!   `tenant = ''` being the instance default. A tenant uses its own row, or else the
//!   default when the default's registered callback is the tenant's callback or a prefix
//!   of it (§3). The secret is sealed ([`crate::seal`]).
//! - **A flow** is a ten-minute handle in `private.oauth_flow`, keyed by the hash of the
//!   `state` sent to the provider: the session it began in, its purpose, the PKCE
//!   verifier and, once the provider has answered, the outcome (§5). A login that found
//!   no account keeps its flow as the session's **pending identity** (§6).
//! - **The provider** is reached through [`Upstream`], whose HTTP implementation is
//!   [`HttpUpstream`]: the code exchange and one profile call, after which both tokens
//!   are dropped (§2); and the source's users API, which vouching asks (§8).

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use scatter_actors::issuer::{Issuer, IssuerRegistry, OAuthEndpoints};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tokio_postgres::GenericClient;

use crate::{AccountsError, Secret, random_token, seal};

/// How long a flow, and a pending identity, lasts (0088 §5).
pub const FLOW_LIFETIME: Duration = Duration::from_mins(10);

/// How long a request to a provider may take (0088 §2).
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The path of the callback under a tenant's base: the URL an operator registers.
#[must_use]
pub fn callback_url(tenant_base: &str, issuer: &str) -> String {
    format!(
        "{}/w/rest.php/triplespace/v0/auth/{issuer}/callback",
        tenant_base.trim_end_matches('/')
    )
}

/// Whether a consumer registered with `registered` can send a person back to
/// `callback`: the same URL, or one it is a prefix of (0088 §3).
#[must_use]
pub fn callback_serves(registered: &str, callback: &str) -> bool {
    !registered.is_empty() && callback.starts_with(registered)
}

const SEAL_TABLE: &str = "private.oauth_client";

/// A client credential, as the CLI lists it: never the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRow {
    /// The tenant, `""` for the instance default.
    pub tenant: String,
    /// The issuer.
    pub issuer: String,
    /// The client ID.
    pub client_id: String,
    /// The callback the consumer was registered with.
    pub callback: String,
}

/// A credential ready to use.
#[derive(Clone, PartialEq, Eq)]
pub struct Client {
    /// The client ID.
    pub client_id: String,
    /// The client secret, opened.
    pub secret: String,
    /// The tenant's own callback URL, the `redirect_uri` of its flows.
    pub redirect_uri: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

/// Stores (or replaces) a client credential, sealing its secret.
///
/// # Errors
///
/// [`AccountsError::Invalid`] for an issuer that does not serve login through OAuth;
/// otherwise a store or random-source error.
#[allow(clippy::too_many_arguments)]
pub async fn set_client<C: GenericClient>(
    client: &C,
    secret: &Secret,
    registry: &IssuerRegistry,
    tenant: &str,
    issuer: &str,
    client_id: &str,
    client_secret: &str,
    callback: &str,
) -> Result<(), AccountsError> {
    if registry.by_code(issuer).is_none_or(|i| i.oauth.is_none()) {
        return Err(AccountsError::Invalid(format!(
            "`{issuer}` does not serve login through OAuth; the issuers that do: {}",
            registry
                .oauth_providers()
                .map(|i| i.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let (client_id, client_secret) = (client_id.trim(), client_secret.trim());
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(AccountsError::Invalid(
            "a client ID and a secret are both needed".into(),
        ));
    }
    let sealed = seal::seal(
        secret,
        &[SEAL_TABLE, tenant, issuer],
        client_secret.as_bytes(),
    )?;
    client
        .execute(
            "INSERT INTO private.oauth_client (tenant, issuer, client_id, secret, callback)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant, issuer) DO UPDATE SET client_id = EXCLUDED.client_id,
               secret = EXCLUDED.secret, callback = EXCLUDED.callback, updated = now()",
            &[&tenant, &issuer, &client_id, &sealed, &callback],
        )
        .await?;
    Ok(())
}

/// Removes a client credential. `false` when there was none.
///
/// # Errors
///
/// If the store fails.
pub async fn remove_client<C: GenericClient>(
    client: &C,
    tenant: &str,
    issuer: &str,
) -> Result<bool, AccountsError> {
    Ok(client
        .execute(
            "DELETE FROM private.oauth_client WHERE tenant = $1 AND issuer = $2",
            &[&tenant, &issuer],
        )
        .await?
        == 1)
}

/// Every client credential, the instance defaults first.
///
/// # Errors
///
/// If the store fails.
pub async fn list_clients<C: GenericClient>(client: &C) -> Result<Vec<ClientRow>, AccountsError> {
    let rows = client
        .query(
            "SELECT tenant, issuer, client_id, callback FROM private.oauth_client
             ORDER BY tenant <> '', tenant, issuer",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| ClientRow {
            tenant: r.get(0),
            issuer: r.get(1),
            client_id: r.get(2),
            callback: r.get(3),
        })
        .collect())
}

/// Which row a tenant resolves to for an issuer: its own, else the instance default
/// when the default's callback serves the tenant's (0088 §3).
///
/// # Errors
///
/// If the store fails.
pub async fn resolve_row<C: GenericClient>(
    client: &C,
    tenant: &str,
    tenant_base: &str,
    issuer: &str,
) -> Result<Option<ClientRow>, AccountsError> {
    let callback = callback_url(tenant_base, issuer);
    let rows = client
        .query(
            "SELECT tenant, issuer, client_id, callback FROM private.oauth_client
             WHERE issuer = $2 AND tenant IN ($1, '') ORDER BY tenant = '' ",
            &[&tenant, &issuer],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| ClientRow {
            tenant: r.get(0),
            issuer: r.get(1),
            client_id: r.get(2),
            callback: r.get(3),
        })
        .find(|row| !row.tenant.is_empty() || callback_serves(&row.callback, &callback)))
}

/// The credential a tenant uses for an issuer, opened, or `None` when it has none.
///
/// # Errors
///
/// [`AccountsError::Sealed`] when the secret does not open; a store error.
pub async fn client_for<C: GenericClient>(
    client: &C,
    secret: &Secret,
    tenant: &str,
    tenant_base: &str,
    issuer: &str,
) -> Result<Option<Client>, AccountsError> {
    let Some(row) = resolve_row(client, tenant, tenant_base, issuer).await? else {
        return Ok(None);
    };
    let sealed: Vec<u8> = client
        .query_one(
            "SELECT secret FROM private.oauth_client WHERE tenant = $1 AND issuer = $2",
            &[&row.tenant, &issuer],
        )
        .await?
        .get(0);
    let opened = seal::open(secret, &[SEAL_TABLE, &row.tenant, issuer], &sealed)?;
    Ok(Some(Client {
        client_id: row.client_id,
        secret: String::from_utf8(opened).map_err(|_| AccountsError::Sealed)?,
        redirect_uri: callback_url(tenant_base, issuer),
    }))
}

/// Whether `login.issuers` lists an issuer: `*` lists every one (0088 §4).
#[must_use]
pub fn listed(login_issuers: &[String], code: &str) -> bool {
    login_issuers.iter().any(|c| c == "*" || c == code)
}

/// The issuers a tenant offers for login, in registry order: those that serve login
/// through OAuth, have a credential that resolves, and that `login.issuers` lists.
///
/// # Errors
///
/// If the store fails.
pub async fn offered<'r, C: GenericClient>(
    client: &C,
    registry: &'r IssuerRegistry,
    tenant: &str,
    tenant_base: &str,
    login_issuers: &[String],
) -> Result<Vec<&'r Issuer>, AccountsError> {
    let mut out = Vec::new();
    for i in registry.oauth_providers() {
        if listed(login_issuers, &i.code)
            && resolve_row(client, tenant, tenant_base, &i.code)
                .await?
                .is_some()
        {
            out.push(i);
        }
    }
    Ok(out)
}

/// What a flow is for (0088 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Logging in, which may end in a pending identity.
    Login,
    /// Creating an account with a name chosen first.
    Create,
    /// Adding a sign-in method to the session's account.
    Link,
}

impl Purpose {
    /// The column's word.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Create => "create",
            Self::Link => "link",
        }
    }

    /// From the column's word.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "login" => Some(Self::Login),
            "create" => Some(Self::Create),
            "link" => Some(Self::Link),
            _ => None,
        }
    }
}

/// Which door a flow came through (0088 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Door {
    /// The REST routes the site's forms post to.
    Site,
    /// The Action API's AuthManager modules.
    Action,
}

impl Door {
    fn as_str(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Action => "action",
        }
    }
}

/// What the provider told a flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Who the person is there.
    Identity {
        /// The normalized subject.
        subject: String,
        /// Their username there.
        username: String,
    },
    /// Why not: a message code (`ts-oauth-denied`, `ts-oauth-failed`).
    Error(String),
}

impl Outcome {
    fn to_json(&self) -> Value {
        match self {
            Self::Identity { subject, username } => {
                json!({"subject": subject, "username": username})
            }
            Self::Error(code) => json!({"error": code}),
        }
    }

    fn from_json(v: &Value) -> Option<Self> {
        if let Some(code) = v["error"].as_str() {
            return Some(Self::Error(code.to_string()));
        }
        Some(Self::Identity {
            subject: v["subject"].as_str()?.to_string(),
            username: v["username"].as_str().unwrap_or_default().to_string(),
        })
    }
}

/// A flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    /// The hash of its `state`, which names it.
    pub state_hash: String,
    /// The tenant.
    pub tenant: String,
    /// The session it began in.
    pub session: String,
    /// The issuer.
    pub issuer: String,
    /// What it is for.
    pub purpose: Purpose,
    /// Which door it came through.
    pub door: Door,
    /// The PKCE code verifier.
    pub verifier: String,
    /// The name chosen, for [`Purpose::Create`].
    pub name: Option<String>,
    /// Where to return.
    pub returnto: Option<String>,
    /// What the provider answered, once it has.
    pub outcome: Option<Outcome>,
}

/// A flow about to begin.
#[derive(Debug, Clone, Copy)]
pub struct NewFlow<'a> {
    /// The tenant.
    pub tenant: &'a str,
    /// The session it begins in.
    pub session: &'a str,
    /// The issuer.
    pub issuer: &'a str,
    /// What it is for.
    pub purpose: Purpose,
    /// Which door.
    pub door: Door,
    /// The name chosen, for [`Purpose::Create`].
    pub name: Option<&'a str>,
    /// Where to return.
    pub returnto: Option<&'a str>,
}

/// The SHA-256 of a `state`, hex.
#[must_use]
pub fn state_hash(state: &str) -> String {
    Sha256::digest(state.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The PKCE `S256` challenge of a verifier (RFC 7636 §4.2).
#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The provider's authorization URL for a flow.
#[must_use]
pub fn authorize_url(
    endpoints: &OAuthEndpoints,
    client: &Client,
    state: &str,
    verifier: &str,
) -> String {
    let mut u = url::Url::parse(&endpoints.authorize)
        .unwrap_or_else(|_| url::Url::parse("https://invalid.invalid/").expect("a URL"));
    u.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", &client.redirect_uri)
        .append_pair("state", state)
        .append_pair("code_challenge", &pkce_challenge(verifier))
        .append_pair("code_challenge_method", "S256");
    u.to_string()
}

/// Begins a flow: stores its handle and returns the `state` and the URL to send the
/// person to.
///
/// # Errors
///
/// If the store or the random source fails.
pub async fn begin<C: GenericClient>(
    client: &C,
    flow: NewFlow<'_>,
    endpoints: &OAuthEndpoints,
    credential: &Client,
) -> Result<(String, String), AccountsError> {
    sweep(client).await?;
    let state = random_token(32)?;
    let verifier = random_token(48)?;
    let expires = SystemTime::now() + FLOW_LIFETIME;
    client
        .execute(
            "INSERT INTO private.oauth_flow
               (state_hash, tenant, session, issuer, purpose, door, verifier, name, returnto, expires)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            &[
                &state_hash(&state),
                &flow.tenant,
                &flow.session,
                &flow.issuer,
                &flow.purpose.as_str(),
                &flow.door.as_str(),
                &verifier,
                &flow.name,
                &flow.returnto,
                &expires,
            ],
        )
        .await?;
    let url = authorize_url(endpoints, credential, &state, &verifier);
    Ok((state, url))
}

fn flow_of(r: &tokio_postgres::Row) -> Option<Flow> {
    let outcome: Option<Value> = r.get("outcome");
    Some(Flow {
        state_hash: r.get("state_hash"),
        tenant: r.get("tenant"),
        session: r.get("session"),
        issuer: r.get("issuer"),
        purpose: Purpose::parse(r.get("purpose"))?,
        door: if r.get::<_, &str>("door") == "action" {
            Door::Action
        } else {
            Door::Site
        },
        verifier: r.get("verifier"),
        name: r.get("name"),
        returnto: r.get("returnto"),
        outcome: outcome.as_ref().and_then(Outcome::from_json),
    })
}

const FLOW_COLUMNS: &str =
    "state_hash, tenant, session, issuer, purpose, door, verifier, name, returnto, outcome";

/// A live flow on a tenant by its `state`.
///
/// # Errors
///
/// If the store fails.
pub async fn find<C: GenericClient>(
    client: &C,
    tenant: &str,
    state: &str,
) -> Result<Option<Flow>, AccountsError> {
    let row = client
        .query_opt(
            &format!(
                "SELECT {FLOW_COLUMNS} FROM private.oauth_flow
                 WHERE state_hash = $1 AND tenant = $2 AND expires > now()"
            ),
            &[&state_hash(state), &tenant],
        )
        .await?;
    Ok(row.as_ref().and_then(flow_of))
}

/// Records what the provider answered.
///
/// # Errors
///
/// If the store fails.
pub async fn set_outcome<C: GenericClient>(
    client: &C,
    state_hash: &str,
    outcome: &Outcome,
) -> Result<(), AccountsError> {
    client
        .execute(
            "UPDATE private.oauth_flow SET outcome = $2 WHERE state_hash = $1",
            &[&state_hash, &outcome.to_json()],
        )
        .await?;
    Ok(())
}

/// Ends a flow.
///
/// # Errors
///
/// If the store fails.
pub async fn finish<C: GenericClient>(client: &C, state_hash: &str) -> Result<(), AccountsError> {
    client
        .execute(
            "DELETE FROM private.oauth_flow WHERE state_hash = $1",
            &[&state_hash],
        )
        .await?;
    Ok(())
}

/// Keeps a login's identity as the session's pending identity (0088 §6): the flow lives
/// on, under the session, until it expires. Any other pending identity of the session
/// is dropped.
///
/// # Errors
///
/// If the store fails.
pub async fn keep_pending<C: GenericClient>(
    client: &C,
    flow: &Flow,
    session: &str,
) -> Result<(), AccountsError> {
    client
        .execute(
            "DELETE FROM private.oauth_flow
             WHERE tenant = $1 AND session = $2 AND state_hash <> $3 AND purpose = 'login'
               AND outcome ? 'subject'",
            &[&flow.tenant, &session, &flow.state_hash],
        )
        .await?;
    client
        .execute(
            "UPDATE private.oauth_flow SET session = $2, purpose = 'login' WHERE state_hash = $1",
            &[&flow.state_hash, &session],
        )
        .await?;
    Ok(())
}

/// The session's pending identity: a login flow whose provider named someone with no
/// account here (0088 §6).
///
/// # Errors
///
/// If the store fails.
pub async fn pending<C: GenericClient>(
    client: &C,
    tenant: &str,
    session: &str,
) -> Result<Option<Flow>, AccountsError> {
    let row = client
        .query_opt(
            &format!(
                "SELECT {FLOW_COLUMNS} FROM private.oauth_flow
                 WHERE tenant = $1 AND session = $2 AND purpose = 'login'
                   AND outcome ? 'subject' AND expires > now()
                 ORDER BY expires DESC LIMIT 1"
            ),
            &[&tenant, &session],
        )
        .await?;
    Ok(row.as_ref().and_then(flow_of))
}

/// Drops expired flows.
///
/// # Errors
///
/// If the store fails.
pub async fn sweep<C: GenericClient>(client: &C) -> Result<u64, AccountsError> {
    Ok(client
        .execute("DELETE FROM private.oauth_flow WHERE expires <= now()", &[])
        .await?)
}

/// Who the provider says a token's holder is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// `sub`, as the provider wrote it.
    pub sub: String,
    /// `username`.
    pub username: String,
}

/// A user on a source wiki, as `list=users&usprop=centralids` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceUser {
    /// The wiki's own user ID.
    pub userid: u64,
    /// The CentralAuth ID, where the wiki has CentralAuth.
    pub central: Option<u64>,
}

/// Why a provider or a source wiki could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct UpstreamError(pub String);

/// A boxed future, for [`Upstream`]'s object safety.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a flow asks of the world outside: the provider's token and profile endpoints,
/// and a source wiki's users API (0088 §2, §8). Tests put a fake behind it.
pub trait Upstream: Send + Sync {
    /// Exchanges a code for a token, asks the profile endpoint who holds it, and drops
    /// the token.
    fn identify<'a>(
        &'a self,
        endpoints: &'a OAuthEndpoints,
        client: &'a Client,
        code: &'a str,
        verifier: &'a str,
    ) -> BoxFuture<'a, Result<Profile, UpstreamError>>;

    /// Looks a user up by name on a wiki's users API, with its central IDs.
    fn source_user<'a>(
        &'a self,
        users_api: &'a str,
        username: &'a str,
    ) -> BoxFuture<'a, Result<Option<SourceUser>, UpstreamError>>;
}

/// [`Upstream`] over HTTP.
#[derive(Debug, Clone)]
pub struct HttpUpstream {
    http: reqwest::Client,
}

impl HttpUpstream {
    /// A client with the 10-second timeout of 0088 §2.
    ///
    /// # Errors
    ///
    /// If the TLS backend cannot be set up.
    pub fn new(user_agent: &str) -> Result<Self, UpstreamError> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| UpstreamError(e.to_string()))?;
        Ok(Self { http })
    }
}

fn upstream(e: impl std::fmt::Display) -> UpstreamError {
    UpstreamError(e.to_string())
}

impl Upstream for HttpUpstream {
    fn identify<'a>(
        &'a self,
        endpoints: &'a OAuthEndpoints,
        client: &'a Client,
        code: &'a str,
        verifier: &'a str,
    ) -> BoxFuture<'a, Result<Profile, UpstreamError>> {
        Box::pin(async move {
            let token: Value = self
                .http
                .post(&endpoints.token)
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("redirect_uri", client.redirect_uri.as_str()),
                    ("client_id", client.client_id.as_str()),
                    ("client_secret", client.secret.as_str()),
                    ("code_verifier", verifier),
                ])
                .send()
                .await
                .map_err(upstream)?
                .error_for_status()
                .map_err(upstream)?
                .json()
                .await
                .map_err(upstream)?;
            let access = token["access_token"]
                .as_str()
                .ok_or_else(|| UpstreamError("the token response has no access_token".into()))?;
            let profile: Value = self
                .http
                .get(&endpoints.profile)
                .bearer_auth(access)
                .send()
                .await
                .map_err(upstream)?
                .error_for_status()
                .map_err(upstream)?
                .json()
                .await
                .map_err(upstream)?;
            let sub = match &profile["sub"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(UpstreamError("the profile has no sub".into())),
            };
            Ok(Profile {
                sub,
                username: profile["username"].as_str().unwrap_or_default().to_string(),
            })
        })
    }

    fn source_user<'a>(
        &'a self,
        users_api: &'a str,
        username: &'a str,
    ) -> BoxFuture<'a, Result<Option<SourceUser>, UpstreamError>> {
        Box::pin(async move {
            let v: Value = self
                .http
                .get(users_api)
                .query(&[
                    ("action", "query"),
                    ("list", "users"),
                    ("ususers", username),
                    ("usprop", "centralids"),
                    ("format", "json"),
                    ("formatversion", "2"),
                ])
                .send()
                .await
                .map_err(upstream)?
                .error_for_status()
                .map_err(upstream)?
                .json()
                .await
                .map_err(upstream)?;
            Ok(parse_source_user(&v))
        })
    }
}

/// Reads `list=users&usprop=centralids`'s first user.
#[must_use]
pub fn parse_source_user(v: &Value) -> Option<SourceUser> {
    let u = v["query"]["users"].get(0)?;
    if u.get("missing").is_some() || u.get("invalid").is_some() {
        return None;
    }
    Some(SourceUser {
        userid: u["userid"].as_u64()?,
        central: u["centralids"]["CentralAuth"].as_u64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callbacks_and_prefixes() {
        let cb = callback_url("https://librarybase.org/", "wikimedia-central");
        assert_eq!(
            cb,
            "https://librarybase.org/w/rest.php/triplespace/v0/auth/wikimedia-central/callback"
        );
        assert!(callback_serves(&cb, &cb));
        assert!(callback_serves("https://librarybase.org/w/rest.php/", &cb));
        assert!(!callback_serves(
            &callback_url("https://example.wiki", "wikimedia-central"),
            &cb
        ));
        assert!(!callback_serves("", &cb));
    }

    #[test]
    fn pkce_is_rfc_7636() {
        // RFC 7636 appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_authorize_url_carries_the_flow() {
        let ep = OAuthEndpoints {
            authorize: "https://meta.wikimedia.org/w/rest.php/oauth2/authorize".into(),
            token: "https://meta.wikimedia.org/w/rest.php/oauth2/access_token".into(),
            profile: "https://meta.wikimedia.org/w/rest.php/oauth2/resource/profile".into(),
            subject: scatter_actors::issuer::SubjectKind::Central,
            register: None,
            label: "Wikimedia".into(),
        };
        let c = Client {
            client_id: "abc".into(),
            secret: "s".into(),
            redirect_uri: "https://w.example/cb".into(),
        };
        let u = url::Url::parse(&authorize_url(&ep, &c, "st", "ver")).unwrap();
        let q: std::collections::BTreeMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "abc");
        assert_eq!(q["redirect_uri"], "https://w.example/cb");
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], pkce_challenge("ver"));
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(
            !format!("{c:?}").contains("secret: "),
            "the secret is not printed"
        );
    }

    #[test]
    fn listed_and_source_users() {
        assert!(listed(&["*".into()], "wikimedia-central"));
        assert!(!listed(&[], "wikimedia-central"));
        assert!(listed(&["wikimedia-central".into()], "wikimedia-central"));
        let v = json!({"query": {"users": [{"userid": 45, "name": "A", "centralids": {"CentralAuth": 99, "local": 45}}]}});
        assert_eq!(
            parse_source_user(&v),
            Some(SourceUser {
                userid: 45,
                central: Some(99)
            })
        );
        let missing = json!({"query": {"users": [{"name": "B", "missing": true}]}});
        assert_eq!(parse_source_user(&missing), None);
    }

    #[test]
    fn outcomes_round_trip() {
        for o in [
            Outcome::Identity {
                subject: "7".into(),
                username: "A".into(),
            },
            Outcome::Error("ts-oauth-denied".into()),
        ] {
            assert_eq!(Outcome::from_json(&o.to_json()), Some(o.clone()));
        }
    }
}
