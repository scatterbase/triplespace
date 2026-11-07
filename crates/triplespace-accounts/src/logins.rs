//! Instance logins (0089 §2, §4; 07 §2.8): `private.login`, the index of the `logins`
//! partition's `scatter:v0/login` records, and the two lookups between a login and the
//! tenant accounts bound to it through the built-in `login` issuer, whose subjects are
//! login IDs. A login is not an actor: nothing here touches `view.actor`.
//!
//! The record and its row are written together, in the caller's unit of work, as bindings
//! are; the ID comes from the instance sequence `private.login_id`, so that a login's ID is
//! minted before its record is drafted and the record's key, `login:{n}`, names it. With
//! `triplespace_api_ingest::accounts::create_login` this implements 0089 §2 (A3).

use std::time::SystemTime;

use scatter_actors::issuer::IssuerRegistry;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use tokio_postgres::GenericClient;

use crate::AccountsError;
use crate::bindings::{self, Binding};

/// The payload type of a login record (payloads-actors.md, Login).
pub const PAYLOAD_LOGIN: &str = "scatter:v0/login";
/// The built-in issuer whose subjects are login IDs (0089 §2).
pub const ISSUER: &str = "login";

/// One login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    /// The instance-wide ID.
    pub id: i64,
    /// The private login name.
    pub name: String,
    /// Its lower-cased form.
    pub name_key: String,
    /// When it was made.
    pub created: SystemTime,
    /// When it last authenticated.
    pub last_used: Option<SystemTime>,
    /// The login record's partition and offset.
    pub partition: u64,
    /// The login record's offset.
    pub offset: u64,
}

/// The record key of a login: `login:{n}`, the holder form a login's bindings use.
#[must_use]
pub fn key_of(id: i64) -> String {
    format!("login:{id}")
}

/// The ID a `login:{n}` key names.
#[must_use]
pub fn id_of(key: &str) -> Option<i64> {
    key.strip_prefix("login:")?.parse().ok()
}

/// The lower-cased form a login name is compared in.
#[must_use]
pub fn name_key(name: &str) -> String {
    name.to_lowercase()
}

#[allow(clippy::cast_possible_wrap)]
fn partition_to_db(partition: u64) -> i64 {
    partition as i64
}

#[allow(clippy::cast_sign_loss)]
fn partition_from_db(db: i64) -> u64 {
    db as u64
}

fn row_of(r: &tokio_postgres::Row) -> Login {
    Login {
        id: r.get("id"),
        name: r.get("name"),
        name_key: r.get("name_key"),
        created: r.get("created"),
        last_used: r.get("last_used"),
        partition: partition_from_db(r.get("partition")),
        offset: u64::try_from(r.get::<_, i64>("offset")).unwrap_or_default(),
    }
}

const COLUMNS: &str = "id, name, name_key, created, last_used, \"partition\", \"offset\"";

/// Mints the next login ID.
///
/// # Errors
///
/// If the store fails.
pub async fn next_id<C: GenericClient>(client: &C) -> Result<i64, AccountsError> {
    Ok(client
        .query_one("SELECT nextval('private.login_id')", &[])
        .await?
        .get(0))
}

/// The login record (payloads-actors.md, Login): `{"name": …}` keyed `login:{n}`.
///
/// # Errors
///
/// If the body cannot be built.
pub fn draft(id: i64, name: &str, attested_by: &str, now: u64) -> Result<Draft, AccountsError> {
    let content = Value::map(vec![(Value::text("name"), Value::text(name))]);
    Ok(Draft {
        appended_at: now,
        payload_type: PAYLOAD_LOGIN.into(),
        key: Some(key_of(id)),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &content,
            &Value::Null,
            &Value::map(vec![(Value::text("actor"), Value::text(attested_by))]),
        )
        .map_err(|e| AccountsError::Invalid(format!("login body: {e}")))?,
    })
}

/// Whether a login name is held, case-insensitively.
///
/// # Errors
///
/// If the store fails.
pub async fn name_taken<C: GenericClient>(client: &C, name: &str) -> Result<bool, AccountsError> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM private.login WHERE name_key = $1",
            &[&name_key(name)],
        )
        .await?
        .is_some())
}

/// Indexes a login record just appended.
///
/// # Errors
///
/// A store error, including the name being held.
pub async fn insert<C: GenericClient>(
    client: &C,
    id: i64,
    name: &str,
    partition: u64,
    offset: u64,
) -> Result<(), AccountsError> {
    client
        .execute(
            "INSERT INTO private.login (id, name, name_key, \"partition\", \"offset\") VALUES ($1, $2, $3, $4, $5)",
            &[
                &id,
                &name,
                &name_key(name),
                &partition_to_db(partition),
                &i64::try_from(offset).map_err(|e| AccountsError::Invalid(e.to_string()))?,
            ],
        )
        .await?;
    Ok(())
}

/// A login by ID.
///
/// # Errors
///
/// If the store fails.
pub async fn get<C: GenericClient>(client: &C, id: i64) -> Result<Option<Login>, AccountsError> {
    Ok(client
        .query_opt(
            &format!("SELECT {COLUMNS} FROM private.login WHERE id = $1"),
            &[&id],
        )
        .await?
        .as_ref()
        .map(row_of))
}

/// A login by name, case-insensitively.
///
/// # Errors
///
/// If the store fails.
pub async fn by_name<C: GenericClient>(
    client: &C,
    name: &str,
) -> Result<Option<Login>, AccountsError> {
    Ok(client
        .query_opt(
            &format!("SELECT {COLUMNS} FROM private.login WHERE name_key = $1"),
            &[&name_key(name)],
        )
        .await?
        .as_ref()
        .map(row_of))
}

/// Whether the instance requires every login to hold an external identity: the
/// instance setting `login.external_required` (0101 §1; 07 §2.7).
///
/// # Errors
///
/// If the store fails.
pub async fn external_required<C: GenericClient>(client: &C) -> Result<bool, AccountsError> {
    let (settings, _) = crate::settings::load(client, None).await?;
    Ok(settings
        .value("login.external_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

/// The logins holding no external identity, oldest first: what enacting
/// `login.external_required` waits on (0101 §2).
///
/// # Errors
///
/// If the store fails.
pub async fn without_external<C: GenericClient>(client: &C) -> Result<Vec<Login>, AccountsError> {
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM private.login l
                 WHERE NOT EXISTS (
                     SELECT 1 FROM private.binding b
                     WHERE b.tenant = '' AND b.issuer <> 'login' AND b.holder = 'login:' || l.id)
                 ORDER BY l.id"
            ),
            &[],
        )
        .await?;
    Ok(rows.iter().map(row_of).collect())
}

/// Whether the instance derives every account's name from its login's external
/// identity: `login.names = provider`, strict names (0102 §1; 07 §2.10).
///
/// # Errors
///
/// If the store fails.
pub async fn strict_names<C: GenericClient>(client: &C) -> Result<bool, AccountsError> {
    let (settings, _) = crate::settings::load(client, None).await?;
    Ok(settings
        .value("login.names")
        .and_then(|v| v.as_str().map(|s| s == "provider"))
        .unwrap_or(false))
}

/// The name an account has under strict names: `{label}-{username}`, the issuer's
/// `oauth.label`, a hyphen and the username the provider's profile gave, normalized
/// as every name is (0102 §1).
#[must_use]
pub fn derived_name(label: &str, username: &str) -> String {
    scatter_actors::actor::normalize_name(&format!("{label}-{username}"))
}

/// The strict name for an issuer's username, where the issuer serves login through
/// OAuth; `None` for any other issuer, which has no label to derive from.
#[must_use]
pub fn derived_name_for(registry: &IssuerRegistry, issuer: &str, username: &str) -> Option<String> {
    let label = &registry.by_code(issuer)?.oauth.as_ref()?.label;
    Some(derived_name(label, username))
}

/// A login's external identity: its only one under strict names (0102 §4), the oldest
/// otherwise.
///
/// # Errors
///
/// If the store fails.
pub async fn identity_of<C: GenericClient>(
    client: &C,
    login: i64,
) -> Result<Option<Binding>, AccountsError> {
    Ok(bindings::of_login(client, login).await?.into_iter().next())
}

/// The logins holding more than one external identity, oldest first: what enacting
/// strict names waits on (0102 §5).
///
/// # Errors
///
/// If the store fails.
pub async fn with_several_identities<C: GenericClient>(
    client: &C,
) -> Result<Vec<Login>, AccountsError> {
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM private.login l
                 WHERE (SELECT count(*) FROM private.binding b
                        WHERE b.tenant = '' AND b.issuer <> 'login' AND b.holder = 'login:' || l.id) > 1
                 ORDER BY l.id"
            ),
            &[],
        )
        .await?;
    Ok(rows.iter().map(row_of).collect())
}

/// One account's name beside the name strict names would give it (0102 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameCheck {
    /// The tenant.
    pub tenant: String,
    /// The account.
    pub actor_key: String,
    /// Its name.
    pub name: String,
    /// The derived name, or `None` while the identity's username is not yet known:
    /// it is learnt when the identity next authenticates (0102 A1).
    pub derived: Option<String>,
}

impl NameCheck {
    /// Whether the account's name is the derived one.
    #[must_use]
    pub fn matches(&self) -> bool {
        self.derived.as_deref() == Some(self.name.as_str())
    }
}

/// Every tenant account bound to a login that holds an external identity, with the
/// name strict names derive from that identity's username, oldest identity first
/// (0102 §5).
///
/// # Errors
///
/// If the store fails.
pub async fn name_checks<C: GenericClient>(
    client: &C,
    registry: &IssuerRegistry,
) -> Result<Vec<NameCheck>, AccountsError> {
    let rows = client
        .query(
            "SELECT a.tenant, a.holder, v.name, e.issuer, e.username
             FROM private.binding a
             JOIN view.actor v ON v.tenant = a.tenant AND v.actor_key = a.holder
             JOIN LATERAL (
                 SELECT issuer, username FROM private.binding e
                 WHERE e.tenant = '' AND e.issuer <> 'login' AND e.holder = 'login:' || a.subject
                 ORDER BY e.added LIMIT 1) e ON true
             WHERE a.issuer = 'login' AND v.name IS NOT NULL
             ORDER BY a.tenant, v.name",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let issuer: String = r.get(3);
            let username: Option<String> = r.get(4);
            NameCheck {
                tenant: r.get(0),
                actor_key: r.get(1),
                name: r.get(2),
                derived: username.and_then(|u| derived_name_for(registry, &issuer, &u)),
            }
        })
        .collect())
}

/// Notes that the login authenticated.
///
/// # Errors
///
/// If the store fails.
pub async fn touch<C: GenericClient>(client: &C, id: i64) -> Result<(), AccountsError> {
    client
        .execute(
            "UPDATE private.login SET last_used = now() WHERE id = $1",
            &[&id],
        )
        .await?;
    Ok(())
}

/// The account a login reaches on a tenant: its binding `(login, n)` there (0089 §4).
///
/// # Errors
///
/// If the store fails.
pub async fn account_of<C: GenericClient>(
    client: &C,
    tenant: &str,
    login: i64,
) -> Result<Option<String>, AccountsError> {
    Ok(client
        .query_opt(
            "SELECT holder FROM private.binding WHERE tenant = $1 AND issuer = $2 AND subject = $3",
            &[&tenant, &ISSUER, &login.to_string()],
        )
        .await?
        .map(|r| r.get(0)))
}

/// The login a tenant account is bound to, if any.
///
/// # Errors
///
/// If the store fails.
pub async fn login_of<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
) -> Result<Option<i64>, AccountsError> {
    let subject: Option<String> = client
        .query_opt(
            "SELECT subject FROM private.binding WHERE tenant = $1 AND issuer = $2 AND holder = $3",
            &[&tenant, &ISSUER, &actor_key],
        )
        .await?
        .map(|r| r.get(0));
    Ok(subject.and_then(|s| s.parse().ok()))
}

/// Every tenant account bound to a login: `(tenant, actor key)`.
///
/// # Errors
///
/// If the store fails.
pub async fn accounts_of<C: GenericClient>(
    client: &C,
    login: i64,
) -> Result<Vec<(String, String)>, AccountsError> {
    let rows = client
        .query(
            "SELECT tenant, holder FROM private.binding WHERE issuer = $1 AND subject = $2 ORDER BY added",
            &[&ISSUER, &login.to_string()],
        )
        .await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// A name for a login made from an account (0089 §15): the account's name, or
/// `{name} ({tenant slug})` where that is held.
///
/// # Errors
///
/// If the store fails.
pub async fn name_for<C: GenericClient>(
    client: &C,
    tenant: &str,
    account_name: &str,
) -> Result<String, AccountsError> {
    if !name_taken(client, account_name).await? {
        return Ok(account_name.to_string());
    }
    let qualified = format!("{account_name} ({tenant})");
    if !name_taken(client, &qualified).await? {
        return Ok(qualified);
    }
    for n in 2..1000 {
        let candidate = format!("{account_name} ({tenant} {n})");
        if !name_taken(client, &candidate).await? {
            return Ok(candidate);
        }
    }
    Err(AccountsError::Invalid(format!(
        "no free login name for `{account_name}`"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        assert_eq!(key_of(17), "login:17");
        assert_eq!(id_of("login:17"), Some(17));
        assert_eq!(id_of("librarybase:17"), None);
        assert_eq!(name_key("Alice"), "alice");
    }

    #[test]
    fn strict_names_derive_from_the_label_and_username() {
        // 0102 §1
        assert_eq!(derived_name("Wikimedia", "Alice"), "Wikimedia-Alice");
        assert_eq!(
            derived_name("Wikimedia", "alice_b"),
            "Wikimedia-alice b",
            "normalized as a name is: underscores are spaces"
        );
        let r = IssuerRegistry::default_registry();
        assert_eq!(
            derived_name_for(r, "wikimedia-central", "Bob").as_deref(),
            Some("Wikimedia-Bob")
        );
        assert_eq!(derived_name_for(r, "password", "Bob"), None);
        let c = NameCheck {
            tenant: "t".into(),
            actor_key: "t:1".into(),
            name: "Wikimedia-Bob".into(),
            derived: Some("Wikimedia-Bob".into()),
        };
        assert!(c.matches());
        assert!(
            !NameCheck {
                derived: None,
                ..c.clone()
            }
            .matches(),
            "unknown is not a match"
        );
    }

    #[test]
    fn login_is_a_core_record() {
        let d = draft(17, "Alice", "instance:abc", 1).unwrap();
        assert_eq!(d.payload_type, PAYLOAD_LOGIN);
        assert_eq!(d.key.as_deref(), Some("login:17"));
    }
}
