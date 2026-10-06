//! `view.registry` (0013 §5.5): the current configuration, one row per `{kind}:{code}`
//! of a `config` partition. A later record for the same key replaces the row; a `null`
//! content retires it (0015 §3). Instance-level kinds have the empty tenant.

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::registry::PAYLOAD_CONFIG;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};

use crate::backend::PgCx;
use crate::common::{content, offset_db, split_key, tenant_of, to_jsonb};
use scatter_log_postgres::PgClient;

/// The registry projection.
#[derive(Debug, Default, Clone, Copy)]
pub struct RegistryProjection;

/// The projection's name.
pub const NAME: &str = "registry";

impl<B: Backend<Cx = PgCx>> Projection<B> for RegistryProjection {
    fn name(&self) -> &'static str {
        NAME
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CONFIG
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let key = h.key.as_deref().ok_or("a config record has a key")?;
            let (kind, code) = split_key(key).ok_or("a config key is kind:code")?;
            let tenant = tenant_of(cx, h.partition).await?;
            let rows = match content(record)? {
                None => cx
                    .execute(
                        "DELETE FROM view.registry WHERE tenant = $1 AND kind = $2 AND code = $3",
                        &[&tenant, &kind, &code],
                    )
                    .await
                    .map_err(|e| e.to_string())?,
                Some(v) => cx
                    .execute(
                        "INSERT INTO view.registry (tenant, kind, code, config, \"offset\")
                         VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT (tenant, kind, code) DO UPDATE
                         SET config = EXCLUDED.config, \"offset\" = EXCLUDED.\"offset\"",
                        &[&tenant, &kind, &code, &to_jsonb(&v), &offset_db(record)?],
                    )
                    .await
                    .map_err(|e| e.to_string())?,
            };
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = tenant_of(cx, partition).await?;
            cx.execute("DELETE FROM view.registry WHERE tenant = $1", &[&tenant])
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }
}

/// Reads one current entry.
pub async fn entry<C: PgClient>(
    client: &C,
    tenant: &str,
    kind: &str,
    code: &str,
) -> Result<Option<serde_json::Value>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT config FROM view.registry WHERE tenant = $1 AND kind = $2 AND code = $3",
            &[&tenant, &kind, &code],
        )
        .await?;
    Ok(row.map(|r| r.get(0)))
}
