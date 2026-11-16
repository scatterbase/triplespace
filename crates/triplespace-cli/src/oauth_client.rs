//! `triplespace oauth-client set|list|callback|remove` (0088 §3): the instance's own client
//! credentials for issuers that serve login through OAuth, in `private.oauth_client`.
//! No API route writes them and none returns a secret.
//!
//! A row is the instance default (no `--tenant`) or a tenant's own. The default serves a
//! tenant only when the callback it was registered with is the tenant's callback or a
//! prefix of it; `callback` prints the URL to register. The secret comes from a file or
//! `TRIPLESPACE_OAUTH_CLIENT_SECRET`, never the command line (0033 §12), and is sealed
//! under the instance sealing key, which derives from the key file: rotating that key
//! means setting the secrets again.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_actors::issuer::IssuerRegistry;
use tokio_postgres::Client;
use triplespace_accounts::{Secret, oauth};

use crate::accounts::secret_from;
use crate::common::{connect, farm_of, signing_key};

/// The `oauth-client` commands.
#[derive(Debug, Subcommand)]
pub enum OAuthClient {
    /// Store (or replace) a client ID and secret for an issuer.
    Set(Set),
    /// List the credentials and which one each tenant uses.
    List(Target),
    /// Print the callback URL to register with the provider.
    Callback(Which),
    /// Remove a credential.
    Remove(Which),
}

/// The database.
#[derive(Debug, Args)]
pub struct Target {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// An issuer, and a tenant or the instance default.
#[derive(Debug, Args)]
pub struct Which {
    /// The database.
    #[command(flatten)]
    pub target: Target,
    /// The issuer, such as `wikimedia-central`.
    pub issuer: String,
    /// A tenant's own credential; without it, the instance default.
    #[arg(long)]
    pub tenant: Option<String>,
}

/// `oauth-client set`.
#[derive(Debug, Args)]
pub struct Set {
    /// The issuer and the tenant.
    #[command(flatten)]
    pub which: Which,
    /// The client ID the provider issued.
    #[arg(long)]
    pub client_id: String,
    /// A file holding the client secret; else `TRIPLESPACE_OAUTH_CLIENT_SECRET`.
    #[arg(long)]
    pub secret_file: Option<PathBuf>,
    /// The callback URL the consumer was registered with, where it is not this tenant's
    /// (or, for the default, the primary tenant's) own: a prefix registered as one.
    #[arg(long)]
    pub callback: Option<String>,
    /// The instance key file, from which the sealing key derives.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: PathBuf,
}

/// Runs an `oauth-client` command.
pub async fn run(cmd: OAuthClient) -> Result<()> {
    match cmd {
        OAuthClient::Set(args) => set(args).await,
        OAuthClient::List(args) => list(args).await,
        OAuthClient::Callback(args) => callback(args).await,
        OAuthClient::Remove(args) => remove(args).await,
    }
}

/// A tenant's base, or for the default the farm's.
async fn base_of(client: &Client, tenant: Option<&str>) -> Result<String> {
    match tenant {
        None => Ok(farm_of(client).await?.base),
        Some(slug) => client
            .query_opt(
                "SELECT config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
                &[&slug],
            )
            .await?
            .and_then(|r| r.get::<_, Option<String>>(0))
            .with_context(|| format!("no tenant `{slug}`")),
    }
}

fn oauth_issuer(code: &str) -> Result<&'static scatter_actors::issuer::Issuer> {
    let registry = IssuerRegistry::default_registry();
    match registry.by_code(code) {
        Some(i) if i.oauth.is_some() => Ok(i),
        _ => bail!(
            "`{code}` does not serve login through OAuth; the issuers that do: {}",
            registry
                .oauth_providers()
                .map(|i| i.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

async fn set(args: Set) -> Result<()> {
    oauth_issuer(&args.which.issuer)?;
    let secret_text = secret_from(
        args.secret_file.as_deref(),
        "TRIPLESPACE_OAUTH_CLIENT_SECRET",
    )?;
    let key = signing_key(&args.key_file)?;
    let secret = Secret::derive(&key.to_bytes());
    let client = connect(&args.which.target.database).await?;
    let base = base_of(&client, args.which.tenant.as_deref()).await?;
    let callback = args
        .callback
        .unwrap_or_else(|| oauth::callback_url(&base, &args.which.issuer));
    let tenant = args.which.tenant.as_deref().unwrap_or("");
    oauth::set_client(
        &client,
        &secret,
        IssuerRegistry::default_registry(),
        tenant,
        &args.which.issuer,
        &args.client_id,
        secret_text.trim(),
        &callback,
    )
    .await?;
    println!(
        "{} for {}: client {} registered for {callback}",
        args.which.issuer,
        if tenant.is_empty() {
            "the instance default"
        } else {
            tenant
        },
        args.client_id
    );
    Ok(())
}

async fn list(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    let rows = oauth::list_clients(&client).await?;
    if rows.is_empty() {
        println!("no client credentials; `triplespace oauth-client set` stores one");
    }
    for r in &rows {
        let whose = if r.tenant.is_empty() {
            "(instance default)".to_string()
        } else {
            r.tenant.clone()
        };
        println!("{whose}\t{}\t{}\t{}", r.issuer, r.client_id, r.callback);
    }
    let tenants = client
        .query(
            "SELECT code, config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' ORDER BY code",
            &[],
        )
        .await?;
    for t in &tenants {
        let (slug, base): (String, Option<String>) = (t.get(0), t.get(1));
        let Some(base) = base else { continue };
        for i in IssuerRegistry::default_registry().oauth_providers() {
            let used = oauth::resolve_row(&client, &slug, &base, &i.code).await?;
            println!(
                "tenant {slug} uses for {}: {}",
                i.code,
                match used {
                    Some(r) if r.tenant.is_empty() => "the instance default".to_string(),
                    Some(_) => "its own".to_string(),
                    None => format!("none (register {})", oauth::callback_url(&base, &i.code)),
                }
            );
        }
    }
    Ok(())
}

async fn callback(args: Which) -> Result<()> {
    let issuer = oauth_issuer(&args.issuer)?;
    let client = connect(&args.target.database).await?;
    let base = base_of(&client, args.tenant.as_deref()).await?;
    println!("{}", oauth::callback_url(&base, &args.issuer));
    if let Some(register) = issuer.oauth.as_ref().and_then(|o| o.register.as_deref()) {
        eprintln!(
            "Register an OAuth 2.0 confidential client for identity only (no real name or email) at {register}"
        );
    }
    Ok(())
}

async fn remove(args: Which) -> Result<()> {
    let client = connect(&args.target.database).await?;
    let tenant = args.tenant.as_deref().unwrap_or("");
    if oauth::remove_client(&client, tenant, &args.issuer).await? {
        println!("removed");
        Ok(())
    } else {
        bail!("no credential for `{}` there", args.issuer)
    }
}
