//! The fetch queue (0070 §3, A5, A9): entities queued for a tenant's entity source are
//! read by its follower's poll from a stub wiki and written to `source/{name}`; one the
//! wiki reports missing writes nothing; the rows go as the batch commits; and a source
//! with nothing loaded and no position is followed from now once entities wait. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use triplespace_cli::{fetch, follow, instance, source};

const DB: &str = "tsfetch_test";
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

fn item(id: &str, label: &str, revid: u64) -> Value {
    json!({"type": "item", "id": id, "labels": {"en": {"language": "en", "value": label}},
        "descriptions": {}, "aliases": {}, "claims": {}, "sitelinks": {},
        "lastrevid": revid, "modified": "2026-11-20T00:00:00Z",
        "pageid": revid, "ns": 120, "title": format!("Item:{id}")})
}

/// What the stub wiki was asked: each request's line.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A wiki with no recent changes whose `wbgetentities` knows `Q1` and `Q2`, fails any
/// request that names `Q666`, and reports any other ID missing.
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
                let answer = if line.contains("wbgetentities") && line.contains("Q666") {
                    json!({"error": {"code": "internal_api_error", "info": "boom"}})
                } else if line.contains("wbgetentities") {
                    let ids = line
                        .split(['?', '&', ' '])
                        .find_map(|p| p.strip_prefix("ids="))
                        .unwrap_or_default()
                        .replace("%7C", "|");
                    let mut entities = serde_json::Map::new();
                    for id in ids.split('|') {
                        let e = match id {
                            "Q1" => item("Q1", "One", 11),
                            "Q2" => item("Q2", "Two", 12),
                            other => json!({"id": other, "missing": ""}),
                        };
                        entities.insert(id.to_string(), e);
                    }
                    json!({"success": 1, "entities": entities})
                } else {
                    json!({
                        "batchcomplete": true,
                        "curtimestamp": "2026-11-21T00:00:00Z",
                        "query": {
                            "recentchanges": [],
                            "namespaces": {
                                "120": {"id": 120, "canonical": "Item", "defaultcontentmodel": "wikibase-item"},
                                "122": {"id": 122, "canonical": "Property", "defaultcontentmodel": "wikibase-property"},
                            },
                        },
                    })
                }
                .to_string();
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

// 0070 §3, A5, A9.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn queued_entities_are_fetched_by_the_followers_poll() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsfetch-{}", std::process::id()));
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
        iri: "https://librarybase.org/entity/{upstream_id}".into(),
        api: Some(api.clone()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
    }))
    .await
    .expect("source add");

    let follow_args = || follow::Follow {
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        once: true,
        from: None,
        interval: None,
        batch: 50,
        key_file: key_file.clone(),
        anonymous: true,
        api: None,
    };
    // Nothing loaded, no position, nothing queued: nowhere to start.
    let refused = follow::run(follow_args()).await.unwrap_err().to_string();
    assert!(refused.contains("no position"), "{refused}");

    // Queued in both forms, once each; an entity of no source is refused.
    let queue = |ids: &[&str]| fetch::Fetch {
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        ids: ids.iter().map(ToString::to_string).collect(),
    };
    fetch::run(queue(&["Q1", "lb:Q2", "Q404", "lb:Q1", "Q666"]))
        .await
        .expect("fetch");
    assert_eq!(
        strings(
            &database,
            "SELECT entity_id || ' ' || reason FROM ops.entity_fetch
             WHERE tenant = 'reader' AND provider = 'lb' ORDER BY entity_id"
        )
        .await,
        ["Q1 manual", "Q2 manual", "Q404 manual", "Q666 manual"]
    );
    assert!(fetch::run(queue(&["xx:Q3"])).await.is_err());

    // The follower starts from now, and its poll fetches them.
    follow::run(follow_args()).await.expect("follow");
    assert!(
        asked
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("wbgetentities")),
        "the queue was read through wbgetentities"
    );
    assert_eq!(
        strings(
            &database,
            "SELECT entity_id FROM view.entity_source
             WHERE tenant = 'reader' AND graph = 'source/lb' ORDER BY entity_id"
        )
        .await,
        ["lb:Q1", "lb:Q2"],
        "the two the wiki has; the missing one writes nothing (A5)"
    );
    // The request that named `Q666` failed: the rest were asked one at a time, and
    // `Q666` alone stays queued with an attempt counted.
    assert_eq!(
        strings(
            &database,
            "SELECT entity_id || ' ' || attempts FROM ops.entity_fetch WHERE tenant = 'reader'"
        )
        .await,
        ["Q666 1"],
        "the rows fetched went with the batch"
    );
    // At the last attempt the row is dropped, as a reject of the follower's open job.
    triplespace_cli::common::connect(&database)
        .await
        .unwrap()
        .execute("UPDATE ops.entity_fetch SET attempts = 4", &[])
        .await
        .unwrap();
    follow::run(follow_args()).await.expect("follow again");
    assert_eq!(
        strings(
            &database,
            "SELECT count(*)::text FROM ops.entity_fetch WHERE tenant = 'reader'"
        )
        .await,
        ["0"],
        "dropped after five attempts"
    );
    let rejects = strings(
        &database,
        "SELECT (job->'open'->'rejects')::text FROM ops.repo_cursor WHERE tenant = 'reader' AND name = 'lb'",
    )
    .await
    .join(" ");
    assert!(rejects.contains("Q666"), "{rejects}");
    assert_eq!(
        strings(
            &database,
            "SELECT (rc_time > now() - interval '1 hour')::text FROM ops.repo_cursor
             WHERE tenant = 'reader' AND kind = 'source' AND name = 'lb'"
        )
        .await,
        ["true"],
        "followed from now"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
