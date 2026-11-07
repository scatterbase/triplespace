//! Creating an account and its login (0087 §1, §6; 0088 §6; 0089 §2–§4; 07 §2.6–§2.8):
//! the one writer that `action=createaccount` and the approval of a request share, with
//! a password or bound to a subject of an external issuer; and the other writes of
//! sign-in methods: adding and removing an external identity of the login (0088 §7),
//! and reclaiming an adopted account (0088 §8).
//!
//! An account is its first actor record in the tenant's `actors` partition, appended
//! with its projection in the caller's unit of work, and its binding `(login, n)` to an
//! instance login in the tenant's `accounts` partition. The login is a `scatter:v0/login`
//! record in the instance's `logins` partition, indexed by `private.login`, made here
//! when the credential is a password (the hash goes to `private.password`, keyed by the
//! login) or an external identity bound to no login yet; an identity bound to a login
//! already binds the new account to that login. On a single-tenant instance the login
//! takes the account's name (0089 §12, A3). The actor record is attested by the account
//! itself when a person registers, which `list=logevents` shows as `newusers/create`,
//! and by its creator or reviewer otherwise, `newusers/create2` (16 §2.3).

use scatter_actors::actor::{ActorRecord, check_name, normalize_name};
use scatter_actors::key::ActorKey;
use scatter_ingest::store::{IngestStore as _, Sequence};
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::body::{ATTESTATION, COMMENT, CONTENT};
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use triplespace_accounts::{bindings, logins, password, requests};
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
    /// The instance requires an external identity, and a password makes a login
    /// without one: `ts-external-required` (0101 §1).
    #[error("this instance creates a login only from an external identity")]
    ExternalRequired,
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
    // The tenant's issuer code, which its actor keys carry (0079 §1).
    let issuer = triplespace_projections::tenants::issuer_of(cx.conn(), tenant)
        .await
        .map_err(|e| CreateError::Store(e.to_string()))?
        .ok_or_else(|| CreateError::Store(format!("tenant `{tenant}` is not registered")))?;
    let id = store
        .next_id(cx, tenant, &Sequence::User)
        .await
        .map_err(store_error)?;
    let key = ActorKey::numeric(&issuer, id);
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
    let login = match credential {
        Credential::External { issuer, subject } => {
            let held = bindings::find(cx.conn(), "", issuer, subject)
                .await
                .map_err(store_error)?
                .and_then(|h| logins::id_of(&h));
            // The identity's login exists: the new account is one more of its accounts.
            if let Some(login) = held {
                login
            } else {
                let login = create_login(store, pipeline, cx, tenant, &name, attested, now).await?;
                bind_external(store, pipeline, cx, login, issuer, subject, attested, now).await?;
                login
            }
        }
        Credential::Password(_) | Credential::Hash(_) => {
            // Under `login.external_required` no login is made from a password
            // (0101 §1); the API refuses earlier, this is the invariant's own guard.
            if logins::external_required(cx.conn())
                .await
                .map_err(store_error)?
            {
                return Err(CreateError::ExternalRequired);
            }
            create_login(store, pipeline, cx, tenant, &name, attested, now).await?
        }
    };
    match credential {
        Credential::Password(p) => password::set(cx.conn(), login, p)
            .await
            .map_err(|e| CreateError::BadPassword(e.to_string()))?,
        Credential::Hash(h) => password::set_hash(cx.conn(), login, h)
            .await
            .map_err(|e| CreateError::BadPassword(e.to_string()))?,
        Credential::External { .. } => {}
    }
    bind_account(store, pipeline, cx, tenant, &key_text, login, attested, now).await?;
    Ok(key_text)
}

/// The instance's `logins` partition.
async fn logins_partition(store: &PgIngest, cx: &mut PgCx) -> Result<u64, CreateError> {
    store
        .partition(cx, "", "logins")
        .await
        .map_err(store_error)?
        .ok_or_else(|| {
            CreateError::Store(
                "the instance has no logins partition; run `triplespace migrate`".into(),
            )
        })
}

/// Makes a login (0089 §2): mints its ID, appends its record to the instance's `logins`
/// partition and indexes it. The name is the account's, or `{name} ({tenant})` where that
/// is held (0089 §15; A3).
///
/// # Errors
///
/// A [`CreateError`]; the caller rolls back.
pub async fn create_login(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    account_name: &str,
    attested_by: &str,
    now: u64,
) -> Result<i64, CreateError> {
    let logins_partition = logins_partition(store, cx).await?;
    let name = logins::name_for(cx.conn(), tenant, account_name)
        .await
        .map_err(store_error)?;
    let id = logins::next_id(cx.conn()).await.map_err(store_error)?;
    let d = logins::draft(id, &name, attested_by, now).map_err(store_error)?;
    let (record, _) = append_projected(store, pipeline, cx, logins_partition, d, Budget::NONE)
        .await
        .map_err(store_error)?;
    logins::insert(
        cx.conn(),
        id,
        &name,
        logins_partition,
        record.header().offset,
    )
    .await
    .map_err(store_error)?;
    Ok(id)
}

/// Binds a tenant account to its login: the binding record `(login, n)` keyed by the
/// account in the tenant's `accounts` partition, and its row. An account bound already,
/// or a login that has an account on the tenant already, is [`CreateError::Bound`].
/// This implements 0089 §4 (A3).
///
/// # Errors
///
/// A [`CreateError`]; the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn bind_account(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor_key: &str,
    login: i64,
    attested_by: &str,
    now: u64,
) -> Result<(), CreateError> {
    if logins::account_of(cx.conn(), tenant, login)
        .await
        .map_err(store_error)?
        .is_some()
        || logins::login_of(cx.conn(), tenant, actor_key)
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
    let subject = login.to_string();
    let d = bindings::draft(actor_key, logins::ISSUER, &subject, attested_by, now)
        .map_err(store_error)?;
    let (record, _) = append_projected(store, pipeline, cx, accounts, d, Budget::NONE)
        .await
        .map_err(store_error)?;
    bindings::insert(
        cx.conn(),
        tenant,
        logins::ISSUER,
        &subject,
        actor_key,
        accounts,
        record.header().offset,
    )
    .await
    .map_err(store_error)?;
    Ok(())
}

/// Binds a subject of an external issuer to a login: the binding record keyed
/// `login:{n}` in the instance's `logins` partition, and its row with the empty tenant. A
/// subject bound already is [`CreateError::Bound`]. With the password keyed by the login
/// in `triplespace_accounts::password` and [`unbind`], this implements 0089 §3 (A3).
///
/// # Errors
///
/// A [`CreateError`]; the caller rolls back.
#[allow(clippy::too_many_arguments)]
pub async fn bind_external(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    login: i64,
    issuer: &str,
    subject: &str,
    attested_by: &str,
    now: u64,
) -> Result<(), CreateError> {
    if bindings::find(cx.conn(), "", issuer, subject)
        .await
        .map_err(store_error)?
        .is_some()
    {
        return Err(CreateError::Bound);
    }
    let logins_partition = logins_partition(store, cx).await?;
    let holder = logins::key_of(login);
    let d = bindings::draft(&holder, issuer, subject, attested_by, now).map_err(store_error)?;
    let (record, _) = append_projected(store, pipeline, cx, logins_partition, d, Budget::NONE)
        .await
        .map_err(store_error)?;
    bindings::insert(
        cx.conn(),
        "",
        issuer,
        subject,
        &holder,
        logins_partition,
        record.header().offset,
    )
    .await
    .map_err(store_error)?;
    Ok(())
}

/// Adds a subject of an external issuer to the login of a tenant account (0088 §7;
/// 0089 §3), in the caller's unit of work. A subject bound to any login already is
/// [`CreateError::Bound`].
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
    let login = logins::login_of(cx.conn(), tenant, actor_key)
        .await
        .map_err(store_error)?
        .ok_or_else(|| CreateError::Store(format!("`{actor_key}` is bound to no login")))?;
    bind_external(
        store,
        pipeline,
        cx,
        login,
        issuer,
        subject,
        attested_by,
        now,
    )
    .await
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
    /// It is the login's last external identity and the instance requires one:
    /// `ts-external-required` (0101 §1).
    #[error("this instance requires an external identity, so the last one cannot be removed")]
    ExternalRequired,
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
}

/// Removes an external identity from the login of a tenant account (0088 §7; 0089 §3):
/// appends an `erase` of the binding record, reason class `privacy`, erases every part of
/// it, and deletes its row, in the caller's unit of work. The last way to log in, a
/// password counted, is refused.
///
/// # Errors
///
/// An [`UnbindError`]; the caller rolls back.
pub async fn unbind(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor_key: &str,
    issuer: &str,
    now: u64,
) -> Result<(), UnbindError> {
    let unbind_store = |e: &dyn std::fmt::Display| UnbindError::Store(e.to_string());
    let Some(login) = logins::login_of(cx.conn(), tenant, actor_key)
        .await
        .map_err(|e| unbind_store(&e))?
    else {
        return Err(UnbindError::NotBound);
    };
    let holder = logins::key_of(login);
    let Some(b) = bindings::take(cx.conn(), &holder, issuer)
        .await
        .map_err(|e| unbind_store(&e))?
    else {
        return Err(UnbindError::NotBound);
    };
    if bindings::ways_to_log_in(cx.conn(), login)
        .await
        .map_err(|e| unbind_store(&e))?
        <= 1
    {
        return Err(UnbindError::Last);
    }
    // The last external identity stays under the requirement, password or not.
    // This implements 0101 §1.
    if logins::external_required(cx.conn())
        .await
        .map_err(|e| unbind_store(&e))?
        && bindings::external_count(cx.conn(), login)
            .await
            .map_err(|e| unbind_store(&e))?
            <= 1
    {
        return Err(UnbindError::ExternalRequired);
    }
    erase_binding(store, pipeline, cx, &b, actor_key, "unlinkaccount", now)
        .await
        .map_err(|e| unbind_store(&e))?;
    bindings::delete(cx.conn(), &holder, issuer)
        .await
        .map_err(|e| unbind_store(&e))?;
    Ok(())
}

/// Appends an `erase` of a binding record, reason class `privacy`, and erases its parts.
///
/// # Errors
///
/// If the store fails.
pub async fn erase_binding(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    b: &bindings::Binding,
    actor: &str,
    authority: &str,
    now: u64,
) -> Result<(), CreateError> {
    let content = serde_json::json!({
        "targets": {"offsets": [b.offset]},
        "reason": "privacy",
        "authority": authority,
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
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(store_error)?,
    };
    let (erase, _) = append_projected(store, pipeline, cx, b.partition, d, Budget::NONE)
        .await
        .map_err(store_error)?;
    scatter_log_postgres::log::erase_parts(
        cx.conn(),
        b.partition,
        b.offset,
        &[CONTENT, COMMENT, ATTESTATION],
        Some(erase.header().offset),
    )
    .await
    .map_err(store_error)?;
    Ok(())
}

/// What enacting `login.external_required` waits on (0101 §2; 07 §2.7): the auth host
/// must offer an issuer, every login must hold an external identity, and no credential
/// may still be keyed by a tenant account.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Readiness {
    /// The external issuers the auth host offers, by code.
    pub offered: Vec<String>,
    /// The logins holding no external identity, by name.
    pub without_external: Vec<String>,
    /// Password hashes still keyed by tenant accounts: `logins migrate` is due.
    pub unmigrated_hashes: usize,
    /// External identities still keyed by tenant accounts: `logins migrate` is due.
    pub unmigrated_identities: usize,
}

impl Readiness {
    /// What stands in the way, one line each; empty when the requirement can be enacted.
    #[must_use]
    pub fn blockers(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.offered.is_empty() {
            out.push(
                "the auth host offers no external issuer: set a client credential with `oauth-client set` and list the issuer in `login.issuers`"
                    .to_string(),
            );
        }
        match self.without_external.len() {
            0 => {}
            1 => out.push("1 login holds no external identity".to_string()),
            n => out.push(format!("{n} logins hold no external identity")),
        }
        if self.unmigrated_hashes + self.unmigrated_identities > 0 {
            out.push(format!(
                "{} passwords and {} external identities are still keyed by tenant accounts: run `logins migrate`",
                self.unmigrated_hashes, self.unmigrated_identities
            ));
        }
        out
    }
}

/// Reads what enacting `login.external_required` waits on. The issuers offered are the
/// instance default of `login.issuers` with a credential that resolves for the tenant
/// at the farm base, whose base is the auth base on a single-tenant instance (0089 §12).
/// This implements 0101 §2.
///
/// # Errors
///
/// If the store fails.
pub async fn readiness<C: tokio_postgres::GenericClient>(
    client: &C,
    registry: &scatter_actors::issuer::IssuerRegistry,
    farm_base: &str,
) -> Result<Readiness, CreateError> {
    let (settings, _) = triplespace_accounts::settings::load(client, None)
        .await
        .map_err(store_error)?;
    let issuers: Vec<String> = settings
        .value("login.issuers")
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_else(|| vec!["*".into()]);
    let primary = client
        .query_opt(
            "SELECT code, config->>'base' FROM view.registry
             WHERE tenant = '' AND kind = 'tenant' AND (config->>'base') = $1
             ORDER BY \"offset\" LIMIT 1",
            &[&farm_base],
        )
        .await
        .map_err(store_error)?;
    let (slug, base): (String, String) = primary.map_or_else(
        || (String::new(), farm_base.to_string()),
        |r| (r.get(0), r.get::<_, Option<String>>(1).unwrap_or_default()),
    );
    let offered = triplespace_accounts::oauth::offered(client, registry, &slug, &base, &issuers)
        .await
        .map_err(store_error)?
        .into_iter()
        .map(|i| i.code.clone())
        .collect();
    Ok(Readiness {
        offered,
        without_external: logins::without_external(client)
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|l| l.name)
            .collect(),
        unmigrated_hashes: password::unmigrated(client)
            .await
            .map_err(store_error)?
            .len(),
        unmigrated_identities: bindings::unmigrated(client)
            .await
            .map_err(store_error)?
            .len(),
    })
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
    let held = bindings::find(cx.conn(), "", issuer, subject)
        .await
        .map_err(store_error)?
        .and_then(|h| logins::id_of(&h));
    let login = if let Some(login) = held {
        login
    } else {
        let name: Option<String> = cx
            .conn()
            .query_opt(
                "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                &[&tenant, &actor_key],
            )
            .await
            .map_err(store_error)?
            .and_then(|r| r.get(0));
        let login = create_login(
            store,
            pipeline,
            cx,
            tenant,
            name.as_deref().unwrap_or(actor_key),
            actor_key,
            now,
        )
        .await?;
        bind_external(store, pipeline, cx, login, issuer, subject, actor_key, now).await?;
        login
    };
    bind_account(
        store, pipeline, cx, tenant, actor_key, login, actor_key, now,
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
