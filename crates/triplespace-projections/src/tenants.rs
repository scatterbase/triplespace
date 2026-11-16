//! The instance's tenants as the registry holds them (0018 §1, §3; 0046 §2, §6; 0079):
//! the `farm` record, each `tenant:` record and the `primary` record of the instance
//! `config`, read from `view.registry`.
//!
//! A tenant's **slug** names it on this instance and keys its rows in `view` and `log`;
//! its **issuer code**, derived from its founding record (0079 §1), is what its actor keys
//! carry. The `tenant:` record holds both, so that reading an actor key back to its
//! tenant, or minting one for it, never derives anything at request time.

use tokio_postgres::GenericClient;

use crate::actors::Farm;

/// A registered tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantEntry {
    /// The slug: the tenant's name on this instance.
    pub slug: String,
    /// The base URI, `https://librarybase.org`.
    pub base: String,
    /// The issuer code its actor keys carry (0079 §1).
    pub issuer: String,
    /// The source wiki it adopted, if any (0035).
    pub adopted_from: Option<String>,
}

impl TenantEntry {
    fn from_config(slug: String, config: &serde_json::Value) -> Option<Self> {
        let text = |k: &str| {
            config
                .get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        Some(Self {
            slug,
            base: text("base")?,
            issuer: text("issuer")?,
            adopted_from: text("adopted_from"),
        })
    }
}

/// Every registered tenant, by slug. A `tenant:` record without a base or an issuer code
/// is not a tenant this version can serve, and is left out.
pub async fn tenants<C: GenericClient>(
    client: &C,
) -> Result<Vec<TenantEntry>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT code, config FROM view.registry WHERE tenant = '' AND kind = 'tenant' ORDER BY code",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| TenantEntry::from_config(r.get(0), &r.get::<_, serde_json::Value>(1)))
        .collect())
}

/// One tenant by slug.
pub async fn tenant<C: GenericClient>(
    client: &C,
    slug: &str,
) -> Result<Option<TenantEntry>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT config FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&slug],
        )
        .await?;
    Ok(row.and_then(|r| TenantEntry::from_config(slug.to_string(), &r.get(0))))
}

/// The tenant whose actor keys carry `issuer`, if it is one of this instance's.
pub async fn by_issuer<C: GenericClient>(
    client: &C,
    issuer: &str,
) -> Result<Option<TenantEntry>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT code, config FROM view.registry
             WHERE tenant = '' AND kind = 'tenant' AND config->>'issuer' = $1",
            &[&issuer],
        )
        .await?;
    Ok(row.and_then(|r| TenantEntry::from_config(r.get(0), &r.get(1))))
}

/// A tenant's issuer code, by slug.
pub async fn issuer_of<C: GenericClient>(
    client: &C,
    slug: &str,
) -> Result<Option<String>, tokio_postgres::Error> {
    Ok(tenant(client, slug).await?.map(|t| t.issuer))
}

/// The slug of the primary tenant (0046 §2): the `primary` record's.
pub async fn primary<C: GenericClient>(
    client: &C,
) -> Result<Option<String>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT config->>'slug' FROM view.registry WHERE tenant = '' AND kind = 'primary' AND code = 'primary'",
            &[],
        )
        .await?;
    Ok(row.and_then(|r| r.get(0)))
}

/// The farm (0046 §6; 0079 §4): the instance `config`'s `farm` record.
pub async fn farm<C: GenericClient>(client: &C) -> Result<Option<Farm>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT config FROM view.registry WHERE tenant = '' AND kind = 'farm' AND code = 'farm'",
            &[],
        )
        .await?;
    Ok(row.and_then(|r| {
        let c: serde_json::Value = r.get(0);
        let text = |k: &str| {
            c.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        Some(Farm {
            slug: text("slug")?,
            code: text("code")?,
            base: text("base")?,
        })
    }))
}

/// The tenancy policy's switches (0028 §1): each `tenancy:{switch}` record's value.
pub async fn tenancy<C: GenericClient>(
    client: &C,
) -> Result<std::collections::BTreeMap<String, String>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT code, config->>'value' FROM view.registry WHERE tenant = '' AND kind = 'tenancy' ORDER BY code",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| Some((r.get::<_, String>(0), r.get::<_, Option<String>>(1)?)))
        .collect())
}
