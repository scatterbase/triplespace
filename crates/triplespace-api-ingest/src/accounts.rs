//! Creating an account (0087 §1, §6; 0088 §6; 07 §2.6, §2.7): the one writer that
//! `action=createaccount` and the approval of a request share, with a password or bound to
//! a subject of an external issuer; and the other writes of sign-in methods: adding and
//! removing an external binding (0088 §7), and reclaiming an adopted account (0088 §8).
//!
//! An account is its first actor record in the tenant's `actors` partition and the
//! binding record of its password in `accounts`, appended with their projections in the
//! caller's unit of work, and the hash in `private.password`. The actor record is
//! attested by the account itself when a person registers, which `list=logevents` shows
//! as `newusers/create`, and by its creator or reviewer otherwise, `newusers/create2`
//! (16 §2.3).

use scatter_actors::actor::{ActorRecord, check_name, normalize_name};
use scatter_actors::key::ActorKey;
use scatter_ingest::store::{IngestStore as _, Sequence};
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::body::{ATTESTATION, COMMENT, CONTENT};
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use triplespace_accounts::{bindings, password, requests};
use triplespace_projections::PgCx;
use triplespace_projections::actors::payload;

use crate::PgIngest;

/// What an account is created with.
#[derive(Debug, Clone, Copy)]
pub enum Credential<'a> {
    /// A password in clear, from the registration form; hashed here.
    Password(&'a str),
    /// A password's hash, made when a request was filed (0087 §6).
    Hash(&'a str),
    /// A subject of an external issuer (0088 §6): no password.
    External {
        /// The issuer.
        issuer: &'a str,
        /// The subject, normalized.
        subject: &'a str,
    },
}

/// Why an account was not created. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    /// The name breaks the name rules (19 §2.3): MediaWiki's `invaliduser`.
    #[error("{0}")]
    InvalidName(&'static str),
    /// An account or a request holds the name: MediaWiki's `userexists`.
    #[error("the name is taken")]
    Taken,
    /// The password is refused: `passwordtooshort`.
    #[error("{0}")]
    BadPassword(String),
    /// The subject is bound to an account already: `ts-oauth-bound`.
    #[error("that identity is bound to an account already")]
    Bound,
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

fn store_error(e: impl std::fmt::Display) -> CreateError {
    CreateError::Store(e.to_string())
}

/// Checks a name against the name rules and the names taken, without writing.
///
/// # Errors
///
/// [`CreateError::InvalidName`] or [`CreateError::Taken`].
pub async fn check_available(
    cx: &mut PgCx,
    tenant: &str,
    name: &str,
) -> Result<String, CreateError> {
    let name = normalize_name(name);
    check_name(&name).map_err(CreateError::InvalidName)?;
    if requests::name_taken(cx.conn(), tenant, &name)
        .await
        .map_err(store_error)?
    {
        return Err(CreateError::Taken);
    }
    Ok(name)
}

fn draft(
    payload_type: &str,
    key: &str,
    content: &serde_json::Value,
    comment: &str,
    actor: &str,
    now: u64,
) -> Result<Draft, CreateError> {
    Ok(Draft {
        appended_at: now,
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::from_json(content),
            &Value::text(comment),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(store_error)?,
    })
}

/// Creates an account in the caller's unit of work and returns its actor key.
/// `performer` is who created it, or `None` for a person registering themselves.
///
/// # Errors
///
/// A [`CreateError`]; the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn create(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    name: &str,
    credential: Credential<'_>,
    performer: Option<&str>,
    comment: &str,
    now: u64,
) -> Result<String, CreateError> {
    let name = check_available(cx, tenant, name).await?;
    if let Credential::Password(p) = credential
        && p.chars().count() < 8
    {
        return Err(CreateError::BadPassword(
            "a password has at least 8 characters".into(),
        ));
    }
    let id = store
        .next_id(cx, tenant, &Sequence::User)
        .await
        .map_err(store_error)?;
    let key = ActorKey::numeric(tenant, id);
    let record = ActorRecord::registered(&name);
    record
        .validate(&key)
        .map_err(|e| CreateError::Store(format!("actor record: {e}")))?;
    let key_text = key.to_string();
    let attested = performer.unwrap_or(&key_text);
    let actors = store
        .partition(cx, tenant, "actors")
        .await
        .map_err(store_error)?
        .ok_or_else(|| CreateError::Store(format!("`{tenant}` has no actors partition")))?;
    let accounts = store
        .partition(cx, tenant, "accounts")
        .await
        .map_err(store_error)?
        .ok_or_else(|| CreateError::Store(format!("`{tenant}` has no accounts partition")))?;
    let mut d = draft(
        payload::ACTOR,
        &key_text,
        &serde_json::to_value(&record).map_err(store_error)?,
        comment,
        attested,
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
    if let Credential::External { issuer, subject } = credential {
        bind(
            store, pipeline, cx, tenant, &key_text, issuer, subject, attested, now,
        )
        .await?;
        return Ok(key_text);
    }
    match credential {
        Credential::Password(p) => password::set(cx.conn(), &key_text, p).await,
        Credential::Hash(h) => password::set_hash(cx.conn(), &key_text, h).await,
        Credential::External { .. } => unreachable!("handled above"),
    }
    .map_err(|e| CreateError::BadPassword(e.to_string()))?;
    let binding = password::binding_draft(&key_text, id, attested, now).map_err(store_error)?;
    append_projected(store, pipeline, cx, accounts, binding, Budget::NONE)
        .await
        .map_err(store_error)?;
    Ok(key_text)
}

/// Binds a subject of an external issuer to an account (0088 §7): appends the binding
/// record to the tenant's `accounts` partition and indexes it, in the caller's unit of
/// work. A subject bound already on the tenant is [`CreateError::Bound`].
///
/// # Errors
///
/// A [`CreateError`]; the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn bind(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor_key: &str,
    issuer: &str,
    subject: &str,
    attested_by: &str,
    now: u64,
) -> Result<(), CreateError> {
    if bindings::find(cx.conn(), tenant, issuer, subject)
        .await
        .map_err(store_error)?
        .is_some()
    {
        return Err(CreateError::Bound);
    }
    let accounts = store
        .partition(cx, tenant, "accounts")
        .await
        .map_err(store_error)?
        .ok_or_else(|| CreateError::Store(format!("`{tenant}` has no accounts partition")))?;
    let d = bindings::draft(actor_key, issuer, subject, attested_by, now).map_err(store_error)?;
    let (record, _) = append_projected(store, pipeline, cx, accounts, d, Budget::NONE)
        .await
        .map_err(store_error)?;
    bindings::insert(
        cx.conn(),
        tenant,
        issuer,
        subject,
        actor_key,
        accounts,
        record.header().offset,
    )
    .await
    .map_err(store_error)?;
    Ok(())
}

/// Why a sign-in method was not removed.
#[derive(Debug, thiserror::Error)]
pub enum UnbindError {
    /// The account has no binding to that issuer.
    #[error("no such sign-in method")]
    NotBound,
    /// It is the last way to log in: `ts-last-binding`.
    #[error("the last way to log in cannot be removed")]
    Last,
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

/// Removes an account's binding to an external issuer (0088 §7): appends an `erase` of
/// the binding record, reason class `privacy`, erases every part of it, and deletes its
/// row, in the caller's unit of work. The last way to log in, a password counted, is
/// refused.
///
/// # Errors
///
/// An [`UnbindError`]; the caller rolls back.
pub async fn unbind(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    actor_key: &str,
    issuer: &str,
    now: u64,
) -> Result<(), UnbindError> {
    let unbind_store = |e: &dyn std::fmt::Display| UnbindError::Store(e.to_string());
    let Some(b) = bindings::take(cx.conn(), actor_key, issuer)
        .await
        .map_err(|e| unbind_store(&e))?
    else {
        return Err(UnbindError::NotBound);
    };
    if bindings::ways_to_log_in(cx.conn(), actor_key)
        .await
        .map_err(|e| unbind_store(&e))?
        <= 1
    {
        return Err(UnbindError::Last);
    }
    let content = serde_json::json!({
        "targets": {"offsets": [b.offset]},
        "reason": "privacy",
        "authority": "unlinkaccount",
    });
    let d = Draft {
        appended_at: now,
        payload_type: PAYLOAD_ERASE.into(),
        key: None,
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::from_json(&content),
            &Value::Null,
            &Value::map(vec![(Value::text("actor"), Value::text(actor_key))]),
        )
        .map_err(|e| unbind_store(&e))?,
    };
    let (erase, _) = append_projected(store, pipeline, cx, b.partition, d, Budget::NONE)
        .await
        .map_err(|e| unbind_store(&e))?;
    scatter_log_postgres::log::erase_parts(
        cx.conn(),
        b.partition,
        b.offset,
        &[CONTENT, COMMENT, ATTESTATION],
        Some(erase.header().offset),
    )
    .await
    .map_err(|e| unbind_store(&e))?;
    bindings::delete(cx.conn(), actor_key, issuer)
        .await
        .map_err(|e| unbind_store(&e))?;
    Ok(())
}

/// The payload type of an `erase` record (payloads.md §6).
pub const PAYLOAD_ERASE: &str = "scatter:v0/erase";

/// The payload type of a `reclaim` record (payloads-actors.md, Reclaim; 0088 §8).
pub use triplespace_projections::common::PAYLOAD_RECLAIM;

/// Reclaims an adopted account for the person a source vouched for (0088 §8): binds the
/// subject and appends a public `scatter:v0/reclaim` record, `{"method": "vouched"}`, to
/// the tenant's `log` partition, attested by the account, in the caller's unit of work.
///
/// # Errors
///
/// A [`CreateError`]; the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn reclaim(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor_key: &str,
    issuer: &str,
    subject: &str,
    now: u64,
) -> Result<(), CreateError> {
    bind(
        store, pipeline, cx, tenant, actor_key, issuer, subject, actor_key, now,
    )
    .await?;
    let log = store
        .partition(cx, tenant, "log")
        .await
        .map_err(store_error)?
        .ok_or_else(|| CreateError::Store(format!("`{tenant}` has no log partition")))?;
    let mut d = draft(
        PAYLOAD_RECLAIM,
        actor_key,
        &serde_json::json!({"method": "vouched"}),
        "",
        actor_key,
        now,
    )?;
    d.logid = Some(
        store
            .next_id(cx, tenant, &Sequence::Log)
            .await
            .map_err(store_error)?,
    );
    append_projected(store, pipeline, cx, log, d, Budget::NONE)
        .await
        .map_err(store_error)?;
    Ok(())
}
