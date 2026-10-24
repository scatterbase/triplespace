//! `action=block` and `action=unblock` (09 §3.3, §6.3; 0016 §3; 0023 §8), in MediaWiki's
//! shapes, written through [`triplespace_api_ingest::blocks`].
//!
//! - **`action=block`**: `user` or `userid`; `expiry` (`infinite` by default); `reason`;
//!   `nocreate`, which also removes `createaccount`; `reblock` to replace a block in
//!   force (otherwise `alreadyblocked`); `hidename`, which needs `hideuser` and writes an
//!   `actor` ACL beside the block, restricting the name to the suppression group
//!   (`suppress/hide-user`); as in MediaWiki, only on an indefinite block. Triplespace adds `tsallbutread`: a block of everything but
//!   `read`, where MediaWiki's default removes `edit` and `createpage`. The other
//!   MediaWiki options (`autoblock`, `noemail`, `allowusertalk`, partial blocks) are not
//!   served and are ignored with a warning.
//! - **`action=unblock`**: `user`, `userid` or `id` (a block's log ID); `reason`. An account
//!   not blocked is `cantunblock`. As in MediaWiki, an unblock, or a `reblock` without
//!   `hidename`, by a caller holding `hideuser` also lifts the hide on the name (retiring
//!   the `actor` ACL, 09 §4.3); an administrator without `hideuser` lifts the block alone,
//!   and the name stays hidden.
//!
//! The answer's `id` is the block record's log ID: blocks have no IDs of their own.

use scatter_actors::Timestamp;
use scatter_actors::acl::{Acl, ReadKind, Target};
use scatter_actors::key::ActorKey;
use scatter_actors::membership::{Block, BlockScope};
use scatter_projection::Backend as _;
use serde_json::json;
use triplespace_api_ingest::blocks::{BlockError, write};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::response::{ApiError, ApiResponse};

fn refused(e: BlockError) -> ApiError {
    match e {
        BlockError::Denied => ApiError::permission_denied("block"),
        BlockError::AlreadyBlocked => ApiError::new(
            "alreadyblocked",
            "The user you tried to block was already blocked.",
        ),
        BlockError::NotBlocked => ApiError::new(
            "cantunblock",
            "The block you specified was not found. It may have been unblocked already.",
        ),
        BlockError::Store(m) => ApiError::internal(m),
    }
}

/// The account named by `user` or `userid`, or for an unblock by `id`: key and name.
async fn target(ctx: &Ctx) -> Result<(String, String), ApiError> {
    let tenant = &ctx.tenant.slug;
    if let Some(id) = ctx.params.get("id") {
        let n: i64 = id.parse().map_err(|_| ApiError::bad_value("id", id))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT a.target_id, v.name FROM view.activity a
                 JOIN view.actor v ON v.tenant = a.tenant AND v.actor_key = a.target_id
                 WHERE a.tenant = $1 AND a.logid = $2 AND a.log_type = 'block'",
                &[tenant, &n],
            )
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchblockid", format!("There is no block with ID {n}."))
            })?;
        // A block ID reaches an account whose name may be hidden from the caller (09
        // §6.3): the account is acted on, and the name is not told.
        let key: String = row.get(0);
        let effective = &ctx.caller.effective;
        let hidden = triplespace_projections::acl_read::hidden_actors(
            ctx.db(),
            tenant,
            std::slice::from_ref(&key),
            |g| effective.reads_in(g),
            Timestamp(now_micros()),
        )
        .await
        .map_err(ApiError::internal)?;
        let name = if hidden.contains(&key) {
            String::new()
        } else {
            row.get::<_, Option<String>>(1).unwrap_or_default()
        };
        return Ok((key, name));
    }
    let row = if let Some(id) = ctx.params.get("userid") {
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value("userid", id))?;
        ctx.db()
            .query_opt(
                "SELECT actor_key, name FROM view.actor
                 WHERE tenant = $1 AND name IS NOT NULL AND split_part(actor_key, ':', 2) = $2
                 ORDER BY actor_key LIMIT 1",
                &[tenant, &n.to_string()],
            )
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchuserid", format!("There is no user with ID {n}."))
            })?
    } else {
        let name = ctx.params.required("user")?;
        let key = crate::activity::actor_key_of(ctx, name)
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchuser", format!("The user \"{name}\" does not exist."))
            })?;
        ctx.db()
            .query_one(
                "SELECT actor_key, name FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                &[tenant, &key],
            )
            .await?
    };
    Ok((
        row.get(0),
        row.get::<_, Option<String>>(1).unwrap_or_default(),
    ))
}

async fn save(
    ctx: &Ctx,
    account: &str,
    block: &Block,
    reblock: bool,
    reason: &str,
) -> Result<Option<u64>, ApiError> {
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("block"))?;
    let store = ctx.app.store();
    let mut cx = store.begin().await.map_err(ApiError::internal)?;
    match write(
        store,
        ctx.app.pipeline(),
        &mut cx,
        &ctx.tenant.slug,
        &actor,
        &ctx.caller.effective,
        account,
        block,
        reblock,
        reason,
        now_micros(),
    )
    .await
    {
        Ok(record) => {
            store.commit(cx).await.map_err(ApiError::internal)?;
            Ok(record.header().logid)
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(refused(e))
        }
    }
}

/// `action=block`.
pub async fn block(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require("block")?;
    let hidename = ctx.params.flag("hidename");
    if hidename {
        ctx.caller.require("hideuser")?;
    }
    let (account, name) = target(ctx).await?;
    if Some(account.as_str()) == ctx.caller.identity.actor_key.as_deref() {
        return Err(ApiError::new(
            "ts-cantblock-self",
            "You cannot block yourself.",
        ));
    }
    let expiry_text = ctx.params.get("expiry").unwrap_or("infinite").to_string();
    let expires = super::moderation::expiry(&expiry_text)?;
    if hidename && expires.is_some() {
        return Err(ApiError::new(
            "ipb_expiry_temp",
            "Hidden username blocks must be indefinite.",
        ));
    }
    let nocreate = ctx.params.flag("nocreate");
    let removes = if ctx.params.flag("tsallbutread") {
        BlockScope::AllButRead
    } else {
        let mut p: std::collections::BTreeSet<String> =
            ["edit".to_string(), "createpage".to_string()].into();
        if nocreate {
            p.insert("createaccount".into());
        }
        BlockScope::Permissions(p)
    };
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let mut response = ApiResponse::default();
    for p in ["autoblock", "noemail", "allowusertalk", "partial"] {
        if ctx.params.flag(p) {
            response.warn(
                "block",
                &format!("The \"{p}\" option is not served and was ignored."),
            );
        }
    }
    let logid = save(
        ctx,
        &account,
        &Block::Block { removes, expires },
        ctx.params.flag("reblock"),
        &reason,
    )
    .await?;
    if hidename {
        let key = ActorKey::parse(&account).map_err(|e| ApiError::internal(e.to_string()))?;
        let (_, suppress_group) = super::moderation::groups(ctx).await?;
        let target = Target::Actor(key);
        let acl = Acl::read(target.clone(), ReadKind::Moderation, &suppress_group, None);
        super::moderation::save(ctx, target, Some(acl), &reason, &suppress_group, None).await?;
    } else if ctx.params.flag("reblock") {
        unhide(ctx, &account, &reason).await?;
    }
    let mut out = json!({"block": {
        "user": name,
        "userID": crate::activity::user_id(Some(&account)),
        "expiry": match expires {
            None => "infinite".to_string(),
            Some(Timestamp(t)) => triplespace_projections::feed::iso8601(t),
        },
        "id": logid,
        "reason": reason,
    }});
    for (flag, on) in [("nocreate", nocreate), ("hidename", hidename)] {
        if on {
            out["block"][flag] = json!(true);
        }
    }
    response.body = serde_json::from_value(out).unwrap_or_default();
    Ok(response)
}

/// Lifts the hide on `account`'s name, when there is one and the caller holds `hideuser`
/// (09 §4.3: an unhide is a retirement of the `actor` ACL). A caller without it leaves the
/// name hidden; nothing tells them it was.
async fn unhide(ctx: &Ctx, account: &str, reason: &str) -> Result<(), ApiError> {
    if ctx.caller.require("hideuser").is_err() {
        return Ok(());
    }
    let key = ActorKey::parse(account).map_err(|e| ApiError::internal(e.to_string()))?;
    let target = Target::Actor(key);
    let hidden = triplespace_projections::acl_read::current(
        ctx.db(),
        &ctx.tenant.slug,
        std::slice::from_ref(&target),
    )
    .await
    .map_err(ApiError::internal)?;
    if hidden.is_empty() {
        return Ok(());
    }
    let (_, suppress_group) = super::moderation::groups(ctx).await?;
    super::moderation::save(ctx, target, None, reason, &suppress_group, None).await?;
    Ok(())
}

/// `action=unblock`.
pub async fn unblock(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require("block")?;
    let (account, name) = target(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let logid = save(ctx, &account, &Block::Unblock, false, &reason).await?;
    unhide(ctx, &account, &reason).await?;
    let mut out = json!({"unblock": {
        "id": logid,
        "userid": crate::activity::user_id(Some(&account)),
        "reason": reason,
    }});
    if name.is_empty() {
        out["unblock"]["userhidden"] = json!(true);
    } else {
        out["unblock"]["user"] = json!(name);
    }
    Ok(ApiResponse::ok(out))
}
