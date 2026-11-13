//! `POST /export` (0047 §8.1, §8.2; 21 §4.2, §4.4): an export of selected pages, streamed.
//!
//! Two formats. `xml` is MediaWiki XML, export schema 0.11, assembled by
//! `triplespace_rdf::xml` and streamed as `application/xml` while it is written, no
//! faster than the client takes it, a failure after the first byte breaking the transfer
//! rather than ending it: the
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

/// How many chunks may wait for the client: the writer stops reading the database while
/// this many are untaken, so a slow client holds a few chunks in memory, never the document.
const WAITING_CHUNKS: usize = 2;

/// Where a streamed body's writer puts its chunks.
struct Chunks(tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>);

impl Chunks {
    /// Hands the body its next chunk, waiting while the client has not taken the ones
    /// before it.
    async fn send(&self, chunk: Vec<u8>) -> Result<(), BuildError> {
        self.0
            .send(Ok(Bytes::from(chunk)))
            .await
            .map_err(|_| BuildError::Io("the client went away".into()))
    }
}

/// A response body that `write` fills chunk by chunk, each chunk waiting for the client
/// to take the ones before it. Where `write` fails the body fails with it, so the
/// transfer ends as a broken one (a chunked response without its last chunk, a reset
/// stream) and never as a complete response holding part of a document.
fn streamed<F, Fut>(write: F) -> Body
where
    F: FnOnce(Chunks) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), BuildError>> + Send,
{
    let (tx, rx) = tokio::sync::mpsc::channel(WAITING_CHUNKS);
    let failed = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = write(Chunks(tx)).await {
            let _ = failed.send(Err(std::io::Error::other(e.to_string()))).await;
        }
    });
    Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|chunk| (chunk, rx))
    }))
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
    // A failure mid-document fails the body: the transfer breaks, and what did arrive is
    // a document without its closing tag, which no importer accepts whole.
    let body = streamed(move |chunks| async move {
        xml::write_chunks(&*client, &planned, xml::CHUNK, |chunk| chunks.send(chunk)).await
    });
    let disposition = format!("attachment; filename=\"{}-{now}.xml\"", tenant.slug);
    let mut r = Response::new(body);
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    // 0047 §8.1: a failure after the first byte breaks the transfer; it never ends it.
    #[tokio::test]
    async fn a_failed_export_fails_its_body() {
        let whole = streamed(|chunks| async move {
            chunks.send(b"<mediawiki>".to_vec()).await?;
            chunks.send(b"</mediawiki>".to_vec()).await
        });
        assert_eq!(
            axum::body::to_bytes(whole, usize::MAX).await.unwrap(),
            "<mediawiki></mediawiki>"
        );
        let cut = streamed(|chunks| async move {
            chunks.send(b"<mediawiki><page>".to_vec()).await?;
            Err(BuildError::Database("the connection was lost".into()))
        });
        let e = axum::body::to_bytes(cut, usize::MAX).await.unwrap_err();
        assert!(
            format!("{e:?}").contains("the connection was lost"),
            "{e:?}"
        );
    }

    // 0047 §8.1: the export is written no faster than the client takes it.
    #[tokio::test]
    async fn a_slow_client_holds_the_writer_back() {
        let sent = Arc::new(AtomicUsize::new(0));
        let counter = sent.clone();
        let body = streamed(move |chunks| async move {
            for _ in 0..100 {
                chunks.send(vec![b'x'; 10]).await?;
                counter.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        });
        // Nobody reads the body: the writer gets as far as the waiting chunks and stops.
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(sent.load(Ordering::SeqCst), WAITING_CHUNKS);
        // Read, and the rest follows.
        let all = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(all.len(), 1000);
        assert_eq!(sent.load(Ordering::SeqCst), 100);
    }

    // The client going away ends the writer at its next chunk.
    #[tokio::test]
    async fn a_client_that_leaves_stops_the_writer() {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let body = streamed(move |chunks| async move {
            let r = async {
                loop {
                    chunks.send(vec![b'x'; 10]).await?;
                }
            }
            .await;
            let _ = done_tx.send(matches!(&r, Err(BuildError::Io(_))));
            r
        });
        drop(body);
        assert!(done_rx.await.unwrap());
    }
}
