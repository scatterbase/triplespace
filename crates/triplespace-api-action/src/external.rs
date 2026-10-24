//! Login through external issuers (0088 §4–§8; 07 §2.7): what both doors share. The
//! site's door is the REST routes (`triplespace-api-rest`), the Action API's the
//! AuthManager modules ([`crate::modules::account`], [`crate::modules::login`]); each
//! begins a flow with [`start`], the provider returns to the REST callback, which runs
//! [`answer`], and the outcome is applied in the session that began the flow by
//! [`conclude`].

use scatter_actors::issuer::{Issuer, IssuerRegistry};
use serde_json::Value;
use triplespace_accounts::login::{LoginError, open_for};
use triplespace_accounts::oauth::{self, Door, Flow, NewFlow, Outcome, Purpose};
use triplespace_accounts::requests::{self, Outcome as RequestOutcome};
use triplespace_accounts::session::Session;
use triplespace_accounts::{bindings, challenge, password};
use triplespace_api_ingest::accounts::{CreateError, Credential, create, reclaim};

use crate::app::App;
use crate::response::ApiError;
use crate::tenant::Tenant;

/// A tenant's login settings (0088 §4, §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPolicy {
    /// `login.issuers`.
    pub issuers: Vec<String>,
    /// `login.password`.
    pub password: bool,
    /// `reclaim.vouching`: the issuer that vouches, or `None` for `off`.
    pub vouching: Option<String>,
}

impl LoginPolicy {
    /// The tenant's settings, resolved through the instance's defaults and locks.
    ///
    /// # Errors
    ///
    /// If the store fails.
    pub async fn load(db: &tokio_postgres::Client, tenant: &str) -> Result<Self, ApiError> {
        let (settings, _) = triplespace_accounts::settings::load(db, Some(tenant)).await?;
        Ok(Self {
            issuers: settings
                .value("login.issuers")
                .and_then(|v| {
                    v.as_array().map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(str::to_owned))
                            .collect()
                    })
                })
                .unwrap_or_else(|| vec!["*".into()]),
            password: settings
                .value("login.password")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            vouching: settings
                .value("reclaim.vouching")
                .and_then(|v| v.as_str().map(str::to_owned))
                .filter(|v| v != "off" && !v.is_empty()),
        })
    }
}

/// The scope a flow runs in.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    /// The state.
    pub app: &'a App,
    /// A connection.
    pub db: &'a tokio_postgres::Client,
    /// The tenant.
    pub tenant: &'a Tenant,
}

impl Scope<'_> {
    fn registry() -> &'static IssuerRegistry {
        IssuerRegistry::default_registry()
    }

    /// The base the person sees the wiki under, which callbacks are registered for.
    fn public_base(&self) -> &str {
        &self.tenant.public_base
    }
}

/// The issuers the tenant offers for login (0088 §4), in registry order.
///
/// # Errors
///
/// If the store fails.
pub async fn offered(scope: Scope<'_>) -> Result<Vec<&'static Issuer>, ApiError> {
    let policy = LoginPolicy::load(scope.db, &scope.tenant.slug).await?;
    Ok(oauth::offered(
        scope.db,
        Scope::registry(),
        &scope.tenant.slug,
        scope.public_base(),
        &policy.issuers,
    )
    .await?)
}

/// Why a flow could not go on: a message code and its English text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The message code.
    pub code: &'static str,
    /// The text.
    pub message: String,
}

impl Refusal {
    /// A refusal by code, with the text the code has.
    #[must_use]
    pub fn of(code: &'static str) -> Self {
        Self {
            code,
            message: message(code).to_string(),
        }
    }
}

/// The English text of a message code of 0088.
#[must_use]
pub fn message(code: &str) -> &'static str {
    match code {
        "ts-oauth-denied" => {
            "You did not allow this wiki to learn which account you hold there, so you were not logged in."
        }
        "ts-oauth-state" => "That sign-in has expired or did not begin here. Start again.",
        "ts-oauth-failed" => {
            "The account provider could not be reached or gave an answer this wiki did not understand. Try again later."
        }
        "ts-oauth-returnurl" => "The return URL must be on this wiki's own site.",
        "ts-oauth-unavailable" => "This wiki does not offer logging in with that provider.",
        "ts-oauth-bound" => {
            "That account already logs in to an account on this wiki; log in with it instead."
        }
        "ts-oauth-bound-elsewhere" => {
            "That account already logs in to another account on this wiki."
        }
        "ts-last-binding" => "This is your last way to log in, so it cannot be removed.",
        "ts-no-login-method" => "That would leave this wiki with no way to log in.",
        "ts-reclaimed" => {
            "Welcome back: the wiki this one was moved from confirmed this account is yours."
        }
        "ts-reclaim-taken" => {
            "The wiki this one was moved from says this is your account, but someone already holds it here. Ask a bureaucrat."
        }
        "ts-reclaim-unavailable" => {
            "The wiki this one was moved from could not be asked who you are. Try again later."
        }
        "ts-account-pending" => {
            "Your request for an account is waiting for review. Try again later."
        }
        "ts-account-rejected" => "Your request for an account was not approved.",
        _ => "The sign-in did not complete.",
    }
}

/// Begins a flow for an issuer the tenant offers, in a session. Returns the URL to send
/// the person to.
///
/// # Errors
///
/// `Ok(Err(_))` with `ts-oauth-unavailable` when the tenant does not offer the issuer;
/// `Err` if the store fails.
pub async fn start(
    scope: Scope<'_>,
    session: &str,
    issuer: &str,
    purpose: Purpose,
    door: Door,
    name: Option<&str>,
    returnto: Option<&str>,
) -> Result<Result<String, Refusal>, ApiError> {
    let offered = offered(scope).await?;
    let Some(i) = offered.into_iter().find(|i| i.code == issuer) else {
        return Ok(Err(Refusal::of("ts-oauth-unavailable")));
    };
    let Some(endpoints) = &i.oauth else {
        return Ok(Err(Refusal::of("ts-oauth-unavailable")));
    };
    let credential = match oauth::client_for(
        scope.db,
        scope.app.secret(),
        &scope.tenant.slug,
        scope.public_base(),
        issuer,
    )
    .await
    {
        Ok(Some(c)) => c,
        Ok(None) | Err(triplespace_accounts::AccountsError::Sealed) => {
            return Ok(Err(Refusal::of("ts-oauth-unavailable")));
        }
        Err(e) => return Err(e.into()),
    };
    let (_, url) = oauth::begin(
        scope.db,
        NewFlow {
            tenant: &scope.tenant.slug,
            session,
            issuer,
            purpose,
            door,
            name,
            returnto,
        },
        endpoints,
        &credential,
    )
    .await?;
    Ok(Ok(url))
}

/// The provider's answer at the callback (0088 §5): finds the flow by `state`, records
/// the provider's `error` or exchanges the code, and returns the flow with its outcome.
///
/// # Errors
///
/// `Ok(Err(_))` with `ts-oauth-state` for an unknown, expired or mismatched flow; `Err`
/// if the store fails.
pub async fn answer(
    scope: Scope<'_>,
    issuer: &str,
    state: &str,
    code: Option<&str>,
    error: Option<&str>,
) -> Result<Result<Flow, Refusal>, ApiError> {
    let Some(mut flow) = oauth::find(scope.db, &scope.tenant.slug, state).await? else {
        return Ok(Err(Refusal::of("ts-oauth-state")));
    };
    if flow.issuer != issuer || flow.outcome.is_some() {
        return Ok(Err(Refusal::of("ts-oauth-state")));
    }
    let outcome = match (error, code) {
        (Some("access_denied"), _) => Outcome::Error("ts-oauth-denied".into()),
        (Some(_), _) | (None, None) => Outcome::Error("ts-oauth-failed".into()),
        (None, Some(code)) => identify(scope, &flow, code).await?,
    };
    oauth::set_outcome(scope.db, &flow.state_hash, &outcome).await?;
    flow.outcome = Some(outcome);
    Ok(Ok(flow))
}

async fn identify(scope: Scope<'_>, flow: &Flow, code: &str) -> Result<Outcome, ApiError> {
    let failed = || Outcome::Error("ts-oauth-failed".into());
    let Some(endpoints) = Scope::registry()
        .by_code(&flow.issuer)
        .and_then(|i| i.oauth.as_ref())
    else {
        return Ok(failed());
    };
    let credential = match oauth::client_for(
        scope.db,
        scope.app.secret(),
        &scope.tenant.slug,
        scope.public_base(),
        &flow.issuer,
    )
    .await
    {
        Ok(Some(c)) => c,
        Ok(None) | Err(triplespace_accounts::AccountsError::Sealed) => return Ok(failed()),
        Err(e) => return Err(e.into()),
    };
    let Ok(profile) = scope
        .app
        .upstream()
        .identify(endpoints, &credential, code, &flow.verifier)
        .await
    else {
        return Ok(failed());
    };
    Ok(match endpoints.subject.normalize(&profile.sub) {
        Some(subject) => Outcome::Identity {
            subject,
            username: profile.username,
        },
        None => failed(),
    })
}

/// What a flow came to, in the session that began it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conclusion {
    /// Logged in: the new session, the account's name, and whether a source vouched for
    /// an adopted account (`ts-reclaimed`).
    LoggedIn {
        /// The session.
        session: Session,
        /// The account's name.
        name: String,
        /// Whether the account was reclaimed.
        reclaimed: bool,
    },
    /// No account: the identity is now the session's pending identity.
    Pending {
        /// The provider's username, to offer as a suggestion.
        username: String,
        /// The issuer.
        issuer: String,
    },
    /// An account request was filed (`ts-account-requested`).
    Requested,
    /// A sign-in method was added to the session's account.
    Linked,
    /// Refused.
    Refused(Refusal),
    /// An account request holds the identity and is waiting, or was rejected.
    Request(RequestOutcome),
    /// The name chosen for `create` is taken.
    NameTaken,
}

/// Who the caller is, for [`conclude`].
#[derive(Debug, Clone, Copy)]
pub struct Caller<'a> {
    /// The session the request came with.
    pub session: Option<&'a str>,
    /// The account logged in, if any.
    pub actor_key: Option<&'a str>,
    /// The address key, for the `createaccount` throttle.
    pub address: &'a str,
    /// Whether the caller holds `noratelimit`.
    pub noratelimit: bool,
}

/// Applies a flow's outcome in the session that began it (0088 §6–§8). The flow ends,
/// except as a pending identity.
///
/// # Errors
///
/// If the store fails.
pub async fn conclude(
    scope: Scope<'_>,
    flow: &Flow,
    caller: Caller<'_>,
) -> Result<Conclusion, ApiError> {
    if caller.session != Some(flow.session.as_str()) {
        return Ok(Conclusion::Refused(Refusal::of("ts-oauth-state")));
    }
    let (subject, username) = match &flow.outcome {
        Some(Outcome::Identity { subject, username }) => (subject.clone(), username.clone()),
        Some(Outcome::Error(code)) => {
            oauth::finish(scope.db, &flow.state_hash).await?;
            let code = match code.as_str() {
                "ts-oauth-denied" => "ts-oauth-denied",
                _ => "ts-oauth-failed",
            };
            return Ok(Conclusion::Refused(Refusal::of(code)));
        }
        None => return Ok(Conclusion::Refused(Refusal::of("ts-oauth-state"))),
    };
    let tenant = scope.tenant.slug.as_str();
    let bound = bindings::find(scope.db, tenant, &flow.issuer, &subject).await?;
    let conclusion = match flow.purpose {
        Purpose::Login => match bound {
            Some(actor_key) => {
                bindings::touch(scope.db, tenant, &flow.issuer, &subject).await?;
                log_in(scope, &actor_key, caller.session, false).await?
            }
            None => {
                return unbound_login(scope, flow, caller, &subject, &username).await;
            }
        },
        Purpose::Create => {
            if bound.is_some() {
                Conclusion::Refused(Refusal::of("ts-oauth-bound"))
            } else {
                let name = flow.name.clone().unwrap_or_default();
                create_bound(scope, caller, &flow.issuer, &subject, &name, None).await?
            }
        }
        Purpose::Link => match (bound, caller.actor_key) {
            (_, None) => Conclusion::Refused(Refusal::of("ts-oauth-state")),
            (Some(a), Some(me)) if a == me => Conclusion::Linked,
            (Some(_), Some(_)) => Conclusion::Refused(Refusal::of("ts-oauth-bound-elsewhere")),
            (None, Some(me)) => link(scope, me, &flow.issuer, &subject).await?,
        },
    };
    oauth::finish(scope.db, &flow.state_hash).await?;
    Ok(conclusion)
}

async fn log_in(
    scope: Scope<'_>,
    actor_key: &str,
    previous: Option<&str>,
    reclaimed: bool,
) -> Result<Conclusion, ApiError> {
    match open_for(scope.db, &scope.tenant.slug, actor_key, previous).await {
        Ok(session) => {
            let name: Option<String> = scope
                .db
                .query_opt(
                    "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                    &[&scope.tenant.slug, &actor_key],
                )
                .await
                .map_err(ApiError::internal)?
                .and_then(|r| r.get(0));
            Ok(Conclusion::LoggedIn {
                session,
                name: name.unwrap_or_default(),
                reclaimed,
            })
        }
        Err(LoginError::Accounts(e)) => Err(e.into()),
        Err(_) => Ok(Conclusion::Refused(Refusal::of("ts-oauth-failed"))),
    }
}

async fn unbound_login(
    scope: Scope<'_>,
    flow: &Flow,
    caller: Caller<'_>,
    subject: &str,
    username: &str,
) -> Result<Conclusion, ApiError> {
    let tenant = scope.tenant.slug.as_str();
    if let Some(o) = requests::outcome_external(scope.db, tenant, &flow.issuer, subject).await? {
        oauth::finish(scope.db, &flow.state_hash).await?;
        return Ok(Conclusion::Request(o));
    }
    let policy = LoginPolicy::load(scope.db, tenant).await?;
    if policy.vouching.as_deref() == Some(flow.issuer.as_str()) {
        match vouch(scope, &flow.issuer, subject, username, caller.session).await? {
            Vouched::NoUser => {}
            Vouched::Done(c) => {
                oauth::finish(scope.db, &flow.state_hash).await?;
                return Ok(c);
            }
        }
    }
    let session = caller.session.unwrap_or(flow.session.as_str());
    oauth::keep_pending(scope.db, flow, session).await?;
    Ok(Conclusion::Pending {
        username: username.to_string(),
        issuer: flow.issuer.clone(),
    })
}

enum Vouched {
    /// The source names no user: the login goes on unbound.
    NoUser,
    /// Reclaimed and logged in, or refused.
    Done(Conclusion),
}

/// The users API of the wiki the tenant adopted: the registry entry under its
/// `adopted_from` origin, else `{adopted_from}w/api.php` (0088 §8).
async fn source_api(scope: Scope<'_>) -> Result<Option<String>, ApiError> {
    let adopted: Option<String> = scope
        .db
        .query_opt(
            "SELECT config->>'adopted_from' FROM view.registry
             WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&scope.tenant.slug],
        )
        .await
        .map_err(ApiError::internal)?
        .and_then(|r| r.get(0));
    let Some(adopted) = adopted else {
        return Ok(None);
    };
    let origin = adopted.trim_end_matches('/');
    let registered = Scope::registry()
        .issuers()
        .iter()
        .filter_map(|i| i.users_api.as_deref())
        .find(|u| u.starts_with(&format!("{origin}/")));
    Ok(Some(registered.map_or_else(
        || format!("{origin}/w/api.php"),
        str::to_owned,
    )))
}

async fn vouch(
    scope: Scope<'_>,
    issuer: &str,
    subject: &str,
    username: &str,
    previous: Option<&str>,
) -> Result<Vouched, ApiError> {
    let Some(i) = Scope::registry().by_code(issuer) else {
        return Ok(Vouched::NoUser);
    };
    let Some(endpoints) = &i.oauth else {
        return Ok(Vouched::NoUser);
    };
    let userid = match endpoints.subject {
        scatter_actors::issuer::SubjectKind::Local => subject.parse::<u64>().ok(),
        scatter_actors::issuer::SubjectKind::Central => {
            let Some(api) = source_api(scope).await? else {
                return Ok(Vouched::NoUser);
            };
            match scope.app.upstream().source_user(&api, username).await {
                Ok(Some(u)) if u.central.map(|c| c.to_string()).as_deref() == Some(subject) => {
                    Some(u.userid)
                }
                Ok(_) => None,
                Err(_) => {
                    return Ok(Vouched::Done(Conclusion::Refused(Refusal::of(
                        "ts-reclaim-unavailable",
                    ))));
                }
            }
        }
    };
    let Some(n) = userid else {
        return Ok(Vouched::NoUser);
    };
    let tenant = scope.tenant.slug.as_str();
    let actor_key = format!("{}:{n}", scope.tenant.issuer);
    let exists = scope
        .db
        .query_opt(
            "SELECT 1 FROM view.actor WHERE tenant = $1 AND actor_key = $2 AND kind = 'registered'",
            &[&tenant, &actor_key],
        )
        .await
        .map_err(ApiError::internal)?
        .is_some();
    if !exists {
        return Ok(Vouched::NoUser);
    }
    let held = !bindings::of(scope.db, &actor_key).await?.is_empty()
        || password::has(scope.db, &actor_key).await?;
    if held {
        return Ok(Vouched::Done(Conclusion::Refused(Refusal::of(
            "ts-reclaim-taken",
        ))));
    }
    let store = scope.app.store();
    let mut cx = scatter_projection::Backend::begin(store).await?;
    let now = crate::edit::now_micros();
    let done = reclaim(
        store,
        scope.app.pipeline(),
        &mut cx,
        tenant,
        &actor_key,
        issuer,
        subject,
        now,
    )
    .await;
    match done {
        Ok(()) => scatter_projection::Backend::commit(store, cx).await?,
        Err(CreateError::Bound) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            return Ok(Vouched::Done(Conclusion::Refused(Refusal::of(
                "ts-reclaim-taken",
            ))));
        }
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            return Err(ApiError::internal(e));
        }
    }
    Ok(Vouched::Done(
        log_in(scope, &actor_key, previous, true).await?,
    ))
}

async fn link(
    scope: Scope<'_>,
    actor_key: &str,
    issuer: &str,
    subject: &str,
) -> Result<Conclusion, ApiError> {
    let store = scope.app.store();
    let mut cx = scatter_projection::Backend::begin(store).await?;
    let done = triplespace_api_ingest::accounts::bind(
        store,
        scope.app.pipeline(),
        &mut cx,
        &scope.tenant.slug,
        actor_key,
        issuer,
        subject,
        actor_key,
        crate::edit::now_micros(),
    )
    .await;
    match done {
        Ok(()) => {
            scatter_projection::Backend::commit(store, cx).await?;
            Ok(Conclusion::Linked)
        }
        Err(CreateError::Bound) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            Ok(Conclusion::Refused(Refusal::of("ts-oauth-bound-elsewhere")))
        }
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            Err(ApiError::internal(e))
        }
    }
}

/// Creates an account bound to an identity, or files a request for one under
/// `account.approval` (0088 §6); then logs in. No password and no challenge; the
/// `createaccount` throttle counts it.
///
/// # Errors
///
/// If the store fails.
pub async fn create_bound(
    scope: Scope<'_>,
    caller: Caller<'_>,
    issuer: &str,
    subject: &str,
    name: &str,
    statement: Option<&str>,
) -> Result<Conclusion, ApiError> {
    let tenant = scope.tenant.slug.as_str();
    if bindings::find(scope.db, tenant, issuer, subject)
        .await?
        .is_some()
    {
        return Ok(Conclusion::Refused(Refusal::of("ts-oauth-bound")));
    }
    let approval = crate::modules::account::Policy::load_for(scope.db, tenant).await?;
    if !caller.noratelimit {
        let made = challenge::count(
            scope.db,
            tenant,
            challenge::CREATEACCOUNT,
            caller.address,
            challenge::CREATEACCOUNT_WINDOW,
        )
        .await?;
        if made >= approval.creation_limit {
            return Ok(Conclusion::Refused(Refusal {
                code: "acct_creation_throttle_hit",
                message: format!(
                    "Visitors to this wiki using your IP address have created {} accounts in the last day, which is the maximum allowed in this time period.",
                    approval.creation_limit
                ),
            }));
        }
    }
    let store = scope.app.store();
    {
        let mut cx = scatter_projection::Backend::begin(store).await?;
        let checked =
            triplespace_api_ingest::accounts::check_available(&mut cx, tenant, name).await;
        scatter_projection::Backend::rollback(store, cx).await?;
        match checked {
            Ok(_) => {}
            Err(CreateError::InvalidName(_) | CreateError::Taken) => {
                return Ok(Conclusion::NameTaken);
            }
            Err(e) => return Err(ApiError::internal(e)),
        }
    }
    if approval.approval {
        let statement = (approval.statement != "off").then_some(statement).flatten();
        let filed = requests::file_external(
            scope.db,
            tenant,
            name,
            issuer,
            subject,
            statement,
            caller.address,
            approval.expiry_secs,
        )
        .await;
        if let Err(e) = filed {
            if e.to_string().contains("account_request_name") {
                return Ok(Conclusion::NameTaken);
            }
            return Err(ApiError::internal(e));
        }
        challenge::record(scope.db, tenant, challenge::CREATEACCOUNT, caller.address).await?;
        return Ok(Conclusion::Requested);
    }
    let mut cx = scatter_projection::Backend::begin(store).await?;
    let created = create(
        store,
        scope.app.pipeline(),
        &mut cx,
        tenant,
        name,
        Credential::External { issuer, subject },
        None,
        "createaccount",
        crate::edit::now_micros(),
    )
    .await;
    let actor_key = match created {
        Ok(k) => {
            scatter_projection::Backend::commit(store, cx).await?;
            k
        }
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            return match e {
                CreateError::InvalidName(_) | CreateError::Taken => Ok(Conclusion::NameTaken),
                CreateError::Bound => Ok(Conclusion::Refused(Refusal::of("ts-oauth-bound"))),
                other => Err(ApiError::internal(other)),
            };
        }
    };
    challenge::record(scope.db, tenant, challenge::CREATEACCOUNT, caller.address).await?;
    log_in(scope, &actor_key, caller.session, false).await
}

/// The session's pending identity, as `GET /auth/pending` shows it: the issuer's code
/// and name and the provider's username.
///
/// # Errors
///
/// If the store fails.
pub async fn pending(scope: Scope<'_>, session: &str) -> Result<Option<(Flow, Value)>, ApiError> {
    let Some(flow) = oauth::pending(scope.db, &scope.tenant.slug, session).await? else {
        return Ok(None);
    };
    let Some(Outcome::Identity { username, .. }) = &flow.outcome else {
        return Ok(None);
    };
    let name = Scope::registry()
        .by_code(&flow.issuer)
        .map_or_else(|| flow.issuer.clone(), |i| i.name.clone());
    let v = serde_json::json!({
        "issuer": flow.issuer,
        "issuer_name": name,
        "username": username,
    });
    Ok(Some((flow, v)))
}

/// The origin of an issuer's authorization endpoint, which a page posting to the API's
/// start route lets its forms be redirected to (`form-action`, 0088 §5).
#[must_use]
pub fn authorize_origin(issuer: &Issuer) -> Option<String> {
    let o = issuer.oauth.as_ref()?;
    url::Url::parse(&o.authorize)
        .ok()
        .map(|u| u.origin().ascii_serialization())
}

/// What a login button names an issuer: its `oauth.label`, else its name.
#[must_use]
pub fn label(issuer: &Issuer) -> String {
    issuer
        .oauth
        .as_ref()
        .map_or_else(|| issuer.name.clone(), |o| o.label.clone())
}

/// Whether a return URL is on the tenant's own origin (0088 §5).
#[must_use]
pub fn same_origin(tenant: &Tenant, url: &str) -> bool {
    let (Ok(u), Ok(b)) = (url::Url::parse(url), url::Url::parse(&tenant.public_base)) else {
        return false;
    };
    u.scheme() == b.scheme()
        && u.host_str() == b.host_str()
        && u.port_or_known_default() == b.port_or_known_default()
}
