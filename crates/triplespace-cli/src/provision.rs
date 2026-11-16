//! Tenant requests and the provisioner (0111 §2–§6; 08 §1.4).
//!
//! **A request** is a `tenant/request` log event in the instance `log`, attested by the
//! operator who filed it, an actor of the primary tenant holding `ts-createtenant`. It is
//! keyed `tenant:{slug}`. Its `params` hold only the slug and the base, which the activity
//! projection serves with the instance `log`'s other public events; the wiki's name, the
//! owner and the owner login source are under `request`, which nothing projects (0111 A1).
//!
//! **The provisioner** finds the requests that are neither carried out nor refused, checks
//! each (0111 §4), and carries it out: the tenant is created as `tenant create` always
//! created one, except that its `tenant:` record is attested by the instance with the
//! request as authority and names the request's offset, which is how a request already
//! carried out is recognised. A refused request gets a `tenant/refuse` log event under the
//! same key, attested by the instance with the request as authority, whose target is the
//! request and whose `params.reason` is the refusal's code. When the request names an
//! owner login source, the new owner account is bound to that account's login (0111 §5),
//! through the accounts service's connection, which may write `private`.
//!
//! `triplespace provision` runs it: a pass over the pending requests whenever a
//! `NOTIFY ts_tenant_request` arrives, and at least once a minute, under an advisory lock so
//! that two provisioners never work at once (0111 §3); `--once` makes one pass and exits.
//! `tenant create` files a request and carries it out at once in the same process (0111 §6).
//!
//! This implements 0111 §1; it implements 0111 §2 (A1); with the role and the two functions
//! of `triplespace-db`'s `0041_provision` it implements 0111 §3 (A1); and it
//! implements 0111 §4 (A1). Of 0111 §5 it builds the binding to a named login; the
//! invitation is `triplespace-api-rest`'s.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;
use ed25519_dalek::SigningKey;
use scatter_actors::evaluate::instance_rights;
use scatter_actors::key::ActorKey;
use scatter_integrity::attest::Authority;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log_postgres::{log, sequences};
use scatter_projection::Backend as _;
use tokio_postgres::Client;
use triplespace_api_ingest::tenant_requests;
use triplespace_projections::Farm;
use triplespace_projections::common::PAYLOAD_LOGEVENT;
use triplespace_projections::tenants;

use crate::common::{connect, draft_attested, farm_of, now, store};
use crate::tenant::{
    Attester, Created, Spec, check_base, check_free, create, current_key, host_of, project,
    provision, transaction,
};

pub use triplespace_api_ingest::tenant_requests::{Ask, CHANNEL, RIGHT};

/// The advisory lock a provisioner holds while it works (0111 §3): the first eight bytes of
/// `ts_tenant_request` as a number.
const LOCK: i64 = 0x7473_5f74_656e_616e;

/// How long the provisioner waits for a notification before it looks anyway.
const IDLE: Duration = Duration::from_mins(1);

/// A request in the instance `log`.
#[derive(Debug, Clone)]
pub struct Request {
    /// Where it is: the instance `log`, its offset and leaf.
    pub authority: Authority,
    /// The operator who filed it, absent if erased.
    pub actor: Option<String>,
    /// What it asks for.
    pub ask: Ask,
}

/// What became of a request.
#[derive(Debug)]
pub enum Outcome {
    /// The tenant was created now.
    Created(Box<Created>),
    /// The request was refused now: the code, and why.
    Refused(String, String),
    /// It had been carried out or refused already.
    Done,
}

/// What carrying out a request needs.
#[derive(Debug, Clone)]
pub struct Provisioner {
    /// The URL of the provisioner's own login (`ts_provision`).
    pub database: String,
    /// The URL of the accounts service's login, for the owner's binding.
    pub accounts_database: String,
    /// The instance key.
    pub key: SigningKey,
    /// The auth base, `server.auth_base`.
    pub auth_base: Option<String>,
}

/// The instance `log` partition.
pub async fn instance_log(client: &Client) -> Result<u64> {
    let row = client
        .query_opt(
            "SELECT partition FROM log.partition WHERE tenant IS NULL AND name = 'log'",
            &[],
        )
        .await?
        .context("the instance has no `log` partition")?;
    Ok(scatter_log_postgres::ids::partition_from_db(row.get(0)))
}

/// Files a request as `operator`, announces it, and returns where it is. The caller has
/// checked nothing: the provisioner checks everything (0111 §4).
pub async fn file(
    database: &str,
    operator: &str,
    ask: &Ask,
    reason: Option<&str>,
) -> Result<Authority> {
    let client = connect(database).await?;
    let farm = farm_of(&client).await?;
    let (store, pipeline) = store(database, &farm)?;
    let mut cx = store.begin().await?;
    let filed =
        tenant_requests::file(&store, &pipeline, &mut cx, ask, operator, reason, now()).await?;
    store.commit(cx).await?;
    client.batch_execute(&format!("NOTIFY {CHANNEL}")).await?;
    Ok(Authority {
        partition: filed.partition,
        offset: filed.offset,
        leaf: filed.leaf,
    })
}

/// The requests that are neither carried out nor refused, oldest first.
pub async fn pending(client: &Client) -> Result<Vec<Request>> {
    Ok(tenant_requests::list(client)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .into_iter()
        .filter(|e| e.state == tenant_requests::State::Pending)
        .map(|e| Request {
            authority: Authority {
                partition: e.filed.partition,
                offset: e.filed.offset,
                leaf: e.filed.leaf,
            },
            actor: e.actor,
            ask: e.ask,
        })
        .collect())
}

async fn created_for(client: &Client, request: &Request) -> Result<bool> {
    tenant_requests::created_for(client, &request.ask.slug, request.authority.offset)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// A refusal: its code and why.
struct Refusal(&'static str, String);

impl Provisioner {
    /// Carries out every pending request, under the advisory lock; returns each request's
    /// slug with what became of it. Does nothing if another provisioner holds the lock.
    pub async fn pass(&self) -> Result<Vec<(String, Outcome)>> {
        let client = connect(&self.database).await?;
        let locked: bool = client
            .query_one("SELECT pg_try_advisory_lock($1)", &[&LOCK])
            .await?
            .get(0);
        if !locked {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let result = async {
            for request in pending(&client).await? {
                let outcome = self.carry_out(&client, &request).await?;
                out.push((request.ask.slug.clone(), outcome));
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        client
            .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
            .await?;
        result?;
        Ok(out)
    }

    /// Checks a request and carries it out, or refuses it (0111 §4).
    pub async fn carry_out(&self, client: &Client, request: &Request) -> Result<Outcome> {
        if created_for(client, request).await? {
            return Ok(Outcome::Done);
        }
        let farm = farm_of(client).await?;
        match self.check(client, &farm, request).await? {
            Err(Refusal(code, why)) => {
                self.refuse(client, &farm, request, code, &why).await?;
                Ok(Outcome::Refused(code.into(), why))
            }
            Ok(spec) => {
                let t = now();
                let created = transaction(
                    client,
                    create(
                        client,
                        &self.key,
                        &farm,
                        &spec,
                        &Attester::Request(request.authority),
                        t,
                    ),
                )
                .await?;
                // Projected on the accounts service's connection: the pipeline ends in
                // addressing, which writes `private.inbox`, as the server's does on a login
                // that may (0013 §4).
                let projector = connect(&self.accounts_database).await?;
                project(
                    &self.accounts_database,
                    &farm,
                    projector,
                    &[CONFIG_PARTITION, created.config],
                )
                .await?;
                if let Some(source) = &request.ask.owner_login_source {
                    self.bind_owner(client, &farm, &created, source, t).await?;
                }
                Ok(Outcome::Created(Box::new(created)))
            }
        }
    }

    /// The checks of 0111 §4, in order; the spec to create on success.
    async fn check(
        &self,
        client: &Client,
        farm: &Farm,
        request: &Request,
    ) -> Result<std::result::Result<Spec, Refusal>> {
        // 1. The operator holds `ts-createtenant` on the primary tenant.
        if !self.entitled(client, request.actor.as_deref()).await? {
            return Ok(Err(Refusal(
                "ts-not-entitled",
                format!(
                    "{} does not hold `{RIGHT}` on the primary tenant",
                    request.actor.as_deref().unwrap_or("the filer")
                ),
            )));
        }
        // 2. The slug and the base, and the auth base a second tenant needs.
        let ask = &request.ask;
        if let Some(name) = &ask.name
            && let Err(e) = triplespace_accounts::settings::plan::check_site_name(name)
        {
            return Ok(Err(Refusal("ts-tenant-invalid", format!("the name: {e}"))));
        }
        let spec = Spec {
            slug: ask.slug.clone(),
            base: ask.base.clone(),
            name: ask.name.clone(),
            adopt: ask.adopt.clone(),
            owner: ask.owner_id,
            owner_name: ask.owner_name.clone(),
        };
        let auth_base = self.auth_base.as_deref().filter(|a| !a.is_empty());
        if let Err(e) = check_auth_base(client, farm, auth_base).await? {
            return Ok(Err(Refusal("ts-auth-base-required", e)));
        }
        if let Err(e) = check_free(client, &spec, farm, auth_base).await {
            return Ok(Err(Refusal("ts-tenant-invalid", format!("{e:#}"))));
        }
        // 3. The owner login source is an account with a login.
        if let Some(source) = &ask.owner_login_source
            && self.login_of_source(client, source).await?.is_none()
        {
            return Ok(Err(Refusal(
                "ts-owner-no-login",
                format!("{source} is no account on this instance with a login"),
            )));
        }
        Ok(Ok(spec))
    }

    /// Whether `actor` holds the right on the tenant that is primary now. A request is
    /// carried out as soon as it is seen, so now is its offset (0111 A1).
    async fn entitled(&self, client: &Client, actor: Option<&str>) -> Result<bool> {
        let Some(actor) = actor else {
            return Ok(false);
        };
        let Some(primary) = tenants::primary(client).await? else {
            return Ok(false);
        };
        let issuer = crate::common::issuer_of(client, &primary).await?;
        if ActorKey::parse(actor).ok().map(|k| k.issuer().to_string()) != Some(issuer) {
            return Ok(false);
        }
        let effective = triplespace_accounts::identity::effective_of(client, &primary, actor)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(instance_rights(effective.as_ref(), None).contains(RIGHT))
    }

    /// The login of the account `source` names, if it is an account on this instance and
    /// has one.
    async fn login_of_source(&self, client: &Client, source: &str) -> Result<Option<i64>> {
        let Ok(key) = ActorKey::parse(source) else {
            return Ok(None);
        };
        let Some(tenant) = tenants::by_issuer(client, key.issuer()).await? else {
            return Ok(None);
        };
        let accounts = connect(&self.accounts_database).await?;
        triplespace_accounts::logins::login_of(&accounts, &tenant.slug, source)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Binds the new owner account to the login of `source` (0111 §5).
    async fn bind_owner(
        &self,
        client: &Client,
        farm: &Farm,
        created: &Created,
        source: &str,
        t: u64,
    ) -> Result<()> {
        let login = self
            .login_of_source(client, source)
            .await?
            .with_context(|| format!("{source} has no login"))?;
        let (store, pipeline) = store(&self.accounts_database, farm)?;
        let mut cx = store.begin().await?;
        triplespace_api_ingest::accounts::bind_account(
            &store,
            &pipeline,
            &mut cx,
            &created.entry.slug,
            &created.owner.to_string(),
            login,
            &farm.operator().to_string(),
            t,
        )
        .await?;
        store.commit(cx).await?;
        Ok(())
    }

    /// Appends the refusal of `request`, attested by the instance with the request as
    /// authority.
    async fn refuse(
        &self,
        client: &Client,
        farm: &Farm,
        request: &Request,
        code: &str,
        why: &str,
    ) -> Result<()> {
        let t = now();
        let event = tenant_requests::refusal_event(
            &tenant_requests::Filed {
                partition: request.authority.partition,
                offset: request.authority.offset,
                leaf: request.authority.leaf,
            },
            code,
            t,
        );
        transaction(client, async {
            let mut d = draft_attested(
                PAYLOAD_LOGEVENT,
                &tenant_requests::key(&request.ask.slug),
                &event,
                Some(why),
                t,
                provision(&self.key, farm, &request.authority),
            )?;
            d.logid = Some(
                sequences::next(client, sequences::INSTANCE, &sequences::Sequence::Log).await?,
            );
            log::append(client, request.authority.partition, d).await?;
            Ok(())
        })
        .await
    }
}

/// Checks that a second tenant has an auth base, and that it is neither the farm base nor
/// a tenant's (0089 §1): `Err` with why not.
pub async fn check_auth_base(
    client: &Client,
    farm: &Farm,
    auth_base: Option<&str>,
) -> Result<std::result::Result<(), String>> {
    let existing = tenants::tenants(client).await?;
    if !existing.is_empty() && auth_base.is_none() {
        return Ok(Err(
            "ts-auth-base-required: an instance with more than one tenant needs an auth host; \
             set server.auth_base (--auth-base or TRIPLESPACE_AUTH_BASE) first (0089 §1)"
                .into(),
        ));
    }
    if let Some(a) = auth_base {
        let host = match check_base(a) {
            Ok(h) => h,
            Err(e) => return Ok(Err(format!("the auth base: {e:#}"))),
        };
        if host_of(&farm.base).as_deref() == Some(host.as_str())
            || existing
                .iter()
                .any(|t| host_of(&t.base).as_deref() == Some(host.as_str()))
        {
            return Ok(Err(format!(
                "the auth base `{a}` is the farm base or a tenant's base (0089 §1)"
            )));
        }
    }
    Ok(Ok(()))
}

/// `triplespace provision`.
#[derive(Debug, Args)]
pub struct Provision {
    /// The provisioner's Postgres URL: a login granted `ts_provision` (0111 §3).
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The accounts service's Postgres URL, for binding a new owner to a login; the one
    /// above by default.
    #[arg(long, env = "TRIPLESPACE_ACCOUNTS_DATABASE_URL")]
    pub accounts_database: Option<String>,
    /// The instance signing key, which must be the instance's current key.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    pub key_file: std::path::PathBuf,
    /// The auth host, `server.auth_base` (0089 §1): required for a second tenant.
    #[arg(long, env = "TRIPLESPACE_AUTH_BASE")]
    pub auth_base: Option<String>,
    /// Carry out what is pending, then exit.
    #[arg(long)]
    pub once: bool,
}

/// Prints what became of a request.
pub fn print_outcome(slug: &str, outcome: &Outcome) {
    match outcome {
        Outcome::Created(c) => println!(
            "created   {slug}  {}  issuer {}",
            c.entry.base, c.entry.issuer
        ),
        Outcome::Refused(code, why) => println!("refused   {slug}  {code}: {why}"),
        Outcome::Done => println!("done      {slug}"),
    }
}

/// Runs `triplespace provision`.
pub async fn run(args: Provision) -> Result<()> {
    let client = connect(&args.database).await?;
    let provisioner = Provisioner {
        accounts_database: args
            .accounts_database
            .clone()
            .unwrap_or_else(|| args.database.clone()),
        key: current_key(&client, &args.key_file).await?,
        auth_base: args.auth_base.clone(),
        database: args.database.clone(),
    };
    drop(client);
    if args.once {
        for (slug, outcome) in provisioner.pass().await? {
            print_outcome(&slug, &outcome);
        }
        return Ok(());
    }
    let wake = std::sync::Arc::new(tokio::sync::Notify::new());
    let listener = {
        let wake = wake.clone();
        let database = args.database.clone();
        tokio::spawn(async move { listen(&database, &wake).await })
    };
    eprintln!("provisioner: listening on {CHANNEL}");
    loop {
        for (slug, outcome) in provisioner.pass().await? {
            print_outcome(&slug, &outcome);
        }
        if listener.is_finished() {
            bail!("the LISTEN connection closed");
        }
        let _ = tokio::time::timeout(IDLE, wake.notified()).await;
    }
}

/// Holds a connection on `LISTEN ts_tenant_request` and wakes the provisioner on each
/// notification, as the server's delivery worker listens on `ts_delivery`.
async fn listen(database: &str, wake: &tokio::sync::Notify) -> Result<()> {
    let config: tokio_postgres::Config = database.parse().context("database URL")?;
    let (client, mut connection) = config.connect(tokio_postgres::NoTls).await?;
    let statement = format!("LISTEN {CHANNEL}");
    let listen = client.batch_execute(&statement);
    let drive = async {
        loop {
            match std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                None => return Ok::<(), anyhow::Error>(()),
                Some(Ok(tokio_postgres::AsyncMessage::Notification(_))) => wake.notify_one(),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
            }
        }
    };
    tokio::pin!(drive);
    tokio::select! {
        r = &mut drive => r,
        r = listen => {
            r?;
            drive.await
        }
    }
}
