//! The other tenant acts, filed and carried out as tenant requests are (0111 §10, A3;
//! 08 §1.4, §6.3, §6.6, §7.5):
//!
//! - **Freezing** a tenant read-only, as for a move, and **unfreezing** it. Every one of the
//!   tenant's record tables gets, or loses, a row-level trigger that refuses inserts
//!   (`log.set_frozen`), so nothing written by any path reaches a frozen tenant's log; the
//!   server answers its write modules with `readonly` before that.
//! - **Deleting** a tenant: frozen, with the status `deleted` and the time it is erased, the
//!   instance setting `tenants.delete_grace_days` on (90 days by default; 0028 A26), and
//!   **cancelling** a deletion before then. The primary tenant is neither frozen nor deleted
//!   (`ts-primary-tenant`, 08 §7.6).
//! - **Erasing** a deleted tenant whose grace period is over, on the provisioner's pass: a
//!   `tenant/erase` log event, the tenant's rows in `view`, `ops` and `private` deleted, its
//!   record tables, log bookkeeping and sequences dropped (`log.drop_tenant_storage`), and the
//!   status `erased`, which frees its slug and host.
//! - **Accepting the primary role** offered to a tenant: the acceptance, filed by a member of
//!   `owner` on the offered tenant, is checked against an open offer, and the new `primary`
//!   record is appended, attested by the instance with the acceptance as its authority.
//!
//! Each act's outcome is a record in the instance `config` naming the request's offset in
//! `request`: the tenant's next `tenant:` record, with its new `status`, or the `primary`
//! record; a request is done once one names it. This implements 0111 §10 (A3).

use anyhow::{Context, Result};
use scatter_actors::key::ActorKey;
use scatter_integrity::attest::Authority;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::PAYLOAD_CONFIG;
use scatter_log_postgres::{log, sequences};
use tokio_postgres::Client;
use triplespace_api_ingest::tenant_requests::{self, act};
use triplespace_projections::Farm;
use triplespace_projections::common::PAYLOAD_LOGEVENT;
use triplespace_projections::tenants;

use crate::common::{connect, draft_attested, now};
use crate::provision::{Outcome, Provisioner, Refusal, Request};
use crate::tenant::{project, provision, transaction};

/// `tenants.delete_grace_days` when the instance sets none (0028 A26).
pub const DEFAULT_GRACE_DAYS: u64 = 90;

const MICROS_PER_DAY: u64 = 86_400 * 1_000_000;

/// The tenant's partitions.
async fn partitions_of(client: &Client, slug: &str) -> Result<Vec<i64>> {
    Ok(client
        .query(
            "SELECT partition FROM log.partition WHERE tenant = $1",
            &[&slug],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect())
}

/// The tenant's `tenant:` record content, as the registry has it.
async fn tenant_config(client: &Client, slug: &str) -> Result<Option<serde_json::Value>> {
    Ok(client
        .query_opt(
            "SELECT config FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = $1",
            &[&slug],
        )
        .await?
        .map(|r| r.get(0)))
}

/// The instance's grace period for a deleted tenant, in days.
async fn grace_days(client: &Client) -> Result<u64> {
    let (settings, _) = triplespace_accounts::settings::load(client, None)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(settings
        .value("tenants.delete_grace_days")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_GRACE_DAYS))
}

/// Whether `actor` is a member of `owner` on `slug`, and one of its own accounts.
async fn owner_of_tenant(client: &Client, slug: &str, actor: &str) -> Result<bool> {
    let Some(t) = tenants::tenant(client, slug).await? else {
        return Ok(false);
    };
    if ActorKey::parse(actor).ok().map(|k| k.issuer().to_string()) != Some(t.issuer) {
        return Ok(false);
    }
    Ok(client
        .query_opt(
            "SELECT 1 FROM view.membership WHERE tenant = $1 AND \"group\" = 'owner' AND actor_key = $2",
            &[&slug, &actor],
        )
        .await?
        .is_some())
}

impl Provisioner {
    /// Checks a request for an act other than creation and carries it out, or refuses it.
    pub(crate) async fn carry_out_act(
        &self,
        client: &Client,
        farm: &Farm,
        request: &Request,
    ) -> Result<Outcome> {
        if let Err(Refusal(code, why)) = self.check_act(client, request).await? {
            self.refuse(client, farm, request, code, &why).await?;
            return Ok(Outcome::Refused(code.into(), why));
        }
        let ask = &request.ask;
        let t = now();
        transaction(client, async {
            match ask.act() {
                act::FREEZE => {
                    self.set_frozen(client, &ask.slug, true).await?;
                    self.set_status(client, farm, request, "frozen", None, t)
                        .await
                }
                act::UNFREEZE | act::UNDELETE => {
                    self.set_frozen(client, &ask.slug, false).await?;
                    self.set_status(client, farm, request, "active", None, t)
                        .await
                }
                act::DELETE => {
                    self.set_frozen(client, &ask.slug, true).await?;
                    let erase_after = t + grace_days(client).await? * MICROS_PER_DAY;
                    self.set_status(client, farm, request, "deleted", Some(erase_after), t)
                        .await
                }
                act::ACCEPT_PRIMARY => self.accept_primary(client, farm, request, t).await,
                other => anyhow::bail!("`{other}` is no act"),
            }
        })
        .await?;
        let projector = connect(&self.accounts_database).await?;
        project(
            &self.accounts_database,
            farm,
            projector,
            &[CONFIG_PARTITION],
        )
        .await?;
        Ok(Outcome::Carried(ask.act().to_string()))
    }

    /// The checks of an act (0111 §10; 08 §6.3, §6.6, §7.5, §7.6).
    async fn check_act(
        &self,
        client: &Client,
        request: &Request,
    ) -> Result<std::result::Result<(), Refusal>> {
        let ask = &request.ask;
        let actor = request.actor.as_deref();
        let primary = tenants::primary(client).await?;
        let Some(tenant) = tenants::tenant(client, &ask.slug).await? else {
            return Ok(Err(Refusal(
                "ts-tenant-invalid",
                format!("no tenant `{}`", ask.slug),
            )));
        };
        if ask.act() == act::ACCEPT_PRIMARY {
            if !matches!(actor, Some(a) if owner_of_tenant(client, &ask.slug, a).await?) {
                return Ok(Err(Refusal(
                    "ts-not-entitled",
                    format!(
                        "only a member of `owner` on `{}` accepts its offer",
                        ask.slug
                    ),
                )));
            }
            let open = tenant_requests::open_offers(client, now())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if !open
                .iter()
                .any(|o| Some(o.offset) == ask.offer && o.tenant == ask.slug)
            {
                return Ok(Err(Refusal(
                    "ts-no-offer",
                    format!("no open offer of the primary role names `{}`", ask.slug),
                )));
            }
            if primary.as_deref() == Some(ask.slug.as_str()) {
                return Ok(Err(Refusal(
                    "ts-tenant-state",
                    format!("`{}` is the primary tenant already", ask.slug),
                )));
            }
            return Ok(Ok(()));
        }
        if !self.entitled(client, actor).await? {
            return Ok(Err(Refusal(
                "ts-not-entitled",
                format!(
                    "{} does not hold `{}` on the primary tenant",
                    actor.unwrap_or("the filer"),
                    tenant_requests::RIGHT
                ),
            )));
        }
        if matches!(ask.act(), act::FREEZE | act::DELETE)
            && primary.as_deref() == Some(ask.slug.as_str())
        {
            return Ok(Err(Refusal(
                "ts-primary-tenant",
                format!(
                    "`{}` is the primary tenant; transfer the role first",
                    ask.slug
                ),
            )));
        }
        let status = tenant.status.as_str();
        let allowed = match ask.act() {
            act::FREEZE => status == "active",
            act::UNFREEZE => status == "frozen",
            act::DELETE => matches!(status, "active" | "frozen"),
            act::UNDELETE => status == "deleted" && tenant.erase_after.is_some_and(|e| e > now()),
            _ => false,
        };
        if !allowed {
            return Ok(Err(Refusal(
                "ts-tenant-state",
                format!(
                    "`{}` is {status}; it cannot be asked to {}",
                    ask.slug,
                    ask.act()
                ),
            )));
        }
        Ok(Ok(()))
    }

    /// Attaches or drops the refusing trigger on every record table of the tenant.
    async fn set_frozen(&self, client: &Client, slug: &str, frozen: bool) -> Result<()> {
        for p in partitions_of(client, slug).await? {
            client
                .execute(
                    &format!("SELECT {}($1, $2)", triplespace_db::grants::SET_FROZEN),
                    &[&p, &frozen],
                )
                .await?;
        }
        Ok(())
    }

    /// Appends the tenant's next `tenant:` record: what it held, with `status`, the request
    /// it carries out, and for a deleted tenant when it is erased.
    async fn set_status(
        &self,
        client: &Client,
        farm: &Farm,
        request: &Request,
        status: &str,
        erase_after: Option<u64>,
        t: u64,
    ) -> Result<()> {
        let slug = &request.ask.slug;
        let mut config = tenant_config(client, slug)
            .await?
            .with_context(|| format!("no `tenant:` record for `{slug}`"))?;
        config["status"] = serde_json::json!(status);
        config["request"] = serde_json::json!(request.authority.offset);
        match erase_after {
            Some(e) => config["erase_after"] = serde_json::json!(e),
            None => {
                if let Some(o) = config.as_object_mut() {
                    o.remove("erase_after");
                }
            }
        }
        let d = draft_attested(
            PAYLOAD_CONFIG,
            &tenant_requests::key(slug),
            &config,
            Some(&format!("tenant {}", request.ask.act())),
            t,
            provision(&self.key, farm, &request.authority),
        )?;
        log::append(client, CONFIG_PARTITION, d).await?;
        Ok(())
    }

    /// Appends the new `primary` record, naming the offer and the acceptance (08 §7.5).
    async fn accept_primary(
        &self,
        client: &Client,
        farm: &Farm,
        request: &Request,
        t: u64,
    ) -> Result<()> {
        let record = serde_json::json!({
            "kind": "primary", "slug": request.ask.slug,
            "offer": request.ask.offer, "request": request.authority.offset,
        });
        let d = draft_attested(
            PAYLOAD_CONFIG,
            "primary",
            &record,
            Some("primary transfer"),
            t,
            provision(&self.key, farm, &request.authority),
        )?;
        log::append(client, CONFIG_PARTITION, d).await?;
        Ok(())
    }

    /// Erases every deleted tenant whose grace period is over (08 §6.6), returning their
    /// slugs.
    pub(crate) async fn erase_due(&self, client: &Client, farm: &Farm) -> Result<Vec<String>> {
        let due: Vec<(String, u64)> = client
            .query(
                "SELECT code, (config->>'request')::bigint FROM view.registry
                 WHERE tenant = '' AND kind = 'tenant' AND config->>'status' = 'deleted'
                   AND (config->>'erase_after')::bigint <= $1",
                &[&i64::try_from(now()).unwrap_or(i64::MAX)],
            )
            .await?
            .iter()
            .map(|r| (r.get(0), u64::try_from(r.get::<_, i64>(1)).unwrap_or(0)))
            .collect();
        let mut erased = Vec::new();
        for (slug, request) in due {
            self.erase(client, farm, &slug, request).await?;
            erased.push(slug);
        }
        Ok(erased)
    }

    /// Erases one deleted tenant: the `tenant/erase` record, its rows, its storage, and the
    /// status `erased`.
    async fn erase(&self, client: &Client, farm: &Farm, slug: &str, request: u64) -> Result<()> {
        let instance_log = crate::provision::instance_log(client).await?;
        let leaf: Vec<u8> = client
            .query_one(
                "SELECT leaf FROM log.record WHERE partition = $1 AND \"offset\" = $2",
                &[
                    &scatter_log_postgres::ids::partition_to_db(instance_log),
                    &i64::try_from(request)?,
                ],
            )
            .await?
            .get(0);
        let authority = Authority {
            partition: instance_log,
            offset: request,
            leaf: leaf
                .try_into()
                .map_err(|_| anyhow::anyhow!("leaf is not 32 bytes"))?,
        };
        let partitions = partitions_of(client, slug).await?;
        let t = now();
        // The tenant's private rows, on the accounts service's connection.
        let accounts = connect(&self.accounts_database).await?;
        delete_rows(&accounts, &["private"], slug, &partitions).await?;
        transaction(client, async {
            let event = serde_json::json!({
                "type": "tenant", "action": "erase", "time": t,
                "target": {"kind": "record", "partition": instance_log, "offset": request},
                "params": {"slug": slug, "partitions": partitions.len()},
            });
            let mut d = draft_attested(
                PAYLOAD_LOGEVENT,
                &tenant_requests::key(slug),
                &event,
                Some("the grace period of a deleted tenant is over"),
                t,
                provision(&self.key, farm, &authority),
            )?;
            d.logid = Some(
                sequences::next(client, sequences::INSTANCE, &sequences::Sequence::Log).await?,
            );
            log::append(client, instance_log, d).await?;
            delete_rows(client, &["view", "ops"], slug, &partitions).await?;
            client
                .execute(
                    &format!("SELECT {}($1)", triplespace_db::grants::DROP_TENANT_STORAGE),
                    &[&slug],
                )
                .await?;
            let request = Request {
                authority,
                actor: None,
                ask: tenant_requests::Ask::on(act::DELETE, slug),
            };
            self.set_status(client, farm, &request, "erased", None, t)
                .await
        })
        .await?;
        let projector = connect(&self.accounts_database).await?;
        project(
            &self.accounts_database,
            farm,
            projector,
            &[CONFIG_PARTITION],
        )
        .await?;
        Ok(())
    }
}

/// Deletes the tenant's rows from every table in `schemas` with a `tenant` or a `partition`
/// column: by tenant, and by the tenant's partitions. Found from the catalogue, so a table
/// added later is covered.
async fn delete_rows(
    client: &Client,
    schemas: &[&str],
    slug: &str,
    partitions: &[i64],
) -> Result<()> {
    let schemas: Vec<String> = schemas.iter().map(|s| (*s).to_string()).collect();
    let tables = client
        .query(
            "SELECT c.table_schema, c.table_name,
                    bool_or(c.column_name = 'tenant'), bool_or(c.column_name = 'partition')
             FROM information_schema.columns c
             JOIN information_schema.tables t
               ON t.table_schema = c.table_schema AND t.table_name = c.table_name
             WHERE c.table_schema = ANY($1) AND t.table_type = 'BASE TABLE'
               AND c.column_name IN ('tenant', 'partition')
             GROUP BY 1, 2",
            &[&schemas],
        )
        .await?;
    for row in &tables {
        let schema: String = row.get(0);
        let table: String = row.get(1);
        let by_tenant: bool = row.get(2);
        let by_partition: bool = row.get(3);
        if by_tenant {
            client
                .execute(
                    &format!("DELETE FROM \"{schema}\".\"{table}\" WHERE tenant = $1"),
                    &[&slug],
                )
                .await?;
        }
        if by_partition && !partitions.is_empty() {
            client
                .execute(
                    &format!("DELETE FROM \"{schema}\".\"{table}\" WHERE \"partition\" = ANY($1)"),
                    &[&partitions],
                )
                .await?;
        }
    }
    Ok(())
}
