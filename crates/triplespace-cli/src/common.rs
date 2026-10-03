//! What every command needs: a connection, a pool-backed store, the pipeline, the clock.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use scatter_actors::issuer::IssuerRegistry;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_projection::Pipeline;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::PgIngest;
use triplespace_projections::{Farm, PgBackend, milestone_pipeline};

/// Microseconds since the epoch, now.
pub fn now() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}

/// A random 64-bit partition ID (0018 §2), never 0.
pub fn random_partition() -> Result<u64> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).context("random partition ID")?;
    Ok(u64::from_le_bytes(bytes).max(1))
}

/// A plain connection.
pub async fn connect(database: &str) -> Result<Client> {
    let config: Config = database.parse().context("database URL")?;
    let (client, connection) = config.connect(NoTls).await.context("connect")?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection: {e}");
        }
    });
    Ok(client)
}

/// The store and its pipeline.
pub fn store(database: &str, farm: &Farm) -> Result<(PgIngest, Pipeline<PgIngest>)> {
    let pool = triplespace_db::pool::pool(database, 8).context("pool")?;
    let store = PgIngest::new(PgBackend::new(pool));
    let pipeline = milestone_pipeline(IssuerRegistry::default_registry().clone(), farm.clone())
        .context("pipeline")?;
    Ok((store, pipeline))
}

/// A draft with a JSON content, an optional comment and an actor's attestation.
pub fn draft(
    payload_type: &str,
    key: &str,
    content: &serde_json::Value,
    actor: &str,
    comment: Option<&str>,
    now: u64,
) -> Result<Draft> {
    Ok(Draft {
        appended_at: now,
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::from_json(content),
            &comment.map_or(Value::Null, Value::text),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(|e| anyhow::anyhow!("body: {e}"))?,
    })
}

/// Reads or creates the instance signing key: 32 seed bytes in hex.
pub fn signing_key(path: &std::path::Path) -> Result<ed25519_dalek::SigningKey> {
    if path.exists() {
        let hex = std::fs::read_to_string(path).context("read key file")?;
        let bytes = decode_hex(hex.trim()).context("key file is 64 hex digits")?;
        return Ok(ed25519_dalek::SigningKey::from_bytes(&bytes));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).context("random key")?;
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let hex = scatter_log::hash::hex(&seed);
    std::fs::write(path, format!("{hex}\n")).context("write key file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .context("key file mode")?;
    }
    Ok(key)
}

fn decode_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}
