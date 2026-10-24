//! `action=userrights` (09 §3.3; 0016 A35–A38): MediaWiki's module for changing an
//! account's groups, written through [`triplespace_api_ingest::membership`].
//!
//! Parameters as MediaWiki's: `user` or `userid`; `add`, `remove` (`|`-separated);
//! `expiry`, one for every added group or one for them all (`infinite` by default);
//! `reason`; and the `userrights` token. The answer is MediaWiki's,
//! `{"userrights": {"user", "userid", "added", "removed"}}`, where a removal from a group
//! the account was not in is left out.
//!
//! **Unlike MediaWiki,** a group the caller may not change is refused (`permissiondenied`
//! naming the group) and nothing is written, where MediaWiki leaves such a group out and
//! writes the rest; a membership change is a public act, and half of one is a surprise.
//! Removing the last member of `owner` while something still needs it is
//! `ts-last-owner`, naming the permission nobody else would hold (09 §3.6).

use std::collections::BTreeMap;

use scatter_actors::Timestamp;
use scatter_actors::evaluate::Effective;
use scatter_actors::group::GroupRegistry;
use scatter_projection::Backend as _;
use serde_json::{Value, json};
use triplespace_api_ingest::membership::{Change, MembershipError, Writer, change};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::response::{ApiError, ApiResponse};

/// The account a request names by `user` or `userid`: its key and name.
async fn account(ctx: &Ctx) -> Result<(String, String), ApiError> {
    let tenant = &ctx.tenant.slug;
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
            .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
            .ok_or_else(|| {
                ApiError::new("nosuchuserid", format!("There is no user with ID {n}."))
            })?
    } else {
        let name = ctx.params.required("user")?;
        let key = crate::activity::actor_key_of(ctx, name)
            .await?
            .ok_or_else(|| no_such_user(name))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                &[tenant, &key],
            )
            .await?
            .ok_or_else(|| no_such_user(name))?;
        (
            key,
            row.get::<_, Option<String>>(0)
                .unwrap_or_else(|| name.to_string()),
        )
    };
    Ok(row)
}

fn no_such_user(name: &str) -> ApiError {
    ApiError::new("nosuchuser", format!("The user \"{name}\" does not exist."))
}

/// The expiry of each added group: one each, or one for them all.
fn expiries(ctx: &Ctx, add: &[String]) -> Result<Vec<Option<Timestamp>>, ApiError> {
    let given = ctx.params.list("expiry");
    let parse = |t: &str| super::moderation::expiry(t);
    match given.len() {
        0 => Ok(vec![None; add.len()]),
        1 => {
            let e = parse(&given[0])?;
            Ok(vec![e; add.len()])
        }
        n if n == add.len() => given.iter().map(|t| parse(t)).collect(),
        _ => Err(ApiError::new(
            "toofewexpiries",
            format!(
                "{} expiry timestamps were provided where {} were needed.",
                given.len(),
                add.len()
            ),
        )),
    }
}

fn refused(e: MembershipError) -> ApiError {
    match e {
        MembershipError::Refused(r) => ApiError::new("permissiondenied", r.to_string()),
        MembershipError::UnknownGroup(g) => ApiError::bad_value("add", &g),
        MembershipError::LastOwner(p) => ApiError::new(
            "ts-last-owner",
            format!(
                "The last member of owner may leave only once a group other than owner, with a member, holds \"{p}\"."
            ),
        ),
        MembershipError::Store(m) => ApiError::internal(m),
    }
}

/// `action=userrights`.
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller
        .check_token(&ctx.app, &ctx.params, "userrights")?;
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("userrights"))?;
    let (key, name) = account(ctx).await?;
    let add = ctx.params.list("add");
    let remove = ctx.params.list("remove");
    let expires = expiries(ctx, &add)?;
    let registry = triplespace_accounts::groups::registry(ctx.db(), &ctx.tenant.slug).await?;
    let c = Change {
        account: key.clone(),
        add: add.into_iter().zip(expires).collect(),
        remove,
        reason: ctx.params.get("reason").unwrap_or_default().to_string(),
    };
    let store = ctx.app.store();
    let mut cx = store.begin().await.map_err(ApiError::internal)?;
    let out = change(
        store,
        ctx.app.pipeline(),
        &mut cx,
        &ctx.tenant.slug,
        &actor,
        Writer::Actor(&ctx.caller.effective),
        &registry,
        &c,
        now_micros(),
    )
    .await;
    let out = match out {
        Ok(o) => {
            store.commit(cx).await.map_err(ApiError::internal)?;
            o
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            return Err(refused(e));
        }
    };
    Ok(ApiResponse::ok(json!({"userrights": {
        "user": name,
        "userid": crate::activity::user_id(Some(&key)),
        "added": out.added,
        "removed": out.removed,
    }})))
}

/// The groups an actor may change, as `meta=userinfo&uiprop=changeablegroups` and
/// `meta=siteinfo&siprop=usergroups` report them: `add`, `remove`, `add-self`,
/// `remove-self`.
#[must_use]
pub fn changeable(who: &Effective, registry: &GroupRegistry) -> Value {
    let mut out: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (key, adding, own) in [
        ("add", true, false),
        ("remove", false, false),
        ("add-self", true, true),
        ("remove-self", false, true),
    ] {
        let list = out.entry(key).or_default();
        for g in registry.groups() {
            if scatter_actors::may_change_membership(who, registry, &g.name, adding, own).is_ok() {
                list.push(g.mediawiki_name.clone());
            }
        }
    }
    json!(out)
}

/// A group's own delegation lists, as `siprop=usergroups` reports them beside its rights.
#[must_use]
pub fn delegates(g: &scatter_actors::group::Group) -> Value {
    let mut m = serde_json::Map::new();
    for (key, list) in [
        ("add", &g.adds),
        ("remove", &g.removes),
        ("add-self", &g.adds_self),
        ("remove-self", &g.removes_self),
    ] {
        if !list.is_empty() {
            m.insert(key.into(), json!(list));
        }
    }
    Value::Object(m)
}
