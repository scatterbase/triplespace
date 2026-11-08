//! The record and proof routes (0012 §5; 17 §1.1): one record of a published partition,
//! the checkpoint that first covers it, its inclusion proof, and the proofs of a range of
//! records against one checkpoint with their shared nodes sent once. With them a third
//! party checks that a record is in the log against any checkpoint that covers it
//! without holding the log, which implements 0006 §9.4 (01 §8.4).
//!
//! - `GET /record/{partition}/{offset}`: the record as JSON, its header decoded, each
//!   part with its leaf and, when present, its value, the canonical CBOR in `record` and
//!   the signature of its attestation; `Accept: application/cbor` sends the bytes alone.
//! - `GET /record/{partition}/{offset}/checkpoint`: the earliest checkpoint that covers it,
//!   as the C2SP signed note.
//! - `GET /record/{partition}/{offset}/proof?checkpoint={size}`: the RFC 9162 inclusion
//!   proof against the checkpoint of that size, the latest by default.
//! - `GET /record/proofs?partition=&checkpoint=&from=&to=`: every record of `from..to`
//!   (at most [`MAX_RANGE`]) against one checkpoint: the nodes once, each leaf's path as
//!   positions in them.
//!
//! Only a partition published to the caller is served: the tenant's own, `public`, of no
//! graph hidden from the caller ([`triplespace_rdf::bundle::published`]); any other
//! answers as one that does not exist. A record is shown as an export shows it to the
//! caller (01 §8): parts the caller may not read are their commitments, listed in
//! `withheld`, and a record of a target a confidential restriction hides is its leaf
//! alone, as a compacted record is (0103 A1). Partition IDs are 64 random bits, so JSON
//! carries them as decimal strings; hashes and bytes are base64.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use deadpool_postgres::Object;
use scatter_log::body::{ATTESTATION, COMMENT, CONTENT};
use scatter_log::cbor::Value as Cbor;
use scatter_log::record::Record;
use scatter_log_postgres::{checkpoints, ids, log, merkle};
use serde_json::{Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::bearer;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_projections::feed::iso8601;
use triplespace_rdf::bundle::{
    Offsets, PartitionRef, PartitionRow, Reader, Shown, published, redact,
};

use crate::entity_route::{error, internal};
use crate::settings::prelude;

/// The most records one `GET /record/proofs` covers.
pub const MAX_RANGE: u64 = 1000;

/// A published partition, resolved for the caller.
struct Opened {
    client: Object,
    tenant: String,
    reader: Reader,
    row: PartitionRow,
    size: u64,
    credentialed: bool,
}

fn no_record(partition: u64, offset: u64) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "record-not-found",
        format!("no published record {partition}:{offset}"),
    )
}

fn no_partition(partition: u64) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "partition-not-found",
        format!("no published partition {partition}"),
    )
}

/// The tenant, the caller and the partition, if it is published to the caller.
async fn open(
    app: &App,
    origin: &Origin,
    headers: &HeaderMap,
    partition: u64,
) -> Result<Opened, Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let (tenant, caller) = prelude(app, &client, origin, headers).await?;
    let credentialed = caller.has_session() || bearer(headers).is_some();
    let Ok(reader) = Reader::new(&**client, &tenant.slug, caller.effective.clone()).await else {
        return Err(no_partition(partition));
    };
    let row = published(
        &**client,
        &tenant.slug,
        &reader,
        PartitionRef::Id(partition),
    )
    .await
    .map_err(|e| internal(&e))?
    .ok_or_else(|| no_partition(partition))?;
    let size = log::head(&**client, partition)
        .await
        .map_err(|e| internal(&e))?
        .size;
    Ok(Opened {
        client,
        tenant: tenant.slug,
        reader,
        row,
        size,
        credentialed,
    })
}

fn respond(method: &Method, headers: &HeaderMap, credentialed: bool, body: &Value) -> Response {
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Public.permitted(method, credentialed, false, false),
        &[],
    )
}

fn part_name(index: usize) -> Option<&'static str> {
    match index {
        CONTENT => Some("content"),
        COMMENT => Some("comment"),
        ATTESTATION => Some("attestation"),
        _ => None,
    }
}

/// The record as JSON, its withheld parts named.
fn record_json(partition: u64, record: &Record, withheld: &[usize]) -> Value {
    let h = record.header();
    let parts: Vec<Value> = record
        .body()
        .parts()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let state = if withheld.contains(&i) {
                "withheld"
            } else if p.is_erased() {
                "erased"
            } else {
                "present"
            };
            let value = p
                .value()
                .ok()
                .flatten()
                .and_then(|v| v.to_json().ok())
                .unwrap_or(Value::Null);
            json!({
                "index": i,
                "name": part_name(i),
                "state": state,
                "leaf": B64.encode(p.leaf(u8::try_from(i).unwrap_or(u8::MAX))),
                "value": value,
            })
        })
        .collect();
    let signature = record
        .body()
        .attestation()
        .value()
        .ok()
        .flatten()
        .and_then(|a| a.get("signature").cloned())
        .map_or(Value::Null, |s| {
            json!({
                "key": s.get("key").and_then(Cbor::as_text),
                "alg": s.get("alg").and_then(Cbor::as_text),
                "sig": s.get("sig").and_then(Cbor::as_bytes).map(|b| B64.encode(b)),
            })
        });
    json!({
        "partition": partition.to_string(),
        "offset": h.offset,
        "leaf": B64.encode(record.leaf()),
        "header": {
            "version": h.version,
            "appended_at": iso8601(h.appended_at),
            "payload_type": h.payload_type,
            "key": h.key,
            "commitment": B64.encode(h.commitment),
            "revid": h.revid,
            "logid": h.logid,
            "page_id": h.page_id,
        },
        "parts": parts,
        "withheld": withheld,
        "record": B64.encode(record.encode()),
        "signature": signature,
    })
}

fn wants_cbor(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/cbor"))
}

/// `GET /record/{partition}/{offset}`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path((partition, offset)): Path<(u64, u64)>,
) -> Response {
    let o = match open(&app, &origin, &headers, partition).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    if offset >= o.size {
        return no_record(partition, offset);
    }
    let slot = match log::read(&**o.client, partition, offset).await {
        Ok(s) => s,
        Err(e) => return internal(&e),
    };
    let withhold = match o
        .reader
        .decide(
            &**o.client,
            &o.tenant,
            &[ids::partition_to_db(partition)],
            Offsets::Range(offset..offset + 1),
        )
        .await
    {
        Ok(w) => w,
        Err(e) => return internal(&e),
    };
    let shown = match redact(slot, withhold.get(&(partition, offset))) {
        Ok(s) => s,
        Err(e) => return internal(&e),
    };
    match shown {
        Shown::Leaf(leaf) => {
            if wants_cbor(&headers) {
                return error(
                    StatusCode::NOT_FOUND,
                    "record-leaf-only",
                    format!("only the leaf of {partition}:{offset} is published"),
                );
            }
            let body = json!({
                "partition": partition.to_string(), "offset": offset, "leaf": B64.encode(leaf),
                "header": null, "parts": null, "withheld": null, "record": null, "signature": null,
            });
            respond(&method, &headers, o.credentialed, &body)
        }
        Shown::Record { record, withheld } => {
            if wants_cbor(&headers) {
                let class = Class::Public.permitted(&method, o.credentialed, false, false);
                let mut r = (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/cbor")],
                    record.encode(),
                )
                    .into_response();
                if let Ok(v) = header::HeaderValue::from_str(&class.cache_control()) {
                    r.headers_mut().insert(header::CACHE_CONTROL, v);
                }
                r.headers_mut()
                    .insert(header::VARY, header::HeaderValue::from_static("Accept"));
                return r;
            }
            respond(
                &method,
                &headers,
                o.credentialed,
                &record_json(partition, &record, &withheld),
            )
        }
    }
}

/// `GET /record/{partition}/{offset}/checkpoint`.
pub async fn checkpoint(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path((partition, offset)): Path<(u64, u64)>,
) -> Response {
    let o = match open(&app, &origin, &headers, partition).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    if offset >= o.size {
        return no_record(partition, offset);
    }
    if o.row.integrity != "logged" {
        return error(
            StatusCode::NOT_FOUND,
            "no-checkpoints",
            format!(
                "partition {partition} is `{}`: its segments have manifests, not checkpoints",
                o.row.integrity
            ),
        );
    }
    match checkpoints::covering(&**o.client, partition, offset).await {
        Ok(Some(c)) => http_cache::respond(
            &headers,
            StatusCode::OK,
            "text/plain; charset=utf-8",
            c.to_text(),
            Class::Public.permitted(&method, o.credentialed, false, false),
            &[],
        ),
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            "not-checkpointed",
            format!("no checkpoint covers {partition}:{offset} yet"),
        ),
        Err(e) => internal(&e),
    }
}

/// The checkpoint `size` names, or the latest, of a `logged` partition.
async fn signed(o: &Opened, size: Option<&String>) -> Result<(u64, String), Response> {
    let partition = o.row.partition;
    if o.row.integrity != "logged" {
        return Err(error(
            StatusCode::NOT_FOUND,
            "no-checkpoints",
            format!("partition {partition} has no checkpoints"),
        ));
    }
    let all = checkpoints::list(&**o.client, partition)
        .await
        .map_err(|e| internal(&e))?;
    let found = match size {
        None => all.last(),
        Some(s) => {
            let Ok(want) = s.parse::<u64>() else {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid-checkpoint",
                    "checkpoint is a tree size",
                ));
            };
            all.iter().find(|c| c.checkpoint.size == want)
        }
    };
    found
        .map(|c| (c.checkpoint.size, c.to_text()))
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "checkpoint-not-found",
                format!("partition {partition} has no such checkpoint"),
            )
        })
}

/// `GET /record/{partition}/{offset}/proof?checkpoint={size}`.
pub async fn proof(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path((partition, offset)): Path<(u64, u64)>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let o = match open(&app, &origin, &headers, partition).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    if offset >= o.size {
        return no_record(partition, offset);
    }
    let (size, note) = match signed(&o, q.get("checkpoint")).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if offset >= size {
        return error(
            StatusCode::CONFLICT,
            "not-in-checkpoint",
            format!("{partition}:{offset} is past the checkpoint of size {size}"),
        );
    }
    let path = match merkle::inclusion_proof(&**o.client, partition, offset, size).await {
        Ok(p) => p,
        Err(e) => return internal(&e),
    };
    let body = json!({
        "leaf_index": offset,
        "tree_size": size,
        "audit_path": path.iter().map(|h| B64.encode(h)).collect::<Vec<_>>(),
        "checkpoint": note,
    });
    let class = Class::Public.permitted(&method, o.credentialed, false, false);
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &[],
    )
}

/// `GET /record/proofs?partition=&checkpoint=&from=&to=`.
pub async fn proofs(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let number = |k: &str| q.get(k).and_then(|s| s.parse::<u64>().ok());
    let (Some(partition), Some(from), Some(to)) =
        (number("partition"), number("from"), number("to"))
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid-range",
            "partition, from and to are required, as numbers",
        );
    };
    if from >= to || to - from > MAX_RANGE {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid-range",
            format!("from must be below to, and the range at most {MAX_RANGE} records"),
        );
    }
    let o = match open(&app, &origin, &headers, partition).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let (size, note) = match signed(&o, q.get("checkpoint")).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if to > size {
        return error(
            StatusCode::CONFLICT,
            "not-in-checkpoint",
            format!("the range runs past the checkpoint of size {size}"),
        );
    }
    let (nodes, paths) = match merkle::range_proof(&**o.client, partition, from, to, size).await {
        Ok(x) => x,
        Err(e) => return internal(&e),
    };
    let body = json!({
        "tree_size": size,
        "checkpoint": note,
        "nodes": nodes.iter().map(|(level, index, hash)| json!({
            "level": level, "index": index, "hash": B64.encode(hash),
        })).collect::<Vec<_>>(),
        "proofs": paths.iter().zip(from..to).map(|(path, i)| json!({
            "leaf_index": i, "path": path,
        })).collect::<Vec<_>>(),
    });
    let class = Class::Public.permitted(&method, o.credentialed, false, false);
    http_cache::respond(
        &headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &[],
    )
}
