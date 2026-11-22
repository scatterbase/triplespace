//! The SPARQL Update stream's routes (02 §6.4; 0032 §6): `GET /updates/cursor`,
//! `GET /updates` for consumers that poll, and `GET /updates/stream` as server-sent
//! events. Each event is one composing transaction's deltas as one SPARQL 1.1 Update
//! request; its cursor is `{epoch}:{seq}`. The instance serves and never pushes.
//!
//! `graphs=` takes `resolved` (the default), `local`, or both; with one graph the triples
//! carry no `GRAPH` clause. A `from` of another epoch, or older than the horizon, is
//! `410 Gone` with the URL of the dumps, the newest of which continues into the window.
//! A tenant whose stream is off, or that an anonymous reader may not read, has none.
//!
//! **Not yet:** `graphs=full`, `metadata=` and mirror graphs; the record (`partition`,
//! `offset`, `revid`) in an event's provenance line, which names the tenant, the entities
//! and the time; the `412` for a consumer that loaded a dump's blank-node form, which the
//! instance cannot see; the `stream` rate class, which nothing in REST enforces yet.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_rdf::updates::{self, Cursor, Event, Head};

use crate::entity_route::{error, internal};
use crate::settings::prelude;

/// How often an idle stream looks for new events.
const POLL: Duration = Duration::from_secs(1);

/// What a request subscribes to, once checked.
struct Subscription {
    tenant: String,
    base: String,
    graphs: Vec<String>,
    head: Head,
}

#[allow(clippy::result_large_err)] // a ready response, as the async routes return
fn graphs_of(q: &BTreeMap<String, String>) -> Result<Vec<String>, Response> {
    let mut graphs: Vec<String> = Vec::new();
    for g in q
        .get("graphs")
        .map_or("resolved", String::as_str)
        .split(',')
    {
        if !updates::GRAPHS.contains(&g) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "ts-updates-graphs",
                format!("`graphs` takes `resolved` and `local`; `{g}` is not served yet"),
            ));
        }
        if !graphs.iter().any(|x| x == g) {
            graphs.push(g.to_string());
        }
    }
    Ok(graphs)
}

async fn subscription(
    app: &App,
    origin: &Origin,
    headers: &HeaderMap,
    q: &BTreeMap<String, String>,
) -> Result<Subscription, Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let (tenant, _caller) = prelude(app, &client, origin, headers).await?;
    let graphs = graphs_of(q)?;
    let head = updates::head(&**client, &tenant.slug)
        .await
        .map_err(|e| internal(&e))?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "ts-updates-off",
                "this wiki serves no update stream",
            )
        })?;
    Ok(Subscription {
        tenant: tenant.slug.clone(),
        base: tenant.base.clone(),
        graphs,
        head,
    })
}

/// Where a request resumes: `from`, or the SSE `Last-Event-ID`; refused with `410` when
/// the stream cannot be resumed there.
#[allow(clippy::result_large_err)] // a ready response, as the async routes return
fn resume(sub: &Subscription, given: Option<&str>) -> Result<Cursor, Response> {
    let Some(given) = given else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "ts-updates-from",
            "`from` is the cursor of the dump loaded, `{epoch}:{seq}`",
        ));
    };
    let Some(from) = Cursor::parse(given) else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "ts-updates-from",
            "a cursor is `{epoch}:{seq}`",
        ));
    };
    if from.epoch != sub.head.cursor.epoch || from.seq < sub.head.horizon {
        let dumps = format!("{}/dumps/", sub.base.trim_end_matches('/'));
        let body = json!({
            "code": "ts-updates-gone",
            "message": "the stream cannot be resumed from this cursor; load the newest dump and follow from its cursor",
            "dumps": dumps,
            "cursor": sub.head.cursor.to_string(),
        });
        return Err((
            StatusCode::GONE,
            [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
            body.to_string(),
        )
            .into_response());
    }
    Ok(from)
}

/// An event's provenance, for its comment line: nothing of it enters the triples.
fn provenance(sub: &Subscription, event: &Event) -> String {
    format!(
        "cursor {}:{}; tenant {}; entities {}; time {}",
        sub.head.cursor.epoch,
        event.seq,
        sub.tenant,
        event.entities.join(" "),
        event.time
    )
}

/// `GET /updates/cursor`.
pub async fn cursor(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
) -> Response {
    let sub = match subscription(&app, &origin, &headers, &BTreeMap::new()).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let body = json!({
        "epoch": sub.head.cursor.epoch,
        "seq": sub.head.cursor.seq,
        "cursor": sub.head.cursor.to_string(),
        "horizon": sub.head.horizon,
        "graphs": updates::GRAPHS,
    });
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body.to_string(),
    )
        .into_response()
}

/// `GET /updates`.
pub async fn poll(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let sub = match subscription(&app, &origin, &headers, &q).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let from = match resume(&sub, q.get("from").map(String::as_str)) {
        Ok(f) => f,
        Err(r) => return r,
    };
    let limit: i64 = q
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
        .clamp(1, 10_000);
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let events = match updates::events(
        &**client,
        &sub.tenant,
        &sub.base,
        &sub.graphs,
        from.seq,
        limit,
    )
    .await
    {
        Ok(e) => e,
        Err(e) => return internal(&e),
    };
    let mut body = String::new();
    for event in &events {
        body.push_str("# ");
        body.push_str(&provenance(&sub, event));
        body.push('\n');
        body.push_str(&event.request(&sub.base, &sub.graphs));
        body.push_str(" ;\n");
    }
    let next = Cursor {
        epoch: from.epoch,
        seq: events.last().map_or(from.seq, |e| e.seq),
    };
    let link = format!(
        "<{}/rest.php/triplespace/v0/updates?graphs={}&from={next}>; rel=\"next\"",
        sub.base.trim_end_matches('/'),
        sub.graphs.join(",")
    );
    (
        [
            (
                header::CONTENT_TYPE,
                "application/sparql-update".to_string(),
            ),
            (header::LINK, link),
            (
                header::HeaderName::from_static("updates-cursor"),
                next.to_string(),
            ),
        ],
        body,
    )
        .into_response()
}

/// `GET /updates/stream`.
pub async fn stream(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let sub = match subscription(&app, &origin, &headers, &q).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let given = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .or_else(|| q.get("from").map(String::as_str));
    let from = match resume(&sub, given) {
        Ok(f) => f,
        Err(r) => return r,
    };
    // One page of events at a time; when there are none, wait and look again. The stream
    // ends on a database failure, and the consumer resumes from its last event's ID.
    let state = (
        app,
        sub,
        from.seq,
        std::collections::VecDeque::<Event>::new(),
    );
    let events = futures_util::stream::unfold(state, |(app, sub, mut seq, mut queue)| async move {
        loop {
            if let Some(event) = queue.pop_front() {
                seq = event.seq;
                let sse = SseEvent::default()
                    .comment(provenance(&sub, &event))
                    .id(format!("{}:{}", sub.head.cursor.epoch, event.seq))
                    .event("update")
                    .data(event.request(&sub.base, &sub.graphs));
                return Some((Ok::<_, Infallible>(sse), (app, sub, seq, queue)));
            }
            let client = app.pool().get().await.ok()?;
            let page = updates::events(&**client, &sub.tenant, &sub.base, &sub.graphs, seq, 500)
                .await
                .ok()?;
            drop(client);
            if page.is_empty() {
                tokio::time::sleep(POLL).await;
            }
            queue.extend(page);
        }
    });
    Sse::new(events)
        .keep_alive(KeepAlive::default())
        .into_response()
}
