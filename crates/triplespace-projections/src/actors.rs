//! The actor-side projections of step 1 (0013 §5.4, §7; 0016 §9): `view.actor` and
//! `view.account_link` from `actor` and `link-account` records, `view.group` from the
//! registry's `group:` entries, `view.membership` and `actor.groups` from `membership`
//! records, `view.block` from `block` records, and `view.acl` from `acl` records.

use scatter_actors::acl::Acl;
use scatter_actors::actor::ActorRecord;
use scatter_actors::iri::{IriContext, actor_iri};
use scatter_actors::issuer::IssuerRegistry;
use scatter_actors::key::ActorKey;
use scatter_actors::membership::{Block, BlockScope, Membership, MembershipAction};
use scatter_log::cbor::{self, Value};
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::registry::PAYLOAD_CONFIG;
use scatter_projection::{Applied, BoxFuture, Projection, Step};

use crate::backend::{PgBackend, PgCx};
use crate::common::{content, layer_of, offset_db, quote, split_key, tenant_of, to_jsonb};

/// Payload types.
pub mod payload {
    /// An actor record.
    pub const ACTOR: &str = "scatter:v0/actor";
    /// A public account link.
    pub const LINK: &str = "scatter:v0/link-account";
    /// A membership change.
    pub const MEMBERSHIP: &str = "scatter:v0/membership";
    /// A block or unblock.
    pub const BLOCK: &str = "scatter:v0/block";
    /// An ACL.
    pub const ACL: &str = "scatter:v0/acl";
}

/// The farm the instance is: what instance and farm IRIs are minted under (0046 §6–7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Farm {
    /// The farm slug, the instance's own.
    pub slug: String,
    /// The farm base, `https://scatter.example`.
    pub base: String,
}

fn sql(e: &tokio_postgres::Error) -> String {
    e.to_string()
}

/// The base of a tenant, from its `tenant:` registry entry.
async fn tenant_base(cx: &PgCx, slug: &str) -> Result<String, String> {
    let entry = crate::registry::entry(cx.conn(), "", "tenant", slug)
        .await
        .map_err(|e| sql(&e))?
        .ok_or_else(|| format!("tenant `{slug}` has no registry entry"))?;
    entry
        .get("base")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("tenant `{slug}` has no base"))
}

// --- actors and account links --------------------------------------------------------

/// `view.actor` and `view.account_link`.
#[derive(Debug)]
pub struct ActorProjection {
    issuers: IssuerRegistry,
    farm: Farm,
}

impl ActorProjection {
    /// With the issuer registry IRIs are minted from and the farm.
    #[must_use]
    pub fn new(issuers: IssuerRegistry, farm: Farm) -> Self {
        Self { issuers, farm }
    }

    async fn iri(&self, cx: &PgCx, key: &ActorKey, tenant: &str) -> Result<String, String> {
        let tenant_base = if tenant.is_empty() {
            self.farm.base.clone()
        } else {
            tenant_base(cx, tenant).await?
        };
        let ctx = IriContext {
            tenant_slug: tenant,
            tenant_base: &tenant_base,
            farm_slug: &self.farm.slug,
            farm_base: &self.farm.base,
        };
        actor_iri(key, &self.issuers, ctx)
            .map(oxrdf::NamedNode::into_string)
            .map_err(|e| e.to_string())
    }

    async fn apply_actor(
        &self,
        cx: &mut PgCx,
        record: &Record,
        tenant: &str,
        key: &ActorKey,
    ) -> Result<u64, String> {
        let Some(value) = content(record)? else {
            // An erased actor record: the name and raw value go (0013 §5.4), and the
            // account reads as vanished (0010 A6). On a replay the record may be the
            // first for its key, so the row is made if it is missing; its kind is then
            // the default, since the erased content held it.
            let iri = self.iri(cx, key, tenant).await?;
            return cx
                .conn()
                .execute(
                    "INSERT INTO view.actor (tenant, actor_key, issuer, subject, kind, name, status, raw, iri, \"offset\")
                     VALUES ($1, $2, $3, $4, 'registered', NULL, 'vanished', NULL, $5, $6)
                     ON CONFLICT (tenant, actor_key) DO UPDATE
                     SET name = NULL, raw = NULL, status = 'vanished', \"offset\" = EXCLUDED.\"offset\"",
                    &[&tenant, &key.to_string(), &key.issuer(), &key.subject(), &iri, &offset_db(record)?],
                )
                .await
                .map_err(|e| sql(&e));
        };
        let actor: ActorRecord = cbor::from_value(value).map_err(|e| e.to_string())?;
        let iri = self.iri(cx, key, tenant).await?;
        cx.conn()
            .execute(
                "INSERT INTO view.actor (tenant, actor_key, issuer, subject, kind, name, status, raw, operator, iri, \"offset\")
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT (tenant, actor_key) DO UPDATE
                 SET kind = EXCLUDED.kind, name = EXCLUDED.name, status = EXCLUDED.status, raw = EXCLUDED.raw,
                     operator = EXCLUDED.operator, iri = EXCLUDED.iri, \"offset\" = EXCLUDED.\"offset\"",
                &[
                    &tenant,
                    &key.to_string(),
                    &key.issuer(),
                    &key.subject(),
                    &actor.kind.name(),
                    &actor.name,
                    &actor.status.name(),
                    &actor.raw,
                    &actor.operator.as_ref().map(ToString::to_string),
                    &iri,
                    &offset_db(record)?,
                ],
            )
            .await
            .map_err(|e| sql(&e))
    }

    async fn apply_link(
        cx: &mut PgCx,
        record: &Record,
        tenant: &str,
        key: &ActorKey,
    ) -> Result<u64, String> {
        let Some(value) = content(record)? else {
            // An erased link is an unlink (0007 §7).
            return cx
                .conn()
                .execute(
                    "DELETE FROM view.account_link WHERE tenant = $1 AND local_actor = $2 AND \"offset\" = $3",
                    &[&tenant, &key.to_string(), &offset_db(record)?],
                )
                .await
                .map_err(|e| sql(&e));
        };
        let foreign = value
            .get("foreign")
            .and_then(Value::as_text)
            .ok_or("a link-account record has `foreign`")?;
        cx.conn()
            .execute(
                "INSERT INTO view.account_link (tenant, local_actor, foreign_actor, \"offset\")
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (tenant, local_actor, foreign_actor) DO UPDATE SET \"offset\" = EXCLUDED.\"offset\"",
                &[&tenant, &key.to_string(), &foreign, &offset_db(record)?],
            )
            .await
            .map_err(|e| sql(&e))
    }
}

fn actor_key_of(header: &Header) -> Result<ActorKey, String> {
    let key = header
        .key
        .as_deref()
        .ok_or("an actor-side record has a key")?;
    ActorKey::parse(key).map_err(|e| e.to_string())
}

impl Projection<PgBackend> for ActorProjection {
    fn name(&self) -> &'static str {
        "actor"
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == payload::ACTOR || header.payload_type == payload::LINK
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx.conn(), h.partition).await?;
            let key = actor_key_of(h)?;
            let rows = if h.payload_type == payload::ACTOR {
                self.apply_actor(cx, record, &tenant, &key).await?
            } else {
                Self::apply_link(cx, record, &tenant, &key).await?
            };
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = tenant_of(cx.conn(), partition).await?;
            cx.conn()
                .batch_execute(&format!(
                    "DELETE FROM view.account_link WHERE tenant = {t}; DELETE FROM view.actor WHERE tenant = {t};",
                    t = quote(&tenant)
                ))
                .await
                .map_err(|e| sql(&e))
        })
    }
}

// --- groups --------------------------------------------------------------------------

/// `view.group` from the registry's `group:` entries.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroupProjection;

impl Projection<PgBackend> for GroupProjection {
    fn name(&self) -> &'static str {
        "group"
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CONFIG
            && header
                .key
                .as_deref()
                .is_some_and(|k| k.starts_with("group:"))
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx.conn(), h.partition).await?;
            let (_, name) = split_key(h.key.as_deref().unwrap_or_default()).ok_or("group:name")?;
            let rows = match content(record)? {
                None => cx
                    .conn()
                    .execute(
                        "DELETE FROM view.\"group\" WHERE tenant = $1 AND name = $2",
                        &[&tenant, &name],
                    )
                    .await
                    .map_err(|e| sql(&e))?,
                Some(v) => {
                    let j = to_jsonb(&v);
                    let permissions: Vec<String> = j
                        .get("permissions")
                        .and_then(serde_json::Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|p| p.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    let scope = j.get("scope").and_then(serde_json::Value::as_str);
                    cx.conn()
                        .execute(
                            "INSERT INTO view.\"group\" (tenant, name, permissions, scope, \"offset\")
                             VALUES ($1, $2, $3, $4, $5)
                             ON CONFLICT (tenant, name) DO UPDATE
                             SET permissions = EXCLUDED.permissions, scope = EXCLUDED.scope, \"offset\" = EXCLUDED.\"offset\"",
                            &[&tenant, &name, &permissions, &scope, &offset_db(record)?],
                        )
                        .await
                        .map_err(|e| sql(&e))?
                }
            };
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = tenant_of(cx.conn(), partition).await?;
            cx.conn()
                .execute("DELETE FROM view.\"group\" WHERE tenant = $1", &[&tenant])
                .await
                .map_err(|e| sql(&e))?;
            Ok(())
        })
    }
}

// --- memberships ----------------------------------------------------------------------

/// `view.membership`, and `view.actor.groups` denormalized from it (0016 §9).
#[derive(Debug, Default, Clone, Copy)]
pub struct MembershipProjection;

async fn refresh_groups(cx: &PgCx, tenant: &str, actor_key: &str) -> Result<(), String> {
    // Current memberships of the actor across layers, unexpired at the record's time
    // are kept as rows; `groups` lists them all, expiry being evaluated on read.
    cx.conn()
        .execute(
            "UPDATE view.actor SET groups = COALESCE(
                 (SELECT array_agg(DISTINCT \"group\" ORDER BY \"group\") FROM view.membership
                  WHERE tenant = $1 AND actor_key = $2), '{}')
             WHERE tenant = $1 AND actor_key = $2",
            &[&tenant, &actor_key],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(())
}

impl Projection<PgBackend> for MembershipProjection {
    fn name(&self) -> &'static str {
        "membership"
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == payload::MEMBERSHIP
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx.conn(), h.partition).await?;
            let key = actor_key_of(h)?.to_string();
            let Some(value) = content(record)? else {
                return Ok(Applied::rows(0));
            };
            let m: Membership = cbor::from_value(value).map_err(|e| e.to_string())?;
            let layer = layer_of(&tenant);
            let rows = match m.action {
                MembershipAction::Add => cx
                    .conn()
                    .execute(
                        "INSERT INTO view.membership (tenant, actor_key, \"group\", layer, expires, \"offset\")
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT (tenant, actor_key, \"group\", layer) DO UPDATE
                         SET expires = EXCLUDED.expires, \"offset\" = EXCLUDED.\"offset\"",
                        &[
                            &tenant,
                            &key,
                            &m.group,
                            &layer,
                            &m.expires.map(|t| i64::try_from(t.0).unwrap_or(i64::MAX)),
                            &offset_db(record)?,
                        ],
                    )
                    .await
                    .map_err(|e| sql(&e))?,
                MembershipAction::Remove => cx
                    .conn()
                    .execute(
                        "DELETE FROM view.membership WHERE tenant = $1 AND actor_key = $2 AND \"group\" = $3 AND layer = $4",
                        &[&tenant, &key, &m.group, &layer],
                    )
                    .await
                    .map_err(|e| sql(&e))?,
            };
            // The actor row lives with its tenant; an instance-layer membership of a
            // tenant actor is keyed by the actor's own tenant, which the actor key names.
            let actor_tenant = actor_key_of(h)?;
            let actor_tenant = if actor_tenant.is_local() {
                tenant.clone()
            } else {
                String::new()
            };
            refresh_groups(cx, &actor_tenant, &key).await?;
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = tenant_of(cx.conn(), partition).await?;
            cx.conn()
                .execute(
                    "DELETE FROM view.membership WHERE tenant = $1 AND layer = $2",
                    &[&tenant, &layer_of(&tenant)],
                )
                .await
                .map_err(|e| sql(&e))?;
            cx.conn()
                .execute(
                    "UPDATE view.actor SET groups = '{}' WHERE tenant = $1",
                    &[&tenant],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(())
        })
    }
}

// --- blocks ------------------------------------------------------------------------------

/// `view.block`: the latest block or unblock per actor and layer.
#[derive(Debug, Default, Clone, Copy)]
pub struct BlockProjection;

impl Projection<PgBackend> for BlockProjection {
    fn name(&self) -> &'static str {
        "block"
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == payload::BLOCK
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx.conn(), h.partition).await?;
            let key = actor_key_of(h)?.to_string();
            let layer = layer_of(&tenant);
            let Some(value) = content(record)? else {
                return Ok(Applied::rows(0));
            };
            let block: Block = cbor::from_value(value).map_err(|e| e.to_string())?;
            let rows = match block {
                Block::Block { removes, expires } => {
                    let removes: Option<Vec<String>> = match removes {
                        BlockScope::AllButRead => None,
                        BlockScope::Permissions(p) => Some(p.into_iter().collect()),
                    };
                    cx.conn()
                        .execute(
                            "INSERT INTO view.block (tenant, actor_key, layer, removes, expires, \"offset\")
                             VALUES ($1, $2, $3, $4, $5, $6)
                             ON CONFLICT (tenant, actor_key, layer) DO UPDATE
                             SET removes = EXCLUDED.removes, expires = EXCLUDED.expires, \"offset\" = EXCLUDED.\"offset\"",
                            &[
                                &tenant,
                                &key,
                                &layer,
                                &removes,
                                &expires.map(|t| i64::try_from(t.0).unwrap_or(i64::MAX)),
                                &offset_db(record)?,
                            ],
                        )
                        .await
                        .map_err(|e| sql(&e))?
                }
                Block::Unblock => cx
                    .conn()
                    .execute(
                        "DELETE FROM view.block WHERE tenant = $1 AND actor_key = $2 AND layer = $3",
                        &[&tenant, &key, &layer],
                    )
                    .await
                    .map_err(|e| sql(&e))?,
            };
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = tenant_of(cx.conn(), partition).await?;
            cx.conn()
                .execute(
                    "DELETE FROM view.block WHERE tenant = $1 AND layer = $2",
                    &[&tenant, &layer_of(&tenant)],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(())
        })
    }
}

// --- ACLs ---------------------------------------------------------------------------

/// `view.acl`: the current ACL per target; a `null` content retires it.
#[derive(Debug, Default, Clone, Copy)]
pub struct AclProjection;

impl Projection<PgBackend> for AclProjection {
    fn name(&self) -> &'static str {
        "acl"
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == payload::ACL
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx.conn(), h.partition).await?;
            let target = h
                .key
                .as_deref()
                .ok_or("an acl record is keyed by its target")?;
            let rows = match content(record)? {
                None => cx
                    .conn()
                    .execute(
                        "DELETE FROM view.acl WHERE tenant = $1 AND target = $2",
                        &[&tenant, &target],
                    )
                    .await
                    .map_err(|e| sql(&e))?,
                Some(v) => {
                    let acl: Acl = cbor::from_value(v.clone()).map_err(|e| e.to_string())?;
                    if acl.target.key() != target {
                        return Err(format!(
                            "acl target `{}` under key `{target}`",
                            acl.target.key()
                        ));
                    }
                    let j = to_jsonb(&v);
                    let restrictions = j
                        .get("restrictions")
                        .cloned()
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::default()));
                    let read_kind = j
                        .get("read_kind")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    let mut extra = serde_json::Map::new();
                    for k in ["parts", "reserved", "talk", "name", "members"] {
                        if let Some(x) = j.get(k) {
                            extra.insert(k.into(), x.clone());
                        }
                    }
                    let extra = (!extra.is_empty()).then_some(serde_json::Value::Object(extra));
                    cx.conn()
                        .execute(
                            "INSERT INTO view.acl (tenant, target, restrictions, read_kind, extra, \"offset\")
                             VALUES ($1, $2, $3, $4, $5, $6)
                             ON CONFLICT (tenant, target) DO UPDATE
                             SET restrictions = EXCLUDED.restrictions, read_kind = EXCLUDED.read_kind,
                                 extra = EXCLUDED.extra, \"offset\" = EXCLUDED.\"offset\"",
                            &[&tenant, &target, &restrictions, &read_kind, &extra, &offset_db(record)?],
                        )
                        .await
                        .map_err(|e| sql(&e))?
                }
            };
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = tenant_of(cx.conn(), partition).await?;
            cx.conn()
                .execute("DELETE FROM view.acl WHERE tenant = $1", &[&tenant])
                .await
                .map_err(|e| sql(&e))?;
            Ok(())
        })
    }
}
