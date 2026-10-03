//! What every projection needs: the tenant a partition belongs to, the record's content
//! as JSON, and the shape of errors.

use scatter_log::cbor::Value;
use scatter_log::record::Record;
use scatter_log_postgres::ids::partition_to_db;
use tokio_postgres::GenericClient;

/// The tenant of a partition, `""` for an instance partition (0018 §2).
pub async fn tenant_of<C: GenericClient>(client: &C, partition: u64) -> Result<String, String> {
    let row = client
        .query_opt(
            "SELECT tenant FROM log.partition WHERE partition = $1",
            &[&partition_to_db(partition)],
        )
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("partition {partition} is not registered"))?;
    Ok(row.get::<_, Option<String>>(0).unwrap_or_default())
}

/// The layer a membership or block record is in: `tenant` for a tenant's own partition,
/// `instance` for an instance partition (0040 §5).
#[must_use]
pub fn layer_of(tenant: &str) -> &'static str {
    if tenant.is_empty() {
        "instance"
    } else {
        "tenant"
    }
}

/// A record's content part, decoded; `None` when erased or `null`.
pub fn content(record: &Record) -> Result<Option<Value>, String> {
    match record.body().content().value().map_err(|e| e.to_string())? {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(v)),
    }
}

/// CBOR to JSON for a `jsonb` column: byte strings, which JSON lacks, become lower-case
/// hex text, so a `key:` record's public key is still inspectable.
#[must_use]
pub fn to_jsonb(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => i64::try_from(*i)
            .map(serde_json::Value::from)
            .or_else(|_| u64::try_from(*i).map(serde_json::Value::from))
            .unwrap_or_else(|_| serde_json::Value::String(i.to_string())),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Bytes(b) => serde_json::Value::String(scatter_log::hash::hex(b)),
        Value::Text(t) => serde_json::Value::String(t.clone()),
        Value::Array(a) => serde_json::Value::Array(a.iter().map(to_jsonb).collect()),
        Value::Map(m) => serde_json::Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Value::Text(t) => t.clone(),
                        other => other.to_string(),
                    };
                    (key, to_jsonb(v))
                })
                .collect(),
        ),
    }
}

/// A header key split at its first colon: `kind:code`, `acl:page:7` → `("acl", "page:7")`.
#[must_use]
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}

/// The time a header's µs-since-epoch becomes, for `timestamptz` columns.
#[must_use]
pub fn time_of(micros: u64) -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_micros(micros)
}

/// The offset as `bigint`.
pub fn offset_db(record: &Record) -> Result<i64, String> {
    scatter_log_postgres::ids::to_db(record.header().offset).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonb_conversion() {
        let v = Value::map(vec![
            (Value::text("k"), Value::Bytes(vec![0xab, 0xcd])),
            (Value::text("n"), Value::Int(5)),
            (Value::text("big"), Value::Int(i128::from(u64::MAX))),
            (
                Value::text("a"),
                Value::Array(vec![Value::Bool(true), Value::Null]),
            ),
        ]);
        let j = to_jsonb(&v);
        assert_eq!(j["k"], "abcd");
        assert_eq!(j["n"], 5);
        assert_eq!(j["big"], u64::MAX);
        assert_eq!(j["a"][0], true);
        assert_eq!(split_key("acl:page:7"), Some(("acl", "page:7")));
        assert_eq!(layer_of(""), "instance");
        assert_eq!(layer_of("librarybase"), "tenant");
    }
}
