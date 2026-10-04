//! `action=login` (bot passwords, 0024 §4, §8), `action=clientlogin` (primary accounts,
//! 0007 §3; 0012 §4) and `action=logout`. The shapes are MediaWiki's:
//! `{"login": {"result": "Success", "lguserid", "lgusername"}}`, `NeedToken` with a
//! fresh token, `WrongToken`, and `Failed` with a `reason`; `ts-use-oauth` is reported
//! as `Failed` with that `code`. `clientlogin` answers `{"clientlogin": {"status": "PASS",
//! "username"}}` or `FAIL` with `message` and `messagecode`.

use serde_json::{Value, json};
use triplespace_accounts::login::{LoginError, bot_login, password_login};
use triplespace_accounts::session;

use crate::api::Ctx;
use crate::auth::{clear_cookie, set_cookie};
use crate::response::{ApiError, ApiResponse};

/// Checks a login token against the caller's session; `None` when it checks out.
fn token_problem(ctx: &Ctx, name: &str) -> Option<&'static str> {
    let Some(presented) = ctx.params.get(name) else {
        return Some("NeedToken");
    };
    match ctx.caller.identity.token_binding() {
        Some(b) if ctx.app.secret().check("login", b, presented) => None,
        Some(_) => Some("WrongToken"),
        None => Some("NeedToken"),
    }
}

/// A fresh anonymous session and its login token, for a `NeedToken` answer.
async fn need_token(ctx: &mut Ctx) -> Result<String, ApiError> {
    if ctx.caller.identity.token_binding().is_none() {
        let s = session::create(ctx.db(), &ctx.tenant.slug, None, None).await?;
        ctx.set_cookie = Some(set_cookie(
            &ctx.app,
            &s.id,
            session::ANONYMOUS_LIFETIME.as_secs(),
        ));
        ctx.caller.identity.session = Some(s);
    }
    Ok(ctx.caller.token(&ctx.app, "login"))
}

fn numeric_id(actor_key: &str) -> u64 {
    actor_key
        .rsplit_once(':')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}

/// `action=login`.
pub async fn login(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    if !ctx.params.posted {
        return Err(ApiError::new(
            "mustbeposted",
            "The login module requires a POST request.",
        ));
    }
    let name = ctx.params.required("lgname")?.to_string();
    let password = ctx.params.get("lgpassword").unwrap_or_default().to_string();
    if let Some(problem) = token_problem(ctx, "lgtoken") {
        let token = need_token(ctx).await?;
        return Ok(ApiResponse::ok(
            json!({"login": {"result": problem, "token": token}}),
        ));
    }
    let previous = ctx.caller.identity.session.as_ref().map(|s| s.id.clone());
    match bot_login(
        ctx.db(),
        &ctx.tenant.slug,
        &name,
        &password,
        previous.as_deref(),
    )
    .await
    {
        Ok((s, key)) => {
            ctx.set_cookie = Some(set_cookie(&ctx.app, &s.id, session::LIFETIME.as_secs()));
            let display = name.rsplit_once('@').map_or(name.as_str(), |(n, _)| n);
            Ok(ApiResponse::ok(json!({"login": {
                "result": "Success",
                "lguserid": numeric_id(&key.actor_key),
                "lgusername": display,
            }})))
        }
        Err(LoginError::UseOauth(n)) => Ok(ApiResponse::ok(json!({"login": {
            "result": "Failed",
            "code": "ts-use-oauth",
            "reason": format!("\"{n}\" is a primary account. Log in with action=clientlogin, or with a bot password of one of its subsidiaries (name@label)."),
        }}))),
        Err(LoginError::Failed | LoginError::SubsidiaryPassword(_)) => {
            Ok(ApiResponse::ok(json!({"login": {
                "result": "Failed",
                "reason": "Incorrect username, label or bot password.",
            }})))
        }
        Err(LoginError::Accounts(e)) => Err(ApiError::internal(e)),
        Err(e) => Err(ApiError::internal(e)),
    }
}

/// `action=clientlogin`: `username`, `password`, `logintoken` (and `loginreturnurl`,
/// accepted and ignored).
pub async fn clientlogin(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    if !ctx.params.posted {
        return Err(ApiError::new(
            "mustbeposted",
            "The clientlogin module requires a POST request.",
        ));
    }
    if let Some(problem) = token_problem(ctx, "logintoken") {
        if problem == "NeedToken" && !ctx.params.flag("logintoken") {
            // MediaWiki's first step: no token yet.
            let _ = need_token(ctx).await?;
            return Err(ApiError::missing_param("logintoken"));
        }
        return Err(ApiError::new("badtoken", "Invalid login token."));
    }
    let username = ctx.params.required("username")?.to_string();
    let password = ctx.params.required("password")?.to_string();
    let previous = ctx.caller.identity.session.as_ref().map(|s| s.id.clone());
    match password_login(
        ctx.db(),
        &ctx.tenant.slug,
        &username,
        &password,
        previous.as_deref(),
    )
    .await
    {
        Ok(s) => {
            ctx.set_cookie = Some(set_cookie(&ctx.app, &s.id, session::LIFETIME.as_secs()));
            Ok(ApiResponse::ok(json!({"clientlogin": {
                "status": "PASS",
                "username": scatter_actors::actor::normalize_name(&username),
            }})))
        }
        Err(LoginError::SubsidiaryPassword(n)) => Ok(ApiResponse::ok(json!({"clientlogin": {
            "status": "FAIL",
            "message": format!("\"{n}\" is a subsidiary account; it logs in with action=login and a bot password."),
            "messagecode": "ts-subsidiary-password",
        }}))),
        Err(LoginError::Failed | LoginError::UseOauth(_)) => {
            Ok(ApiResponse::ok(json!({"clientlogin": {
                "status": "FAIL",
                "message": "Incorrect username or password entered. Please try again.",
                "messagecode": "wrongpassword",
            }})))
        }
        Err(LoginError::Accounts(e)) => Err(ApiError::internal(e)),
        Err(e) => Err(ApiError::internal(e)),
    }
}

/// `action=logout`: ends the session.
pub async fn logout(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    if let Some(s) = &ctx.caller.identity.session {
        session::delete(ctx.db(), &s.id).await?;
    }
    ctx.set_cookie = Some(clear_cookie());
    Ok(ApiResponse::ok(
        json!({"logout": Value::Object(serde_json::Map::default())}),
    ))
}
