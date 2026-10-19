//! External issuers in the Action API (0088 §5–§7; 18 §2.3): MediaWiki's AuthManager
//! redirect flow over [`crate::external`].
//!
//! - **The request.** For each issuer the tenant offers, `meta=authmanagerinfo` lists
//!   `TriplespaceIssuerAuthenticationRequest:{issuer}` with one `button` field,
//!   `tsissuer-{issuer}`, for `login`, `create` and `link`; for `remove`, one per binding
//!   the caller's account holds.
//! - **The redirect.** `clientlogin`, `createaccount` or `linkaccount` with that field and
//!   a return URL on the tenant's own origin answers `status: REDIRECT` with the
//!   provider's URL and a continue request whose one field is `state`.
//! - **The continue.** The provider returns to the REST callback, which appends `state`
//!   to the return URL; `logincontinue`, `createcontinue` or `linkcontinue` with it, in
//!   the session that began the flow, gets the outcome.
//! - **`createpreservestate`** creates an account from the session's pending identity.
//! - **`action=unlinkaccount`** removes a binding, erasing its record.

use scatter_actors::issuer::Issuer;
use serde_json::{Value, json};
use triplespace_accounts::bindings;
use triplespace_accounts::oauth::{Door, Purpose};
use triplespace_accounts::requests::Outcome as RequestOutcome;
use triplespace_api_ingest::accounts::{UnbindError, unbind};

use crate::api::Ctx;
use crate::auth::set_cookie;
use crate::external::{self, Caller, Conclusion, Refusal, Scope};
use crate::response::{ApiError, ApiResponse};

/// The request ID of an issuer.
#[must_use]
pub fn request_id(code: &str) -> String {
    format!("TriplespaceIssuerAuthenticationRequest:{code}")
}

/// The button field of an issuer.
#[must_use]
pub fn field(code: &str) -> String {
    format!("tsissuer-{code}")
}

fn scope(ctx: &Ctx) -> Scope<'_> {
    Scope {
        app: &ctx.app,
        db: ctx.db(),
        tenant: &ctx.tenant,
    }
}

/// The issuer requests `meta=authmanagerinfo` lists for `login`, `create` and `link`.
///
/// # Errors
///
/// If the store fails.
pub async fn requests(ctx: &Ctx) -> Result<Vec<Value>, ApiError> {
    let offered = external::offered(scope(ctx)).await?;
    Ok(offered.iter().map(|i| request(i)).collect())
}

fn request(i: &Issuer) -> Value {
    let label = external::label(i);
    json!({
        "id": request_id(&i.code),
        "metadata": {"issuer": i.code},
        "required": "primary-required",
        "provider": label,
        "account": request_id(&i.code),
        "fields": {
            field(&i.code): {
                "type": "button",
                "label": format!("Log in with {label}"),
                "help": format!("Log in with your {label} account; you will be sent there and back."),
            },
        },
    })
}

/// The requests `amirequestsfor=remove` lists: one per binding of the caller's account.
///
/// # Errors
///
/// If the store fails.
pub async fn remove_requests(ctx: &Ctx) -> Result<Vec<Value>, ApiError> {
    let Some(me) = ctx.caller.identity.actor_key.as_deref() else {
        return Ok(Vec::new());
    };
    let registry = scatter_actors::issuer::IssuerRegistry::default_registry();
    Ok(bindings::of(ctx.db(), me)
        .await?
        .into_iter()
        .map(|b| {
            let label = registry
                .by_code(&b.issuer)
                .map_or_else(|| b.issuer.clone(), external::label);
            json!({
                "id": request_id(&b.issuer),
                "metadata": {"issuer": b.issuer},
                "required": "optional",
                "provider": label,
                "account": request_id(&b.issuer),
                "fields": {},
            })
        })
        .collect())
}

/// The issuer whose button a request pressed, if any.
#[must_use]
pub fn pressed(ctx: &Ctx) -> Option<String> {
    ctx.params
        .names()
        .find_map(|n| n.strip_prefix("tsissuer-"))
        .map(str::to_owned)
}

#[allow(clippy::needless_pass_by_value)]
fn status(module: &str, body: Value) -> ApiResponse {
    ApiResponse::ok(json!({ module: body }))
}

fn fail(module: &str, r: &Refusal) -> ApiResponse {
    status(
        module,
        json!({"status": "FAIL", "message": r.message, "messagecode": r.code}),
    )
}

/// Begins a flow and answers `REDIRECT` (0088 §5).
///
/// # Errors
///
/// `missingparam` without a return URL; a store error.
pub async fn redirect(
    ctx: &mut Ctx,
    module: &str,
    issuer: &str,
    purpose: Purpose,
    returnurl_param: &str,
    name: Option<&str>,
) -> Result<ApiResponse, ApiError> {
    let Some(returnurl) = ctx.params.get(returnurl_param).map(str::to_owned) else {
        return Err(ApiError::missing_param(returnurl_param));
    };
    if !external::same_origin(&ctx.tenant, &returnurl) {
        return Ok(fail(module, &Refusal::of("ts-oauth-returnurl")));
    }
    let session = super::account::binding(ctx).await?;
    let started = external::start(
        scope(ctx),
        &session,
        issuer,
        purpose,
        Door::Action,
        name,
        Some(&returnurl),
    )
    .await?;
    Ok(match started {
        Ok(url) => status(
            module,
            json!({
                "status": "REDIRECT",
                "redirecttarget": url,
                "requests": [{
                    "id": request_id(issuer),
                    "metadata": {"issuer": issuer},
                    "required": "required",
                    "provider": issuer,
                    "account": request_id(issuer),
                    "fields": {"state": {"type": "string", "label": "State", "help": "The state the return URL was given."}},
                }],
            }),
        ),
        Err(r) => fail(module, &r),
    })
}

/// The caller, as a flow's conclusion needs them.
fn caller(ctx: &Ctx) -> (Option<String>, Option<String>, String, bool) {
    (
        ctx.caller.identity.session.as_ref().map(|s| s.id.clone()),
        ctx.caller.identity.actor_key.clone(),
        super::account::address(ctx),
        ctx.caller.effective.holds("noratelimit"),
    )
}

/// `*continue` with `state` (0088 §5): the flow's outcome, in the session that began it.
///
/// # Errors
///
/// `missingparam` without `state`; a store error.
pub async fn resume(
    ctx: &mut Ctx,
    module: &str,
    purpose: Purpose,
) -> Result<ApiResponse, ApiError> {
    let state = ctx.params.required("state")?.to_string();
    let Some(flow) = triplespace_accounts::oauth::find(ctx.db(), &ctx.tenant.slug, &state).await?
    else {
        return Ok(fail(module, &Refusal::of("ts-oauth-state")));
    };
    if flow.purpose != purpose || flow.outcome.is_none() {
        return Ok(fail(module, &Refusal::of("ts-oauth-state")));
    }
    let (session, actor, address, noratelimit) = caller(ctx);
    let c = external::conclude(
        scope(ctx),
        &flow,
        Caller {
            session: session.as_deref(),
            actor_key: actor.as_deref(),
            address: &address,
            noratelimit,
        },
    )
    .await?;
    Ok(answer(ctx, module, c))
}

/// `createaccount&createpreservestate=1` with a `username`: the account for the session's
/// pending identity (0088 §6). `None` when the session holds none, so the module goes
/// on as for a password.
///
/// # Errors
///
/// A store error.
pub async fn create_preserved(
    ctx: &mut Ctx,
    username: &str,
    statement: Option<&str>,
) -> Result<Option<ApiResponse>, ApiError> {
    let (session, actor, address, noratelimit) = caller(ctx);
    let Some(session) = session else {
        return Ok(None);
    };
    let Some((flow, _)) = external::pending(scope(ctx), &session).await? else {
        return Ok(None);
    };
    let Some(triplespace_accounts::oauth::Outcome::Identity { subject, .. }) = &flow.outcome else {
        return Ok(None);
    };
    let c = external::create_bound(
        scope(ctx),
        Caller {
            session: Some(&session),
            actor_key: actor.as_deref(),
            address: &address,
            noratelimit,
        },
        &flow.issuer,
        subject,
        username,
        statement,
    )
    .await?;
    if matches!(c, Conclusion::LoggedIn { .. } | Conclusion::Requested) {
        triplespace_accounts::oauth::finish(ctx.db(), &flow.state_hash).await?;
    }
    Ok(Some(answer(ctx, "createaccount", c)))
}

/// A conclusion in a module's shape.
fn answer(ctx: &mut Ctx, module: &str, c: Conclusion) -> ApiResponse {
    match c {
        Conclusion::LoggedIn {
            session,
            name,
            reclaimed,
        } => {
            ctx.set_cookie = Some(set_cookie(
                &ctx.app,
                &session.id,
                triplespace_accounts::session::LIFETIME.as_secs(),
            ));
            let mut body = json!({"status": "PASS", "username": name});
            if reclaimed {
                body["messagecode"] = json!("ts-reclaimed");
                body["message"] = json!(external::message("ts-reclaimed"));
            }
            status(module, body)
        }
        Conclusion::Pending { username, issuer } => status(
            module,
            json!({
                "status": "RESTART",
                "message": "You signed in, but no account on this wiki is linked to that identity. Create one with action=createaccount and createpreservestate=1.",
                "messagecode": "authmanager-authn-no-local-user-link",
                "tsissuer": issuer,
                "tsusername": username,
            }),
        ),
        Conclusion::Requested => status(
            module,
            json!({
                "status": "FAIL",
                "message": "Your request for an account has been filed. Someone who reviews new accounts on this wiki will decide on it; log in the same way to learn the outcome.",
                "messagecode": "ts-account-requested",
            }),
        ),
        Conclusion::Linked => status(module, json!({"status": "PASS"})),
        Conclusion::Refused(r) => fail(module, &r),
        Conclusion::Request(RequestOutcome::Pending) => {
            fail(module, &Refusal::of("ts-account-pending"))
        }
        Conclusion::Request(RequestOutcome::Rejected(reason)) => status(
            module,
            json!({
                "status": "FAIL",
                "message": format!("Your request for an account was not approved: {reason}"),
                "messagecode": "ts-account-rejected",
                "reason": reason,
            }),
        ),
        Conclusion::NameTaken => status(
            module,
            json!({
                "status": "FAIL",
                "message": "Username entered already in use. Please choose a different name.",
                "messagecode": "userexists",
            }),
        ),
    }
}

fn check_csrf_named(ctx: &Ctx, name: &str) -> Result<(), ApiError> {
    if !ctx.params.posted {
        return Err(ApiError::new(
            "mustbeposted",
            "The module requires a POST request.",
        ));
    }
    let Some(presented) = ctx.params.get(name) else {
        return Err(ApiError::missing_param(name));
    };
    let ok = ctx
        .caller
        .identity
        .token_binding()
        .is_some_and(|b| ctx.app.secret().check("csrf", b, presented));
    if ok {
        Ok(())
    } else {
        Err(ApiError::new("badtoken", "Invalid CSRF token."))
    }
}

fn primary_account(ctx: &Ctx) -> Result<String, ApiError> {
    let Some(me) = ctx.caller.identity.actor_key.clone() else {
        return Err(ApiError::new(
            "notloggedin",
            "Please log in to manage your sign-in methods.",
        ));
    };
    if ctx.caller.identity.operator.is_some() || ctx.caller.identity.credential.is_some() {
        return Err(ApiError::new(
            "ts-subsidiary",
            "Subsidiary accounts have no sign-in methods.",
        ));
    }
    Ok(me)
}

/// `action=linkaccount` (0088 §7): `linktoken` (a CSRF token), the issuer's button and
/// `linkreturnurl`, or `linkcontinue` with `state`.
///
/// # Errors
///
/// As MediaWiki's: `badtoken`, `notloggedin`, `missingparam`; a store error.
pub async fn linkaccount(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    check_csrf_named(ctx, "linktoken")?;
    primary_account(ctx)?;
    if ctx.params.flag("linkcontinue") {
        return resume(ctx, "linkaccount", Purpose::Link).await;
    }
    let Some(issuer) = pressed(ctx) else {
        return Err(ApiError::new(
            "missingparam",
            "Press an issuer's button (tsissuer-{issuer}) or continue with linkcontinue.",
        ));
    };
    redirect(
        ctx,
        "linkaccount",
        &issuer,
        Purpose::Link,
        "linkreturnurl",
        None,
    )
    .await
}

/// `action=unlinkaccount` (0088 §7): `request=TriplespaceIssuerAuthenticationRequest:{issuer}`
/// and `token`.
///
/// # Errors
///
/// `badtoken`, `notloggedin`, `ts-last-binding`, `badvalue`; a store error.
pub async fn unlinkaccount(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    check_csrf_named(ctx, "token")?;
    let me = primary_account(ctx)?;
    let request = ctx.params.required("request")?.to_string();
    let Some(issuer) = request.strip_prefix("TriplespaceIssuerAuthenticationRequest:") else {
        return Err(ApiError::new(
            "badvalue",
            format!("Unrecognized value for parameter \"request\": {request}."),
        ));
    };
    let store = ctx.app.store();
    let mut cx = scatter_projection::Backend::begin(store).await?;
    let done = unbind(
        store,
        ctx.app.pipeline(),
        &mut cx,
        &me,
        issuer,
        crate::edit::now_micros(),
    )
    .await;
    match done {
        Ok(()) => {
            scatter_projection::Backend::commit(store, cx).await?;
            Ok(ApiResponse::ok(json!({"unlinkaccount": {}})))
        }
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            match e {
                UnbindError::NotBound => Err(ApiError::new(
                    "badvalue",
                    format!("Unrecognized value for parameter \"request\": {request}."),
                )),
                UnbindError::Last => Err(ApiError::new(
                    "ts-last-binding",
                    external::message("ts-last-binding"),
                )),
                UnbindError::Store(m) => Err(ApiError::internal(m)),
            }
        }
    }
}
