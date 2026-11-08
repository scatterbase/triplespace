//! `POST /export` (0047 §8.1, §8.2; 21 §4.2, §4.4): an export of selected pages, streamed.
//!
//! Two formats. `xml` is MediaWiki XML, export schema 0.11, assembled by
//! `triplespace_rdf::xml` and streamed as `application/xml` while it is written: the
//! selected document pages, one `<revision>` per record, and the selected local entities
//! as `wikibase-item` and `wikibase-property` pages, one `<revision>` per local
//! revision with the canonical JSON of the local-graph state. `records` is the
//! verifiable format of 21 §4.4, which implements 0047 §8.2: the records with inclusion
//! proofs as one canonical-CBOR document that `triplespace verify --records` checks.
//!
//! The body is JSON: `{"format": "xml" | "records", "pages": [title, …] | "Title\nTitle",
//! "history": bool}`, and for `xml` also `"categories": [name, …]` (`addcat`) and, where
//! `export.from_namespaces` is on, `"namespaces": [number, …]` (`addns`), each adding up
//! to MediaWiki's page-list limit of pages, and `"templates": bool`, accepted and
//! answered with a notice until `view.transclusion` exists (0047 A33). A title in an
//! entity namespace (`Item:Q6`) names that local entity. `history` (off by default, as
//! `Special:Export`'s "current revision only" is on) is allowed only where
//! `export.history` is, and is capped by `export.max_history` per page and
//! `export.max_entity_history` per entity; a selection over `export.sync_revisions`
//! revisions is refused with `413 export-too-large` until export jobs exist. Export
//! needs only `read`; every record is shown as the export scope shows it to the caller
//! (01 §8), a hidden or erased part written as revision-deleted content, and a page or
//! entity the caller may not know of is listed as missing, as one that does not exist
//! is: `x-export-missing` counts them. `x-export-notice` carries the export's notices.
//! Mirrored pages, `templates` proper and files are not in the XML yet (21 §4.2).

use std::io::Write;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::pages::{self, Site};
use triplespace_projections::pages::TenantNames;
use triplespace_rdf::bundle::Reader;
use triplespace_rdf::records::{self, BuildError, MEDIA_TYPE};
use triplespace_rdf::xml;

use crate::entity_route::{error, internal};
use crate::settings::prelude;

fn setting_usize(snap: &triplespace_accounts::settings::Snapshot, key: &str, or: usize) -> usize {
    snap.value(key)
        .and_then(|v| v.as_u64())
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(or)
}

/// The request's string list: an array of strings, or one per line.
fn list_of(request: &Value, key: &str) -> Vec<String> {
    let raw: Vec<String> = match request.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => s.lines().map(str::to_string).collect(),
        _ => Vec::new(),
    };
    raw.into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// A sink that hands what is written to it to the response body, in chunks, through a
/// channel a task drains into the body's pipe.
struct ChunkSink {
    tx: tokio::sync::mpsc::UnboundedSender<Bytes>,
    buf: Vec<u8>,
}

impl ChunkSink {
    const CHUNK: usize = 64 * 1024;

    fn send(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let chunk = Bytes::from(std::mem::take(&mut self.buf));
        self.tx
            .send(chunk)
            .map_err(|_| std::io::Error::other("the client went away"))
    }
}

impl Write for ChunkSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        if self.buf.len() >= Self::CHUNK {
            self.send()?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.send()
    }
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
    let format = match request.get("format").and_then(Value::as_str) {
        Some("records") => "records",
        Some("xml") | None => "xml",
        Some(other) => {
            return error(
                StatusCode::BAD_REQUEST,
                "unknown-format",
                format!("no export format `{other}`"),
            );
        }
    };
    let titles = list_of(&request, "pages");
    let categories = list_of(&request, "categories");
    let namespaces: Vec<i32> = list_of(&request, "namespaces")
        .iter()
        .filter_map(|n| n.parse().ok())
        .collect();
    if titles.is_empty() && categories.is_empty() && namespaces.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "no-pages",
            "name at least one page, category or namespace",
        );
    }
    let (snap, _) = match triplespace_accounts::settings::load(&**client, Some(&tenant.slug)).await
    {
        Ok(s) => s,
        Err(e) => return internal(&e),
    };
    let setting_bool =
        |key: &str, or: bool| snap.value(key).and_then(|v| v.as_bool()).unwrap_or(or);
    if !namespaces.is_empty() && !setting_bool("export.from_namespaces", false) {
        return error(
            StatusCode::FORBIDDEN,
            "export-namespaces-off",
            "exporting a whole namespace is off here (`export.from_namespaces`)",
        );
    }
    let history = setting_bool("export.history", true)
        && request
            .get("history")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let templates = request
        .get("templates")
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
    let max_page_history = setting_usize(&snap, "export.max_history", 1000);
    let max_entity_history = setting_usize(&snap, "export.max_entity_history", 100);
    let mut page_targets: Vec<(u64, String)> = Vec::new();
    let mut entity_targets: Vec<(String, String)> = Vec::new();
    let mut missing = Vec::new();
    for t in &titles {
        if let Some(id) = pages::entity_of_title(&site, t) {
            entity_targets.push((id.as_str().to_string(), t.clone()));
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
            Ok(Some(p)) => page_targets.push((p.page_id, t.clone())),
            Ok(None) => missing.push(t.clone()),
            Err(e) => return internal(&e.info),
        }
    }
    // `addcat` and `addns`: each adds the pages it lists, by title as the site writes it;
    // a page the caller may not know of is left out, as a listing leaves it out.
    for c in &categories {
        match xml::pages_in_category(&**client, &tenant.slug, c).await {
            Ok(found) => {
                for (id, ns, title) in found {
                    if !reader.viewer.hidden.page(ns, id) {
                        page_targets.push((id, site.prefixed(ns, &title)));
                    }
                }
            }
            Err(e) => return internal(&e),
        }
    }
    for ns in &namespaces {
        match xml::pages_in_namespace(&**client, &tenant.slug, *ns).await {
            Ok(found) => {
                for (id, ns, title) in found {
                    if !reader.viewer.hidden.page(ns, id) {
                        page_targets.push((id, site.prefixed(ns, &title)));
                    }
                }
            }
            Err(e) => return internal(&e),
        }
    }
    let limit = setting_usize(&snap, "export.sync_revisions", 5000);
    let now = triplespace_api_action::edit::now_micros();

    if format == "records" {
        let selection = records::Selection {
            pages: page_targets,
            entities: entity_targets,
            history,
            max_page_history,
            max_entity_history,
        };
        let mut export =
            match records::build(&**client, &tenant.slug, &reader, &selection, limit, now).await {
                Ok(x) => x,
                Err(e @ BuildError::TooLarge { .. }) => return too_large(&e),
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
        return r;
    }

    // MediaWiki XML: planned first, so a refusal comes before the body, then streamed.
    let names = match TenantNames::load(&client, &tenant.slug).await {
        Ok(mut n) => {
            n.site_name = Some(site.name.clone());
            n
        }
        Err(e) => return internal(&e),
    };
    let base = format!(
        "{}/wiki/{}",
        tenant.public_base.trim_end_matches('/'),
        triplespace_api_action::modules::query::MAIN_PAGE.replace(' ', "_")
    );
    let site_info = xml::site_info(
        &names,
        &site.lang,
        &site.name,
        &tenant.slug.replace('-', "_"),
        &base,
        &app.config().generator,
    );
    let selection = xml::Selection {
        pages: page_targets,
        entities: entity_targets,
        history,
        max_page_history,
        max_entity_history,
        templates,
    };
    let planned = match xml::plan(
        &**client,
        &tenant.slug,
        &reader,
        &selection,
        site_info,
        limit,
    )
    .await
    {
        Ok(p) => p,
        Err(e @ BuildError::TooLarge { .. }) => return too_large(&e),
        Err(e) => return internal(&e),
    };
    missing.extend(planned.missing.iter().cloned());
    let notice = planned.notices.join("; ");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Bytes>();
    let (mut pipe_in, pipe_out) = tokio::io::duplex(ChunkSink::CHUNK);
    tokio::spawn(async move {
        let sink = ChunkSink {
            tx,
            buf: Vec::new(),
        };
        // A failure mid-document cuts the body short: the reader sees a document
        // without its closing tag, which no importer accepts whole.
        if let Ok(mut sink) = xml::write(&*client, &planned, sink).await {
            let _ = sink.flush();
        }
    });
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        while let Some(chunk) = rx.recv().await {
            if pipe_in.write_all(&chunk).await.is_err() {
                break;
            }
        }
        let _ = pipe_in.shutdown().await;
    });
    let disposition = format!("attachment; filename=\"{}-{now}.xml\"", tenant.slug);
    let mut r = Response::new(Body::from_stream(tokio_util::io::ReaderStream::new(
        pipe_out,
    )));
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/xml; charset=utf-8"),
    );
    h.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    if let Ok(v) = header::HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert("x-export-missing", header::HeaderValue::from(missing.len()));
    if !notice.is_empty()
        && let Ok(v) = header::HeaderValue::from_str(&notice)
    {
        h.insert("x-export-notice", v);
    }
    r
}

fn too_large(e: &BuildError) -> Response {
    error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "export-too-large",
        format!("{e}; export jobs are not built yet, so narrow the selection"),
    )
}
