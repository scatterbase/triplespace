//! `triplespace logins migrate` (0089 §15; 07 §2.8): turns every credential an instance
//! holds against a tenant account into an instance login, idempotently.
//!
//! 1. The instance's `logins` partition is opened if it does not exist, as an instance act
//!    attested with the instance key, and the config record projected.
//! 2. Every password still keyed by an actor key (`private.password.actor_key`) moves to
//!    the account's login: the login is made if the account has none, named after the
//!    account (`{name} ({tenant})` where that is held), and the account bound to it.
//! 3. Every external identity still keyed by a tenant account becomes an identity of the
//!    account's login: accounts sharing an identity form one login; the binding record is
//!    appended again in `logins` and the old one erased with reason class `privacy`, as an
//!    instance act. An account that already has a login of its own while its identity
//!    belongs to another is left as it is and reported (0089 Q5).
//! 4. The password binding records accounts held before 0089 §3 are erased likewise.
//!
//! `triplespace migrate` says when this needs running; `instance create` makes logins
//! from the start. This implements 0089 §15 (A3).
//!
//! `triplespace logins check` (0101 §2) reports what enacting `login.external_required`
//! waits on: whether it is on, the issuers the auth host offers, the logins holding no
//! external identity by name, which only the operator may see, and the credentials still
//! keyed by tenant accounts.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{GraphRegistry, PAYLOAD_CONFIG};
use scatter_log_postgres::log;
use scatter_projection::Backend as _;
use triplespace_accounts::{bindings, logins, password};
use triplespace_api_ingest::accounts::{bind_account, bind_external, create_login, erase_binding};

use crate::common::{connect, draft_attested, farm_of, now, signing_key, store};
use crate::tenant::{instance_founding, open_partition, project, provision};

/// `logins …`.
#[derive(Debug, Subcommand)]
pub enum LoginsCommand {
    /// Turn every tenant-account credential into an instance login (0089 §15).
    Migrate(Migrate),
    /// Report what requiring an external sign-in method waits on (0101 §2).
    Check(Check),
}

/// `logins check`.
#[derive(Debug, Args)]
pub struct Check {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// `logins migrate`.
#[derive(Debug, Args)]
pub struct Migrate {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The instance key file, for the records this appends as instance acts.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: PathBuf,
}

/// Runs `logins …`.
pub async fn run(command: LoginsCommand) -> Result<()> {
    match command {
        LoginsCommand::Migrate(args) => migrate(args).await,
        LoginsCommand::Check(args) => check(args).await,
    }
}

/// Runs `logins check`: prints the requirement's state and what enacting it waits on.
/// This implements 0101 §2.
pub async fn check(args: Check) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let required = logins::external_required(&client).await?;
    let ready = triplespace_api_ingest::accounts::readiness(
        &client,
        scatter_actors::issuer::IssuerRegistry::default_registry(),
        &farm.base,
    )
    .await?;
    println!(
        "login.external_required is {}",
        if required { "on" } else { "off" }
    );
    println!(
        "external issuers the auth host offers: {}",
        if ready.offered.is_empty() {
            "none".to_string()
        } else {
            ready.offered.join(", ")
        }
    );
    println!(
        "logins without an external identity: {}",
        ready.without_external.len()
    );
    for name in &ready.without_external {
        println!("  {name}");
    }
    if ready.unmigrated_hashes + ready.unmigrated_identities > 0 {
        println!(
            "credentials still keyed by tenant accounts: {} passwords, {} external identities (run `logins migrate`)",
            ready.unmigrated_hashes, ready.unmigrated_identities
        );
    }
    let blockers = ready.blockers();
    if blockers.is_empty() {
        println!("the requirement can be enacted");
    } else {
        println!("enacting the requirement waits on:");
        for b in &blockers {
            println!("  {b}");
        }
    }
    Ok(())
}

/// What a run did.
#[derive(Debug, Default)]
pub struct Done {
    /// Whether the `logins` partition was opened.
    pub partition_opened: bool,
    /// Logins made.
    pub logins: usize,
    /// Password hashes moved to their login.
    pub hashes_moved: usize,
    /// External identities moved to their login.
    pub identities: usize,
    /// Old password binding records erased.
    pub erased: usize,
    /// Accounts whose identity belongs to another login, left as they were.
    pub skipped: Vec<String>,
}

/// Whether the instance still has credentials keyed by tenant accounts, for `migrate`'s
/// note.
pub async fn pending<C: tokio_postgres::GenericClient>(client: &C) -> Result<bool> {
    let passwords = password::unmigrated(client).await?;
    let identities = bindings::unmigrated(client).await?;
    let partition = client
        .query_opt(
            "SELECT 1 FROM log.partition WHERE tenant IS NULL AND name = 'logins'",
            &[],
        )
        .await?;
    Ok(!passwords.is_empty() || !identities.is_empty() || partition.is_none())
}

/// Runs the migration (the module docs).
pub async fn migrate(args: Migrate) -> Result<()> {
    let done = run_migration(&args.database, &args.key_file).await?;
    if done.partition_opened {
        println!("opened the instance's logins partition");
    }
    println!(
        "{} logins made, {} hashes and {} identities moved, {} old records erased",
        done.logins, done.hashes_moved, done.identities, done.erased
    );
    for s in &done.skipped {
        println!("skipped: {s}");
    }
    Ok(())
}

/// The tenant and name of an account.
async fn account_name<C: tokio_postgres::GenericClient>(
    client: &C,
    actor: &str,
) -> Result<Option<(String, Option<String>)>> {
    let row = client
        .query_opt(
            "SELECT tenant, name FROM view.actor WHERE actor_key = $1 ORDER BY tenant LIMIT 1",
            &[&actor],
        )
        .await?;
    Ok(row.map(|r| (r.get::<_, String>(0), r.get::<_, Option<String>>(1))))
}

/// The migration, for the command and for tests.
#[allow(clippy::too_many_lines)]
pub async fn run_migration(database: &str, key_file: &std::path::Path) -> Result<Done> {
    let mut done = Done::default();
    let client = connect(database).await?;
    let farm = farm_of(&client).await?;
    let t = now();

    // 1. The partition.
    let exists = client
        .query_opt(
            "SELECT 1 FROM log.partition WHERE tenant IS NULL AND name = 'logins'",
            &[],
        )
        .await?
        .is_some();
    if exists {
        drop(client);
    } else {
        let key = signing_key(key_file)?;
        let founding = instance_founding(&client).await?;
        let provide = provision(&key, &farm, &founding);
        let (_, entry) = open_partition(
            &client,
            &GraphRegistry::embedded(),
            "logins",
            None,
            &farm.base,
        )
        .await?;
        let d = draft_attested(
            PAYLOAD_CONFIG,
            &entry.key(),
            &serde_json::to_value(&entry)?,
            Some("logins migrate"),
            t,
            &provide,
        )?;
        log::append(&client, CONFIG_PARTITION, d).await?;
        project(database, &farm, client, &[CONFIG_PARTITION]).await?;
        done.partition_opened = true;
    }

    // 2–4. The credentials, in one transaction.
    let (store, pipeline) = store(database, &farm)?;
    let operator = farm.operator().to_string();
    let mut cx = store.begin().await?;
    for (actor_key, _) in password::unmigrated(cx.conn()).await? {
        let Some((tenant, name)) = account_name(cx.conn(), &actor_key).await? else {
            done.skipped.push(format!("{actor_key}: no account row"));
            continue;
        };
        let login = if let Some(l) = logins::login_of(cx.conn(), &tenant, &actor_key).await? {
            l
        } else {
            let l = create_login(
                &store,
                &pipeline,
                &mut cx,
                &tenant,
                name.as_deref().unwrap_or(&actor_key),
                &operator,
                t,
            )
            .await?;
            bind_account(
                &store, &pipeline, &mut cx, &tenant, &actor_key, l, &operator, t,
            )
            .await?;
            done.logins += 1;
            l
        };
        password::migrate_row(cx.conn(), &actor_key, login).await?;
        done.hashes_moved += 1;
    }

    for (tenant, b) in bindings::unmigrated(cx.conn()).await? {
        let actor_key = b.holder.clone();
        let identity_login = bindings::find(cx.conn(), "", &b.issuer, &b.subject)
            .await?
            .and_then(|h| logins::id_of(&h));
        let account_login = logins::login_of(cx.conn(), &tenant, &actor_key).await?;
        let login = match (identity_login, account_login) {
            (Some(l), None) => {
                bind_account(
                    &store, &pipeline, &mut cx, &tenant, &actor_key, l, &operator, t,
                )
                .await?;
                l
            }
            (None, Some(l)) => {
                bind_external(
                    &store, &pipeline, &mut cx, l, &b.issuer, &b.subject, &operator, t,
                )
                .await?;
                l
            }
            (None, None) => {
                let name = account_name(cx.conn(), &actor_key)
                    .await?
                    .and_then(|(_, n)| n)
                    .unwrap_or_else(|| actor_key.clone());
                let l =
                    create_login(&store, &pipeline, &mut cx, &tenant, &name, &operator, t).await?;
                bind_external(
                    &store, &pipeline, &mut cx, l, &b.issuer, &b.subject, &operator, t,
                )
                .await?;
                bind_account(
                    &store, &pipeline, &mut cx, &tenant, &actor_key, l, &operator, t,
                )
                .await?;
                done.logins += 1;
                l
            }
            (Some(l1), Some(l2)) if l1 == l2 => l1,
            (Some(l1), Some(l2)) => {
                done.skipped.push(format!(
                    "{actor_key}: its identity {}:{} belongs to login {l1}, the account to login {l2} (0089 Q5)",
                    b.issuer, b.subject
                ));
                continue;
            }
        };
        let _ = login;
        erase_binding(
            &store,
            &pipeline,
            &mut cx,
            &b,
            &operator,
            "logins migrate",
            t,
        )
        .await?;
        bindings::delete_row(cx.conn(), &tenant, &b.issuer, &b.subject).await?;
        done.identities += 1;
    }

    // 4. The password binding records accounts held before 0089 §3.
    let accounts: Vec<(i64, String)> = cx
        .conn()
        .query(
            "SELECT partition, tenant FROM log.partition WHERE name = 'accounts' AND tenant IS NOT NULL",
            &[],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    for (partition_db, _tenant) in accounts {
        #[allow(clippy::cast_sign_loss)]
        let partition = partition_db as u64;
        let mut from = 0u64;
        loop {
            let batch = log::scan(cx.conn(), partition, from, 500).await?;
            let Some((last, _)) = batch.last() else { break };
            let next = last + 1;
            for (offset, record) in batch {
                let h = record.header();
                if h.payload_type != password::PAYLOAD_BINDING {
                    continue;
                }
                let body = record.body();
                if body.content().is_erased() {
                    continue;
                }
                let is_password = body
                    .content()
                    .value()
                    .ok()
                    .flatten()
                    .and_then(|v| v.get("issuer").and_then(|i| i.as_text().map(str::to_owned)))
                    .is_some_and(|i| i == scatter_actors::issuer::PASSWORD);
                if !is_password {
                    continue;
                }
                let b = bindings::Binding {
                    issuer: scatter_actors::issuer::PASSWORD.to_string(),
                    subject: String::new(),
                    holder: h.key.clone().unwrap_or_default(),
                    partition,
                    offset,
                    added: std::time::SystemTime::UNIX_EPOCH,
                    last_used: None,
                };
                erase_binding(
                    &store,
                    &pipeline,
                    &mut cx,
                    &b,
                    &operator,
                    "logins migrate",
                    t,
                )
                .await?;
                done.erased += 1;
            }
            from = next;
        }
    }
    store.commit(cx).await.context("commit")?;
    Ok(done)
}
