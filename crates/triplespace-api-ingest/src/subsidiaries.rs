//! Subsidiary accounts as their operator manages them (0024 §2; 07 §4.2, §4.4): creating
//! one, renaming it, and retiring and reactivating it. The one writer that
//! `/account/subsidiaries` and `Special:Account` share.
//!
//! A subsidiary is its actor records in the tenant's `actors` partition, kind `bot`, with
//! its operator (07 §4.1). Creating one appends its first record, attested by the
//! operator, which `list=logevents` shows as `newusers/create2`; a hand-made subsidiary is
//! never pending and holds no explicit group (07 §4.3). Renaming, retiring and
//! reactivating each append a new record that keeps the operator. Retiring also revokes
//! every key and every session opened with one (07 §4.4); reactivating only clears the
//! status, and the operator then issues a key.

use scatter_actors::actor::{ActorRecord, ActorStatus, normalize_name};
use scatter_actors::key::ActorKey;
use scatter_ingest::store::{IngestStore as _, Sequence};
use scatter_ingest::write::append_projected;
use scatter_projection::{Budget, Pipeline};
use triplespace_accounts::{Secret, keys, requests};
use triplespace_projections::PgCx;
use triplespace_projections::actors::payload;

use crate::PgIngest;
use crate::accounts::{CreateError, available, draft, release, store_error};

/// A subsidiary as its operator sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subsidiary {
    /// Its actor key.
    pub actor_key: String,
    /// Its current name.
    pub name: String,
    /// `active`, `retired` or `pending`.
    pub status: String,
    /// The groups it holds explicitly: `bot` is the bot flag (07 §4.3).
    pub groups: Vec<String>,
}

/// Why a subsidiary write was refused, beyond the name rules of [`CreateError`].
#[derive(Debug, thiserror::Error)]
pub enum SubsidiaryError {
    /// The name, or the store.
    #[error(transparent)]
    Create(#[from] CreateError),
    /// The operator holds `subsidiaries.max_per_account` subsidiaries already:
    /// `ts-subsidiary-limit`.
    #[error("an account may operate at most {0} subsidiaries")]
    Limit(i64),
    /// No subsidiary of the operator has that name: `ts-no-subsidiary`.
    #[error("you operate no subsidiary of that name")]
    NotFound,
    /// The subsidiary is retired, and the change needs it active: `ts-subsidiary-retired`.
    #[error("the subsidiary is retired")]
    Retired,
    /// The subsidiary is not retired: `ts-subsidiary-active`.
    #[error("the subsidiary is not retired")]
    NotRetired,
}

/// The operator's subsidiaries, by name, retired ones included.
///
/// # Errors
///
/// If the store fails.
pub async fn list<C: tokio_postgres::GenericClient>(
    client: &C,
    tenant: &str,
    operator: &str,
) -> Result<Vec<Subsidiary>, CreateError> {
    let rows = client
        .query(
            "SELECT a.actor_key, a.name, a.status,
                    COALESCE((SELECT array_agg(DISTINCT m.\"group\" ORDER BY m.\"group\")
                              FROM view.membership m
                              WHERE m.tenant = a.tenant AND m.actor_key = a.actor_key), '{}')
             FROM view.actor a
             WHERE a.tenant = $1 AND a.operator = $2 AND a.kind = 'bot'
             ORDER BY lower(a.name)",
            &[&tenant, &operator],
        )
        .await
        .map_err(store_error)?;
    Ok(rows
        .iter()
        .map(|r| Subsidiary {
            actor_key: r.get(0),
            name: r.get::<_, Option<String>>(1).unwrap_or_default(),
            status: r.get(2),
            groups: r.get(3),
        })
        .collect())
}

/// The operator's subsidiary named `name`.
///
/// # Errors
///
/// [`SubsidiaryError::NotFound`], or the store.
pub async fn find<C: tokio_postgres::GenericClient>(
    client: &C,
    tenant: &str,
    operator: &str,
    name: &str,
) -> Result<Subsidiary, SubsidiaryError> {
    let key = requests::name_key(&normalize_name(name));
    list(client, tenant, operator)
        .await?
        .into_iter()
        .find(|s| requests::name_key(&s.name) == key)
        .ok_or(SubsidiaryError::NotFound)
}

/// Appends an actor record for the subsidiary with `name` and `status`, keeping its
/// operator.
#[allow(clippy::too_many_arguments)]
async fn append(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    key: &ActorKey,
    record: &ActorRecord,
    attested_by: &str,
    comment: &str,
    now: u64,
) -> Result<(), CreateError> {
    record
        .validate(key)
        .map_err(|e| CreateError::Store(format!("actor record: {e}")))?;
    let actors = store
        .partition(cx, tenant, "actors")
        .await
        .map_err(store_error)?
        .ok_or_else(|| CreateError::Store(format!("`{tenant}` has no actors partition")))?;
    let mut d = draft(
        payload::ACTOR,
        &key.to_string(),
        &serde_json::to_value(record).map_err(store_error)?,
        comment,
        attested_by,
        now,
    )?;
    d.logid = Some(
        store
            .next_id(cx, tenant, &Sequence::Log)
            .await
            .map_err(store_error)?,
    );
    append_projected(store, pipeline, cx, actors, d, Budget::NONE)
        .await
        .map_err(store_error)?;
    Ok(())
}

/// Creates a subsidiary of `operator` in the caller's unit of work and returns its actor
/// key (07 §4.2). The name follows the rules of any hand-made name; the operator may hold
/// at most `limit` subsidiaries that are not retired. The caller checks that the operator
/// is a primary account holding `createaccount`.
///
/// # Errors
///
/// A [`SubsidiaryError`]; the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn create(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    secret: &Secret,
    tenant: &str,
    name: &str,
    operator: &str,
    limit: i64,
    now: u64,
) -> Result<String, SubsidiaryError> {
    let held = list(cx.conn(), tenant, operator)
        .await?
        .iter()
        .filter(|s| s.status != ActorStatus::Retired.name())
        .count();
    if i64::try_from(held).unwrap_or(i64::MAX) >= limit {
        return Err(SubsidiaryError::Limit(limit));
    }
    // `subsidiaries.name_pattern` (07 §4.2) is not built yet: no pattern is required.
    let name = available(cx, secret, tenant, name, false).await?;
    let operator_key = ActorKey::parse(operator).map_err(|e| CreateError::Store(e.to_string()))?;
    let issuer = triplespace_projections::tenants::issuer_of(cx.conn(), tenant)
        .await
        .map_err(store_error)?
        .ok_or_else(|| CreateError::Store(format!("tenant `{tenant}` is not registered")))?;
    let id = store
        .next_id(cx, tenant, &Sequence::User)
        .await
        .map_err(store_error)?;
    let key = ActorKey::numeric(&issuer, id);
    let record = ActorRecord::subsidiary(&name, operator_key);
    append(
        store,
        pipeline,
        cx,
        tenant,
        &key,
        &record,
        operator,
        "subsidiary created",
        now,
    )
    .await?;
    Ok(key.to_string())
}

/// What [`change`] does to a subsidiary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'a> {
    /// Renames it; the old name is released and never reused.
    Rename(&'a str),
    /// Retires it and revokes its keys.
    Retire,
    /// Reactivates a retired one.
    Reactivate,
}

/// Renames, retires or reactivates the operator's subsidiary `name` in the caller's unit
/// of work (07 §4.4, §4.5). Returns the subsidiary as it now is.
///
/// # Errors
///
/// A [`SubsidiaryError`]; the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn change(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    secret: &Secret,
    tenant: &str,
    operator: &str,
    name: &str,
    what: Change<'_>,
    now: u64,
) -> Result<Subsidiary, SubsidiaryError> {
    let mut sub = find(cx.conn(), tenant, operator, name).await?;
    let retired = sub.status == ActorStatus::Retired.name();
    let key = ActorKey::parse(&sub.actor_key).map_err(|e| CreateError::Store(e.to_string()))?;
    let operator_key = ActorKey::parse(operator).map_err(|e| CreateError::Store(e.to_string()))?;
    let status = match sub.status.as_str() {
        "pending" => ActorStatus::Pending,
        "retired" => ActorStatus::Retired,
        _ => ActorStatus::Active,
    };
    let (new_name, status, comment) = match what {
        Change::Rename(to) => {
            if retired {
                return Err(SubsidiaryError::Retired);
            }
            let to = available(cx, secret, tenant, to, false).await?;
            (to, status, "subsidiary renamed")
        }
        Change::Retire => {
            if retired {
                return Err(SubsidiaryError::Retired);
            }
            for k in keys::list(cx.conn(), &sub.actor_key)
                .await
                .map_err(store_error)?
            {
                if k.revoked_at.is_none() {
                    keys::revoke(cx.conn(), &sub.actor_key, &k.key_id)
                        .await
                        .map_err(store_error)?;
                }
            }
            (sub.name.clone(), ActorStatus::Retired, "subsidiary retired")
        }
        Change::Reactivate => {
            if !retired {
                return Err(SubsidiaryError::NotRetired);
            }
            (
                sub.name.clone(),
                ActorStatus::Active,
                "subsidiary reactivated",
            )
        }
    };
    let mut record = ActorRecord::subsidiary(&new_name, operator_key);
    record.status = status;
    append(
        store, pipeline, cx, tenant, &key, &record, operator, comment, now,
    )
    .await?;
    if requests::name_key(&new_name) != requests::name_key(&sub.name) {
        release(cx, secret, tenant, &sub.actor_key, &sub.name).await?;
    }
    sub.name = new_name;
    sub.status = status.name().to_string();
    Ok(sub)
}
