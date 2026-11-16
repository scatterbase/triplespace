//! Who a request is (0013 §7, step 1): the session its cookie names or the key its bearer
//! header carries, the actor behind it from `view.actor`, the memberships and blocks in
//! force, and the effective permissions that follow (0016 §3; 0024 §3–4).

use std::collections::BTreeSet;

use scatter_actors::actor::{ActorKind, ActorStatus};
use scatter_actors::evaluate::{Credential, Effective, Principal, evaluate};
use scatter_actors::grant::GrantRegistry;
use tokio_postgres::GenericClient;

use crate::keys::KeyAuth;
use crate::session::{self, Session};
use crate::{AccountsError, now_micros};

/// What a request carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth<'a> {
    /// Nothing.
    None,
    /// The session cookie's value.
    Cookie(&'a str),
    /// `Authorization: Bearer {key ID}.{secret}`.
    Bearer(&'a str),
}

/// The actor behind a request, as far as the store knows them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identity {
    /// The actor key; `None` for an anonymous request.
    pub actor_key: Option<String>,
    /// The current name.
    pub name: Option<String>,
    /// The kind.
    pub kind: Option<ActorKind>,
    /// The status.
    pub status: ActorStatus,
    /// The operator, for a subsidiary.
    pub operator: Option<String>,
    /// Explicit memberships in force.
    pub groups: BTreeSet<String>,
    /// The key the request authenticated with, directly or through its session.
    pub credential: Option<KeyAuth>,
    /// The session, for a cookie request (anonymous sessions included).
    pub session: Option<Session>,
    /// The instance login the session holds (0089 §2), for a person's cookie session.
    pub login: Option<i64>,
}

impl Identity {
    /// Whether someone is logged in.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.actor_key.is_some()
    }

    /// What CSRF tokens bind to: the session ID, else the key ID (a stateless bearer
    /// request); `None` for an anonymous request without a session.
    #[must_use]
    pub fn token_binding(&self) -> Option<&str> {
        self.session
            .as_ref()
            .map(|s| s.id.as_str())
            .or(self.credential.as_ref().map(|c| c.key_id.as_str()))
    }
}

/// An identity with its permissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Who.
    pub identity: Identity,
    /// What they may do.
    pub effective: Effective,
}

/// An actor's row.
struct ActorRow {
    name: Option<String>,
    kind: ActorKind,
    status: ActorStatus,
    operator: Option<String>,
}

async fn actor<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
) -> Result<Option<ActorRow>, AccountsError> {
    let row = client
        .query_opt(
            "SELECT name, kind, status, operator FROM view.actor WHERE tenant = $1 AND actor_key = $2",
            &[&tenant, &actor_key],
        )
        .await?;
    Ok(row.map(|r| ActorRow {
        name: r.get(0),
        kind: parse_enum(&r.get::<_, String>(1)).unwrap_or(ActorKind::Registered),
        status: parse_enum(&r.get::<_, String>(2)).unwrap_or_default(),
        operator: r.get(3),
    }))
}

fn parse_enum<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// Memberships in force on the tenant and instance layers.
async fn groups<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
) -> Result<BTreeSet<String>, AccountsError> {
    let now = i64::try_from(now_micros()).unwrap_or(i64::MAX);
    let rows = client
        .query(
            "SELECT \"group\" FROM view.membership
             WHERE tenant = $1 AND actor_key = $2 AND (expires IS NULL OR expires > $3)",
            &[&tenant, &actor_key, &now],
        )
        .await?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// What the actor's blocks remove: the union over layers; a row without `removes` is
/// all-but-read (0016 §3).
async fn blocked<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
    all: &BTreeSet<String>,
) -> Result<BTreeSet<String>, AccountsError> {
    let now = i64::try_from(now_micros()).unwrap_or(i64::MAX);
    let rows = client
        .query(
            "SELECT removes FROM view.block
             WHERE tenant = $1 AND actor_key = $2 AND (expires IS NULL OR expires > $3)",
            &[&tenant, &actor_key, &now],
        )
        .await?;
    let mut out = BTreeSet::new();
    for r in rows {
        match r.get::<_, Option<Vec<String>>>(0) {
            Some(list) => out.extend(list),
            None => out.extend(all.iter().filter(|p| *p != "read").cloned()),
        }
    }
    Ok(out)
}

/// Resolves a request's identity and permissions on a tenant.
pub async fn resolve<C: GenericClient>(
    client: &C,
    tenant: &str,
    auth: Auth<'_>,
) -> Result<Resolved, AccountsError> {
    let mut identity = Identity::default();
    match auth {
        Auth::None => {}
        Auth::Cookie(id) => {
            // Through `ops.caller_session`, which reads `private` as its owner, so that a
            // connection of the public API resolves its caller (0013 §4, A101).
            let row = client
                .query_opt(
                    "SELECT actor_key, key_id, expires, login, central, key_grants
                     FROM ops.caller_session($1, $2)",
                    &[&id, &tenant],
                )
                .await?;
            if let Some(r) = row {
                let s = session::Session {
                    id: id.to_string(),
                    tenant: tenant.to_string(),
                    actor_key: r.get(0),
                    key_id: r.get(1),
                    expires: r.get(2),
                    login: r.get(3),
                    central: r.get(4),
                };
                identity.actor_key.clone_from(&s.actor_key);
                identity.login = s.login;
                if let (Some(key_id), Some(actor_key)) = (&s.key_id, &s.actor_key) {
                    // A bot-password session carries its key's grants (0024 §4).
                    let grants: Vec<String> =
                        r.get::<_, Option<Vec<String>>>(5).unwrap_or_default();
                    identity.credential = Some(KeyAuth {
                        actor_key: actor_key.clone(),
                        key_id: key_id.clone(),
                        grants: grants.into_iter().collect(),
                    });
                }
                identity.session = Some(s);
            }
        }
        Auth::Bearer(bearer) => {
            if let Some(key) = crate::keys::caller_bearer(client, bearer).await? {
                identity.actor_key = Some(key.actor_key.clone());
                identity.credential = Some(key);
            }
        }
    }
    let grants_registry = GrantRegistry::default_registry();
    let Some(actor_key) = identity.actor_key.clone() else {
        let groups_registry = crate::groups::registry(client, tenant).await?;
        return Ok(Resolved {
            effective: evaluate(&Principal::anonymous(), &groups_registry, grants_registry),
            identity,
        });
    };
    let Some(row) = actor(client, tenant, &actor_key).await? else {
        return Err(AccountsError::NoAccount(actor_key));
    };
    identity.name.clone_from(&row.name);
    identity.kind = Some(row.kind);
    identity.status = row.status;
    identity.operator.clone_from(&row.operator);
    identity.groups = groups(client, tenant, &actor_key).await?;
    let credential = identity.credential.as_ref().map(|k| Credential {
        grants: k.grants.clone(),
    });
    let effective = effective(
        client,
        tenant,
        &actor_key,
        &row,
        &identity.groups,
        credential,
    )
    .await?;
    Ok(Resolved {
        identity,
        effective,
    })
}

/// The permissions an actor's row, memberships and blocks give, narrowed by a credential.
async fn effective<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
    row: &ActorRow,
    groups: &BTreeSet<String>,
    credential: Option<Credential>,
) -> Result<Effective, AccountsError> {
    let groups_registry = crate::groups::registry_for(client, tenant, groups).await?;
    let all = groups_registry.permissions();
    let mut principal = Principal {
        kind: Some(row.kind),
        status: row.status,
        groups: groups.clone(),
        autoconfirmed: row.kind == ActorKind::Registered,
        blocked: blocked(client, tenant, actor_key, all).await?,
        credential,
        ..Principal::default()
    };
    if let Some(op) = &row.operator {
        principal.operator_blocked = blocked(client, tenant, op, all).await?;
    }
    Ok(evaluate(
        &principal,
        &groups_registry,
        GrantRegistry::default_registry(),
    ))
}

/// What an account may do now in its own right, with no credential narrowing it: what a
/// notification's read check asks of its recipient (16 §5.2). `None` for an actor with no
/// row on the tenant.
///
/// # Errors
///
/// A database failure.
pub async fn effective_of<C: GenericClient>(
    client: &C,
    tenant: &str,
    actor_key: &str,
) -> Result<Option<Effective>, AccountsError> {
    let Some(row) = actor(client, tenant, actor_key).await? else {
        return Ok(None);
    };
    let groups = groups(client, tenant, actor_key).await?;
    Ok(Some(
        effective(client, tenant, actor_key, &row, &groups, None).await?,
    ))
}
