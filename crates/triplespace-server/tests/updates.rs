//! The SPARQL Update stream end to end (0032 §1–§6; 02 §6): the contract is that a dump,
//! with every event after its cursor applied in order, is the dump taken at the later
//! cursor. Items are written through the Action API, the stream is turned on, a dump is
//! taken, more edits follow, and the first dump with the stream applied is compared, line
//! for line, with a second dump, for the resolved view and for the local graph. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;
use triplespace_rdf::updates;

const DB: &str = "tsupdates_test";
const TENANT: &str = "streamtest";
const HOST: &str = "stream.example";
const REST: &str = "/w/rest.php/triplespace/v0";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {DB}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(DB);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{DB}"))
        .unwrap()
}

async fn setup() -> Option<(String, PathBuf, PathBuf)> {
    admin_config()?;
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsupdates-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    let key_file = dir.join("triplespace.key");
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: Some(1),
        owner_name: "Librarybase".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    Some((database, key_file, dir))
}

struct Client {
    router: Router,
    cookie: Option<String>,
}

impl Client {
    async fn call(&mut self, method: &str, uri: &str, form: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let req = match form {
            Some(f) => req
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(f.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        if let Some(sc) = response.headers().get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap_or_default()
                .to_string();
            self.cookie = (!value.is_empty()).then_some(value);
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&mut self, query: &str) -> Value {
        self.call(
            "GET",
            &format!("/w/api.php?{query}&format=json&formatversion=2"),
            None,
        )
        .await
        .1
    }

    async fn post(&mut self, query: &str, form: &str) -> Value {
        self.call(
            "POST",
            &format!("/w/api.php?{query}&format=json&formatversion=2"),
            Some(form),
        )
        .await
        .1
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// One file of the newest dump, as the set of its lines, without comments and without
/// the dump's own header node, whose date is the dump's.
fn dump_lines(dumps: &Path, file: &str) -> (Option<String>, BTreeSet<String>) {
    use std::io::Read as _;
    let dir = dumps.join(TENANT);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| dir.join(n).join("manifest.json").exists())
        .collect();
    names.sort();
    let newest = dir.join(names.last().expect("a dump"));
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(newest.join("manifest.json")).unwrap()).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(newest.join(file)).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("<http://wikiba.se/ontology#Dump>"))
        .map(str::to_string)
        .collect();
    (manifest["cursor"].as_str().map(str::to_string), lines)
}

async fn dump(database: &str, dumps: &Path) {
    // A dump is named by its second.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    triplespace_cli::dump::run(triplespace_cli::dump::Dump {
        database: database.into(),
        replica: None,
        tenant: TENANT.into(),
        dir: dumps.to_path_buf(),
        keep: Some(5),
    })
    .await
    .unwrap();
}

// 0032 §1, §2, §3, §6, §7; 02 §6.1: the stream is the dump, kept current.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_dump_with_the_stream_applied_is_the_later_dump() {
    let Some((database, key_file, dir)) = setup().await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let dumps = dir.join("dumps");
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(&key_file).unwrap();
    let app = App::new(
        triplespace_db::scratch::pools(&database, 4),
        Secret::derive(&signing.to_bytes()),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm,
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let mut api = Client {
        router: router_with(app.clone(), triplespace_api_rest::routes()),
        cookie: None,
    };
    let tokens = api.get("action=query&meta=tokens&type=login").await;
    let login = tokens["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = api
        .post(
            "action=clientlogin",
            &format!(
                "username=Librarybase&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
                enc(&login)
            ),
        )
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let csrf = api.get("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();
    let session = api.cookie.clone().expect("a session");
    let mut write = async |target: &str, data: Value| -> Value {
        let out = api
            .post(
                "action=wbeditentity",
                &format!(
                    "{target}&data={}&token={}",
                    enc(&data.to_string()),
                    enc(&csrf)
                ),
            )
            .await;
        assert_eq!(out["success"], 1, "{out}");
        out["entity"].clone()
    };
    let snak = |property: &str, value: &str| {
        serde_json::json!({"snaktype": "value", "property": property,
            "datavalue": {"value": value, "type": "string"}})
    };
    let reference = |property: &str| serde_json::json!([{"snaks": {property: [snak(property, "a shared source")]}}]);

    // Before the stream: a property and an item with a statement, an unknown value and a
    // reference.
    let p = write(
        "new=property",
        serde_json::json!({"labels": {"en": {"language": "en", "value": "note"}}, "datatype": "string"}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let q1 = write(
        "new=item",
        serde_json::json!({
            "labels": {"en": {"language": "en", "value": "First"}},
            "claims": [
                {"mainsnak": snak(&p, "one"), "type": "statement", "rank": "normal",
                 "references": reference(&p)},
                {"mainsnak": {"snaktype": "somevalue", "property": p}, "type": "statement", "rank": "normal"},
            ],
        }),
    )
    .await;
    let q1_id = q1["id"].as_str().unwrap().to_string();
    app.work_until_idle(100_000).await.unwrap();

    // Off: a dump has no cursor, and no delta was stored.
    dump(&database, &dumps).await;
    assert_eq!(dump_lines(&dumps, "wikibase-skolem.nt.gz").0, None);
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    let deltas = async |c: &tokio_postgres::Client| -> i64 {
        c.query_one("SELECT count(*) FROM view.rdf_delta", &[])
            .await
            .unwrap()
            .get(0)
    };
    assert_eq!(deltas(&client).await, 0);

    // On: the fill records what the graphs show, without deltas, and a dump taken now
    // carries the cursor the stream continues from.
    // The tenant's setting turns it on (02 §6.2), and the shared stream with it.
    assert!(updates::reconcile(&client).await.unwrap().is_empty());
    let settings = async |method: &str, body: Option<String>| -> Value {
        let req = Request::builder()
            .method(method)
            .uri(format!("{REST}/settings"))
            .header(header::HOST, HOST)
            .header(header::COOKIE, format!("triplespace_session={session}"))
            .header(header::CONTENT_TYPE, "application/json");
        let r = router_with(app.clone(), triplespace_api_rest::routes())
            .oneshot(req.body(body.map_or_else(Body::empty, Body::from)).unwrap())
            .await
            .unwrap();
        let status = r.status();
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert!(status.is_success(), "{status}: {v}");
        v
    };
    let base_offset = settings("GET", None).await["base"].clone();
    settings(
        "PATCH",
        Some(
            serde_json::json!({"set": {"updates.enabled": true}, "base": base_offset, "token": csrf})
                .to_string(),
        ),
    )
    .await;
    let mut turned_on = updates::reconcile(&client).await.unwrap();
    turned_on.sort();
    assert_eq!(turned_on, [String::new(), TENANT.to_string()]);
    assert!(
        updates::reconcile(&client).await.unwrap().is_empty(),
        "settled"
    );
    assert_eq!(
        updates::head(&client, TENANT).await.unwrap(),
        None,
        "not filled yet"
    );
    app.work_until_idle(100_000).await.unwrap();
    assert_eq!(deltas(&client).await, 0);
    let head = updates::head(&client, TENANT)
        .await
        .unwrap()
        .expect("filled");
    assert_eq!(head.cursor.epoch, 1);
    dump(&database, &dumps).await;
    let (cursor, mut resolved) = dump_lines(&dumps, "wikibase-skolem.nt.gz");
    let (_, mut local) = dump_lines(&dumps, "local-skolem.nq.gz");
    let from = updates::Cursor::parse(&cursor.expect("a stamped dump")).unwrap();
    assert_eq!(from, head.cursor);
    assert!(
        resolved
            .iter()
            .any(|l| l.contains("https://scatter.red/genid/")),
        "the unknown value is a skolem IRI"
    );

    // Edits after the dump: a label changed; the referenced statement removed from the
    // first item while a second item still cites the same reference; a third item made.
    let _ = write(
        &format!("id={q1_id}"),
        serde_json::json!({"labels": {"en": {"language": "en", "value": "First, renamed"}}}),
    )
    .await;
    let _ = write(
        "new=item",
        serde_json::json!({
            "labels": {"en": {"language": "en", "value": "Second"}},
            "claims": [{"mainsnak": snak(&p, "two"), "type": "statement", "rank": "normal",
                        "references": reference(&p)}],
        }),
    )
    .await;
    let first_statement = q1["claims"][&p][0]["id"].as_str().unwrap().to_string();
    let _ = write(
        &format!("id={q1_id}"),
        serde_json::json!({"claims": [{"id": first_statement, "remove": ""}]}),
    )
    .await;
    let _ = write(
        "new=item",
        serde_json::json!({"labels": {"en": {"language": "en", "value": "Third"}}}),
    )
    .await;
    app.work_until_idle(100_000).await.unwrap();

    // The stream from the dump's cursor, applied in order.
    let base = format!("https://{HOST}/");
    let graphs: Vec<String> = updates::GRAPHS.iter().map(|g| (*g).to_string()).collect();
    let events = updates::events(&client, TENANT, &base, &graphs, from.seq, 1000)
        .await
        .unwrap();
    // An event per entity changed: the first item's two edits were both queued when the
    // consumer ran, and it shows each entity once, as it stands.
    assert_eq!(events.len(), 3, "{events:#?}");
    assert!(events.windows(2).all(|w| w[0].seq < w[1].seq));
    let quad = |l: &str| {
        format!(
            "{} <{base}graph/local> .",
            l.strip_suffix(" .").expect("an N-Triples line")
        )
    };
    for event in &events {
        for (graph, delta) in &event.parts {
            for l in &delta.deleted {
                if graph == "resolved" {
                    resolved.remove(l);
                } else {
                    local.remove(&quad(l));
                }
            }
            for l in &delta.inserted {
                if graph == "resolved" {
                    resolved.insert(l.clone());
                } else {
                    local.insert(quad(l));
                }
            }
        }
        // One request per event, with a `GRAPH` clause since two graphs are subscribed.
        let request = event.request(&base, &graphs);
        assert!(request.contains("INSERT DATA {") || request.contains("DELETE DATA {"));
        assert!(
            request.contains(&format!("GRAPH <{base}graph/")),
            "{request}"
        );
        assert!(!request.contains("_:"), "no blank node: {request}");
    }

    dump(&database, &dumps).await;
    let (later, want) = dump_lines(&dumps, "wikibase-skolem.nt.gz");
    let (_, want_local) = dump_lines(&dumps, "local-skolem.nq.gz");
    let missing: Vec<_> = want.difference(&resolved).collect();
    let extra: Vec<_> = resolved.difference(&want).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "resolved: missing {missing:#?}; extra {extra:#?}"
    );
    let missing: Vec<_> = want_local.difference(&local).collect();
    let extra: Vec<_> = local.difference(&want_local).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "local: missing {missing:#?}; extra {extra:#?}"
    );
    assert_eq!(
        updates::Cursor::parse(&later.unwrap()).unwrap().seq,
        events.last().unwrap().seq,
        "the later dump is stamped where the stream ends"
    );
    // The reference both items cited is still there: the first item's delta did not
    // delete a node the second still shows (0032 A17).
    assert!(want.iter().any(|l| l.contains("/reference/")));

    // The routes (02 §6.4): the head, the polled document, the event stream, and `410`
    // for a cursor the stream cannot be resumed from.
    let router = router_with(app.clone(), triplespace_api_rest::routes());
    let fetch = async |uri: String, last: Option<&str>| {
        let mut req = Request::builder().uri(uri).header(header::HOST, HOST);
        if let Some(id) = last {
            req = req.header("last-event-id", id);
        }
        router
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
    };
    let text = async |r: axum::response::Response| {
        String::from_utf8(r.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    };
    let r = fetch(format!("{REST}/updates/cursor"), None).await;
    assert_eq!(r.status(), StatusCode::OK);
    let head: Value = serde_json::from_str(&text(r).await).unwrap();
    assert_eq!(head["cursor"], format!("1:{}", events.last().unwrap().seq));

    let r = fetch(format!("{REST}/updates?from={from}"), None).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        r.headers()[header::CONTENT_TYPE],
        "application/sparql-update"
    );
    // The next cursor is the last delta of the graph subscribed to, and nothing follows it.
    let next = r.headers()["updates-cursor"].to_str().unwrap().to_string();
    assert!(updates::Cursor::parse(&next).unwrap().seq > from.seq);
    assert!(
        r.headers()[header::LINK]
            .to_str()
            .unwrap()
            .contains("rel=\"next\"")
    );
    let document = text(r).await;
    assert_eq!(
        document.matches("\n# cursor ").count() + 1,
        events.len(),
        "{document}"
    );
    assert!(document.starts_with("# cursor 1:"), "{document}");
    assert!(
        document.contains(&format!("; tenant {TENANT}; entities ")),
        "{document}"
    );
    assert!(
        !document.contains("GRAPH <"),
        "one graph by default: {document}"
    );
    assert!(document.contains("\"First, renamed\"@en"), "{document}");

    // Caught up: an empty document and the same cursor.
    let r = fetch(format!("{REST}/updates?from={next}"), None).await;
    assert_eq!(r.headers()["updates-cursor"], next.as_str());
    assert_eq!(text(r).await, "");

    for (uri, status) in [
        (format!("{REST}/updates?from=0:0"), StatusCode::GONE),
        (format!("{REST}/updates?from=1:0"), StatusCode::GONE),
        (format!("{REST}/updates"), StatusCode::BAD_REQUEST),
        (
            format!("{REST}/updates?from={from}&graphs=full"),
            StatusCode::BAD_REQUEST,
        ),
        (format!("{REST}/updates/stream?from=7:0"), StatusCode::GONE),
    ] {
        let r = fetch(uri.clone(), None).await;
        assert_eq!(r.status(), status, "{uri}");
        if status == StatusCode::GONE {
            let body: Value = serde_json::from_str(&text(r).await).unwrap();
            assert_eq!(body["code"], "ts-updates-gone");
            assert_eq!(body["dumps"], format!("https://{HOST}/dumps/"));
        }
    }

    // The event stream resumes from `Last-Event-ID`: its first event is the second.
    let r = fetch(
        format!("{REST}/updates/stream?graphs=resolved,local"),
        Some(&format!("1:{}", events[0].seq)),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut body = r.into_body();
    let mut seen = String::new();
    while !seen.contains("\n\n") {
        let frame = body.frame().await.expect("a frame").unwrap();
        seen.push_str(&String::from_utf8_lossy(frame.data_ref().unwrap()));
    }
    assert!(seen.starts_with(": cursor "), "{seen}");
    assert!(
        seen.contains(&format!("\nid: 1:{}\n", events[1].seq)),
        "{seen}"
    );
    assert!(seen.contains("\nevent: update\n"), "{seen}");
    assert!(
        seen.contains("\ndata: INSERT DATA {") || seen.contains("\ndata: DELETE DATA {"),
        "{seen}"
    );
    drop(body);

    // `triplespace sparql-sync` (02 §6.4): from the first dump's cursor it posts the
    // stream to a store's update endpoint with the store's token, the marker triple with
    // it, and commits the cursor to its state file once the store has answered.
    let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
    let store = {
        let received = received.clone();
        Router::new().route(
            "/update",
            axum::routing::post(move |headers: axum::http::HeaderMap, body: String| {
                let received = received.clone();
                async move {
                    let auth = headers
                        .get(header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    received.lock().unwrap().push((auth, body));
                    StatusCode::NO_CONTENT
                }
            }),
        )
    };
    // The instance as a consumer reaches it: by its own host name.
    let instance = router_with(app.clone(), triplespace_api_rest::routes()).layer(
        axum::middleware::map_request(|mut req: Request<Body>| async move {
            req.headers_mut()
                .insert(header::HOST, header::HeaderValue::from_static(HOST));
            req
        }),
    );
    let mut bases = Vec::new();
    for service in [instance, store] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        bases.push(format!("http://{}", listener.local_addr().unwrap()));
        tokio::spawn(async move {
            let _ = axum::serve(listener, service).await;
        });
    }
    let state = dir.join("store.cursor");
    let token_file = dir.join("store.token");
    std::fs::write(&token_file, "store-secret\n").unwrap();
    let sync = |from: Option<String>| triplespace_cli::sparql_sync::SparqlSync {
        source: bases[0].clone(),
        graphs: "resolved".into(),
        endpoint: format!("{}/update", bases[1]),
        token_file: Some(token_file.clone()),
        state: state.clone(),
        from,
        limit: 1000,
        interval: 1,
        once: true,
    };
    assert!(
        triplespace_cli::sparql_sync::run(sync(None)).await.is_err(),
        "no state file and no --from"
    );
    let reached = triplespace_cli::sparql_sync::run(sync(Some(from.to_string())))
        .await
        .unwrap();
    assert_eq!(reached, next);
    assert_eq!(std::fs::read_to_string(&state).unwrap().trim(), next);
    {
        let got = received.lock().unwrap();
        assert_eq!(got.len(), 1, "one batch");
        assert_eq!(got[0].0, "Bearer store-secret");
        assert!(got[0].1.contains("\"First, renamed\"@en"), "{}", got[0].1);
        assert!(
            got[0].1.trim_end().ends_with(&format!(
                "INSERT DATA {{ <{}/.well-known/query> <https://scatter.red/terms/v0/cursor> \"{next}\" }}",
                bases[0]
            )),
            "{}",
            got[0].1
        );
    }
    // Run again: it resumes from the state file and has nothing to send.
    triplespace_cli::sparql_sync::run(sync(None)).await.unwrap();
    assert_eq!(received.lock().unwrap().len(), 1);
    // A cursor of another epoch stops it, naming the dumps.
    std::fs::write(&state, "0:0\n").unwrap();
    let gone = triplespace_cli::sparql_sync::run(sync(None))
        .await
        .unwrap_err();
    assert!(gone.to_string().contains("/dumps/"), "{gone}");

    // The retention sweep (03 §4.11): nothing is thirty days old; with no retention at
    // all every delta goes, and the stream cannot be resumed from before them.
    assert_eq!(updates::sweep(&client).await.unwrap(), 0);
    assert!(
        updates::sweep_tenant(&client, TENANT, "PT0S")
            .await
            .unwrap()
            > 0
    );
    let swept = updates::head(&client, TENANT).await.unwrap().unwrap();
    assert_eq!(swept.horizon, events.last().unwrap().seq);
    let r = fetch(format!("{REST}/updates?from={from}"), None).await;
    assert_eq!(r.status(), StatusCode::GONE);

    // Off again: nothing is stored and nothing more is queued.
    updates::set_enabled(&client, TENANT, false).await.unwrap();
    assert_eq!(updates::head(&client, TENANT).await.unwrap(), None);
    assert_eq!(deltas(&client).await, 0);
    let r = fetch(format!("{REST}/updates/cursor"), None).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}
