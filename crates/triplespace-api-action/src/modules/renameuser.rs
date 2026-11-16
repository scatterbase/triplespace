//! `action=renameuser` (19 §2.3; 0010 §10; 16 §2.3): MediaWiki's module for renaming an
//! account by hand, written through [`triplespace_api_ingest::accounts::rename`].
//!
//! Parameters as MediaWiki's: `user` or `userid`, `newuser`, `reason`, and the `csrf`
//! token. Needs `renameuser`. The answer is `{"renameuser": {"from", "to", "userid"}}`.
//! The new name passes the name rules, the reserved prefixes and the tombstones
//! (`invaliduser`, `userexists`, `ts-name-reserved`), and the old name is left as a
//! tombstone; the rename shows in `list=logevents` as `renameuser/renameuser`. Under
//! strict names a name is derived, not chosen, so the module answers `ts-strict-name`
//! (0102 §3, Q1); with names chosen, the one prefixed name a rename may give an account
//! is the name its own login's identity derives, which is how a bureaucrat prepares an
//! instance for strict names (0102 §5, A2). MediaWiki's `movepages` and
//! `suppressredirect` are accepted and ignored: a user page is not moved.

use scatter_actors::actor::normalize_name;
use scatter_actors::issuer::IssuerRegistry;
use scatter_projection::Backend as _;
use serde_json::json;
use triplespace_accounts::logins;
use triplespace_api_ingest::accounts::{CreateError, check_available, check_derived, rename};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::response::{ApiError, ApiResponse};

/// The account a request names by `user` or `userid`: its key and name.
async fn account(ctx: &Ctx) -> Result<(String, String), ApiError> {
    let tenant = &ctx.tenant.slug;
    if let Some(id) = ctx.params.get("userid") {
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value("userid", id))?;
        ctx.db()
            .query_opt(
                "SELECT actor_key, name FROM view.actor
                 WHERE tenant = $1 AND name IS NOT NULL AND kind = 'registered'
                   AND split_part(actor_key, ':', 2) = $2
                 ORDER BY actor_key LIMIT 1",
                &[tenant, &n.to_string()],
            )
            .await?
            .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
            .ok_or_else(|| ApiError::new("nosuchuserid", format!("There is no user with ID {n}.")))
    } else {
        let name = ctx.params.required("user")?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT actor_key, name FROM view.actor
                 WHERE tenant = $1 AND kind = 'registered' AND lower(name) = lower($2)",
                &[tenant, &name.trim()],
            )
            .await?;
        row.map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
            .ok_or_else(|| {
                ApiError::new("nosuchuser", format!("The user \"{name}\" does not exist."))
            })
    }
}

fn refused(e: CreateError) -> ApiError {
    match e {
        CreateError::InvalidName(why) => ApiError::new("invaliduser", why),
        CreateError::Taken => ApiError::new("userexists", "The new name is in use already."),
        CreateError::Reserved(_) => ApiError::new(
            "ts-name-reserved",
            crate::external::message("ts-name-reserved"),
        ),
        CreateError::Tombstoned => ApiError::new(
            "ts-name-reserved",
            "An account held that name once, and a name is never reused.",
        ),
        CreateError::StrictName(_) => {
            ApiError::new("ts-strict-name", crate::external::message("ts-strict-name"))
        }
        other => ApiError::internal(other),
    }
}

/// `action=renameuser` (0010 §10), with `Special:RenameUser`.
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_token(&ctx.app, &ctx.params, "csrf")?;
    ctx.caller.require("renameuser")?;
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("renameuser"))?;
    let (key, from) = account(ctx).await?;
    let new_name = ctx.params.required("newuser")?.to_string();
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    // Under strict names nobody chooses a name, by hand least of all (0102 §3, Q1).
    if logins::strict_names(ctx.db()).await? {
        return Err(refused(CreateError::StrictName(from)));
    }
    // The account's own derived name may carry its issuer's prefix (0102 §5, A2).
    let derived = match logins::login_of(ctx.db(), &ctx.tenant.slug, &key).await? {
        Some(login) => logins::identity_of(ctx.db(), login).await?.and_then(|b| {
            b.username.as_deref().and_then(|u| {
                logins::derived_name_for(IssuerRegistry::default_registry(), &b.issuer, u)
            })
        }),
        None => None,
    };
    let store = ctx.app.accounts_store();
    let mut cx = store.begin().await.map_err(ApiError::internal)?;
    let tenant = ctx.tenant.slug.clone();
    let done = async {
        let to = if derived.as_deref() == Some(normalize_name(&new_name).as_str()) {
            check_derived(&mut cx, ctx.app.secret(), &tenant, &new_name).await?
        } else {
            check_available(&mut cx, ctx.app.secret(), &tenant, &new_name).await?
        };
        let comment = if reason.is_empty() {
            "renameuser".to_string()
        } else {
            format!("renameuser: {reason}")
        };
        rename(
            store,
            ctx.app.pipeline(),
            &mut cx,
            ctx.app.secret(),
            &tenant,
            &key,
            &to,
            &actor,
            &comment,
            now_micros(),
        )
        .await?;
        Ok::<_, CreateError>(to)
    }
    .await;
    let to = match done {
        Ok(to) => {
            store.commit(cx).await.map_err(ApiError::internal)?;
            to
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            return Err(refused(e));
        }
    };
    Ok(ApiResponse::ok(json!({"renameuser": {
        "from": from,
        "to": to,
        "userid": crate::activity::user_id(Some(&key)),
    }})))
}
