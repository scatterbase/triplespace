//! `{base}/.well-known/tlog/` (01 §4.2; 17 §1.1; 0022 §1): a tenant's checkpoints, segment
//! manifests, consistency proofs and the key chain, for a reader that verifies what it
//! takes from the log.
//!
//! - `GET /.well-known/tlog/{name}/checkpoint`: the partition's latest checkpoint, as the
//!   C2SP signed note; `…/checkpoint/{size}`, the one of that tree size;
//! - `GET /.well-known/tlog/{name}/segment/{n}`: segment `n`'s manifest;
//! - `GET /.well-known/tlog/{name}/consistency?from=&to=`: the RFC 9162 consistency proof
//!   between two tree sizes, as `{first, second, consistency}`, hashes in base64;
//! - `GET /.well-known/tlog/keys`: the key chain, every `key:` record of the instance's
//!   `config` partition, which signs every checkpoint, as a CBOR sequence (RFC 8742) of
//!   canonical records.
//!
//! `{name}` is the partition's name on the tenant, as its origin line has it (`local`,
//! `mirror/wd`). Only a partition published to the caller is served, as on the record
//! routes ([`triplespace_rdf::bundle::published`]); any other is 404, as is a tenant the
//! caller may not read.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use scatter_log_postgres::{checkpoints, log, merkle};
use serde_json::json;
use triplespace_rdf::bundle::{PartitionRef, Reader, key_records, published};

use crate::app::App;
use crate::auth::{Caller, bearer};
use crate::forwarded::Origin;
use crate::http_cache::{self, Class};
use crate::tenant;

fn plain(status: StatusCode, text: &str) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        format!("{text}\n"),
    )
        .into_response()
}

fn not_found(what: &str) -> Response {
    plain(StatusCode::NOT_FOUND, what)
}

fn internal(e: &dyn std::fmt::Display) -> Response {
    plain(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
}

/// What a path under `/.well-known/tlog/` asks for.
#[derive(Debug, PartialEq, Eq)]
enum Ask<'a> {
    Keys,
    Latest(&'a str),
    Checkpoint(&'a str, u64),
    Segment(&'a str, u64),
    Consistency(&'a str),
}

/// Reads the path after `/.well-known/tlog/`. A name may hold `/`, so the request is read
/// from the end.
fn ask(rest: &str) -> Option<Ask<'_>> {
    if rest == "keys" {
        return Some(Ask::Keys);
    }
    if let Some(name) = rest.strip_suffix("/checkpoint") {
        return Some(Ask::Latest(name));
    }
    if let Some(name) = rest.strip_suffix("/consistency") {
        return Some(Ask::Consistency(name));
    }
    let (head, last) = rest.rsplit_once('/')?;
    let n: u64 = last.parse().ok()?;
    if let Some(name) = head.strip_suffix("/checkpoint") {
        return Some(Ask::Checkpoint(name, n));
    }
    if let Some(name) = head.strip_suffix("/segment") {
        return Some(Ask::Segment(name, n));
    }
    None
}

/// `GET /.well-known/tlog/{*rest}`.
#[allow(clippy::too_many_lines)]
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(rest): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(asked) = ask(&rest) else {
        return not_found("no such log resource");
    };
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let tenant = match tenant::resolve(&app, &**client, &origin, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return plain(
                StatusCode::MISDIRECTED_REQUEST,
                "not a tenant of this instance",
            );
        }
        Err(e) => return internal(&e),
    };
    let caller = match Caller::resolve(app.accounts_pool(), &tenant.slug, &headers).await {
        Ok(c) => c,
        Err(e) => return internal(&e.info),
    };
    let credentialed = caller.has_session() || bearer(&headers).is_some();
    let class = Class::Public.permitted(&method, credentialed, false, false);
    let Ok(reader) = Reader::new(&**client, &tenant.slug, caller.effective.clone()).await else {
        return not_found("no such log");
    };
    let name = match asked {
        Ask::Keys => {
            let keys = match key_records(&**client).await {
                Ok(k) => k,
                Err(e) => return internal(&e),
            };
            let mut body = Vec::new();
            for k in &keys {
                body.extend(k.encode());
            }
            return respond_bytes("application/cbor-seq", body, class);
        }
        Ask::Latest(n) | Ask::Checkpoint(n, _) | Ask::Segment(n, _) | Ask::Consistency(n) => n,
    };
    let row = match published(&**client, &tenant.slug, &reader, PartitionRef::Name(name)).await {
        Ok(Some(r)) => r,
        Ok(None) => return not_found("no such log"),
        Err(e) => return internal(&e),
    };
    let partition = row.partition;
    let logged = row.integrity == "logged";
    match asked {
        Ask::Keys => unreachable!("answered above"),
        Ask::Latest(_) | Ask::Checkpoint(..) => {
            if !logged {
                return not_found("a hashed partition has segment manifests, not checkpoints");
            }
            let all = match checkpoints::list(&**client, partition).await {
                Ok(a) => a,
                Err(e) => return internal(&e),
            };
            let found = match asked {
                Ask::Checkpoint(_, size) => all.iter().find(|c| c.checkpoint.size == size),
                _ => all.last(),
            };
            match found {
                Some(c) => respond_text(&headers, c.to_text(), class),
                None => not_found("no such checkpoint"),
            }
        }
        Ask::Segment(_, n) => match checkpoints::manifests(&**client, partition).await {
            Ok(all) => match all.iter().find(|(s, _)| *s == n) {
                Some((_, m)) => respond_text(&headers, m.to_text(), class),
                None => not_found("no manifest for that segment"),
            },
            Err(e) => internal(&e),
        },
        Ask::Consistency(_) => {
            if !logged {
                return not_found("a hashed partition has no consistency proofs");
            }
            let number = |k: &str| q.get(k).and_then(|s| s.parse::<u64>().ok());
            let (Some(first), Some(second)) = (number("from"), number("to")) else {
                return plain(StatusCode::BAD_REQUEST, "from and to are tree sizes");
            };
            let size = match log::head(&**client, partition).await {
                Ok(h) => h.size,
                Err(e) => return internal(&e),
            };
            if first == 0 || first > second || second > size {
                return plain(
                    StatusCode::BAD_REQUEST,
                    &format!("need 0 < from <= to <= {size}"),
                );
            }
            let proof = match merkle::consistency_proof(&**client, partition, first, second).await {
                Ok(p) => p,
                Err(e) => return internal(&e),
            };
            let body = json!({
                "first": first,
                "second": second,
                "consistency": proof.iter().map(|h| B64.encode(h)).collect::<Vec<_>>(),
            });
            http_cache::respond(
                &headers,
                StatusCode::OK,
                "application/json; charset=utf-8",
                body.to_string(),
                class,
                &[],
            )
        }
    }
}

fn respond_text(headers: &HeaderMap, text: String, class: Class) -> Response {
    http_cache::respond(
        headers,
        StatusCode::OK,
        "text/plain; charset=utf-8",
        text,
        class,
        &[],
    )
}

fn respond_bytes(content_type: &'static str, body: Vec<u8>, class: Class) -> Response {
    let mut r = (StatusCode::OK, [(header::CONTENT_TYPE, content_type)], body).into_response();
    if let Ok(v) = header::HeaderValue::from_str(&class.cache_control()) {
        r.headers_mut().insert(header::CACHE_CONTROL, v);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    // 17 §1.1: the paths a reader asks.
    #[test]
    fn paths_are_read_from_the_end() {
        assert_eq!(ask("keys"), Some(Ask::Keys));
        assert_eq!(ask("local/checkpoint"), Some(Ask::Latest("local")));
        assert_eq!(
            ask("local/checkpoint/12"),
            Some(Ask::Checkpoint("local", 12))
        );
        assert_eq!(
            ask("mirror/wd/segment/3"),
            Some(Ask::Segment("mirror/wd", 3))
        );
        assert_eq!(ask("local/consistency"), Some(Ask::Consistency("local")));
        assert_eq!(ask("local/checkpoint/x"), None);
        assert_eq!(ask("local"), None);
    }
}
