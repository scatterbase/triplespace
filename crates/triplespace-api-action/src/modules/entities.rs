//! `wbgetentities` (wikibase-compat §4.0–4.1; 0009 §4; 0017 §1; 0044 §1): entities by
//! ID, with `props` and `languages`; `missing` for an absent local or foreign ID, an
//! empty entity for a valid keyed ID no graph holds, `invalid-entity-id` for anything
//! that is not an ID. `titles` with `sites` is not served (no sitelinks in the milestone).

use scatter_normalize::keyed::KeyedRegistry;
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde_json::{Value, json};

use crate::api::Ctx;
use crate::edit;
use crate::entity_json;
use crate::response::{ApiError, ApiResponse};

/// Parses and normalizes an ID as a client wrote it: `q6` → `Q6`, `QQQ6` → `Q6`
/// (0044 §1), `Item:Q6` → `Q6`, `domain:Example.ORG` → `domain:example.org`.
pub fn parse_id(text: &str) -> Result<EntityId, ApiError> {
    let t = text.trim();
    // A prefixed title stands for its ID.
    let t = if let Some(rest) = t
        .strip_prefix("Item:")
        .or_else(|| t.strip_prefix("Property:"))
    {
        rest.to_string()
    } else if let Some(rest) = t.strip_prefix("Domain:") {
        format!("domain:{rest}")
    } else if let Some(rest) = t.strip_prefix("Keyword:") {
        format!("keyword:{rest}")
    } else if let Some(rest) = t.strip_prefix("Notation:") {
        format!("notation:{rest}")
    } else {
        t.to_string()
    };
    let invalid = |why: String| {
        ApiError::new(
            "invalid-entity-id",
            format!("Invalid entity ID \"{text}\": {why}"),
        )
    };
    if let Some((kt, key)) = t.split_once(':')
        && let Some(def) = KeyedRegistry::default_registry().by_name(kt)
    {
        let key = def.normalize(key).map_err(|e| invalid(e.to_string()))?;
        return EntityId::parse(&format!("{kt}:{key}")).map_err(|e| invalid(e.to_string()));
    }
    // `QQQ6` is the tenant-relative form of `Q6` (0044 §1); `q6` is `Q6`.
    let t = t.strip_prefix("QQ").map_or(t.as_str(), |r| r);
    let mut upper = String::with_capacity(t.len());
    for (i, c) in t.chars().enumerate() {
        upper.push(if i == 0 { c.to_ascii_uppercase() } else { c });
    }
    EntityId::parse(&upper).map_err(|e| invalid(e.to_string()))
}

/// Whether a client may read an entity of this form: a local one, a foreign one of a
/// registered provider, a keyed one.
fn check_form(id: &EntityId, ctx: &Ctx) -> Result<(), ApiError> {
    if id.form() == IdForm::Foreign && ctx.app.registry().parse_foreign_id(id.as_str()).is_err() {
        return Err(ApiError::new(
            "invalid-entity-id",
            format!("\"{}\" names no registered provider.", id.as_str()),
        ));
    }
    Ok(())
}

/// The served JSON of one entity, or `missing`.
pub async fn entity_json(ctx: &Ctx, id: &EntityId, as_given: &str) -> Result<Value, ApiError> {
    check_form(id, ctx)?;
    match edit::current(ctx, id).await? {
        Some(current) => Ok(entity_json::present(&current, ctx.app.registry())),
        None if id.form() == IdForm::Keyed => Ok(entity_json::empty_keyed(id)),
        None => Ok(json!({"id": as_given, "missing": ""})),
    }
}

/// Runs `wbgetentities`.
pub async fn wbgetentities(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    let ids = ctx.params.list("ids");
    if ids.is_empty() {
        if ctx.params.get("titles").is_some() {
            return Err(ApiError::new(
                "param-missing",
                "Lookup by sites and titles is not served; use ids.",
            ));
        }
        return Err(ApiError::new(
            "param-missing",
            "Either provide the item \"ids\" or pairs of \"sites\" and \"titles\" for corresponding pages.",
        ));
    }
    if ids.len() > 50 {
        return Err(ApiError::new(
            "toomanyvalues",
            "Too many values supplied for parameter \"ids\". The limit is 50.",
        )
        .with("parameter", json!("ids"))
        .with("limit", json!(50)));
    }
    let props = ctx.params.list("props");
    let languages = ctx.params.list("languages");
    ctx.caller.require("read")?;
    let mut entries = Vec::new();
    for given in ids {
        let id = parse_id(&given)?;
        let mut v = entity_json(ctx, &id, &given).await?;
        ctx.cache_tags.push(format!("entity:{}", id.as_str()));
        if v.get("missing").is_none() {
            entity_json::filter(&mut v, &props, &languages);
        }
        // Keyed by the canonical ID, as Wikibase keys by the normalized one.
        let key = if v.get("missing").is_some() {
            given
        } else {
            id.as_str().to_string()
        };
        entries.push((key, v));
    }
    Ok(ApiResponse::ok(json!({
        "entities": entity_json::entities_object(entries),
        "success": 1,
    })))
}
