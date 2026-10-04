//! From a Wikibase edit to log records (0002 §8.1: the Action API submits a batch of one;
//! 0006 §8; 0013 §7). A write module computes the entity as the editor wants it, and this
//! module turns the difference from the current state into operations:
//!
//! - a new entity is one `create`;
//! - additions of terms, statements, sitelinks are one `add`;
//! - retractions are one `remove` (statements by GUID, terms by language, aliases by
//!   value, sitelinks by site) — of what the local graph asserts;
//! - a changed statement is a `remove` of its GUID and an `add` of the new statement under
//!   the same GUID, in that order, in one unit of work;
//! - a mirrored statement the editor removes or changes is not the tenant's to retract, so
//!   it is suppressed with an `override` (0004 §8), and a changed one re-added under a
//!   fresh local GUID.
//!
//! All the records of one edit go in one transaction with the base-offset check carried
//! from record to record, so a client sees either the whole edit or an `editconflict`.
//! The revision the module reports is the last record's.

use std::collections::{BTreeMap, BTreeSet};

use scatter_ingest::IngestError;
use scatter_ingest::write::{Attestation, Request, write_operation};
use scatter_projection::Backend as _;
use scatter_wikibase_changeset::{Operation, SitelinkPatch};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::{EntityId, StatementId};
use scatter_wikibase_model::statement::Statement;
use serde_json::json;
use triplespace_projections::read::{self, Current};

use crate::api::Ctx;
use crate::response::ApiError;

/// Microseconds since the epoch, now.
#[must_use]
pub fn now_micros() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}

/// What every write module carries beside its own parameters (compat §4.1).
#[derive(Debug, Clone, Default)]
pub struct EditMeta {
    /// `summary`.
    pub summary: Option<String>,
    /// `bot`.
    pub bot: bool,
    /// `baserevid`.
    pub baserevid: Option<u64>,
    /// `tags`.
    pub tags: Vec<String>,
}

impl EditMeta {
    /// From the request.
    pub fn from_params(ctx: &Ctx) -> Result<Self, ApiError> {
        let baserevid = match ctx.params.get("baserevid") {
            Some(v) => Some(v.parse::<u64>().map_err(|_| {
                ApiError::new(
                    "badinteger",
                    format!("Invalid value \"{v}\" for integer parameter \"baserevid\"."),
                )
            })?),
            None => None,
        };
        Ok(Self {
            summary: ctx.params.get("summary").map(str::to_string),
            bot: ctx.params.flag("bot"),
            baserevid,
            tags: ctx.params.list("tags"),
        })
    }
}

/// Reads the entity's current state.
pub async fn current(ctx: &Ctx, id: &EntityId) -> Result<Option<Current>, ApiError> {
    read::current(&ctx.client, ctx.app.order(), &ctx.tenant.slug, id)
        .await
        .map_err(ApiError::internal)
}

/// The statement with this GUID in an entity.
fn by_guid<'a>(e: &'a Entity, guid: &StatementId) -> Option<&'a Statement> {
    e.all_statements().find(|s| s.id.as_ref() == Some(guid))
}

/// Whether two statements say the same thing, hashes aside.
fn same_statement(a: &Statement, b: &Statement) -> bool {
    let h = Hasher::local();
    h.statement(a) == h.statement(b)
        && a.rank == b.rank
        && a.references.len() == b.references.len()
        && a.references
            .iter()
            .zip(&b.references)
            .all(|(x, y)| h.reference(x) == h.reference(y))
}

/// The operations that take `before` to `after`: a `create` for a new entity, else the
/// difference against the resolved state.
#[must_use]
pub fn operations(before: Option<&Current>, after: &Entity) -> Vec<Operation> {
    let Some(before) = before else {
        return vec![Operation::Create {
            id: Some(after.id.clone()),
            temporary_ref: None,
            entity: after.clone(),
            match_key: None,
            via: None,
        }];
    };
    diff(
        &before.resolved.entity,
        &|g| before.is_local_statement(g),
        after,
    )
}

/// The operations that take a state whose local statements are `local` to `after`; for a
/// keyed entity no graph holds yet, `was` is empty and everything is an `add`.
#[must_use]
pub fn operations_from(was: &Entity, local: &[StatementId], after: &Entity) -> Vec<Operation> {
    diff(was, &|g| local.contains(g), after)
}

#[allow(clippy::too_many_lines)]
fn diff(was: &Entity, is_local: &dyn Fn(&StatementId) -> bool, after: &Entity) -> Vec<Operation> {
    let id = after.id.clone();
    let mut remove_statements: Vec<StatementId> = Vec::new();
    let mut suppress: Vec<StatementId> = Vec::new();
    let mut add_claims = scatter_wikibase_model::entity::StatementGroups::default();

    // Statements the editor kept, changed or added.
    let mut seen: BTreeSet<StatementId> = BTreeSet::new();
    for (property, group) in &after.statements {
        for s in group {
            match &s.id {
                Some(guid) => {
                    seen.insert(guid.clone());
                    match by_guid(was, guid) {
                        Some(old) if same_statement(old, s) => {}
                        Some(_) if is_local(guid) => {
                            remove_statements.push(guid.clone());
                            add_claims
                                .entry(property.clone())
                                .or_default()
                                .push(s.clone());
                        }
                        Some(_) => {
                            // A mirrored statement changed: suppress upstream's, add ours
                            // under a fresh GUID.
                            suppress.push(guid.clone());
                            let mut fresh = s.clone();
                            fresh.id = None;
                            add_claims.entry(property.clone()).or_default().push(fresh);
                        }
                        None => add_claims
                            .entry(property.clone())
                            .or_default()
                            .push(s.clone()),
                    }
                }
                None => add_claims
                    .entry(property.clone())
                    .or_default()
                    .push(s.clone()),
            }
        }
    }
    // Statements the editor dropped.
    for s in was.all_statements() {
        if let Some(guid) = &s.id
            && !seen.contains(guid)
        {
            if is_local(guid) {
                remove_statements.push(guid.clone());
            } else {
                suppress.push(guid.clone());
            }
        }
    }

    // Terms and sitelinks.
    let mut add_labels = BTreeMap::new();
    let mut remove_labels = Vec::new();
    for (lang, v) in &after.labels {
        if was.labels.get(lang) != Some(v) {
            add_labels.insert(lang.clone(), v.clone());
        }
    }
    for lang in was.labels.keys() {
        if !after.labels.contains_key(lang) {
            remove_labels.push(lang.clone());
        }
    }
    let mut add_descriptions = BTreeMap::new();
    let mut remove_descriptions = Vec::new();
    for (lang, v) in &after.descriptions {
        if was.descriptions.get(lang) != Some(v) {
            add_descriptions.insert(lang.clone(), v.clone());
        }
    }
    for lang in was.descriptions.keys() {
        if !after.descriptions.contains_key(lang) {
            remove_descriptions.push(lang.clone());
        }
    }
    let mut add_aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut remove_aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let langs: BTreeSet<&String> = was.aliases.keys().chain(after.aliases.keys()).collect();
    for lang in langs {
        let old: Vec<String> = was.aliases.get(lang).cloned().unwrap_or_default();
        let new: Vec<String> = after.aliases.get(lang).cloned().unwrap_or_default();
        let added: Vec<String> = new.iter().filter(|a| !old.contains(a)).cloned().collect();
        let gone: Vec<String> = old.iter().filter(|a| !new.contains(a)).cloned().collect();
        if !added.is_empty() {
            add_aliases.insert(lang.clone(), added);
        }
        if !gone.is_empty() {
            remove_aliases.insert(lang.clone(), gone);
        }
    }
    let mut add_sitelinks = BTreeMap::new();
    let mut remove_sitelinks = Vec::new();
    for (site, link) in &after.sitelinks {
        if was.sitelinks.get(site) != Some(link) {
            add_sitelinks.insert(
                site.clone(),
                SitelinkPatch {
                    title: link.title.clone(),
                    badges: link.badges.clone(),
                },
            );
        }
    }
    for site in was.sitelinks.keys() {
        if !after.sitelinks.contains_key(site) {
            remove_sitelinks.push(site.clone());
        }
    }

    let mut ops = Vec::new();
    if !(remove_statements.is_empty()
        && remove_labels.is_empty()
        && remove_descriptions.is_empty()
        && remove_aliases.is_empty()
        && remove_sitelinks.is_empty())
    {
        ops.push(Operation::Remove {
            id: Some(id.clone()),
            page: None,
            statements: remove_statements,
            labels: remove_labels,
            descriptions: remove_descriptions,
            aliases: remove_aliases,
            sitelinks: remove_sitelinks,
            references: BTreeMap::new(),
            qualifiers: BTreeMap::new(),
            link: None,
        });
    }
    for statement in suppress {
        ops.push(Operation::Override {
            id: Some(id.clone()),
            statement,
            rank: None,
            suppress: Some(true),
        });
    }
    if !(add_claims.is_empty()
        && add_labels.is_empty()
        && add_descriptions.is_empty()
        && add_aliases.is_empty()
        && add_sitelinks.is_empty())
    {
        ops.push(Operation::Add {
            id: Some(id),
            page: None,
            labels: add_labels,
            descriptions: add_descriptions,
            aliases: add_aliases,
            claims: add_claims,
            sitelinks: add_sitelinks,
            references: BTreeMap::new(),
            qualifiers: BTreeMap::new(),
            entity: None,
            match_key: None,
            via: None,
            overwrite: false,
        });
    }
    ops
}

/// What a committed edit reports.
#[derive(Debug, Clone)]
pub struct Saved {
    /// The last record's revision ID.
    pub lastrevid: Option<u64>,
    /// How many records were written.
    pub records: usize,
}

/// The local offset a `baserevid` names (0006 §8): the record with that revision ID in
/// the tenant's `local` partition.
async fn base_offset(ctx: &Ctx, revid: u64) -> Result<Option<u64>, ApiError> {
    let row = ctx
        .client
        .query_opt(
            "SELECT a.\"offset\" FROM view.activity a JOIN log.partition p ON p.partition = a.partition
             WHERE a.tenant = $1 AND a.revid = $2 AND p.name = 'local'",
            &[&ctx.tenant.slug, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await?;
    Ok(row.map(|r| u64::try_from(r.get::<_, i64>(0)).unwrap_or(0)))
}

/// Writes the operations of one edit in one unit of work.
pub async fn commit(
    ctx: &Ctx,
    ops: &[Operation],
    meta: &EditMeta,
    before: Option<&Current>,
) -> Result<Saved, ApiError> {
    if ops.is_empty() {
        return Ok(Saved {
            lastrevid: before.and_then(|c| c.lastrevid),
            records: 0,
        });
    }
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("edit"))?;
    let mut attestation = Attestation::by(&actor);
    attestation.tags.clone_from(&meta.tags);
    if meta.bot && ctx.caller.effective.holds("bot") {
        attestation.tags.push("bot".into());
    }
    let mut request = Request::new(&ctx.tenant.slug, attestation, now_micros());
    request.comment.clone_from(&meta.summary);
    request.base_offset = match (meta.baserevid, before) {
        (Some(revid), Some(_)) => Some(base_offset(ctx, revid).await?.ok_or_else(|| {
            ApiError::new(
                "nosuchrevid",
                format!("There is no revision with ID {revid}."),
            )
        })?),
        _ => None,
    };
    let store = ctx.app.store();
    let pipeline = ctx.app.pipeline();
    let mut cx = store.begin().await?;
    let mut lastrevid = None;
    let mut records = 0;
    for op in ops {
        match write_operation(store, pipeline, &mut cx, &request, op, ctx.app.registry()).await {
            Ok(written) => {
                lastrevid = written.record.header().revid;
                request.base_offset = Some(written.offset());
                records += 1;
            }
            Err(IngestError::Conflict { latest, .. }) => {
                let _ = store.rollback(cx).await;
                let current_revid = before.and_then(|c| c.local_revid);
                return Err(ApiError::new(
                    "editconflict",
                    format!(
                        "Edit conflict: the entity changed since revision {}.",
                        meta.baserevid.unwrap_or(0)
                    ),
                )
                .with("currentrevid", json!(current_revid))
                .with("latestoffset", json!(latest)));
            }
            Err(IngestError::Invalid(problems)) => {
                let _ = store.rollback(cx).await;
                return Err(ApiError::new(
                    "modification-failed",
                    problems
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
            Err(e) => {
                let _ = store.rollback(cx).await;
                return Err(ApiError::new("failed-save", e.to_string()));
            }
        }
    }
    store.commit(cx).await?;
    Ok(Saved { lastrevid, records })
}
