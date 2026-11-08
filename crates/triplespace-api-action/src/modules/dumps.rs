//! `{base}/dumps/`: the tenant's dumps as `triplespace dump` wrote them under `dumps.dir`
//! (02 §4.1; 17 §1.1; 0013 A78). The Wikibase-compatible dump and the local-graph source
//! dump of each snapshot are one directory, `{dumps.dir}/{tenant}/{name}/`, with its
//! `manifest.json`:
//!
//! - `GET /dumps/` lists the dumps, newest first, each by its manifest;
//! - `GET /dumps/{name}/{file}` serves a file of one, and `latest` names the newest;
//! - `GET /dumps/local/` and `GET /dumps/local/{name}/{file}` are the same restricted to
//!   the local-graph source dump, where a `stream` or `verified` reader looks (17 §1.1).
//!
//! A dump holds what an anonymous reader may read, so a tenant an anonymous reader may not
//! read serves none, whatever was written while it was public (09 §4.7; 02 §6.4). Without
//! `dumps.dir` the routes are 404.

use std::path::{Path as FsPath, PathBuf};

use axum::body::Body;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::app::App;
use crate::forwarded::Origin;
use crate::tenant;

fn plain(status: StatusCode, text: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("{text}\n"),
    )
        .into_response()
}

fn not_found() -> Response {
    plain(StatusCode::NOT_FOUND, "no such dump")
}

/// The tenant's dump directory, if the instance has dumps and an anonymous reader may
/// read the tenant.
async fn tenant_dir(app: &App, origin: &Origin, headers: &HeaderMap) -> Result<PathBuf, Response> {
    let Some(root) = app.dumps_dir() else {
        return Err(not_found());
    };
    let client = app
        .pool()
        .get()
        .await
        .map_err(|e| plain(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let tenant = match tenant::resolve(app, &**client, origin, headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(plain(
                StatusCode::MISDIRECTED_REQUEST,
                "not a tenant of this instance",
            ));
        }
        Err(e) => return Err(plain(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    };
    let anonymous = crate::auth::Caller::resolve(&**client, &tenant.slug, &HeaderMap::new())
        .await
        .map_err(|e| plain(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let chain = triplespace_projections::acl_read::current(
        &**client,
        &tenant.slug,
        &[scatter_actors::Target::Tenant(tenant.slug.clone())],
    )
    .await
    .map_err(|e| plain(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    let decision = scatter_actors::read_decision(
        &anonymous.effective,
        &chain,
        scatter_actors::Timestamp(crate::edit::now_micros()),
    );
    if decision != scatter_actors::ReadDecision::Readable {
        return Err(not_found());
    }
    Ok(root.join(&tenant.slug))
}

fn manifest(dir: &FsPath, name: &str) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join(name).join("manifest.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// The manifests, newest first, with each file given its URL; `local` keeps only the
/// local-graph source dump's files.
fn index(dir: &FsPath, local: bool) -> Value {
    let mut names = triplespace_rdf::list(dir).unwrap_or_default();
    names.reverse();
    let prefix = if local { "/dumps/local" } else { "/dumps" };
    let dumps: Vec<Value> = names
        .iter()
        .filter_map(|name| {
            let mut m = manifest(dir, name)?;
            if let Some(files) = m.get_mut("files").and_then(Value::as_array_mut) {
                files.retain(|f| !local || f["product"] == "local");
                for f in files.iter_mut() {
                    let file = f["name"].as_str().unwrap_or_default().to_string();
                    f["url"] = json!(format!("{prefix}/{name}/{file}"));
                }
            }
            m["manifest"] = json!(format!("{prefix}/{name}/manifest.json"));
            Some(m)
        })
        .collect();
    json!({ "dumps": dumps })
}

fn json_response(v: &Value) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        v.to_string(),
    )
        .into_response()
}

/// A file of a dump: `manifest.json`, or one the manifest lists (of the local product
/// only, under `/dumps/local/`).
async fn file(dir: &FsPath, name: &str, file: &str, local: bool) -> Response {
    let name = if name == "latest" {
        match triplespace_rdf::list(dir).ok().and_then(|mut n| n.pop()) {
            Some(n) => n,
            None => return not_found(),
        }
    } else if triplespace_rdf::is_dump_name(name) {
        name.to_string()
    } else {
        return not_found();
    };
    let Some(m) = manifest(dir, &name) else {
        return not_found();
    };
    let listed = m["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|f| f["name"] == file && (!local || f["product"] == "local"))
        .cloned();
    let (content_type, sha) = match (file, &listed) {
        ("manifest.json", _) => ("application/json; charset=utf-8", None),
        (_, Some(f)) => ("application/gzip", f["sha256"].as_str().map(str::to_string)),
        _ => return not_found(),
    };
    let path = dir.join(&name).join(file);
    let Ok(f) = tokio::fs::File::open(&path).await else {
        return not_found();
    };
    let len = f.metadata().await.map(|m| m.len()).ok();
    let mut response = Response::new(Body::from_stream(tokio_util::io::ReaderStream::new(f)));
    let headers = response.headers_mut();
    if let Ok(v) = content_type.parse() {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Some(len) = len {
        headers.insert(header::CONTENT_LENGTH, len.into());
    }
    if let Some(sha) = sha
        && let Ok(v) = format!("\"{sha}\"").parse()
    {
        headers.insert(header::ETAG, v);
    }
    // A dump never changes once written; `latest` does.
    let cache = if m["name"] == name && file != "manifest.json" {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=300"
    };
    if let Ok(v) = cache.parse() {
        headers.insert(header::CACHE_CONTROL, v);
    }
    response
}

/// `GET /dumps/`.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    match tenant_dir(&app, &origin, &headers).await {
        Ok(dir) => json_response(&index(&dir, false)),
        Err(r) => r,
    }
}

/// `GET /dumps/local/`.
pub async fn list_local(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    match tenant_dir(&app, &origin, &headers).await {
        Ok(dir) => json_response(&index(&dir, true)),
        Err(r) => r,
    }
}

/// `GET /dumps/{name}/{file}`.
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((name, f)): Path<(String, String)>,
) -> Response {
    match tenant_dir(&app, &origin, &headers).await {
        Ok(dir) => file(&dir, &name, &f, false).await,
        Err(r) => r,
    }
}

/// `GET /dumps/local/{name}/{file}`.
pub async fn get_local(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((name, f)): Path<(String, String)>,
) -> Response {
    match tenant_dir(&app, &origin, &headers).await {
        Ok(dir) => file(&dir, &name, &f, true).await,
        Err(r) => r,
    }
}
