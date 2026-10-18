//! Creating a `password` account (0087 §1, §6; 07 §2.6): the one writer that
//! `action=createaccount` and the approval of a request share.
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
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use triplespace_accounts::{password, requests};
use triplespace_projections::PgCx;
use triplespace_projections::actors::payload;

use crate::PgIngest;

/// The password an account is created with.
#[derive(Debug, Clone, Copy)]
pub enum Credential<'a> {
    /// In clear, from the registration form; hashed here.
    Password(&'a str),
    /// A hash made when a request was filed (0087 §6).
    Hash(&'a str),
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
    match credential {
        Credential::Password(p) => password::set(cx.conn(), &key_text, p).await,
        Credential::Hash(h) => password::set_hash(cx.conn(), &key_text, h).await,
    }
    .map_err(|e| CreateError::BadPassword(e.to_string()))?;
    let binding = password::binding_draft(&key_text, id, attested, now).map_err(store_error)?;
    append_projected(store, pipeline, cx, accounts, binding, Budget::NONE)
        .await
        .map_err(store_error)?;
    Ok(key_text)
}
