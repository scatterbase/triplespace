//! The update stream on shared rows (0032 §4; 08 §10.5): a provider's mirror is composed
//! once, under the empty-string tenant, and its deltas are stored once, rendered under the
//! reserved base. A tenant's stream reads them under its own base until it overlays the
//! entity, and its own deltas from then on. The check is the stream's contract each time:
//! a dump with every event after its cursor applied is the dump taken later. Needs
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
use triplespace_cli::{instance, sync};
use triplespace_rdf::updates;

const DB: &str = "tsupdates_shared_test";
const TENANT: &str = "sharedtest";
const HOST: &str = "shared.example";

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
    let dir = std::env::temp_dir().join(format!("tsupdates-shared-{}", std::process::id()));
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
        providers: vec!["internetdomains".into()],
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

/// A dump of the provider: two items and two properties, with what varies given.
fn provider_dump(example_label: &str, wikipedia_description: &str) -> String {
    let st = |p: &str, v: &str, guid: &str| {
        serde_json::json!({"mainsnak": {"snaktype": "value", "property": p, "datatype": "string",
            "datavalue": {"value": v, "type": "string"}},
            "type": "statement", "rank": "normal", "id": guid})
    };
    let items = [
        serde_json::json!({"type": "item", "id": "Q9", "labels": {"en": {"language": "en", "value": example_label}},
            "descriptions": {}, "aliases": {},
            "claims": {"P1": [st("P1", "example.org", "Q9$00000000-0000-0000-0000-0000000000A1")]},
            "sitelinks": {}, "lastrevid": 55, "modified": "2026-10-01T00:00:00Z"}),
        serde_json::json!({"type": "item", "id": "Q11", "labels": {"en": {"language": "en", "value": "Wikipedia"}},
            "descriptions": {"en": {"language": "en", "value": wikipedia_description}}, "aliases": {},
            "claims": {"P1": [st("P1", "wikipedia.org", "Q11$00000000-0000-0000-0000-0000000000B1")]},
            "sitelinks": {}, "lastrevid": 56, "modified": "2026-10-01T00:00:00Z"}),
        serde_json::json!({"type": "property", "id": "P1", "datatype": "string",
            "labels": {"en": {"language": "en", "value": "domain name"}},
            "descriptions": {}, "aliases": {}, "claims": {}, "lastrevid": 3, "modified": "2026-10-01T00:00:00Z"}),
    ];
    let lines: Vec<String> = items.iter().map(ToString::to_string).collect();
    format!("[\n{}\n]\n", lines.join(",\n"))
}

async fn mirror(database: &str, dir: &Path, version: &str, dump: String, first: bool) {
    let file = dir.join(format!("internetdomains-{version}.json"));
    std::fs::write(&file, dump).unwrap();
    sync::run(sync::Sync {
        job_id: None,
        database: database.into(),
        provider: Some("internetdomains".into()),
        tenant: None,
        source: None,
        dump: Some(file),
        version: Some(version.into()),
        snapshot: false,
        threshold: 1000,
        batch: 200,
        rejects: None,
        no_bootstrap: !first,
        writers: sync::WRITERS,
        no_gap_check: true,
    })
    .await
    .expect("sync");
}

/// Applies the tenant's events after `from` to `resolved`, and returns where they end.
async fn follow(
    client: &tokio_postgres::Client,
    from: updates::Cursor,
    resolved: &mut BTreeSet<String>,
) -> (usize, i64) {
    let base = format!("https://{HOST}/");
    let events = updates::events(
        client,
        TENANT,
        &base,
        &["resolved".to_string()],
        from.seq,
        1000,
    )
    .await
    .unwrap();
    for event in &events {
        for (_, delta) in &event.parts {
            for l in &delta.deleted {
                resolved.remove(l);
            }
            for l in &delta.inserted {
                resolved.insert(l.clone());
            }
        }
    }
    (events.len(), events.last().map_or(from.seq, |e| e.seq))
}

fn same(applied: &BTreeSet<String>, want: &BTreeSet<String>, when: &str) {
    let missing: Vec<_> = want.difference(applied).collect();
    let extra: Vec<_> = applied.difference(want).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{when}: missing {missing:#?}; extra {extra:#?}"
    );
}

// 0032 §4 (A16); 08 §10.5: shared deltas under the tenant's base, and overlay spans.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn shared_deltas_reach_a_tenant_until_it_overlays_the_entity() {
    let Some((database, key_file, dir)) = setup().await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let dumps = dir.join("dumps");
    mirror(
        &database,
        &dir,
        "20261001",
        provider_dump("example.org", "the free encyclopedia"),
        true,
    )
    .await;
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
    app.work_until_idle(100_000).await.unwrap();
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    let shared_rows: i64 = client
        .query_one("SELECT count(*) FROM view.entity WHERE tenant = ''", &[])
        .await
        .unwrap()
        .get(0);
    assert!(shared_rows >= 2, "the mirror is shared rows: {shared_rows}");

    // On, filled, and a first dump.
    updates::set_enabled(&client, "", true).await.unwrap();
    updates::set_enabled(&client, TENANT, true).await.unwrap();
    app.work_until_idle(100_000).await.unwrap();
    dump(&database, &dumps).await;
    let (cursor, mut resolved) = dump_lines(&dumps, "wikibase-skolem.nt.gz");
    let mut at = updates::Cursor::parse(&cursor.expect("a stamped dump")).unwrap();
    assert!(
        resolved
            .iter()
            .any(|l| l.starts_with(&format!("<https://{HOST}/wiki/Special:EntityData/"))),
        "the mirrored entities' document nodes are the tenant's"
    );

    // 1. The provider changes both items: shared deltas, stored once under the reserved
    // base and read under the tenant's.
    mirror(
        &database,
        &dir,
        "20261002",
        provider_dump("example.org, renamed", "an encyclopedia"),
        false,
    )
    .await;
    app.work_until_idle(100_000).await.unwrap();
    let stored: Vec<(String, Option<String>)> = client
        .query(
            "SELECT tenant, provider FROM view.rdf_delta ORDER BY seq",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    assert!(!stored.is_empty());
    assert!(
        stored
            .iter()
            .all(|(t, p)| t.is_empty() && p.as_deref() == Some("internetdomains")),
        "{stored:?}"
    );
    let reserved: i64 = client
        .query_one(
            "SELECT count(*) FROM view.rdf_shown
              WHERE tenant = '' AND convert_from(triples, 'UTF8') LIKE '%<https://tenant.invalid/%'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        reserved > 0,
        "shared rows are recorded under the reserved base"
    );
    let (n, seq) = follow(&client, at, &mut resolved).await;
    assert!(n > 0);
    at.seq = seq;
    dump(&database, &dumps).await;
    let (_, want) = dump_lines(&dumps, "wikibase-skolem.nt.gz");
    assert!(
        want.iter()
            .any(|l| l.contains("\"example.org, renamed\"@en"))
    );
    assert!(!want.iter().any(|l| l.contains("tenant.invalid")));
    same(&resolved, &want, "after the provider's change");

    // 2. The tenant overlays one of them: a statement of its own on the mirrored entity.
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
    let out = api
        .post(
            "action=wbeditentity",
            &format!(
                "id={}&data={}&token={}",
                enc("domain:wikipedia.org"),
                enc(r#"{"labels":{"fr":{"language":"fr","value":"Wikipédia"}}}"#),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(out["success"], 1, "{out}");
    app.work_until_idle(100_000).await.unwrap();
    let spans = async |c: &tokio_postgres::Client| -> Vec<(String, bool)> {
        c.query(
            "SELECT entity_id, to_seq IS NULL FROM view.rdf_overlay WHERE tenant = $1 ORDER BY from_seq",
            &[&TENANT],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
    };
    assert_eq!(
        spans(&client).await,
        [("domain:wikipedia.org".to_string(), true)]
    );
    let (n, seq) = follow(&client, at, &mut resolved).await;
    assert!(n > 0);
    at.seq = seq;
    dump(&database, &dumps).await;
    let (_, want) = dump_lines(&dumps, "wikibase-skolem.nt.gz");
    assert!(
        want.iter()
            .any(|l| l.contains("\"Wikip\\u00E9dia\"@fr") || l.contains("\"Wikipédia\"@fr"))
    );
    same(&resolved, &want, "after the tenant's overlay");

    // 3. The provider changes both again. The overlaid entity's shared delta is not the
    // tenant's: its own delta carries the change; the other entity's shared delta is.
    mirror(
        &database,
        &dir,
        "20261003",
        provider_dump("example.org, again", "a free encyclopedia"),
        false,
    )
    .await;
    app.work_until_idle(100_000).await.unwrap();
    let (n, _) = follow(&client, at, &mut resolved).await;
    assert!(n > 0);
    dump(&database, &dumps).await;
    let (_, want) = dump_lines(&dumps, "wikibase-skolem.nt.gz");
    assert!(
        want.iter()
            .any(|l| l.contains("\"a free encyclopedia\"@en"))
    );
    assert!(want.iter().any(|l| l.contains("\"example.org, again\"@en")));
    same(
        &resolved,
        &want,
        "after the provider's change under an overlay",
    );
}
