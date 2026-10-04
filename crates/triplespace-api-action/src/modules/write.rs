//! The Wikibase write modules (wikibase-compat §4.0–4.1), each computing the entity as the
//! editor wants it and handing the difference to [`crate::edit`]: `wbeditentity`,
//! `wbcreateclaim`, `wbsetclaim`, `wbremoveclaims`, `wbsetlabel`, `wbsetdescription`,
//! `wbsetaliases`, `wbsetqualifier`, `wbremovequalifiers`, `wbsetreference`,
//! `wbremovereferences`. Every one is POST-only, takes `token`, `summary`, `tags`, `bot`
//! and `baserevid`, and answers with `success: 1` and the new `lastrevid`.

use scatter_wikibase_model::entity::{Entity, EntityType};
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, IdForm, StatementId};
use scatter_wikibase_model::json::Form;
use scatter_wikibase_model::statement::{Reference, Snak, SnakGroups, SnakKind, Statement};
use scatter_wikibase_model::value::{DataType, DataValue};
use serde_json::{Map, Value, json};
use triplespace_projections::read::Current;

use crate::api::Ctx;
use crate::edit::{self, EditMeta};
use crate::entity_json;
use crate::modules::entities::parse_id;
use crate::response::{ApiError, ApiResponse};

/// The subject of a write and what it was before.
struct Subject {
    id: EntityId,
    before: Option<Current>,
    /// The state the edit starts from: the resolved state, or an empty entity for a new
    /// or absent one.
    base: Entity,
}

fn no_such_entity(id: &str) -> ApiError {
    ApiError::new(
        "no-such-entity",
        format!("Could not find an entity with the ID \"{id}\"."),
    )
    .with("id", json!(id))
}

/// Loads an existing subject; a valid keyed ID no graph holds starts empty (0009 §4).
async fn existing(ctx: &Ctx, id: EntityId) -> Result<Subject, ApiError> {
    let before = edit::current(ctx, &id).await?;
    let base = match &before {
        Some(c) => c.resolved.entity.clone(),
        None if id.form() == IdForm::Keyed => {
            let t = id.keyed_parts().map_or("domain", |(t, _)| t).to_string();
            Entity::new(id.clone(), EntityType::parse(&t))
        }
        None => return Err(no_such_entity(id.as_str())),
    };
    Ok(Subject { id, before, base })
}

/// The entity a statement GUID names.
fn guid_entity(guid: &str) -> Result<(StatementId, EntityId), ApiError> {
    let sid = StatementId::parse(guid).map_err(|e| {
        ApiError::new(
            "invalid-guid",
            format!("Invalid claim GUID \"{guid}\": {e}"),
        )
    })?;
    let id = sid
        .entity_id()
        .ok_or_else(|| ApiError::new("invalid-guid", format!("\"{guid}\" names no entity.")))?;
    Ok((sid, id))
}

fn statement_mut<'a>(e: &'a mut Entity, guid: &StatementId) -> Option<&'a mut Statement> {
    e.statements
        .values_mut()
        .flatten()
        .find(|s| s.id.as_ref() == Some(guid))
}

fn no_such_claim(guid: &str) -> ApiError {
    ApiError::new(
        "no-such-claim",
        format!("Could not find a claim with the GUID \"{guid}\"."),
    )
}

/// The hasher for an entity's own hashes.
fn hasher(ctx: &Ctx, id: &EntityId) -> Hasher {
    entity_json::hasher_for(id, ctx.app.registry())
}

/// Saves `after` for the subject and reloads it.
async fn save(
    ctx: &Ctx,
    subject: &Subject,
    after: &Entity,
    meta: &EditMeta,
) -> Result<(Option<u64>, Option<Current>), ApiError> {
    if after.id != subject.id {
        return Err(ApiError::new(
            "invalid-entity-id",
            "The edit changes the entity's ID.",
        ));
    }
    let ops = match &subject.before {
        Some(c) => edit::operations(Some(c), after),
        None if subject.id.form() == IdForm::Keyed => {
            // A Domain exists because its key is valid (0009 §4, §10): the first local
            // assertion is an `add`, never a `create`.
            let empty = Entity::new(subject.id.clone(), subject.base.entity_type.clone());
            edit::operations_from(&empty, &[], after)
        }
        None => edit::operations(None, after),
    };
    let saved = edit::commit(ctx, &ops, meta, subject.before.as_ref()).await?;
    let now = edit::current(ctx, &subject.id).await?;
    let lastrevid = saved
        .lastrevid
        .or_else(|| now.as_ref().and_then(|c| c.lastrevid));
    Ok((lastrevid, now))
}

/// `{"entity": {…}, "success": 1}` with the full entity.
fn entity_response(
    ctx: &Ctx,
    current: Option<&Current>,
    lastrevid: Option<u64>,
    changed: bool,
) -> ApiResponse {
    let mut entity = current.map_or(Value::Null, |c| entity_json::present(c, ctx.app.registry()));
    if let Value::Object(m) = &mut entity {
        if let Some(r) = lastrevid {
            m.insert("lastrevid".into(), json!(r));
        }
        if !changed {
            m.insert("nochange".into(), json!(true));
        }
        for k in ["pageid", "ns", "title", "modified"] {
            m.remove(k);
        }
    }
    ApiResponse::ok(json!({"entity": entity, "success": 1}))
}

/// `{"pageinfo": {"lastrevid": N}, "success": 1}` plus extra fields.
fn pageinfo_response(lastrevid: Option<u64>, extra: Map<String, Value>) -> ApiResponse {
    let mut body = Map::new();
    body.insert("pageinfo".into(), json!({"lastrevid": lastrevid}));
    body.insert("success".into(), json!(1));
    for (k, v) in extra {
        body.insert(k, v);
    }
    ApiResponse::ok(Value::Object(body))
}

/// The Wikibase JSON of one statement, as a response carries it.
fn statement_value(ctx: &Ctx, entity: &Entity, guid: &StatementId) -> Value {
    let v: Value = serde_json::from_str(&entity.to_wikibase_json(&hasher(ctx, &entity.id), None))
        .unwrap_or(Value::Null);
    v.get("claims")
        .and_then(Value::as_object)
        .and_then(|groups| {
            groups
                .values()
                .filter_map(Value::as_array)
                .flatten()
                .find(|s| s.get("id").and_then(Value::as_str) == Some(guid.as_str()))
                .cloned()
        })
        .unwrap_or(Value::Null)
}

/// Dispatches a write module.
pub async fn run(ctx: &mut Ctx, action: &str) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require("edit")?;
    let meta = EditMeta::from_params(ctx)?;
    match action {
        "wbeditentity" => wbeditentity(ctx, &meta).await,
        "wbcreateclaim" => wbcreateclaim(ctx, &meta).await,
        "wbsetclaim" => wbsetclaim(ctx, &meta).await,
        "wbremoveclaims" => wbremoveclaims(ctx, &meta).await,
        "wbsetlabel" => set_term(ctx, &meta, "labels").await,
        "wbsetdescription" => set_term(ctx, &meta, "descriptions").await,
        "wbsetaliases" => wbsetaliases(ctx, &meta).await,
        "wbsetqualifier" => wbsetqualifier(ctx, &meta).await,
        "wbremovequalifiers" => wbremovequalifiers(ctx, &meta).await,
        "wbsetreference" => wbsetreference(ctx, &meta).await,
        "wbremovereferences" => wbremovereferences(ctx, &meta).await,
        other => Err(ApiError::bad_value("action", other)),
    }
}

/// The subject named by `id` / `new` (and not by `site`+`title`).
async fn subject_from_id_or_new(ctx: &Ctx) -> Result<(Subject, bool), ApiError> {
    if let Some(kind) = ctx.params.get("new") {
        if ctx.params.get("id").is_some() {
            return Err(ApiError::new(
                "param-illegal",
                "Either provide \"id\" or \"new\", not both.",
            ));
        }
        let entity_type = match kind {
            "item" => EntityType::Item,
            "property" => EntityType::Property,
            other => return Err(ApiError::bad_value("new", other)),
        };
        ctx.caller
            .require("createpage")
            .or_else(|_| ctx.caller.require("edit"))?;
        let mut cx = scatter_projection::Backend::begin(ctx.app.store()).await?;
        let id = scatter_ingest::write::mint_entity_id(
            ctx.app.store(),
            &mut cx,
            &ctx.tenant.slug,
            entity_type.name(),
        )
        .await
        .map_err(ApiError::internal)?;
        scatter_projection::Backend::commit(ctx.app.store(), cx).await?;
        let mut base = Entity::new(id.clone(), entity_type);
        if base.entity_type == EntityType::Property {
            base.datatype = Some(DataType::String);
        }
        return Ok((
            Subject {
                id,
                before: None,
                base,
            },
            true,
        ));
    }
    if ctx.params.get("site").is_some() || ctx.params.get("title").is_some() {
        return Err(ApiError::new(
            "param-illegal",
            "Lookup by site and title is not served; use id.",
        ));
    }
    let id = parse_id(ctx.params.required("id")?)?;
    Ok((existing(ctx, id).await?, false))
}

/// `wbeditentity`.
async fn wbeditentity(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let data: Value = serde_json::from_str(ctx.params.required("data")?).map_err(|e| {
        ApiError::new(
            "invalid-json",
            format!("The \"data\" parameter is not valid JSON: {e}"),
        )
    })?;
    let Value::Object(data) = data else {
        return Err(ApiError::new(
            "not-recognized",
            "The \"data\" parameter must be a JSON object.",
        ));
    };
    let (subject, is_new) = subject_from_id_or_new(ctx).await?;
    let h = hasher(ctx, &subject.id);
    let mut current: Value = if ctx.params.flag("clear") || is_new {
        let mut e = Entity::new(subject.id.clone(), subject.base.entity_type.clone());
        e.datatype.clone_from(&subject.base.datatype);
        e.to_value(Form::Wikibase(&h))
    } else {
        subject.base.to_value(Form::Wikibase(&h))
    };
    merge_edit_data(&mut current, &data, &subject.id)?;
    let after = Entity::from_value(current)
        .map_err(|e| {
            ApiError::new(
                "invalid-json",
                format!("The edited entity is not valid: {e}"),
            )
        })?
        .entity;
    let (lastrevid, now) = save(ctx, &subject, &after, meta).await?;
    let changed = subject
        .before
        .as_ref()
        .is_none_or(|b| b.lastrevid != lastrevid);
    Ok(entity_response(ctx, now.as_ref(), lastrevid, changed))
}

fn is_remove(v: &Value) -> bool {
    v.get("remove").is_some()
}

/// Merges `wbeditentity`'s `data` into the current Wikibase JSON (compat §4.1).
#[allow(clippy::too_many_lines)]
fn merge_edit_data(
    current: &mut Value,
    data: &Map<String, Value>,
    id: &EntityId,
) -> Result<(), ApiError> {
    let Value::Object(cur) = current else {
        return Err(ApiError::internal("entity JSON is not an object"));
    };
    for (key, value) in data {
        match key.as_str() {
            "labels" | "descriptions" => {
                let terms = cur.entry(key.clone()).or_insert_with(|| json!({}));
                let Value::Object(terms) = terms else {
                    continue;
                };
                for item in term_items(value) {
                    let Some(lang) = item.get("language").and_then(Value::as_str) else {
                        return Err(ApiError::new(
                            "not-recognized",
                            format!("A {key} entry has no language."),
                        ));
                    };
                    if is_remove(&item) || item.get("value").and_then(Value::as_str) == Some("") {
                        terms.remove(lang);
                    } else {
                        terms.insert(lang.to_string(), item.clone());
                    }
                }
            }
            "aliases" => {
                let groups = cur.entry("aliases").or_insert_with(|| json!({}));
                let Value::Object(groups) = groups else {
                    continue;
                };
                // Grouped by language: a list without add/remove flags replaces the
                // language's aliases; flagged entries add or remove one.
                let mut by_lang: std::collections::BTreeMap<String, Vec<Value>> =
                    std::collections::BTreeMap::new();
                for item in term_items(value) {
                    let Some(lang) = item.get("language").and_then(Value::as_str) else {
                        continue;
                    };
                    by_lang.entry(lang.to_string()).or_default().push(item);
                }
                for (lang, items) in by_lang {
                    let flagged = items.iter().any(|i| i.get("add").is_some() || is_remove(i));
                    if !flagged {
                        let list: Vec<Value> = items
                            .into_iter()
                            .filter(|i| {
                                i.get("value")
                                    .and_then(Value::as_str)
                                    .is_some_and(|v| !v.is_empty())
                            })
                            .collect();
                        if list.is_empty() {
                            groups.remove(&lang);
                        } else {
                            groups.insert(lang, Value::Array(list));
                        }
                        continue;
                    }
                    let mut list: Vec<Value> = groups
                        .get(&lang)
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for i in items {
                        let v = i
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if is_remove(&i) {
                            list.retain(|x| x.get("value").and_then(Value::as_str) != Some(&v));
                        } else if !list
                            .iter()
                            .any(|x| x.get("value").and_then(Value::as_str) == Some(&v))
                        {
                            list.push(json!({"language": lang, "value": v}));
                        }
                    }
                    if list.is_empty() {
                        groups.remove(&lang);
                    } else {
                        groups.insert(lang, Value::Array(list));
                    }
                }
            }
            "claims" => {
                let claims = cur.entry("claims").or_insert_with(|| json!({}));
                let Value::Object(claims) = claims else {
                    continue;
                };
                let items: Vec<Value> = match value {
                    Value::Array(a) => a.clone(),
                    Value::Object(o) => o
                        .values()
                        .filter_map(Value::as_array)
                        .flatten()
                        .cloned()
                        .collect(),
                    _ => {
                        return Err(ApiError::new(
                            "not-recognized",
                            "\"claims\" must be an array or an object.",
                        ));
                    }
                };
                for claim in items {
                    let guid = claim.get("id").and_then(Value::as_str).map(str::to_string);
                    if let Some(g) = &guid {
                        for group in claims.values_mut() {
                            if let Value::Array(a) = group {
                                a.retain(|s| s.get("id").and_then(Value::as_str) != Some(g));
                            }
                        }
                    }
                    if is_remove(&claim) {
                        continue;
                    }
                    let Some(property) =
                        claim.pointer("/mainsnak/property").and_then(Value::as_str)
                    else {
                        return Err(ApiError::new(
                            "invalid-claim",
                            "A claim has no main snak property.",
                        ));
                    };
                    let mut claim = claim.clone();
                    if guid.is_none()
                        && let Value::Object(m) = &mut claim
                    {
                        m.remove("id");
                    }
                    if let Value::Object(m) = &mut claim {
                        m.entry("type").or_insert(json!("statement"));
                        m.entry("rank").or_insert(json!("normal"));
                    }
                    claims
                        .entry(property.to_string())
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                        .ok_or_else(|| ApiError::internal("claims group is not an array"))?
                        .push(claim);
                }
                claims.retain(|_, g| g.as_array().is_some_and(|a| !a.is_empty()));
            }
            "sitelinks" => {
                let links = cur.entry("sitelinks").or_insert_with(|| json!({}));
                let Value::Object(links) = links else {
                    continue;
                };
                for item in term_items(value) {
                    let Some(site) = item.get("site").and_then(Value::as_str) else {
                        continue;
                    };
                    if is_remove(&item) || item.get("title").and_then(Value::as_str) == Some("") {
                        links.remove(site);
                    } else {
                        links.insert(site.to_string(), item.clone());
                    }
                }
            }
            "datatype" => {
                cur.insert("datatype".into(), value.clone());
            }
            "id" => {
                if value.as_str() != Some(id.as_str()) {
                    return Err(ApiError::new(
                        "param-invalid",
                        "\"data\" names a different entity than \"id\".",
                    ));
                }
            }
            "type" | "pageid" | "ns" | "title" | "lastrevid" | "modified" => {}
            other => {
                return Err(ApiError::new(
                    "not-recognized",
                    format!("Unknown key in \"data\": {other}."),
                ));
            }
        }
    }
    Ok(())
}

/// Term-like entries as a flat list: an object keyed by language or site, or an array.
fn term_items(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o
            .iter()
            .flat_map(|(_, v)| match v {
                Value::Array(a) => a.clone(),
                other => vec![other.clone()],
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A snak from `snaktype`, `property` and `value` (compat §4.1): `value` is the JSON of
/// the datavalue's `value`; its type follows the property's data type.
async fn snak_from_params(
    ctx: &Ctx,
    snaktype: &str,
    property: &str,
    value: Option<&str>,
) -> Result<Snak, ApiError> {
    let property = parse_id(property)?;
    let datatype = property_datatype(ctx, &property).await?;
    let kind = match snaktype {
        "value" => {
            let text = value.ok_or_else(|| ApiError::missing_param("value"))?;
            let v: Value = serde_json::from_str(text).map_err(|e| {
                ApiError::new("invalid-snak", format!("\"value\" is not valid JSON: {e}"))
            })?;
            let vt = datatype
                .as_ref()
                .and_then(DataType::value_type)
                .map_or("string", |t| t.name());
            let dv: DataValue =
                serde_json::from_value(json!({"value": v, "type": vt})).map_err(|e| {
                    ApiError::new("invalid-snak", format!("The value does not fit {vt}: {e}"))
                })?;
            SnakKind::Value(dv)
        }
        "somevalue" => SnakKind::SomeValue,
        "novalue" => SnakKind::NoValue,
        other => return Err(ApiError::bad_value("snaktype", other)),
    };
    Ok(Snak {
        property,
        kind,
        datatype,
        hash: None,
    })
}

/// A property's data type, from its current state.
async fn property_datatype(ctx: &Ctx, property: &EntityId) -> Result<Option<DataType>, ApiError> {
    Ok(edit::current(ctx, property)
        .await?
        .and_then(|c| c.resolved.entity.datatype))
}

/// `wbcreateclaim`.
async fn wbcreateclaim(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let id = parse_id(ctx.params.required("entity")?)?;
    let subject = existing(ctx, id).await?;
    let snak = snak_from_params(
        ctx,
        ctx.params.required("snaktype")?,
        ctx.params.required("property")?,
        ctx.params.get("value"),
    )
    .await?;
    let mut after = subject.base.clone();
    let property = snak.property.clone();
    let mut statement = Statement::new(snak);
    // A GUID now, so the response can name the statement (the write path would give the
    // same deterministic one).
    statement.id = Some(scatter_wikibase_changeset::deterministic_statement_id(
        &subject.id,
        &statement,
        &hasher(ctx, &subject.id),
    ));
    let guid = statement.id.clone().expect("set above");
    after
        .statements
        .entry(property)
        .or_default()
        .push(statement);
    let (lastrevid, now) = save(ctx, &subject, &after, meta).await?;
    let mut extra = Map::new();
    extra.insert(
        "claim".into(),
        now.as_ref().map_or(Value::Null, |c| {
            statement_value(ctx, &c.resolved.entity, &guid)
        }),
    );
    Ok(pageinfo_response(lastrevid, extra))
}

/// `wbsetclaim`.
async fn wbsetclaim(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let claim: Value = serde_json::from_str(ctx.params.required("claim")?)
        .map_err(|e| ApiError::new("invalid-claim", format!("\"claim\" is not valid JSON: {e}")))?;
    let guid_text = claim
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new("invalid-claim", "\"claim\" needs an \"id\"."))?
        .to_string();
    let (guid, id) = guid_entity(&guid_text)?;
    let subject = existing(ctx, id).await?;
    let mut statement: Statement = serde_json::from_value(claim).map_err(|e| {
        ApiError::new(
            "invalid-claim",
            format!("\"claim\" is not a statement: {e}"),
        )
    })?;
    statement.id = Some(guid.clone());
    let mut after = subject.base.clone();
    let property = statement.mainsnak.property.clone();
    let index = ctx
        .params
        .get("index")
        .and_then(|i| i.parse::<usize>().ok());
    let mut replaced = false;
    for group in after.statements.values_mut() {
        if let Some(pos) = group.iter().position(|s| s.id.as_ref() == Some(&guid)) {
            group[pos] = statement.clone();
            replaced = true;
        }
    }
    if !replaced {
        let group = after.statements.entry(property).or_default();
        match index {
            Some(i) if i < group.len() => group.insert(i, statement),
            _ => group.push(statement),
        }
    }
    let (lastrevid, now) = save(ctx, &subject, &after, meta).await?;
    let mut extra = Map::new();
    extra.insert(
        "claim".into(),
        now.as_ref().map_or(Value::Null, |c| {
            statement_value(ctx, &c.resolved.entity, &guid)
        }),
    );
    Ok(pageinfo_response(lastrevid, extra))
}

/// `wbremoveclaims`.
async fn wbremoveclaims(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let guids = ctx.params.list("claim");
    if guids.is_empty() {
        return Err(ApiError::missing_param("claim"));
    }
    let parsed: Vec<(StatementId, EntityId)> = guids
        .iter()
        .map(|g| guid_entity(g))
        .collect::<Result<_, _>>()?;
    let id = parsed[0].1.clone();
    if parsed.iter().any(|(_, e)| *e != id) {
        return Err(ApiError::new(
            "invalid-guid",
            "All claims must belong to one entity.",
        ));
    }
    let subject = existing(ctx, id).await?;
    let mut after = subject.base.clone();
    for (guid, _) in &parsed {
        if statement_mut(&mut after, guid).is_none() {
            return Err(no_such_claim(guid.as_str()));
        }
        for group in after.statements.values_mut() {
            group.retain(|s| s.id.as_ref() != Some(guid));
        }
    }
    after.statements.retain(|_, g| !g.is_empty());
    let (lastrevid, _) = save(ctx, &subject, &after, meta).await?;
    let mut extra = Map::new();
    extra.insert("claims".into(), json!(guids));
    Ok(pageinfo_response(lastrevid, extra))
}

/// `wbsetlabel` / `wbsetdescription`.
async fn set_term(ctx: &Ctx, meta: &EditMeta, which: &str) -> Result<ApiResponse, ApiError> {
    let language = ctx.params.required("language")?.to_string();
    let value = ctx.params.get("value").unwrap_or_default().to_string();
    let (subject, _) = subject_from_id_or_new(ctx).await?;
    if which == "labels" && subject.id.form() == IdForm::Keyed {
        return Err(ApiError::new(
            "derived-label",
            "A keyed entity's label is derived from its key (0009 §5).",
        ));
    }
    let mut after = subject.base.clone();
    let terms = if which == "labels" {
        &mut after.labels
    } else {
        &mut after.descriptions
    };
    if value.is_empty() {
        terms.remove(&language);
    } else {
        terms.insert(language.clone(), value.clone());
    }
    let (lastrevid, _) = save(ctx, &subject, &after, meta).await?;
    let mut entity = json!({"id": subject.id.as_str(), "type": after.entity_type.name()});
    if let Some(r) = lastrevid {
        entity["lastrevid"] = json!(r);
    }
    entity[which] = if value.is_empty() {
        json!({language.clone(): {"language": language, "removed": ""}})
    } else {
        json!({language.clone(): {"language": language, "value": value}})
    };
    Ok(ApiResponse::ok(json!({"entity": entity, "success": 1})))
}

/// `wbsetaliases`.
async fn wbsetaliases(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let language = ctx.params.required("language")?.to_string();
    let (subject, _) = subject_from_id_or_new(ctx).await?;
    let mut after = subject.base.clone();
    let list = after.aliases.entry(language.clone()).or_default();
    if let Some(set) = ctx.params.get("set") {
        *list = set
            .split('|')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    }
    for a in ctx.params.list("add") {
        if !list.contains(&a) {
            list.push(a);
        }
    }
    for r in ctx.params.list("remove") {
        list.retain(|x| *x != r);
    }
    if list.is_empty() {
        after.aliases.remove(&language);
    }
    let aliases_now: Vec<String> = after.aliases.get(&language).cloned().unwrap_or_default();
    let (lastrevid, _) = save(ctx, &subject, &after, meta).await?;
    let mut entity = json!({"id": subject.id.as_str(), "type": after.entity_type.name()});
    if let Some(r) = lastrevid {
        entity["lastrevid"] = json!(r);
    }
    entity["aliases"] = json!({language.clone(): aliases_now.iter().map(|a| json!({"language": language, "value": a})).collect::<Vec<_>>()});
    Ok(ApiResponse::ok(json!({"entity": entity, "success": 1})))
}

/// `wbsetqualifier`.
async fn wbsetqualifier(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let (guid, id) = guid_entity(ctx.params.required("claim")?)?;
    let subject = existing(ctx, id).await?;
    let snak = snak_from_params(
        ctx,
        ctx.params.get("snaktype").unwrap_or("value"),
        ctx.params.required("property")?,
        ctx.params.get("value"),
    )
    .await?;
    let h = hasher(ctx, &subject.id);
    let mut after = subject.base.clone();
    let s = statement_mut(&mut after, &guid).ok_or_else(|| no_such_claim(guid.as_str()))?;
    if let Some(old) = ctx.params.get("snakhash") {
        for group in s.qualifiers.values_mut() {
            group.retain(|q| h.snak(q) != old);
        }
        s.qualifiers.retain(|_, g| !g.is_empty());
    }
    s.qualifiers
        .entry(snak.property.clone())
        .or_default()
        .push(snak);
    let (lastrevid, now) = save(ctx, &subject, &after, meta).await?;
    let mut extra = Map::new();
    extra.insert(
        "claim".into(),
        now.as_ref().map_or(Value::Null, |c| {
            statement_value(ctx, &c.resolved.entity, &guid)
        }),
    );
    Ok(pageinfo_response(lastrevid, extra))
}

/// `wbremovequalifiers`.
async fn wbremovequalifiers(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let (guid, id) = guid_entity(ctx.params.required("claim")?)?;
    let hashes = ctx.params.list("qualifiers");
    if hashes.is_empty() {
        return Err(ApiError::missing_param("qualifiers"));
    }
    let subject = existing(ctx, id).await?;
    let h = hasher(ctx, &subject.id);
    let mut after = subject.base.clone();
    let s = statement_mut(&mut after, &guid).ok_or_else(|| no_such_claim(guid.as_str()))?;
    for group in s.qualifiers.values_mut() {
        group.retain(|q| !hashes.contains(&h.snak(q)));
    }
    s.qualifiers.retain(|_, g| !g.is_empty());
    let (lastrevid, _) = save(ctx, &subject, &after, meta).await?;
    Ok(pageinfo_response(lastrevid, Map::new()))
}

/// `wbsetreference`.
async fn wbsetreference(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let (guid, id) = guid_entity(ctx.params.required("statement")?)?;
    let snaks: Value = serde_json::from_str(ctx.params.required("snaks")?)
        .map_err(|e| ApiError::new("invalid-snak", format!("\"snaks\" is not valid JSON: {e}")))?;
    let mut groups = SnakGroups::new();
    let order: Vec<String> = ctx
        .params
        .get("snaks-order")
        .and_then(|o| serde_json::from_str::<Vec<String>>(o).ok())
        .unwrap_or_default();
    let snak_map: Map<String, Value> = match snaks {
        Value::Object(o) => o,
        Value::Array(a) => {
            let mut m = Map::new();
            for s in a {
                let p = s
                    .get("property")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if let Some(v) = m.entry(p).or_insert_with(|| json!([])).as_array_mut() {
                    v.push(s.clone());
                }
            }
            m
        }
        _ => {
            return Err(ApiError::new(
                "invalid-snak",
                "\"snaks\" must be an object or an array.",
            ));
        }
    };
    let keys: Vec<String> = if order.is_empty() {
        snak_map.keys().cloned().collect()
    } else {
        order
    };
    for p in keys {
        let Some(list) = snak_map.get(&p) else {
            continue;
        };
        let parsed: Vec<Snak> = serde_json::from_value(list.clone())
            .map_err(|e| ApiError::new("invalid-snak", format!("snaks of {p}: {e}")))?;
        groups.entry(parse_id(&p)?).or_default().extend(parsed);
    }
    let reference = Reference {
        snaks: groups,
        hash: None,
    };
    let subject = existing(ctx, id).await?;
    let h = hasher(ctx, &subject.id);
    let mut after = subject.base.clone();
    let s = statement_mut(&mut after, &guid).ok_or_else(|| no_such_claim(guid.as_str()))?;
    let new_hash = h.reference(&reference);
    if let Some(old) = ctx.params.get("reference") {
        match s.references.iter().position(|r| h.reference(r) == old) {
            Some(pos) => s.references[pos] = reference,
            None => {
                return Err(ApiError::new(
                    "no-such-reference",
                    format!("No reference with hash \"{old}\"."),
                ));
            }
        }
    } else if !s.references.iter().any(|r| h.reference(r) == new_hash) {
        let index = ctx
            .params
            .get("index")
            .and_then(|i| i.parse::<usize>().ok());
        match index {
            Some(i) if i < s.references.len() => s.references.insert(i, reference),
            _ => s.references.push(reference),
        }
    }
    let (lastrevid, now) = save(ctx, &subject, &after, meta).await?;
    let mut extra = Map::new();
    let statement = now.as_ref().map_or(Value::Null, |c| {
        statement_value(ctx, &c.resolved.entity, &guid)
    });
    let reference = statement
        .get("references")
        .and_then(Value::as_array)
        .and_then(|refs| {
            refs.iter()
                .find(|r| r.get("hash").and_then(Value::as_str) == Some(&new_hash))
        })
        .cloned()
        .unwrap_or(Value::Null);
    extra.insert("reference".into(), reference);
    Ok(pageinfo_response(lastrevid, extra))
}

/// `wbremovereferences`.
async fn wbremovereferences(ctx: &Ctx, meta: &EditMeta) -> Result<ApiResponse, ApiError> {
    let (guid, id) = guid_entity(ctx.params.required("statement")?)?;
    let hashes = ctx.params.list("references");
    if hashes.is_empty() {
        return Err(ApiError::missing_param("references"));
    }
    let subject = existing(ctx, id).await?;
    let h = hasher(ctx, &subject.id);
    let mut after = subject.base.clone();
    let s = statement_mut(&mut after, &guid).ok_or_else(|| no_such_claim(guid.as_str()))?;
    s.references.retain(|r| !hashes.contains(&h.reference(r)));
    let (lastrevid, _) = save(ctx, &subject, &after, meta).await?;
    Ok(pageinfo_response(lastrevid, Map::new()))
}
