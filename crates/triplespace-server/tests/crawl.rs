//! A crawl (0092 §4, A16; 05 §5.9): `sync` without a dump lists a stub wiki's entity
//! namespaces page by page and queues every entity listed, and the unlisted ones the
//! source's mirror holds, for the follower; more unlisted than the threshold fails with
//! nothing of them queued and no end recorded; the follower then writes what was listed
//! and tombstones what the wiki no longer has. Needs `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_postgres::{Config, NoTls};

use triplespace_cli::{fetch, follow, instance, source, sync};

const DB: &str = "tscrawl_test";
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

/// A wiki whose `Item` namespace lists `Q1` and `Q2`, then `Q4` on a second page, and whose
/// `Property` namespace is empty. `wbgetentities` knows `Q1`, `Q2` and `Q4`, and `Q3` until
/// `gone` is set.
async fn wiki(gone: Arc<AtomicBool>) -> String {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let gone = gone.clone();
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
                let page = |titles: &[&str]| -> Vec<Value> {
                    titles
                        .iter()
                        .map(|t| json!({"pageid": 1, "ns": 120, "title": t}))
                        .collect()
                };
                let answer = if line.contains("list=allpages") {
                    if line.contains("apnamespace=122") {
                        json!({"batchcomplete": true, "query": {"allpages": []}})
                    } else if line.contains("apcontinue=") {
                        json!({"batchcomplete": true,
                               "query": {"allpages": page(&["Item:Q4"])}})
                    } else {
                        json!({"continue": {"apcontinue": "Q4", "continue": "-||"},
                               "query": {"allpages": page(&["Item:Q1", "Item:Q2"])}})
                    }
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
                            "Q3" if !gone.load(Ordering::SeqCst) => item("Q3", "Three", 13),
                            "Q4" => item("Q4", "Four", 14),
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
    format!("http://{address}/w/api.php")
}

// 0092 §4, A16; 05 §5.9.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_crawl_queues_what_the_wiki_lists_and_what_it_no_longer_does() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tscrawl-{}", std::process::id()));
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
    let gone = Arc::new(AtomicBool::new(false));
    let api = wiki(gone.clone()).await;
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
    let held = || {
        strings(
            &database,
            "SELECT entity_id FROM view.entity_source
             WHERE tenant = 'reader' AND graph = 'source/lb' ORDER BY entity_id",
        )
    };
    let queue = || {
        strings(
            &database,
            "SELECT entity_id || ' ' || reason FROM ops.entity_fetch
             WHERE tenant = 'reader' AND provider = 'lb' ORDER BY entity_id",
        )
    };
    let loaded = || {
        strings(
            &database,
            "SELECT coalesce((loaded_to > now() - interval '1 hour')::text, 'none')
             FROM ops.repo_cursor WHERE tenant = 'reader' AND kind = 'source' AND name = 'lb'",
        )
    };

    // The mirror holds Q1 and Q3, fetched by hand; then upstream deletes Q3.
    fetch::run(fetch::Fetch {
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        ids: vec!["Q1".into(), "Q3".into()],
    })
    .await
    .expect("fetch");
    follow::run(follow_args()).await.expect("follow");
    assert_eq!(held().await, ["lb:Q1", "lb:Q3"]);
    gone.store(true, Ordering::SeqCst);

    let crawl = |threshold| sync::Sync {
        job_id: None,
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        dump: None,
        version: None,
        snapshot: false,
        threshold,
        batch: 200,
        rejects: None,
        no_bootstrap: false,
        writers: 1,
        no_gap_check: false,
    };
    // Q3 is not listed: one more than a threshold of none.
    let refused = sync::run(crawl(0)).await.unwrap_err();
    assert!(format!("{refused:#}").contains("threshold"), "{refused:#}");
    assert_eq!(loaded().await, ["none"], "no end recorded");
    assert!(
        !queue().await.iter().any(|q| q.starts_with("Q3")),
        "nothing unlisted queued"
    );

    // Within the threshold: the three listed across two pages, and Q3 to be checked.
    sync::run(crawl(10)).await.expect("crawl");
    assert_eq!(
        queue().await,
        ["Q1 crawl", "Q2 crawl", "Q3 crawl", "Q4 crawl"]
    );
    assert_eq!(
        loaded().await,
        ["true"],
        "the crawl's start is the load's end"
    );

    // The follower writes what was listed and tombstones what the wiki no longer has.
    follow::run(follow_args()).await.expect("follow the crawl");
    assert_eq!(held().await, ["lb:Q1", "lb:Q2", "lb:Q4"]);
    assert!(queue().await.is_empty(), "{:?}", queue().await);
    let _ = std::fs::remove_dir_all(&dir);
}
