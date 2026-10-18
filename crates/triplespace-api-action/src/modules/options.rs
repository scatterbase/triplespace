//! `action=options` (0027 §6; compat §2.3): MediaWiki's module over the registered
//! preference keys. `reset` restores every default; `change` takes `name=value` pairs,
//! and `optionname` with `optionvalue` one pair whose value may hold `|`; a name without
//! a value resets that key. A key the registry does not know, or a value that does not
//! fit its type, is refused with MediaWiki's validation warning and not stored, and the
//! rest of the request still applies, as MediaWiki does. The caller needs
//! `editmyoptions`, a POST and the CSRF token.

use serde_json::{Value, json};
use triplespace_accounts::preferences::{self, PreferenceRegistry};

use crate::api::Ctx;
use crate::prefs::from_mediawiki;
use crate::response::{ApiError, ApiResponse};

/// `action=options`.
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    let Some(actor) = ctx.caller.identity.actor_key.clone() else {
        return Err(ApiError::new(
            "notloggedin",
            "Anonymous users cannot change preferences.",
        ));
    };
    ctx.caller.require("editmyoptions")?;
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    let registry = PreferenceRegistry::embedded();
    let mut response = ApiResponse::default();
    let reset = ctx.params.flag("reset");
    let mut pairs: Vec<(String, Option<String>)> = ctx
        .params
        .list("change")
        .into_iter()
        .map(|c| match c.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (c, None),
        })
        .collect();
    if let Some(name) = ctx.params.get("optionname") {
        pairs.push((
            name.to_string(),
            ctx.params.get("optionvalue").map(str::to_owned),
        ));
    }
    if !reset && pairs.is_empty() {
        return Err(ApiError::new("nochanges", "No changes were requested."));
    }
    let mut writes: Vec<(String, Option<Value>)> = Vec::new();
    for (key, value) in pairs {
        let Some(def) = registry.lookup(&key) else {
            response.warn(
                "options",
                &format!("Validation error for \"{key}\": not a valid preference."),
            );
            continue;
        };
        match value {
            None => writes.push((key, None)),
            Some(text) => {
                let v = from_mediawiki(def, &text);
                match def.check(&v) {
                    Ok(()) => writes.push((key, Some(v))),
                    Err(e) => {
                        response.warn("options", &format!("Validation error for \"{key}\": {e}."));
                    }
                }
            }
        }
    }
    let mut client = ctx
        .app
        .pool()
        .get()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let tx = client.transaction().await?;
    if reset {
        preferences::reset_all(&*tx, &actor).await?;
    }
    for (key, value) in &writes {
        match value {
            Some(v) => preferences::set(&*tx, &actor, key, v).await?,
            None => {
                preferences::reset(&*tx, &actor, key).await?;
            }
        }
    }
    tx.commit().await?;
    response.body.insert("options".into(), json!("success"));
    Ok(response)
}
