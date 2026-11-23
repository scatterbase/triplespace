//! Shallow following (0070 §2.3, §5; 05 §5.3, §5.7): a source's follower fetches the
//! recent changes of entities its mirror holds and ignores the rest, until the type is
//! `all`; and each entity it writes queues its closure values one step deeper, up to
//! `entities.closure_depth`, so the walk stops there. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use triplespace_cli::{fetch, follow, instance, settings, source};

const DB: &str = "tsshallow_test";
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

fn item(id: &str, revid: u64, instance_of: Option<&str>) -> Value {
    let claims = instance_of.map_or_else(
        || json!({}),
        |of| {
            json!({"P31": [{"mainsnak": {"snaktype": "value", "property": "P31",
                "datavalue": {"value": {"entity-type": "item", "id": of}, "type": "wikibase-entityid"},
                "datatype": "wikibase-item"}, "type": "statement", "rank": "normal",
                "id": format!("{id}$00000000-0000-0000-0000-000000000001")}]})
        },
    );
    json!({"type": "item", "id": id, "labels": {"en": {"language": "en", "value": id}},
        "descriptions": {}, "aliases": {}, "claims": claims, "sitelinks": {},
        "lastrevid": revid, "modified": "2026-11-21T00:00:00Z",
        "pageid": revid, "ns": 120, "title": format!("Item:{id}")})
}

/// What the stub wiki was asked: each request's line.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A wiki whose recent changes name `Q1` and `Q5`, and whose `wbgetentities` knows `Q1`,
/// `Q5`, `Q2` (an instance of `Q7`), `Q7` (an instance of `Q8`) and `Q8`.
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
                let row = |rcid: u64, id: &str, revid: u64| {
                    json!({"type": "edit", "ns": 120, "title": format!("Item:{id}"),
                        "rcid": rcid, "revid": revid, "old_revid": revid - 1,
                        "pageid": revid, "timestamp": "2026-11-21T00:00:00Z"})
                };
                let answer = if line.contains("wbgetentities") {
                    let ids = line
                        .split(['?', '&', ' '])
                        .find_map(|p| p.strip_prefix("ids="))
                        .unwrap_or_default()
                        .replace("%7C", "|");
                    let mut entities = serde_json::Map::new();
                    for id in ids.split('|') {
                        let e = match id {
                            "Q1" => item("Q1", 11, None),
                            "Q2" => item("Q2", 12, Some("Q7")),
                            "Q5" => item("Q5", 15, None),
                            "Q7" => item("Q7", 17, Some("Q8")),
                            "Q8" => item("Q8", 18, None),
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
                            "recentchanges": [row(1, "Q1", 11), row(2, "Q5", 15)],
                            "namespaces": {
                                "120": {"id": 120, "canonical": "Item", "defaultcontentmodel": "wikibase-item"},
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

/// Whether any `wbgetentities` request named `id`.
fn fetched(asked: &Asked, id: &str) -> bool {
    asked.lock().unwrap().iter().any(|l| {
        l.contains("wbgetentities")
            && l.split(['=', '&', ' '])
                .any(|p| p.replace("%7C", "|").split('|').any(|i| i == id))
    })
}

// 0070 §2.3, §5; 05 §5.3, §5.7.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn the_follower_keeps_to_the_set_and_walks_the_closure_to_its_depth() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsshallow-{}", std::process::id()));
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
    let set = |key: &str, value: &str| settings::Set {
        at: settings::Where {
            database: database.clone(),
            tenant: Some(TENANT.into()),
            instance: false,
        },
        key: key.into(),
        value: value.into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    };
    for (key, value) in [
        ("entities.closure", r#"{"lb": ["P31"]}"#),
        ("entities.closure_depth", "1"),
    ] {
        settings::run(settings::Settings::Set(set(key, value)))
            .await
            .unwrap_or_else(|e| panic!("set {key}: {e:#}"));
    }
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
    let held = || {
        strings(
            &database,
            "SELECT entity_id FROM view.entity_source
             WHERE tenant = 'reader' AND graph = 'source/lb' ORDER BY entity_id",
        )
    };

    fetch::run(fetch::Fetch {
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        ids: vec!["Q1".into(), "Q2".into()],
    })
    .await
    .expect("fetch");
    follow::run(follow_args()).await.expect("follow");
    // Q2's instance-of Q7 is one step: fetched in the same run. Q7's Q8 would be two.
    assert_eq!(held().await, ["lb:Q1", "lb:Q2", "lb:Q7"]);
    assert!(!fetched(&asked, "Q8"), "the closure stops at its depth");
    // Q5 changed upstream, but the mirror does not hold it: `linked` ignores it.
    assert!(
        !fetched(&asked, "Q5"),
        "a change outside the set is not fetched"
    );
    assert_eq!(
        strings(&database, "SELECT count(*)::text FROM ops.entity_fetch").await,
        ["0"]
    );

    // Under `all`, every change of the type is fetched.
    settings::run(settings::Settings::Set(set(
        "entities.mirror",
        r#"{"lb": {"item": "all"}}"#,
    )))
    .await
    .expect("set entities.mirror");
    triplespace_cli::common::connect(&database)
        .await
        .unwrap()
        .execute(
            "UPDATE ops.repo_cursor SET rc_seen = NULL WHERE tenant = 'reader' AND name = 'lb'",
            &[],
        )
        .await
        .unwrap();
    follow::run(follow_args()).await.expect("follow under all");
    assert_eq!(held().await, ["lb:Q1", "lb:Q2", "lb:Q5", "lb:Q7"]);
    let _ = std::fs::remove_dir_all(&dir);
}
