//! Tenant requests (0111 §2, §4; 08 §1.4): filing one, and reading the requests with what
//! became of each. Shared by the server's `/tenancy/requests` routes and the CLI's
//! provisioner, which carries them out.
//!
//! A request is a `tenant/request` log event in the instance `log`, keyed `tenant:{slug}`,
//! attested by the operator who filed it. Its `params` hold only the slug and the base; the
//! rest is under `request`, which no projection reads (0111 A1). It is **created** once a
//! `tenant:` record in the instance `config` names its offset in `request`, **refused** once
//! a `tenant/refuse` log event under the same key names it as target, and **pending** until
//! then.

use scatter_ingest::IngestError;
use scatter_ingest::store::{IngestStore, Sequence};
use scatter_ingest::write::append_projected;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::record::Record;
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use serde_json::json;
use tokio_postgres::GenericClient;
use triplespace_projections::PgCx;
use triplespace_projections::common::{PAYLOAD_LOGEVENT, attested, content};

use crate::PgIngest;

/// The channel a filed request is announced on (0111 §2).
pub const CHANNEL: &str = "ts_tenant_request";

/// The instance right that files a request (0111 §8).
pub const RIGHT: &str = "ts-createtenant";

/// The key of a slug's requests and refusals in the instance `log`.
#[must_use]
pub fn key(slug: &str) -> String {
    format!("tenant:{slug}")
}

/// What an operator asks for: the fields of a request (0111 §2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ask {
    /// The slug.
    pub slug: String,
    /// The base URI.
    pub base: String,
    /// The wiki's name, `site.name`.
    pub name: Option<String>,
    /// The owner's account name.
    pub owner_name: String,
    /// The account, by actor key, whose login the owner account is bound to.
    pub owner_login_source: Option<String>,
    /// The source wiki the tenant adopts (0035), its base URL: the CLI's `--adopt`.
    pub adopt: Option<String>,
    /// The owner's user ID: the source wiki's with `adopt`, else 1.
    pub owner_id: Option<u64>,
    /// What is asked for (0111 §10): [`act::CREATE`] when empty.
    pub act: String,
    /// For [`act::ACCEPT_PRIMARY`], the offer accepted: its offset in the instance `log`.
    pub offer: Option<u64>,
}

/// The acts a request asks for (0111 §1, §10; 08 §1.4).
pub mod act {
    /// Create a tenant.
    pub const CREATE: &str = "create";
    /// Freeze a tenant read-only, as for a move (08 §6.3).
    pub const FREEZE: &str = "freeze";
    /// Lift a freeze.
    pub const UNFREEZE: &str = "unfreeze";
    /// Delete a tenant: read-only for the grace period, then erased (08 §6.6).
    pub const DELETE: &str = "delete";
    /// Cancel a deletion within its grace period.
    pub const UNDELETE: &str = "undelete";
    /// Accept the primary role offered to the tenant (08 §7.5).
    pub const ACCEPT_PRIMARY: &str = "accept-primary";
    /// Every act.
    pub const ALL: [&str; 6] = [CREATE, FREEZE, UNFREEZE, DELETE, UNDELETE, ACCEPT_PRIMARY];
}

impl Ask {
    /// The act, [`act::CREATE`] when none is named.
    #[must_use]
    pub fn act(&self) -> &str {
        if self.act.is_empty() {
            act::CREATE
        } else {
            &self.act
        }
    }

    /// A request for `act` on an existing tenant.
    #[must_use]
    pub fn on(act: &str, slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            act: act.to_string(),
            ..Self::default()
        }
    }

    /// The request's log event at `time`.
    #[must_use]
    pub fn event(&self, time: u64) -> serde_json::Value {
        json!({
            "type": "tenant", "action": "request", "time": time, "target": null,
            "params": {"slug": self.slug, "base": self.base, "act": self.act()},
            "request": {
                "act": self.act(), "slug": self.slug, "base": self.base, "name": self.name,
                "owner_name": self.owner_name, "owner_login_source": self.owner_login_source,
                "adopt": self.adopt, "owner_id": self.owner_id, "offer": self.offer,
            },
        })
    }

    /// The fields of a request's log event, `None` if it is not one.
    #[must_use]
    pub fn of_event(event: &serde_json::Value) -> Option<Self> {
        let r = event.get("request")?;
        let text = |k: &str| {
            r.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        Some(Self {
            slug: text("slug")?,
            base: text("base").unwrap_or_default(),
            name: text("name"),
            owner_name: text("owner_name").unwrap_or_else(|| "Owner".into()),
            owner_login_source: text("owner_login_source"),
            adopt: text("adopt"),
            owner_id: r.get("owner_id").and_then(serde_json::Value::as_u64),
            act: text("act").unwrap_or_else(|| act::CREATE.into()),
            offer: r.get("offer").and_then(serde_json::Value::as_u64),
        })
    }

    /// A shape check of what the API accepts: the provisioner checks everything else
    /// (0111 §4), so a request whose slug is taken is filed and refused, not turned away.
    ///
    /// # Errors
    ///
    /// Why the fields cannot be a request.
    pub fn check_shape(&self) -> Result<(), String> {
        if self.slug.is_empty() || self.slug.len() > 64 {
            return Err("`slug` is required, at most 64 characters".into());
        }
        if !act::ALL.contains(&self.act()) {
            return Err(format!("`act` is one of {}", act::ALL.join(", ")));
        }
        if self.act() == act::ACCEPT_PRIMARY && self.offer.is_none() {
            return Err("accepting the primary role names the `offer`".into());
        }
        if self.act() != act::CREATE {
            return Ok(());
        }
        if self.base.is_empty() || self.base.len() > 256 {
            return Err("`base` is required, at most 256 characters".into());
        }
        if self.owner_name.is_empty() || self.owner_name.len() > 255 {
            return Err("`owner_name` is required".into());
        }
        if self.name.as_ref().is_some_and(|n| n.len() > 255) {
            return Err("`name` is at most 255 characters".into());
        }
        Ok(())
    }
}

/// Where a filed request is: the instance `log`, its offset and leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filed {
    /// The instance `log`.
    pub partition: u64,
    /// The request's offset, its ID.
    pub offset: u64,
    /// Its leaf, for the authority that cites it.
    pub leaf: [u8; 32],
}

/// Files a request as `actor` in the caller's unit of work, projected inline. The caller
/// sends `NOTIFY ts_tenant_request` once the unit of work commits.
///
/// # Errors
///
/// The store's or a projection's, or an instance without a `log` partition.
pub async fn file(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    ask: &Ask,
    actor: &str,
    reason: Option<&str>,
    now: u64,
) -> Result<Filed, IngestError> {
    append_instance_log(
        store,
        pipeline,
        cx,
        &key(&ask.slug),
        &ask.event(now),
        actor,
        reason,
        now,
    )
    .await
}

/// What became of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Neither carried out nor refused yet.
    Pending,
    /// The tenant was created.
    Created,
    /// Another act was carried out.
    Done,
    /// Refused: the code, and why.
    Refused {
        /// The refusal's code, `params.reason`.
        code: String,
        /// Its comment.
        why: Option<String>,
    },
}

impl State {
    /// The state's name in the API: `pending`, `created` or `refused`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Created => "created",
            Self::Done => "done",
            Self::Refused { .. } => "refused",
        }
    }
}

/// A request with what became of it.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Where it is.
    pub filed: Filed,
    /// The operator who filed it, absent if erased.
    pub actor: Option<String>,
    /// When it was filed, microseconds.
    pub time: u64,
    /// Its public reason.
    pub reason: Option<String>,
    /// What it asks for.
    pub ask: Ask,
    /// What became of it.
    pub state: State,
}

impl Entry {
    /// The request as the API shows it: everything it holds, to a holder of
    /// `ts-createtenant` (0111 §7).
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut v = json!({
            "id": self.filed.offset,
            "state": self.state.name(),
            "time": self.time,
            "actor": self.actor,
            "reason": self.reason,
            "slug": self.ask.slug,
            "base": self.ask.base,
            "name": self.ask.name,
            "owner_name": self.ask.owner_name,
            "owner_login_source": self.ask.owner_login_source,
            "act": self.ask.act(),
            "offer": self.ask.offer,
        });
        if let State::Refused { code, why } = &self.state {
            v["refusal"] = json!({"code": code, "why": why});
        }
        v
    }
}

fn record_of(header: &[u8], body: &[u8]) -> Result<Record, String> {
    Record::new(
        scatter_log::header::Header::decode(header).map_err(|e| e.to_string())?,
        Body::decode(body).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn json_of(record: &Record) -> Result<Option<serde_json::Value>, String> {
    content(record)?
        .map(|v| v.to_json().map_err(|e| e.to_string()))
        .transpose()
}

/// Every request, oldest first, with what became of it. Read from the log, so that it is
/// current however far the projections are behind.
///
/// # Errors
///
/// The database's, or an undecodable record.
pub async fn list<C: GenericClient>(client: &C) -> Result<Vec<Entry>, String> {
    let partition = match client
        .query_opt(
            "SELECT partition FROM log.partition WHERE tenant IS NULL AND name = 'log'",
            &[],
        )
        .await
        .map_err(|e| e.to_string())?
    {
        Some(r) => scatter_log_postgres::ids::partition_from_db(r.get(0)),
        None => return Ok(Vec::new()),
    };
    let rows = client
        .query(
            "SELECT \"offset\", header, body, leaf FROM log.record
             WHERE partition = $1 AND key LIKE 'tenant:%' ORDER BY \"offset\"",
            &[&scatter_log_postgres::ids::partition_to_db(partition)],
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut refusals: Vec<(u64, State)> = Vec::new();
    for row in &rows {
        let record = record_of(row.get(1), row.get(2))?;
        if record.header().payload_type != PAYLOAD_LOGEVENT {
            continue;
        }
        let Some(event) = json_of(&record)? else {
            continue;
        };
        let text = |k: &str| event.get(k).and_then(serde_json::Value::as_str);
        match (text("type"), text("action")) {
            (Some("tenant"), Some("request")) => {
                let Some(ask) = Ask::of_event(&event) else {
                    continue;
                };
                let offset = u64::try_from(row.get::<_, i64>(0)).map_err(|e| e.to_string())?;
                let leaf: Vec<u8> = row.get(3);
                entries.push(Entry {
                    filed: Filed {
                        partition,
                        offset,
                        leaf: leaf
                            .try_into()
                            .map_err(|_| "leaf is not 32 bytes".to_string())?,
                    },
                    actor: attested(&record).actor,
                    time: event
                        .get("time")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    reason: triplespace_projections::common::comment(&record),
                    ask,
                    state: State::Pending,
                });
            }
            (Some("tenant"), Some("refuse")) => {
                let Some(target) = event
                    .get("target")
                    .and_then(|t| t.get("offset"))
                    .and_then(serde_json::Value::as_u64)
                else {
                    continue;
                };
                let code = event
                    .get("params")
                    .and_then(|p| p.get("reason"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                refusals.push((
                    target,
                    State::Refused {
                        code,
                        why: triplespace_projections::common::comment(&record),
                    },
                ));
            }
            _ => {}
        }
    }
    for (target, state) in refusals {
        if let Some(e) = entries.iter_mut().find(|e| e.filed.offset == target) {
            e.state = state;
        }
    }
    for e in &mut entries {
        if e.state == State::Pending && done_for(client, &e.ask, e.filed.offset).await? {
            e.state = if e.ask.act() == act::CREATE {
                State::Created
            } else {
                State::Done
            };
        }
    }
    Ok(entries)
}

/// Whether the request at `offset` was carried out: a record in the instance `config` that
/// names it in `request`, a `tenant:` record under its slug, or for an acceptance the
/// `primary` record (0111 §4, §10).
///
/// # Errors
///
/// The database's, or an undecodable record.
pub async fn done_for<C: GenericClient>(
    client: &C,
    ask: &Ask,
    offset: u64,
) -> Result<bool, String> {
    let config_key = if ask.act() == act::ACCEPT_PRIMARY {
        "primary".to_string()
    } else {
        key(&ask.slug)
    };
    let rows = client
        .query(
            "SELECT header, body FROM log.record WHERE partition = $1 AND key = $2",
            &[
                &scatter_log_postgres::ids::partition_to_db(CONFIG_PARTITION),
                &config_key,
            ],
        )
        .await
        .map_err(|e| e.to_string())?;
    for row in &rows {
        let record = record_of(row.get(0), row.get(1))?;
        if let Some(v) = json_of(&record)?
            && v.get("request").and_then(serde_json::Value::as_u64) == Some(offset)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The refusal of a request as a log event: its target the request, `params.reason` the
/// code (0111 A1).
#[must_use]
pub fn refusal_event(filed: &Filed, code: &str, time: u64) -> serde_json::Value {
    json!({
        "type": "tenant", "action": "refuse", "time": time,
        "target": {"kind": "record", "partition": filed.partition, "offset": filed.offset},
        "params": {"reason": code},
    })
}

/// How long an offer of the primary role stays open (0046 §5, A11): seven days.
pub const OFFER_DAYS: u64 = 7;

/// An offer of the primary role (0046 §5; 08 §7.5): a `primary/offer` log event in the
/// instance `log`, keyed `primary`, attested by the `owner` of the primary tenant who made
/// it, naming the offered tenant and when it expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// Its offset in the instance `log`, its ID.
    pub offset: u64,
    /// The offered tenant's slug.
    pub tenant: String,
    /// When it expires, microseconds since the epoch.
    pub expires: u64,
    /// Who made it.
    pub actor: Option<String>,
}

#[allow(clippy::too_many_arguments)]
async fn append_instance_log(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    key: &str,
    event: &serde_json::Value,
    actor: &str,
    reason: Option<&str>,
    now: u64,
) -> Result<Filed, IngestError> {
    let partition = store
        .partition(cx, "", "log")
        .await?
        .ok_or_else(|| IngestError::Store("the instance has no `log` partition".into()))?;
    let logid = store.next_id(cx, "", &Sequence::Log).await?;
    let draft = Draft {
        appended_at: now,
        payload_type: PAYLOAD_LOGEVENT.into(),
        key: Some(key.to_string()),
        revid: None,
        logid: Some(logid),
        page_id: None,
        body: Body::core(
            &Value::from_json(event),
            &reason.map_or(Value::Null, Value::text),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(|e| IngestError::Store(format!("body: {e}")))?,
    };
    let (record, _) = append_projected(store, pipeline, cx, partition, draft, Budget::NONE).await?;
    Ok(Filed {
        partition,
        offset: record.header().offset,
        leaf: record.header().leaf(),
    })
}

/// Offers the primary role to `tenant`, as `actor`, open for [`OFFER_DAYS`].
///
/// # Errors
///
/// The store's or a projection's.
#[allow(clippy::too_many_arguments)]
pub async fn offer(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    tenant: &str,
    actor: &str,
    reason: Option<&str>,
    now: u64,
) -> Result<Filed, IngestError> {
    let expires = now + OFFER_DAYS * 86_400 * 1_000_000;
    let event = json!({
        "type": "primary", "action": "offer", "time": now, "target": null,
        "params": {"tenant": tenant, "expires": expires},
    });
    append_instance_log(store, pipeline, cx, "primary", &event, actor, reason, now).await
}

/// Withdraws the open offer at `offset`, as `actor`.
///
/// # Errors
///
/// The store's or a projection's.
pub async fn withdraw(
    store: &PgIngest,
    pipeline: &Pipeline<PgIngest>,
    cx: &mut PgCx,
    offer: &Filed,
    actor: &str,
    now: u64,
) -> Result<Filed, IngestError> {
    let event = json!({
        "type": "primary", "action": "withdraw", "time": now,
        "target": {"kind": "record", "partition": offer.partition, "offset": offer.offset},
        "params": {},
    });
    append_instance_log(store, pipeline, cx, "primary", &event, actor, None, now).await
}

/// The open offers of the primary role: neither withdrawn, expired nor accepted, oldest
/// first. `now` in microseconds.
///
/// # Errors
///
/// The database's, or an undecodable record.
pub async fn open_offers<C: GenericClient>(client: &C, now: u64) -> Result<Vec<Offer>, String> {
    let Some(partition) = client
        .query_opt(
            "SELECT partition FROM log.partition WHERE tenant IS NULL AND name = 'log'",
            &[],
        )
        .await
        .map_err(|e| e.to_string())?
        .map(|r| scatter_log_postgres::ids::partition_from_db(r.get(0)))
    else {
        return Ok(Vec::new());
    };
    let rows = client
        .query(
            "SELECT \"offset\", header, body FROM log.record
             WHERE partition = $1 AND key = 'primary' ORDER BY \"offset\"",
            &[&scatter_log_postgres::ids::partition_to_db(partition)],
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut offers = Vec::new();
    let mut closed = std::collections::BTreeSet::new();
    for row in &rows {
        let record = record_of(row.get(1), row.get(2))?;
        let Some(event) = json_of(&record)? else {
            continue;
        };
        let offset = u64::try_from(row.get::<_, i64>(0)).map_err(|e| e.to_string())?;
        match event.get("action").and_then(serde_json::Value::as_str) {
            Some("offer") => {
                let params = event.get("params");
                let field = |k: &str| params.and_then(|p| p.get(k));
                offers.push(Offer {
                    offset,
                    tenant: field("tenant")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    expires: field("expires")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    actor: attested(&record).actor,
                });
            }
            Some("withdraw") => {
                if let Some(o) = event
                    .get("target")
                    .and_then(|t| t.get("offset"))
                    .and_then(serde_json::Value::as_u64)
                {
                    closed.insert(o);
                }
            }
            _ => {}
        }
    }
    // An offer a `primary` record names as accepted is closed too.
    let accepted = client
        .query(
            "SELECT header, body FROM log.record WHERE partition = $1 AND key = 'primary'",
            &[&scatter_log_postgres::ids::partition_to_db(
                CONFIG_PARTITION,
            )],
        )
        .await
        .map_err(|e| e.to_string())?;
    for row in &accepted {
        let record = record_of(row.get(0), row.get(1))?;
        if let Some(v) = json_of(&record)?
            && let Some(o) = v.get("offer").and_then(serde_json::Value::as_u64)
        {
            closed.insert(o);
        }
    }
    Ok(offers
        .into_iter()
        .filter(|o| !closed.contains(&o.offset) && o.expires > now)
        .collect())
}

/// Microseconds since the epoch, now.
#[must_use]
pub fn now_micros() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}
