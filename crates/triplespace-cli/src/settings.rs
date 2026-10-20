//! `triplespace settings get|set|unset` (0085 §3): the operator's writer of settings,
//! through the same validator as `PATCH /settings`, so the command line cannot write a
//! record the API would refuse.
//!
//! - `get [KEY…]` prints each setting's resolved value and origin, a tenant's or with
//!   `--instance` the instance's own.
//! - `set KEY VALUE` sets a tenant's value; `--instance` the instance's own setting;
//!   `--default` the instance's default for a tenant setting, and `--lock` locks it too;
//!   `--tenancy` a tenancy switch.
//! - `unset KEY` returns a tenant's setting to inheriting; `--instance` removes the
//!   instance's value; `--default` removes the instance's default; `--lock` unlocks.
//!
//! A VALUE is read as JSON where it parses (`true`, `20`, `["en","de"]`) and as a string
//! otherwise (`P30D`, `Librarybase`). The operator holds every right here: the command
//! has the database.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde_json::Value;
use triplespace_accounts::settings::plan::Target;
use triplespace_accounts::settings::{Patch, Scope, load};
use triplespace_api_ingest::settings::{SaveError, save};

use crate::common::{connect, farm_of, now, store};

/// The settings commands.
#[derive(Debug, Subcommand)]
pub enum Settings {
    /// Print resolved settings with their origins.
    Get(Get),
    /// Set a value, a default, a lock or a tenancy switch.
    Set(Set),
    /// Return a setting to inheriting, or remove a default or a lock.
    Unset(Unset),
}

/// Where a command acts.
#[derive(Debug, Args)]
pub struct Where {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant; the first registered one by default.
    #[arg(long)]
    pub tenant: Option<String>,
    /// The instance's own settings rather than a tenant's.
    #[arg(long)]
    pub instance: bool,
}

/// `settings get`.
#[derive(Debug, Args)]
pub struct Get {
    /// Where.
    #[command(flatten)]
    pub at: Where,
    /// Only these keys.
    pub keys: Vec<String>,
    /// Only this group (a tab of the settings page).
    #[arg(long)]
    pub group: Option<String>,
    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

/// `settings set`.
#[derive(Debug, Args)]
pub struct Set {
    /// Where.
    #[command(flatten)]
    pub at: Where,
    /// The setting (or, with `--tenancy`, the switch).
    pub key: String,
    /// The value: JSON where it parses, else a string.
    pub value: String,
    /// The instance's default for this tenant setting.
    #[arg(long)]
    pub default: bool,
    /// Also lock it (implies `--default`).
    #[arg(long)]
    pub lock: bool,
    /// A tenancy switch.
    #[arg(long, conflicts_with_all = ["default", "lock"])]
    pub tenancy: bool,
    /// The summary.
    #[arg(long)]
    pub summary: Option<String>,
}

/// `settings unset`.
#[derive(Debug, Args)]
pub struct Unset {
    /// Where.
    #[command(flatten)]
    pub at: Where,
    /// The setting.
    pub key: String,
    /// Remove the instance's default for it.
    #[arg(long)]
    pub default: bool,
    /// Unlock it.
    #[arg(long)]
    pub lock: bool,
    /// The summary.
    #[arg(long)]
    pub summary: Option<String>,
}

/// A command-line value: JSON where it parses, else the text.
#[must_use]
pub fn parse_value(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
}

async fn tenant_slug(client: &tokio_postgres::Client, given: Option<&str>) -> Result<String> {
    if let Some(t) = given {
        return Ok(t.to_string());
    }
    let row = client
        .query_opt(
            "SELECT code FROM view.registry WHERE tenant = '' AND kind = 'tenant' ORDER BY code LIMIT 1",
            &[],
        )
        .await?
        .context("no tenant is registered")?;
    Ok(row.get(0))
}

/// Runs a settings command.
pub async fn run(command: Settings) -> Result<()> {
    match command {
        Settings::Get(g) => get(g).await,
        Settings::Set(s) => {
            let instance = s.at.instance || s.default || s.lock || s.tenancy;
            let value = parse_value(&s.value);
            let mut patch = Patch {
                summary: s.summary.clone(),
                ..Patch::default()
            };
            if s.tenancy {
                patch.tenancy.insert(s.key.clone(), value);
            } else if s.default || s.lock {
                patch.defaults.insert(s.key.clone(), Some(value));
                if s.lock {
                    patch.locks.insert(s.key.clone(), true);
                }
            } else {
                patch.set.insert(s.key.clone(), value);
            }
            write(&s.at, instance, &patch).await
        }
        Settings::Unset(u) => {
            let instance = u.at.instance || u.default || u.lock;
            let mut patch = Patch {
                summary: u.summary.clone(),
                ..Patch::default()
            };
            if u.lock {
                patch.locks.insert(u.key.clone(), false);
            }
            if u.default {
                patch.defaults.insert(u.key.clone(), None);
            }
            if !u.lock && !u.default {
                patch.unset.push(u.key.clone());
            }
            write(&u.at, instance, &patch).await
        }
    }
}

async fn write(at: &Where, instance: bool, patch: &Patch) -> Result<()> {
    let client = connect(&at.database).await?;
    let farm = farm_of(&client).await?;
    let slug = tenant_slug(&client, at.tenant.as_deref()).await?;
    let (store, pipeline) = store(&at.database, &farm)?;
    let actor = farm.operator().to_string();
    let target = if instance {
        Target::Instance
    } else {
        Target::Tenant
    };
    let all = |_: &str| true;
    match save(
        &store,
        &pipeline,
        Some(&slug),
        target,
        patch,
        &actor,
        &all,
        now(),
    )
    .await
    {
        Ok(saved) => {
            if saved.changes.is_empty() {
                println!("no change");
            }
            for c in &saved.changes {
                println!("{:<9} {}  {} → {}", c.scope, c.key, c.old, c.new);
            }
            Ok(())
        }
        Err(SaveError::Refused(refusals)) => {
            for r in &refusals {
                eprintln!("{}: {}", r.code, r.message);
            }
            bail!("refused; nothing was written")
        }
        Err(e) => Err(e.into()),
    }
}

async fn get(g: Get) -> Result<()> {
    let client = connect(&g.at.database).await?;
    let slug = tenant_slug(&client, g.at.tenant.as_deref()).await?;
    let (snap, _) = load(&client, Some(&slug)).await?;
    let scope = if g.at.instance {
        Scope::Instance
    } else {
        Scope::Tenant
    };
    let rows: Vec<_> = snap
        .resolve_all(scope)
        .into_iter()
        .filter(|r| g.keys.is_empty() || g.keys.contains(&r.key))
        .filter(|r| {
            g.group
                .as_ref()
                .is_none_or(|grp| snap.registry().get(&r.key).is_some_and(|d| &d.group == grp))
        })
        .collect();
    for key in &g.keys {
        if snap.registry().get(key).is_none() {
            bail!("`{key}` is not a registered setting");
        }
    }
    if g.json {
        let out: BTreeMap<&str, &_> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    for r in &rows {
        let value = r
            .value
            .as_ref()
            .map_or_else(|| "—".to_string(), Value::to_string);
        let mut origin = serde_json::to_value(r.origin)?
            .as_str()
            .unwrap_or("")
            .to_string();
        if let Some(c) = &r.capped {
            origin = format!("{origin}, capped at {c}");
        }
        println!("{:<34} {value}  ({origin})", r.key);
    }
    Ok(())
}
