//! Registration and challenges in the Action API (0087 §1–9; 18 §2.3): `action=createaccount`
//! and `meta=authmanagerinfo` with MediaWiki's AuthManager shapes, the challenge carried
//! as a request of their own, `TriplespaceChallengeAuthenticationRequest`, and the
//! `badlogin` challenge that `action=clientlogin` asks for.
//!
//! - **The challenge** is offered where the tenant lists the act's trigger in
//!   `challenge.triggers` and the caller lacks `skipcaptcha` (0087 §2, §7). It is issued
//!   to the caller's session, which `meta=tokens` or this module starts where there is
//!   none, and answered in `tschallengeid` and `tschallengesolution`. A missing, wrong,
//!   stale or spent solution answers `status: UI` with `ts-challenge-required` and a fresh
//!   challenge, as ConfirmEdit answers a wrong CAPTCHA.
//! - **The throttle** is the `createaccount` class: 6 accounts or requests a day by
//!   address key, `noratelimit` exempt (0087 §4), answering `acct_creation_throttle_hit`.
//! - **Approval**: with `account.approval` on, a registration files a request and answers
//!   `FAIL` with `ts-account-requested` (0087 §6, §9); a holder of `confirmaccount`
//!   creates at once.
//!
//! The fields are checked before the challenge, so a solution is not spent on a typo.

use scatter_challenge::{ALGORITHM, Challenge, Trigger};
use serde_json::{Map, Value, json};
use triplespace_accounts::challenge::{self, Unsolved};
use triplespace_accounts::oauth::Purpose;
use triplespace_accounts::{requests, session};
use triplespace_api_ingest::accounts::{CreateError, Credential, create};

use crate::api::Ctx;
use crate::auth::set_cookie;
use crate::response::{ApiError, ApiResponse};

/// A tenant's registration settings (0087 §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// `challenge.triggers`.
    pub triggers: Vec<Trigger>,
    /// `challenge.bits`.
    pub bits: u8,
    /// `challenge.max_bits`.
    pub max_bits: u8,
    /// `account.creation_limit`: accounts one address key may create in a day.
    pub creation_limit: i64,
    /// `account.approval`.
    pub approval: bool,
    /// `account.request_statement`: `off`, `optional` or `required`.
    pub statement: String,
    /// `account.request_expiry`, in seconds.
    pub expiry_secs: i64,
    /// `login.external_required`, the instance's: a login is created only from an
    /// external identity (0101 §1).
    pub external_required: bool,
    /// `login.names`, the instance's: `provider` under strict names, where an account's
    /// name is derived from its login's identity (0102 §1), else `chosen`.
    pub names: String,
}

impl Policy {
    /// The registry's defaults, which a tenant without settings has.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            triggers: Trigger::ALL.to_vec(),
            bits: 20,
            max_bits: 24,
            creation_limit: challenge::CREATEACCOUNT_DEFAULT_LIMIT,
            approval: false,
            statement: "optional".into(),
            expiry_secs: 30 * 86_400,
            external_required: false,
            names: "chosen".into(),
        }
    }

    /// The tenant's settings, resolved through the instance's defaults and locks.
    ///
    /// # Errors
    ///
    /// If the store fails.
    pub async fn load(ctx: &Ctx) -> Result<Self, ApiError> {
        Self::load_for(ctx.accounts().await?, &ctx.tenant.slug).await
    }

    /// A tenant's settings, by its slug.
    ///
    /// # Errors
    ///
    /// If the store fails.
    pub async fn load_for(db: &tokio_postgres::Client, tenant: &str) -> Result<Self, ApiError> {
        let (settings, _) = triplespace_accounts::settings::load(db, Some(tenant)).await?;
        let d = Self::defaults();
        let bits = |key: &str, default: u8| {
            settings
                .value(key)
                .and_then(|v| v.as_u64())
                .and_then(|n| u8::try_from(n).ok())
                .map_or(default, |n| n.min(scatter_challenge::MAX_BITS))
        };
        let triggers = settings.value("challenge.triggers").map_or_else(
            || d.triggers.clone(),
            |v| {
                v.as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|t| t.as_str().and_then(Trigger::parse))
                            .collect()
                    })
                    .unwrap_or_default()
            },
        );
        let base = bits("challenge.bits", d.bits);
        Ok(Self {
            triggers,
            bits: base,
            max_bits: bits("challenge.max_bits", d.max_bits).max(base),
            creation_limit: settings
                .value("account.creation_limit")
                .and_then(|v| v.as_i64())
                .filter(|n| *n >= 1)
                .unwrap_or(d.creation_limit),
            approval: settings
                .value("account.approval")
                .and_then(|v| v.as_bool())
                .unwrap_or(d.approval),
            statement: settings
                .value("account.request_statement")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or(d.statement),
            expiry_secs: settings
                .value("account.request_expiry")
                .and_then(|v| v.as_str().map(str::to_owned))
                .and_then(|s| triplespace_accounts::settings::value::duration_seconds(&s))
                .and_then(|s| {
                    #[allow(clippy::cast_possible_truncation)]
                    let secs = s as i64;
                    (secs > 0).then_some(secs)
                })
                .unwrap_or(d.expiry_secs),
            external_required: settings
                .value("login.external_required")
                .and_then(|v| v.as_bool())
                .unwrap_or(d.external_required),
            names: settings
                .value("login.names")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or(d.names),
        })
    }

    /// Whether a trigger stands in front of an act for this caller: never for a holder of
    /// `skipcaptcha`; the instance's own acts never reach a trigger (implements 0087 §7).
    #[must_use]
    pub fn guards(&self, ctx: &Ctx, trigger: Trigger) -> bool {
        self.triggers.contains(&trigger) && !ctx.caller.effective.holds("skipcaptcha")
    }

    /// What `siprop=triplespace` reports (0087 §9). Reporting `external_required`
    /// implements 0101 §4; reporting `names` implements 0102 §6.
    #[must_use]
    pub fn report(&self) -> (Value, Value) {
        (
            json!({
                "triggers": self.triggers.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
                "algorithm": ALGORITHM,
                "bits": self.bits,
            }),
            json!({"approval": self.approval, "statement": self.statement, "external_required": self.external_required, "names": self.names}),
        )
    }
}

/// The caller's session ID, starting an anonymous session where there is none, as
/// `meta=tokens` does, so that a challenge has something to be bound to.
///
/// # Errors
///
/// If the store fails.
pub async fn binding(ctx: &mut Ctx) -> Result<String, ApiError> {
    if let Some(b) = ctx.caller.identity.token_binding() {
        return Ok(b.to_string());
    }
    let s = session::create(ctx.accounts().await?, &ctx.tenant.slug, None, None).await?;
    ctx.set_cookie = Some(set_cookie(
        &ctx.app,
        &s.id,
        session::ANONYMOUS_LIFETIME.as_secs(),
    ));
    let id = s.id.clone();
    ctx.caller.identity.session = Some(s);
    Ok(id)
}

/// The caller's address key (0087 §4).
#[must_use]
pub fn address(ctx: &Ctx) -> String {
    challenge::address_key(ctx.origin.client.or(ctx.origin.peer))
}

/// A fresh challenge for an act, of any trigger, at the difficulty the caller's address has
/// earned: the base plus one for each account created from it in the last 24 hours, up to
/// the maximum (07 §2.6). The string carries the tenant's slug (0087 A8): this
/// implements 0087 §3 (A8).
///
/// # Errors
///
/// If the store or the random source fails.
pub async fn fresh(
    ctx: &mut Ctx,
    policy: &Policy,
    trigger: Trigger,
) -> Result<Challenge, ApiError> {
    let binding = binding(ctx).await?;
    let bits = challenge::difficulty(
        ctx.accounts().await?,
        &ctx.tenant.slug,
        &address(ctx),
        policy.bits,
        policy.max_bits,
    )
    .await?;
    Ok(challenge::issue(
        ctx.app.secret(),
        &binding,
        &ctx.tenant.slug,
        trigger,
        bits,
    )?)
}

/// The AuthManager request that carries a challenge (0087 §9).
#[must_use]
pub fn challenge_request(c: &Challenge) -> Value {
    json!({
        "id": "TriplespaceChallengeAuthenticationRequest",
        "metadata": {"algorithm": ALGORITHM, "bits": c.bits, "expires": c.expires},
        "required": "required",
        "provider": "Triplespace challenge",
        "account": "TriplespaceChallengeAuthenticationRequest",
        "fields": {
            "tschallengeid": {"type": "hidden", "value": c.to_string(), "label": "Challenge", "help": "The challenge the solution is for."},
            "tschallengeinfo": {"type": "null", "value": "", "label": format!("Find a number n such that SHA-256 of \"{{challenge}}:{{n}}\" begins with {} zero bits.", c.bits), "help": "The browser does this while the form is filled in."},
            "tschallengesolution": {"type": "string", "label": "Solution", "help": "The number found."},
        },
    })
}

/// Checks the challenge the request carries for an act, spending it if it passes.
///
/// # Errors
///
/// `Ok(Err(_))` where the solution is not accepted; `Err` if the store fails.
pub async fn redeem(ctx: &mut Ctx, trigger: Trigger) -> Result<Result<(), Unsolved>, ApiError> {
    let binding = binding(ctx).await?;
    let id = ctx.params.get("tschallengeid").map(str::to_owned);
    let solution = ctx.params.get("tschallengesolution").map(str::to_owned);
    Ok(challenge::redeem(
        ctx.accounts().await?,
        ctx.app.secret(),
        &binding,
        &ctx.tenant.slug,
        trigger,
        id.as_deref(),
        solution.as_deref(),
    )
    .await?)
}

/// The `UI` answer that asks for a challenge's solution.
async fn ask(
    ctx: &mut Ctx,
    policy: &Policy,
    trigger: Trigger,
    module: &str,
    unsolved: Unsolved,
) -> Result<ApiResponse, ApiError> {
    let c = fresh(ctx, policy, trigger).await?;
    let why = match unsolved {
        Unsolved::Missing => "This wiki asks for a short computation first.".to_string(),
        Unsolved::Spent => "That solution has been used already.".to_string(),
        Unsolved::Refused(r) => format!("The solution was not accepted: {r}."),
    };
    Ok(ApiResponse::ok(json!({module: {
        "status": "UI",
        "message": format!("{why} Solve the challenge and send it again."),
        "messagecode": "ts-challenge-required",
        "requests": [challenge_request(&c)],
        "challenge": {"id": c.to_string(), "algorithm": ALGORITHM, "bits": c.bits, "expires": c.expires},
    }})))
}

fn fail(code: &str, message: &str) -> ApiResponse {
    ApiResponse::ok(json!({"createaccount": {
        "status": "FAIL",
        "message": message,
        "messagecode": code,
    }}))
}

/// `action=createaccount` (0087 §1, §6, §9): `username`, `password`, `retype`,
/// `createtoken`, `createreturnurl` or `createcontinue`, and where asked `tschallengeid`,
/// `tschallengesolution` and `tsstatement`.
#[allow(clippy::too_many_lines)]
pub async fn createaccount(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    if !ctx.params.posted {
        return Err(ApiError::new(
            "mustbeposted",
            "The createaccount module requires a POST request.",
        ));
    }
    let Some(presented) = ctx.params.get("createtoken").map(str::to_owned) else {
        return Err(ApiError::missing_param("createtoken"));
    };
    let ok = ctx
        .caller
        .identity
        .token_binding()
        .is_some_and(|b| ctx.app.secret().check("createaccount", b, &presented));
    if !ok {
        return Err(ApiError::new("badtoken", "Invalid CSRF token."));
    }
    if ctx.params.get("createreturnurl").is_none() && !ctx.params.flag("createcontinue") {
        return Err(ApiError::new(
            "missingparam",
            "At least one of the parameters \"createcontinue\" and \"createreturnurl\" is required.",
        ));
    }
    ctx.caller.require("createaccount")?;
    // An external issuer (0088 §5, §6): the continue of a flow, an account for the
    // session's pending identity, or a button pressed after choosing a name.
    if ctx.params.flag("createcontinue") && ctx.params.get("state").is_some() {
        return super::issuers::resume(ctx, "createaccount", Purpose::Create).await;
    }
    let chosen = ctx
        .params
        .get("username")
        .unwrap_or_default()
        .trim()
        .to_string();
    if ctx.params.flag("createpreservestate") {
        let statement = ctx.params.get("tsstatement").map(str::to_owned);
        if let Some(r) =
            super::issuers::create_preserved(ctx, &chosen, statement.as_deref()).await?
        {
            return Ok(r);
        }
    }
    if let Some(issuer) = super::issuers::pressed(ctx) {
        // Under strict names the name is derived when the identity comes back
        // (0102 §1); a chosen one is not carried.
        let strict = triplespace_accounts::logins::strict_names(ctx.accounts().await?).await?;
        if chosen.is_empty() && !strict {
            return Ok(fail("noname", "You have not specified a valid username."));
        }
        let name = if strict {
            None
        } else {
            let tenant = ctx.tenant.slug.clone();
            let mut cx = scatter_projection::Backend::begin(ctx.app.accounts_store()).await?;
            let checked = triplespace_api_ingest::accounts::check_available(
                &mut cx,
                ctx.app.secret(),
                &tenant,
                &chosen,
            )
            .await;
            scatter_projection::Backend::rollback(ctx.app.accounts_store(), cx).await?;
            match checked {
                Ok(n) => Some(n),
                Err(CreateError::InvalidName(_)) => {
                    return Ok(fail("noname", "You have not specified a valid username."));
                }
                Err(CreateError::Taken) => {
                    return Ok(fail(
                        "userexists",
                        "Username entered already in use. Please choose a different name.",
                    ));
                }
                Err(CreateError::Reserved(_) | CreateError::Tombstoned) => {
                    return Ok(fail(
                        "ts-name-reserved",
                        crate::external::message("ts-name-reserved"),
                    ));
                }
                Err(e) => return Err(ApiError::internal(e)),
            }
        };
        return super::issuers::redirect(
            ctx,
            "createaccount",
            &issuer,
            Purpose::Create,
            "createreturnurl",
            name.as_deref(),
        )
        .await;
    }
    let policy = Policy::load(ctx).await?;
    let address = address(ctx);
    let tenant = ctx.tenant.slug.clone();
    if !ctx.caller.effective.holds("noratelimit") {
        let made = challenge::count(
            ctx.accounts().await?,
            &tenant,
            challenge::CREATEACCOUNT,
            &address,
            challenge::CREATEACCOUNT_WINDOW,
        )
        .await?;
        if made >= policy.creation_limit {
            return Ok(fail(
                "acct_creation_throttle_hit",
                &format!(
                    "Visitors to this wiki using your IP address have created {} accounts in the last day, which is the maximum allowed in this time period. As a result, visitors using this IP address cannot create any more accounts at the moment.",
                    policy.creation_limit
                ),
            ));
        }
    }
    let username = ctx
        .params
        .get("username")
        .unwrap_or_default()
        .trim()
        .to_string();
    let password = ctx.params.get("password").unwrap_or_default().to_string();
    let retype = ctx.params.get("retype").map(str::to_owned);
    let statement = ctx.params.get("tsstatement").map(str::to_owned);
    if username.is_empty() {
        return Ok(fail("noname", "You have not specified a valid username."));
    }
    if retype.as_ref().is_some_and(|r| *r != password) {
        return Ok(fail("badretype", "The passwords you entered do not match."));
    }
    // A password would make a login with no external identity (0101 §1): refused for
    // oneself, for another and under approval alike. This implements 0101 §1.
    if policy.external_required {
        return Ok(fail(
            "ts-external-required",
            crate::external::message("ts-external-required"),
        ));
    }
    if password.chars().count() < 8 {
        return Ok(fail(
            "passwordtooshort",
            "Passwords must be at least 8 characters.",
        ));
    }
    // The name, checked in the store's unit of work so the creation sees what was checked.
    let performer = ctx.caller.identity.actor_key.clone();
    let files_request = policy.approval && !ctx.caller.effective.holds("confirmaccount");
    {
        let mut cx = scatter_projection::Backend::begin(ctx.app.accounts_store()).await?;
        let checked = triplespace_api_ingest::accounts::check_available(
            &mut cx,
            ctx.app.secret(),
            &tenant,
            &username,
        )
        .await;
        scatter_projection::Backend::rollback(ctx.app.accounts_store(), cx).await?;
        match checked {
            Ok(_) => {}
            Err(CreateError::InvalidName(_)) => {
                return Ok(fail("noname", "You have not specified a valid username."));
            }
            Err(CreateError::Taken) => {
                return Ok(fail(
                    "userexists",
                    "Username entered already in use. Please choose a different name.",
                ));
            }
            Err(CreateError::Reserved(_) | CreateError::Tombstoned) => {
                return Ok(fail(
                    "ts-name-reserved",
                    crate::external::message("ts-name-reserved"),
                ));
            }
            Err(e) => return Err(ApiError::internal(e)),
        }
    }
    if files_request {
        let text = statement.as_deref().map_or("", str::trim);
        if policy.statement == "required" && text.is_empty() {
            return Ok(fail(
                "ts-account-statement-required",
                "This wiki asks who you are and what you want to do here before it reviews a request.",
            ));
        }
        if text.chars().count() > requests::STATEMENT_MAX {
            return Ok(fail(
                "ts-account-statement-long",
                &format!(
                    "A statement has at most {} characters.",
                    requests::STATEMENT_MAX
                ),
            ));
        }
    }
    if policy.guards(ctx, Trigger::CreateAccount)
        && let Err(unsolved) = redeem(ctx, Trigger::CreateAccount).await?
    {
        return ask(
            ctx,
            &policy,
            Trigger::CreateAccount,
            "createaccount",
            unsolved,
        )
        .await;
    }
    if files_request {
        let statement = (policy.statement != "off")
            .then_some(statement.as_deref())
            .flatten();
        let filed = requests::file(
            ctx.accounts().await?,
            &tenant,
            &username,
            &password,
            statement,
            &address,
            policy.expiry_secs,
        )
        .await;
        if let Err(e) = filed {
            // A request filed for the name at the same moment.
            if e.to_string().contains("account_request_name") {
                return Ok(fail(
                    "userexists",
                    "Username entered already in use. Please choose a different name.",
                ));
            }
            return Err(ApiError::internal(e));
        }
        challenge::record(
            ctx.accounts().await?,
            &tenant,
            challenge::CREATEACCOUNT,
            &address,
        )
        .await?;
        return Ok(fail(
            "ts-account-requested",
            "Your request for an account has been filed. Someone who reviews new accounts on this wiki will decide on it; log in with the name and password you chose to learn the outcome.",
        ));
    }
    let now = crate::edit::now_micros();
    let store = ctx.app.accounts_store();
    let mut cx = scatter_projection::Backend::begin(store).await?;
    let comment = if performer.is_some() {
        "createaccount: created by another"
    } else {
        "createaccount"
    };
    let created = create(
        store,
        ctx.app.pipeline(),
        &mut cx,
        ctx.app.secret(),
        &tenant,
        &username,
        Credential::Password(&password),
        performer.as_deref(),
        comment,
        now,
    )
    .await;
    match created {
        Ok(_) => scatter_projection::Backend::commit(store, cx).await?,
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            return match e {
                CreateError::InvalidName(_) => {
                    Ok(fail("noname", "You have not specified a valid username."))
                }
                CreateError::Taken => Ok(fail(
                    "userexists",
                    "Username entered already in use. Please choose a different name.",
                )),
                CreateError::BadPassword(m) => Ok(fail("passwordtooshort", &m)),
                CreateError::Bound => Ok(fail(
                    "ts-oauth-bound",
                    crate::external::message("ts-oauth-bound"),
                )),
                CreateError::ExternalRequired => Ok(fail(
                    "ts-external-required",
                    crate::external::message("ts-external-required"),
                )),
                CreateError::Reserved(_) | CreateError::Tombstoned => Ok(fail(
                    "ts-name-reserved",
                    crate::external::message("ts-name-reserved"),
                )),
                CreateError::StrictName(_) => Ok(fail(
                    "ts-strict-name",
                    crate::external::message("ts-strict-name"),
                )),
                CreateError::StrictIdentity => Ok(fail(
                    "ts-strict-identity",
                    crate::external::message("ts-strict-identity"),
                )),
                CreateError::Store(m) => Err(ApiError::internal(m)),
            };
        }
    }
    challenge::record(
        ctx.accounts().await?,
        &tenant,
        challenge::CREATEACCOUNT,
        &address,
    )
    .await?;
    Ok(ApiResponse::ok(json!({"createaccount": {
        "status": "PASS",
        "username": scatter_actors::actor::normalize_name(&username),
    }})))
}

fn field(kind: &str, label: &str, help: &str) -> Value {
    json!({"type": kind, "label": label, "help": help})
}

/// `meta=authmanagerinfo` (0087 §9): `amirequestsfor=create` or `login`, with
/// `amimergerequestfields`. The challenge's request is listed where the act is guarded
/// for the caller; for `login`, where the caller's address has failed too often.
#[allow(clippy::too_many_lines)]
pub async fn authmanagerinfo(
    ctx: &mut Ctx,
    query: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    let policy = Policy::load(ctx).await?;
    let mut info = Map::new();
    info.insert("canauthenticatenow".into(), json!(true));
    info.insert(
        "cancreateaccounts".into(),
        json!(ctx.caller.effective.holds("createaccount")),
    );
    let issuer_requests = super::issuers::requests(ctx).await?;
    info.insert("canlinkaccounts".into(), json!(!issuer_requests.is_empty()));
    info.insert("preservedusername".into(), json!(""));
    let mut reqs = Vec::new();
    let username = json!({
        "id": "MediaWiki\\Auth\\UsernameAuthenticationRequest",
        "metadata": {}, "required": "required", "provider": "MediaWiki\\Auth\\PasswordAuthenticationRequest",
        "account": "MediaWiki\\Auth\\UsernameAuthenticationRequest",
        "fields": {"username": field("string", "Username", "Username for authentication.")},
    });
    match ctx.params.get("amirequestsfor").unwrap_or("login") {
        "create" => {
            reqs.push(username);
            reqs.push(json!({
                "id": "MediaWiki\\Auth\\PasswordAuthenticationRequest",
                "metadata": {}, "required": "primary-required", "provider": "Password-based authentication",
                "account": "MediaWiki\\Auth\\PasswordAuthenticationRequest",
                "fields": {
                    "password": field("password", "Password", "Password for authentication."),
                    "retype": field("password", "Confirm password", "Password again to confirm."),
                },
            }));
            if policy.approval
                && !ctx.caller.effective.holds("confirmaccount")
                && policy.statement != "off"
            {
                reqs.push(json!({
                    "id": "TriplespaceAccountRequestAuthenticationRequest",
                    "metadata": {"statement": policy.statement, "max": requests::STATEMENT_MAX},
                    "required": if policy.statement == "required" { "required" } else { "optional" },
                    "provider": "Triplespace account requests",
                    "account": "TriplespaceAccountRequestAuthenticationRequest",
                    "fields": {"tsstatement": field("multiline", "About you", "Who you are and what you want to do here, for the people who review new accounts.")},
                }));
            }
            if policy.guards(ctx, Trigger::CreateAccount) {
                let c = fresh(ctx, &policy, Trigger::CreateAccount).await?;
                reqs.push(challenge_request(&c));
            }
            reqs.extend(issuer_requests);
        }
        "link" if ctx.caller.identity.is_authenticated() => {
            reqs.extend(issuer_requests);
        }
        "remove" => {
            reqs.extend(super::issuers::remove_requests(ctx).await?);
        }
        "login" => {
            reqs.push(json!({
                "id": "MediaWiki\\Auth\\PasswordAuthenticationRequest",
                "metadata": {}, "required": "primary-required", "provider": "Password-based authentication",
                "account": "MediaWiki\\Auth\\PasswordAuthenticationRequest",
                "fields": {
                    "username": field("string", "Username", "Username for authentication."),
                    "password": field("password", "Password", "Password for authentication."),
                },
            }));
            if policy.guards(ctx, Trigger::BadLogin) {
                let a = address(ctx);
                let failed = challenge::count(
                    ctx.accounts().await?,
                    &ctx.tenant.slug,
                    challenge::BADLOGIN_ADDRESS,
                    &a,
                    challenge::BADLOGIN_ADDRESS_LIMIT.1,
                )
                .await?;
                if failed >= challenge::BADLOGIN_ADDRESS_LIMIT.0 {
                    let c = fresh(ctx, &policy, Trigger::BadLogin).await?;
                    reqs.push(challenge_request(&c));
                }
            }
            reqs.extend(issuer_requests);
        }
        _ => {}
    }
    if ctx.params.flag("amimergerequestfields") {
        let mut fields = Map::new();
        for r in &reqs {
            if let Some(f) = r["fields"].as_object() {
                for (k, v) in f {
                    fields.insert(k.clone(), v.clone());
                }
            }
        }
        info.insert("fields".into(), Value::Object(fields));
    }
    info.insert("requests".into(), Value::Array(reqs));
    query.insert("authmanagerinfo".into(), Value::Object(info));
    // A challenge is the caller's alone.
    ctx.cache_class = crate::http_cache::Class::Private;
    Ok(())
}

/// The `badlogin` check in front of `action=clientlogin` (0087 §2): `Some` with the `UI`
/// answer where the caller's address or the named account has failed too often and the
/// request carries no accepted solution.
///
/// # Errors
///
/// If the store fails.
pub async fn guard_login(ctx: &mut Ctx, username: &str) -> Result<Option<ApiResponse>, ApiError> {
    let policy = Policy::load(ctx).await?;
    if !policy.guards(ctx, Trigger::BadLogin) {
        return Ok(None);
    }
    let a = address(ctx);
    if !challenge::badlogin_applies(ctx.accounts().await?, &ctx.tenant.slug, &a, username).await? {
        return Ok(None);
    }
    match redeem(ctx, Trigger::BadLogin).await? {
        Ok(()) => Ok(None),
        Err(unsolved) => Ok(Some(
            ask(ctx, &policy, Trigger::BadLogin, "clientlogin", unsolved).await?,
        )),
    }
}
