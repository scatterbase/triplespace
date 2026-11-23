//! Reading what the instance does not hold (0070 §2.1, §4, A13; 05 §5.2, §5.6): a read of
//! a source entity no graph holds, through `wbgetentities` or the REST entity routes, is
//! answered as missing and queues the entity with the reason `read`, unless the tenant's
//! `entities.mirror` turns the type off; and `GET /labels` names such an entity from the
//! label cache, which asks the source's wiki once and keeps the answer. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::{instance, settings, source};

const DB: &str = "tsondemand_test";
const TENANT: &str = "reader";
const HOST: &str = "reader.example";

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

async fn strings(database: &str, sql: &str) -> Vec<String> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client
        .query(&format!("SELECT * FROM ({sql}) q"), &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, Option<String>>(0).unwrap_or_default())
        .collect()
}

/// What the stub wiki was asked: each request's line.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A wiki whose `wbgetentities` knows `Q9`, labelled "Nine" in English, and reports any
/// other ID missing.
async fn wiki() -> (String, Asked) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let asked = Asked::default();
    let noted = asked.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let noted = noted.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    request.extend_from_slice(&chunk[..n]);
                    if n == 0 || request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let line = text.lines().next().unwrap_or_default().to_string();
                noted.lock().unwrap().push(line.clone());
                let ids = line
                    .split(['?', '&', ' '])
                    .find_map(|p| p.strip_prefix("ids="))
                    .unwrap_or_default()
                    .replace("%7C", "|");
                let mut entities = serde_json::Map::new();
                for id in ids.split('|').filter(|i| !i.is_empty()) {
                    let e = if id == "Q9" {
                        json!({"type": "item", "id": "Q9",
                            "labels": {"en": {"language": "en", "value": "Nine"}}})
                    } else {
                        json!({"id": id, "missing": ""})
                    };
                    entities.insert(id.to_string(), e);
                }
                let answer = json!({"success": 1, "entities": entities}).to_string();
                let _ = socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                             content-length: {}\r\nconnection: close\r\n\r\n{answer}",
                            answer.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    (format!("http://{address}/w/api.php"), asked)
}

async fn get(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

// 0070 §2.1, §4, A13.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_read_queues_what_is_not_held_and_labels_come_from_the_cache() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsondemand-{}", std::process::id()));
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
        owner_name: "Reader".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    let (api, asked) = wiki().await;
    source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: source::At {
                database: database.clone(),
                tenant: TENANT.into(),
            },
            name: "lb".into(),
            reason: "the test".into(),
        },
        label: "Librarybase".into(),
        types: vec!["Q=item".into(), "P=property".into()],
        iri: "https://lb.example/entity/{upstream_id}".into(),
        api: Some(api.clone()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
        namespace: None,
        namespace_aliases: vec![],
    }))
    .await
    .expect("source add");
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
    let router = router_with(app, triplespace_api_rest::routes());
    let queue = || {
        strings(
            &database,
            "SELECT entity_id || ' ' || reason FROM ops.entity_fetch
             WHERE tenant = 'reader' AND provider = 'lb' ORDER BY entity_id",
        )
    };

    // Read through the Action API and the REST entity routes: missing, and queued.
    let (_, v) = get(
        &router,
        "/w/api.php?action=wbgetentities&ids=lb:Q7&format=json&formatversion=2",
    )
    .await;
    assert!(v["entities"]["lb:Q7"].get("missing").is_some(), "{v}");
    let (status, _) = get(&router, "/w/rest.php/triplespace/v0/entity/lb:Q8/page").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(queue().await, ["Q7 read", "Q8 read"]);

    // Labels of entities no graph holds come from the label cache: asked once.
    let labels = "/w/rest.php/triplespace/v0/labels?ids=lb:Q9|lb:Q404&language=de|en";
    let (status, v) = get(&router, labels).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["labels"]["lb:Q9"]["value"], "Nine", "{v}");
    assert_eq!(v["labels"]["lb:Q9"]["language"], "en", "{v}");
    assert!(v["labels"].get("lb:Q404").is_none(), "{v}");
    let label_requests = || {
        asked
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("props=labels"))
            .count()
    };
    assert_eq!(label_requests(), 1);
    let (_, again) = get(&router, labels).await;
    assert_eq!(again["labels"], v["labels"]);
    assert_eq!(label_requests(), 1, "the second answer came from the cache");

    // A type turned off is not queued by a read; its labels are still shown.
    settings::run(settings::Settings::Set(settings::Set {
        at: settings::Where {
            database: database.clone(),
            tenant: Some(TENANT.into()),
            instance: false,
        },
        key: "entities.mirror".into(),
        value: r#"{"lb": {"item": "off"}}"#.into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await
    .expect("set entities.mirror");
    let _ = get(
        &router,
        "/w/api.php?action=wbgetentities&ids=lb:Q10&format=json&formatversion=2",
    )
    .await;
    assert_eq!(
        queue().await,
        ["Q7 read", "Q8 read"],
        "`off` queues nothing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
