//! `POST /export` (0047 §8.1, §11; 21 §4.2): an export of selected pages, streamed. The
//! `records` format is built, which implements 0047 §8.2 (21 §4.4): the selected pages'
//! and local entities' records, each with an inclusion proof against its partition's
//! latest checkpoint, the checkpoint and the key records that check it, as one
//! canonical-CBOR document (`scatter_integrity::records`) that `triplespace verify
//! --records` checks. The MediaWiki XML format, `Special:Export` and export jobs are not
//! built yet.
//!
//! The body is JSON: `{"format": "records", "pages": [title, …] | "Title\nTitle",
//! "history": bool}`. A title in an entity namespace (`Item:Q6`) names that local entity.
//! `history` (off by default, as `Special:Export`'s "current revision only" is on) is
//! allowed only where `export.history` is, and is capped by `export.max_history` per page
//! and `export.max_entity_history` per entity; a selection over `export.sync_revisions`
//! records is refused until export jobs exist. Export needs only `read`; every record is
//! shown as the export scope shows it to the caller (01 §8), and a page or entity the
//! caller may not know of is listed in `missing`, as one that does not exist is.

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::pages::{self, Site};
use triplespace_rdf::bundle::Reader;
use triplespace_rdf::records::{BuildError, MEDIA_TYPE, Selection, build};

use crate::entity_route::{error, internal};
use crate::settings::prelude;

fn setting_usize(snap: &triplespace_accounts::settings::Snapshot, key: &str, or: usize) -> usize {
    snap.value(key)
        .and_then(|v| v.as_u64())
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(or)
}

/// `POST /export`.
#[allow(clippy::too_many_lines)]
pub async fn post(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Ok(request) = serde_json::from_slice::<Value>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid-json",
            "the body is a JSON object",
        );
    };
    match request.get("format").and_then(Value::as_str) {
        Some("records") => {}
        Some("xml") | None => {
            return error(
                StatusCode::NOT_IMPLEMENTED,
                "not-built",
                "only the `records` format is built",
            );
        }
        Some(other) => {
            return error(
                StatusCode::BAD_REQUEST,
                "unknown-format",
                format!("no export format `{other}`"),
            );
        }
    }
    let titles: Vec<String> = match request.get("pages") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s.lines().map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let titles: Vec<String> = titles
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if titles.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "no-pages",
            "name at least one page",
        );
    }
    let (snap, _) = match triplespace_accounts::settings::load(&**client, Some(&tenant.slug)).await
    {
        Ok(s) => s,
        Err(e) => return internal(&e),
    };
    let allowed = snap
        .value("export.history")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let history = allowed
        && request
            .get("history")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let site = match Site::load_for(&client, &tenant.slug).await {
        Ok(s) => s,
        Err(e) => return internal(&e.info),
    };
    let Ok(reader) = Reader::new(&**client, &tenant.slug, caller.effective.clone()).await else {
        return error(
            StatusCode::NOT_FOUND,
            "tenant-not-readable",
            "nothing of this tenant can be exported to you",
        );
    };
    let mut selection = Selection {
        history,
        max_page_history: setting_usize(&snap, "export.max_history", 1000),
        max_entity_history: setting_usize(&snap, "export.max_entity_history", 100),
        ..Selection::default()
    };
    let mut missing = Vec::new();
    for t in &titles {
        if let Some(id) = pages::entity_of_title(&site, t) {
            selection
                .entities
                .push((id.as_str().to_string(), t.clone()));
            continue;
        }
        let Some(title) = pages::resolve_title(&site, t) else {
            missing.push(t.clone());
            continue;
        };
        match pages::by_title(
            &client,
            &tenant.slug,
            &reader.viewer.hidden,
            title.namespace,
            &title.text,
        )
        .await
        {
            Ok(Some(p)) => selection.pages.push((p.page_id, t.clone())),
            Ok(None) => missing.push(t.clone()),
            Err(e) => return internal(&e.info),
        }
    }
    let limit = setting_usize(&snap, "export.sync_revisions", 5000);
    let now = triplespace_api_action::edit::now_micros();
    let mut export = match build(&**client, &tenant.slug, &reader, &selection, limit, now).await {
        Ok(x) => x,
        Err(e @ BuildError::TooLarge { .. }) => {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "export-too-large",
                format!("{e}; export jobs are not built yet, so narrow the selection"),
            );
        }
        Err(e) => return internal(&e),
    };
    missing.append(&mut export.missing);
    export.missing = missing;
    let bytes = export.encode();
    let disposition = format!(
        "attachment; filename=\"{}-records-{now}.cbor\"",
        tenant.slug
    );
    let mut r = (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, MEDIA_TYPE.to_string()),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response();
    r.headers_mut().insert(
        "x-export-missing",
        header::HeaderValue::from(export.missing.len()),
    );
    r
}
