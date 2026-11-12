//! Importing MediaWiki XML into a running tenant (0008 §9.1, §9.2, §9.4, §9.7, A48, A49;
//! 0047 §8.3; 10 §6; 21 §4.3):
//! Librarybase's own wikitext export (`docs/samples/librarybase/dump/librarybase-wikitext.xml.gz`,
//! 39 pages, 147 revisions) imported through `triplespace import` into a tenant that
//! adopted nothing, its pages under minted IDs with their history, timestamps and
//! attributions, its talk pages as threads; an interrupted import run again writing
//! nothing twice; and a round trip through phase 1's exporter into a second tenant.
//! Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_ingest::import::Import as ImportJob;
use scatter_ingest::write::Attestation;
use scatter_mediawiki_xml::{DumpPage, XmlDump};
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_api_ingest::import::{census, classify, import_pages};
use triplespace_api_ingest::talk_import::{self, Authors};
use triplespace_cli::{import_pages as import_cmd, instance, settings, tenant};

const DB: &str = "tsimport_test";
const TENANT: &str = "wiki";
const HOST: &str = "wiki.example";
const SECOND: &str = "two";
const SECOND_HOST: &str = "two.example";
const AUTH: &str = "auth.example";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database(name: &str) -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(name);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap()
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples/librarybase/dump/librarybase-wikitext.xml.gz")
}

/// The dump's pages.
fn dump_pages() -> Vec<DumpPage> {
    let file = std::fs::File::open(fixture()).unwrap();
    let reader = std::io::BufReader::new(flate2::read::MultiGzDecoder::new(file));
    XmlDump::new(reader).map(|p| p.unwrap()).collect()
}

/// An instance with one tenant that adopted nothing.
async fn setup(name: &str) -> Option<(String, PathBuf)> {
    admin_config()?;
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
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
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    Some((database, key_file))
}

async fn query(database: &str, sql: &str) -> Vec<tokio_postgres::Row> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client.query(sql, &[]).await.unwrap()
}

async fn count(database: &str, sql: &str) -> i64 {
    query(database, sql).await[0].get(0)
}

/// The Action API and REST routes over the database, serving both tenants.
async fn router(database: &str, key_file: &std::path::Path) -> Router {
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(key_file).unwrap();
    let app = App::new(
        triplespace_db::scratch::pools(database, 4),
        Secret::derive(&signing.to_bytes()),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm,
            auth_base: Some(format!("https://{AUTH}")),
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    router_with(app, triplespace_api_rest::routes())
}

async fn api(router: &Router, host: &str, query: &str) -> Value {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/w/api.php?{query}&format=json&formatversion=2"))
        .header(header::HOST, host)
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn export_from(router: &Router, host: &str, body: Value) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method("POST")
        .uri("/w/rest.php/triplespace/v0/export")
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

fn import_args(database: &str) -> import_cmd::Import {
    import_cmd::Import {
        database: database.to_string(),
        tenant: TENANT.into(),
        file: fixture(),
        source: "librarybase".into(),
        issuer: None,
        subsidiary: None,
        census_only: false,
        skip_talk: false,
    }
}

/// Pages by namespace, with their revision counts, for a tenant.
async fn pages_of(database: &str, tenant: &str) -> BTreeMap<(i32, String), i32> {
    query(
        database,
        &format!(
            "SELECT ns, title, revisions FROM view.page WHERE tenant = '{tenant}' AND NOT deleted ORDER BY ns, title"
        ),
    )
    .await
    .iter()
    .map(|r| ((r.get(0), r.get(1)), r.get(2)))
    .collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn librarybase_pages_are_imported_with_history_authors_and_talk() {
    let Some((database, key_file)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let dump = dump_pages();
    assert_eq!(dump.len(), 39);

    // The census first (10 §6): the file's shape, what the job will take, and nothing
    // existing yet.
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    let c = census(&client, TENANT, dump.iter()).await.unwrap();
    assert_eq!((c.pages, c.revisions), (39, 147));
    assert_eq!((c.imported, c.talk, c.entities), (33, 6, 0));
    assert!(!c.current_only);
    assert!(c.existing.is_empty());
    assert_eq!(c.templates_in_file.len(), 2, "{:?}", c.templates_in_file);
    assert!(
        c.templates_used.contains_key("Cwbn"),
        "{:?}",
        c.templates_used
    );
    assert_eq!(c.templates_missing.len(), 0, "{:?}", c.templates_missing);
    assert!(
        c.functions.contains_key("REVISIONUSER"),
        "{:?}",
        c.functions
    );

    // An interrupted import: the first ten document pages, then the whole file. The
    // second run finds them present and writes nothing twice (0008 A48).
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(&database, &farm).unwrap();
    // `librarybase` is a registered issuer (issuers.toml), so the file's accounts keep
    // their numeric IDs (0008 A50): the partial import names it as the command will.
    let accounts = talk_import::accounts_of(dump.iter());
    let authors = Authors::Imported {
        issuer: Some("librarybase"),
        accounts: &accounts,
        prefix: "librarybase",
    };
    let ten: Vec<DumpPage> = dump
        .iter()
        .filter(|p| classify(p) == scatter_ingest::import::census::PageClass::Page)
        .take(10)
        .cloned()
        .collect();
    let ten_revisions: u64 = ten.iter().map(|p| p.revisions.len() as u64).sum();
    let (partial, _) = import_pages(
        &store,
        &pipeline,
        TENANT,
        Attestation::by("local:1"),
        ImportJob {
            source: "librarybase".into(),
            version: Some("part".into()),
            history: true,
            summary: None,
            job_id: None,
        },
        &authors,
        ten,
        triplespace_cli::common::now(),
    )
    .await
    .unwrap();
    assert_eq!(partial.counts.created, 10);
    assert_eq!(partial.counts.revisions, ten_revisions);

    import_cmd::run(import_args(&database))
        .await
        .expect("import");

    // 27 Project, 4 User and 2 Template pages, as adoption brings them. Of the 6 User
    // talk pages, 4 convert to threads (ns 214): two are redirects. Adoption converts
    // one, since three belong to users the dump never names as accounts; an import
    // gives every named author a surrogate, so their talk pages have a home (0007 §5).
    let pages = pages_of(&database, TENANT).await;
    let mut by_ns: BTreeMap<i32, i64> = BTreeMap::new();
    for (ns, _) in pages.keys() {
        *by_ns.entry(*ns).or_default() += 1;
    }
    // Harej's registered account gives his talk page a page of its own (ns 3), as
    // adoption does; the three users the file never names as accounts are surrogates.
    assert_eq!(
        by_ns.into_iter().collect::<Vec<_>>(),
        [(2, 4), (3, 1), (4, 27), (10, 2), (214, 4)]
    );
    assert_eq!(pages[&(4, "Home".into())], 18);
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.entity_revision r JOIN view.page p
               ON p.tenant = r.tenant AND r.entity_id = p.page_id::text
             WHERE p.tenant = 'wiki' AND p.ns IN (2, 4, 10)"
        )
        .await,
        140,
        "every revision once, the interrupted run's included"
    );

    // Page IDs are the tenant's own, not the source's (0008 A48): Home was page 489.
    let home = query(
        &database,
        "SELECT page_id, latest_revid FROM view.page WHERE tenant = 'wiki' AND ns = 4 AND title = 'Home'",
    )
    .await;
    assert_ne!(home[0].get::<_, i64>(0), 489);
    assert!(home[0].get::<_, i64>(0) < 100);

    // Each revision keeps its original timestamp and names the upstream revision it came
    // from (0008 A50), and its author is the registered issuer's account, `librarybase:N`
    // (0007 §5), with the name the file gives.
    let dump_home = dump.iter().find(|p| p.title == "Project:Home").unwrap();
    let latest = dump_home.latest().unwrap();
    let router = router(&database, &key_file).await;
    let served = api(
        &router,
        HOST,
        "action=query&prop=revisions&titles=Project:Home&rvprop=ids|timestamp|user|content&rvslots=main&rvlimit=20",
    )
    .await;
    let revisions = served["query"]["pages"][0]["revisions"].as_array().unwrap();
    assert_eq!(revisions.len(), 18);
    assert_eq!(revisions[0]["timestamp"], latest.timestamp);
    assert_eq!(
        revisions[0]["slots"]["main"]["content"].as_str().unwrap(),
        latest.text
    );
    let oldest = dump_home.revisions.iter().min_by_key(|r| r.id).unwrap();
    assert_eq!(revisions[17]["timestamp"], oldest.timestamp);
    assert_eq!(
        revisions[0]["upstream"]["revid"], latest.id,
        "{}",
        revisions[0]
    );
    assert_eq!(revisions[0]["upstream"]["time"], latest.timestamp);
    let actors = query(
        &database,
        "SELECT DISTINCT a.actor_key, x.kind, x.raw FROM view.activity a
         JOIN view.actor x ON x.tenant = a.tenant AND x.actor_key = a.actor_key
         WHERE a.tenant = 'wiki' AND a.target_kind = 2 AND a.revid IS NOT NULL
         ORDER BY a.actor_key",
    )
    .await;
    let keys: Vec<(String, String)> = actors
        .iter()
        .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
        .collect();
    assert!(
        keys.iter()
            .all(|(key, kind)| key.starts_with("librarybase:") && kind == "registered"),
        "{keys:?}"
    );
    let harej = "librarybase:3".to_string();
    assert!(keys.iter().any(|(key, _)| *key == harej), "{keys:?}");
    assert_eq!(
        query(
            &database,
            "SELECT name FROM view.actor WHERE tenant = 'wiki' AND actor_key = 'librarybase:3'"
        )
        .await[0]
            .get::<_, String>(0),
        "Harej"
    );

    // Harej's talk page became a thread on his talk page (0090 §10), with his post
    // attributed to his account.
    let threads = query(
        &database,
        &format!(
            "SELECT t.subject, t.status, t.post_count, t.target_kind, t.target_id FROM view.thread t
             WHERE t.tenant = 'wiki' AND t.target_id = '{harej}'"
        ),
    )
    .await;
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].get::<_, String>(0), "Welcome to Librarybase!");
    assert_eq!(threads[0].get::<_, String>(1), "archived");
    assert_eq!(threads[0].get::<_, i16>(3), 2);
    let posts = query(
        &database,
        "SELECT actor, dt_name FROM view.post WHERE tenant = 'wiki' AND dt_name = 'c-Harej-20250831155600'",
    )
    .await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].get::<_, String>(0), harej);
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.thread WHERE tenant = 'wiki'"
        )
        .await,
        4
    );

    // The job's records: one import job per run, the interrupted one included.
    let jobs = query(
        &database,
        "SELECT mode, status FROM view.job WHERE tenant = 'wiki' ORDER BY job_id",
    )
    .await;
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|j| j.get::<_, String>(0) == "import"));
    assert!(jobs.iter().all(|j| j.get::<_, String>(1) == "finished"));

    // Importing the same file again: every revision is present, nothing is written,
    // and the talk page is not converted twice.
    let before = count(
        &database,
        "SELECT count(*)::bigint FROM view.activity WHERE tenant = 'wiki'",
    )
    .await;
    import_cmd::run(import_args(&database))
        .await
        .expect("import again");
    let after = count(
        &database,
        "SELECT count(*)::bigint FROM view.activity WHERE tenant = 'wiki'",
    )
    .await;
    assert_eq!(after - before, 2, "the job's start and finish only");
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.thread WHERE tenant = 'wiki'"
        )
        .await,
        4
    );
    let c = census(&client, TENANT, dump.iter()).await.unwrap();
    assert_eq!(c.existing.len(), 33, "{:?}", c.existing);
    assert!(c.existing.contains(&"Project:Home".to_string()));

    // The round trip (21 §4.2, §4.3): the tenant's pages exported with history through
    // `POST /export`, imported into a second tenant, read the same.
    tenant::run(tenant::TenantCommand::Create(tenant::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: SECOND.into(),
        base: format!("https://{SECOND_HOST}"),
        name: None,
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: None,
        operator: None,
        auth_base: Some(format!("https://{AUTH}")),
    }))
    .await
    .expect("tenant create");
    let names: BTreeMap<i32, String> = api(
        &router,
        HOST,
        "action=query&meta=siteinfo&siprop=namespaces",
    )
    .await["query"]["namespaces"]
        .as_object()
        .unwrap()
        .values()
        .map(|n| {
            (
                i32::try_from(n["id"].as_i64().unwrap()).unwrap(),
                n["name"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let titles: Vec<String> = pages
        .keys()
        .filter(|(ns, _)| [2, 4, 10].contains(ns))
        .map(|(ns, title)| {
            if *ns == 0 {
                title.clone()
            } else {
                format!("{}:{title}", names[ns])
            }
        })
        .collect();
    let (status, xml) = export_from(
        &router,
        HOST,
        serde_json::json!({"format": "xml", "pages": titles, "history": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&xml));
    let exported: Vec<DumpPage> = XmlDump::new(xml.as_slice()).map(|p| p.unwrap()).collect();
    assert_eq!(exported.len(), 33);
    assert_eq!(
        exported.iter().map(|p| p.revisions.len()).sum::<usize>(),
        140
    );
    let accounts = talk_import::accounts_of(exported.iter());
    let authors = Authors::Imported {
        issuer: None,
        accounts: &accounts,
        prefix: "wiki",
    };
    let (out, read) = import_pages(
        &store,
        &pipeline,
        SECOND,
        Attestation::by("local:1"),
        ImportJob {
            source: "wiki".into(),
            version: None,
            history: true,
            summary: None,
            job_id: None,
        },
        &authors,
        exported.clone(),
        triplespace_cli::common::now(),
    )
    .await
    .unwrap();
    assert_eq!(
        (
            out.counts.created,
            out.counts.revisions,
            out.counts.rejected
        ),
        (33, 140, 0)
    );
    assert_eq!(read.pages, 33);
    let second = pages_of(&database, SECOND).await;
    let first_docs: BTreeMap<_, _> = pages
        .iter()
        .filter(|((ns, _), _)| [2, 4, 10].contains(ns))
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    assert_eq!(
        second, first_docs,
        "the same titles with the same revision counts"
    );
    let served = api(
        &router,
        SECOND_HOST,
        "action=query&prop=revisions&titles=Project:Home&rvprop=ids|timestamp|user|content&rvslots=main&rvlimit=20",
    )
    .await;
    let again = served["query"]["pages"][0]["revisions"].as_array().unwrap();
    assert_eq!(again.len(), 18);
    assert_eq!(
        again[0]["timestamp"], latest.timestamp,
        "timestamps survive the round trip"
    );
    assert_eq!(again[17]["timestamp"], oldest.timestamp);
    assert_eq!(
        again[0]["slots"]["main"]["content"].as_str().unwrap(),
        latest.text
    );
    // `wiki` is no registered issuer, so the second tenant's authors are surrogates
    // under its prefix, as MediaWiki names them.
    let chained = query(
        &database,
        "SELECT DISTINCT x.raw FROM view.activity a
         JOIN view.actor x ON x.tenant = a.tenant AND x.actor_key = a.actor_key
         WHERE a.tenant = 'two' AND a.target_kind = 2 AND a.revid IS NOT NULL",
    )
    .await;
    assert!(
        chained
            .iter()
            .any(|r| r.get::<_, String>(0) == "wiki>Harej"),
        "{:?}",
        chained
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect::<Vec<_>>()
    );
}

/// A browser on the tenant's host: one session cookie, posting forms or multipart bodies.
struct ApiClient {
    router: Router,
    cookie: Option<String>,
}

impl ApiClient {
    async fn call(&mut self, method: &str, query: &str, body: Option<(&str, Vec<u8>)>) -> Value {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/w/api.php?{query}&format=json&formatversion=2"))
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let req = match body {
            Some((ct, b)) => req
                .header(header::CONTENT_TYPE, ct)
                .body(Body::from(b))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    async fn get(&mut self, query: &str) -> Value {
        self.call("GET", query, None).await
    }

    async fn post(&mut self, query: &str, form: &str) -> Value {
        self.call(
            "POST",
            query,
            Some((
                "application/x-www-form-urlencoded",
                form.as_bytes().to_vec(),
            )),
        )
        .await
    }

    /// `action=import` with a multipart body: the fields, then the file as `xml`.
    async fn import(&mut self, fields: &[(&str, &str)], xml: Option<&[u8]>) -> Value {
        const B: &str = "----triplespace-test-boundary";
        let mut body = Vec::new();
        for (k, v) in fields {
            body.extend_from_slice(
                format!("--{B}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                    .as_bytes(),
            );
        }
        if let Some(xml) = xml {
            body.extend_from_slice(
                format!(
                    "--{B}\r\nContent-Disposition: form-data; name=\"xml\"; filename=\"export.xml\"\r\nContent-Type: application/xml\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(xml);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{B}--\r\n").as_bytes());
        let ct = format!("multipart/form-data; boundary={B}");
        self.call("POST", "action=import", Some((&ct, body))).await
    }

    async fn token(&mut self, kind: &str) -> String {
        self.get(&format!("action=query&meta=tokens&type={kind}"))
            .await["query"]["tokens"][format!("{kind}token")]
        .as_str()
        .unwrap()
        .to_string()
    }

    async fn login(&mut self, name: &str) -> Value {
        let token = self.token("login").await;
        let enc =
            |s: &str| -> String { url::form_urlencoded::byte_serialize(s.as_bytes()).collect() };
        self.post(
            "action=clientlogin",
            &format!(
                "username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
                enc(name),
                enc("owner-secret-1234"),
                enc(&token)
            ),
        )
        .await
    }
}

/// The dump as raw XML, for the upload.
fn dump_xml() -> Vec<u8> {
    use std::io::Read as _;
    let file = std::fs::File::open(fixture()).unwrap();
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(file)
        .read_to_end(&mut out)
        .unwrap();
    out
}

/// `action=import` (21 §4.3; 0047 §8.3): the owner uploads Librarybase's export; with
/// `census` the module answers the census and writes nothing; without it the pages are
/// imported and the talk pages converted, and the response lists what each page got.
/// Anyone without `import` is refused.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn action_import_takes_an_upload_and_shows_the_census_first() {
    let Some((database, key_file)) = setup("tsimport_api_test").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let router = router(&database, &key_file).await;
    let xml = dump_xml();
    let mut anon = ApiClient {
        router: router.clone(),
        cookie: None,
    };
    let refused = anon.import(&[("token", "+\\")], Some(&xml)).await;
    assert_eq!(refused["error"]["code"], "permissiondenied", "{refused}");
    let mut owner = ApiClient {
        router: router.clone(),
        cookie: None,
    };
    assert_eq!(owner.login("Owner").await["clientlogin"]["status"], "PASS");
    let csrf = owner.token("csrf").await;

    // No source: refused. The census: answered, nothing written.
    let nothing = owner.import(&[("token", &csrf)], None).await;
    assert_eq!(nothing["error"]["code"], "import-nosource", "{nothing}");
    let shown = owner
        .import(&[("token", &csrf), ("census", "1")], Some(&xml))
        .await;
    assert_eq!(shown["census"]["pages"], 39, "{shown}");
    assert_eq!(shown["census"]["imported"], 33);
    assert_eq!(shown["census"]["talk"], 6);
    assert_eq!(shown["census"]["entities"], 0);
    assert_eq!(shown["import"].as_array().unwrap().len(), 0);
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.page WHERE tenant = 'wiki'"
        )
        .await,
        0,
        "the census writes nothing"
    );

    // The import.
    let done = owner
        .import(
            &[
                ("token", &csrf),
                ("summary", "Librarybase's pages"),
                ("interwikiprefix", "librarybase"),
                ("rootpage", "X"),
            ],
            Some(&xml),
        )
        .await;
    assert_eq!(done["error"], Value::Null, "{done}");
    let imported = done["import"].as_array().unwrap();
    assert_eq!(imported.len(), 33);
    // Titles are prefixed with the wiki's own namespace names (`Wiki:Home` for its
    // Project namespace), as MediaWiki's response spells them.
    let home = imported
        .iter()
        .find(|p| p["ns"] == 4 && p["title"].as_str().unwrap().ends_with(":Home"))
        .expect("Project:Home");
    assert_eq!(home["revisions"], 18);
    assert_eq!(home["created"], true);
    assert_eq!(done["job"]["counts"]["created"], 33);
    assert_eq!(done["job"]["counts"]["revisions"], 140);
    assert_eq!(done["job"]["talk"]["pages"], 4);
    assert!(
        done["warnings"]["import"].to_string().contains("rootpage"),
        "{done}"
    );
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.thread WHERE tenant = 'wiki'"
        )
        .await,
        4
    );
    let job = query(
        &database,
        "SELECT mode, status, params->'args'->>'summary' FROM view.job WHERE tenant = 'wiki'",
    )
    .await;
    assert_eq!(job.len(), 1);
    assert_eq!(job[0].get::<_, String>(0), "import");
    assert_eq!(job[0].get::<_, String>(1), "finished");
    // `interwikiprefix` naming a registered issuer's code gives its accounts their IDs
    // (0008 A50); any other prefix gives surrogates.
    assert_eq!(done["job"]["issuer"], "librarybase", "{done}");
    let attributed = query(
        &database,
        "SELECT DISTINCT a.actor_key FROM view.activity a
         WHERE a.tenant = 'wiki' AND a.target_kind = 2 AND a.revid IS NOT NULL",
    )
    .await;
    assert!(!attributed.is_empty());
    assert!(
        attributed
            .iter()
            .all(|r| r.get::<_, String>(0).starts_with("librarybase:")),
        "{:?}",
        attributed
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect::<Vec<_>>()
    );

    // Again: present, nothing written.
    let again = owner
        .import(
            &[("token", &csrf), ("interwikiprefix", "librarybase")],
            Some(&xml),
        )
        .await;
    assert_eq!(again["job"]["counts"]["revisions"], 0, "{again}");
    assert_eq!(again["job"]["counts"]["present"], 140);
    assert_eq!(again["job"]["talk"]["present"], 4);

    // The upload limit is the wiki's `import.max_upload` (0047 A36): 100 MB by default,
    // reported in `meta=siteinfo`, and an upload over it is refused.
    let si = owner
        .get("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(
        si["query"]["triplespace"]["import"]["max_upload"],
        104_857_600
    );
    settings::run(settings::Settings::Set(settings::Set {
        at: settings::Where {
            database: database.clone(),
            tenant: Some(TENANT.into()),
            instance: false,
        },
        key: "import.max_upload".into(),
        value: "1000".into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await
    .expect("set import.max_upload");
    let si = owner
        .get("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(si["query"]["triplespace"]["import"]["max_upload"], 1000);
    let refused = owner
        .import(&[("token", &csrf), ("census", "1")], Some(&xml))
        .await;
    assert_eq!(refused["error"]["code"], "import-too-large", "{refused}");
}

/// A browser on the site: one session cookie, posting forms or multipart bodies.
struct Browser {
    router: Router,
    cookie: Option<String>,
}

/// What the site answered.
struct Got {
    status: StatusCode,
    body: String,
}

impl Browser {
    async fn send(&mut self, method: &str, path: &str, body: Option<(String, Vec<u8>)>) -> Got {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let req = match body {
            Some((ct, b)) => req
                .header(header::CONTENT_TYPE, ct)
                .body(Body::from(b))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let r = self.router.clone().oneshot(req).await.unwrap();
        let status = r.status();
        for sc in r.headers().get_all(header::SET_COOKIE) {
            if let Some(v) = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
            {
                self.cookie = (!v.is_empty()).then(|| v.to_string());
            }
        }
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        Got {
            status,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&mut self, path: &str) -> Got {
        self.send("GET", path, None).await
    }

    /// A multipart post: the fields, then the file as `xml`.
    async fn post_multipart(
        &mut self,
        path: &str,
        fields: &[(&str, &str)],
        xml: Option<&[u8]>,
    ) -> Got {
        const B: &str = "----triplespace-site-test";
        let mut body = Vec::new();
        for (k, v) in fields {
            body.extend_from_slice(
                format!("--{B}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                    .as_bytes(),
            );
        }
        if let Some(xml) = xml {
            body.extend_from_slice(
                format!("--{B}\r\nContent-Disposition: form-data; name=\"xml\"; filename=\"lb.xml\"\r\nContent-Type: application/xml\r\n\r\n")
                    .as_bytes(),
            );
            body.extend_from_slice(xml);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{B}--\r\n").as_bytes());
        self.send(
            "POST",
            path,
            Some((format!("multipart/form-data; boundary={B}"), body)),
        )
        .await
    }

    /// The value of a hidden field of the page.
    fn field(body: &str, name: &str) -> String {
        let marker = format!("name=\"{name}\" value=\"");
        let at = body
            .find(&marker)
            .unwrap_or_else(|| panic!("no field {name} in {body}"));
        let rest = &body[at + marker.len()..];
        rest.split('"').next().unwrap_or_default().to_string()
    }
}

/// `Special:Import` through the site (21 §4.3; 0047 §8.3): the form for the owner, a
/// refusal for a reader, the census of the uploaded file with its confirmation carrying
/// the file, and the report once confirmed.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn special_import_shows_the_census_then_imports() {
    let Some((database, key_file)) = setup("tsimport_site_test").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let api = router(&database, &key_file).await;
    let site =
        api.clone()
            .fallback_service(triplespace_ui::router(triplespace_client::Client::new(
                triplespace_client::ServiceTransport::new(api.clone()),
            )));
    let xml = dump_xml();

    // A reader sees the page refused; the owner sees the form.
    let mut reader = Browser {
        router: site.clone(),
        cookie: None,
    };
    let refused = reader.get("/wiki/Special:Import").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert!(
        refused.body.contains("You do not have the import right."),
        "{}",
        refused.body
    );
    let mut owner_api = ApiClient {
        router: api.clone(),
        cookie: None,
    };
    assert_eq!(
        owner_api.login("Owner").await["clientlogin"]["status"],
        "PASS"
    );
    let mut owner = Browser {
        router: site.clone(),
        cookie: owner_api.cookie.clone(),
    };
    let form = owner.get("/wiki/Special:Import").await;
    assert_eq!(form.status, StatusCode::OK, "{}", form.body);
    assert!(
        form.body.contains("enctype=\"multipart/form-data\""),
        "{}",
        form.body
    );
    assert!(form.body.contains("type=\"file\"") && form.body.contains("name=\"xml\""));
    assert!(form.body.contains("name=\"interwikisource\""));
    assert!(form.body.contains("name=\"fullhistory\"") && form.body.contains("checked"));
    let token = Browser::field(&form.body, "token");

    // Nothing chosen: the form again, with a warning.
    let none = owner
        .post_multipart(
            "/wiki/Special:Import",
            &[("token", &token), ("wpStep", "census")],
            None,
        )
        .await;
    assert_eq!(none.status, StatusCode::OK);
    assert!(
        none.body
            .contains("Choose a file to upload, or name a wiki and a page there."),
        "{}",
        none.body
    );

    // The census: what the file holds, and the confirmation carrying the file.
    let census = owner
        .post_multipart(
            "/wiki/Special:Import",
            &[
                ("token", &token),
                ("wpStep", "census"),
                ("fullhistory", "1"),
                ("summary", "Librarybase's pages"),
                ("interwikiprefix", "librarybase"),
            ],
            Some(&xml),
        )
        .await;
    assert_eq!(census.status, StatusCode::OK, "{}", census.body);
    assert!(
        census.body.contains("What the file holds"),
        "{}",
        census.body
    );
    assert!(
        census.body.contains("39 pages, 147 revisions."),
        "{}",
        census.body
    );
    assert!(
        census
            .body
            .contains("33 pages will be imported and 6 talk pages converted"),
        "{}",
        census.body
    );
    assert!(
        census
            .body
            .contains("The file carries every template its pages use"),
        "{}",
        census.body
    );
    assert!(
        census.body.contains("name=\"wpXml\""),
        "the file rides along"
    );
    assert!(census.body.contains("name=\"wpStep\" value=\"import\""));
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.page WHERE tenant = 'wiki'"
        )
        .await,
        0,
        "nothing imported yet"
    );
    let blob = Browser::field(&census.body, "wpXml");
    let filename = Browser::field(&census.body, "wpFilename");
    assert_eq!(filename, "lb.xml");

    // Confirmed: the report.
    let done = owner
        .post_multipart(
            "/wiki/Special:Import",
            &[
                ("token", &token),
                ("wpStep", "import"),
                ("fullhistory", "1"),
                ("summary", "Librarybase's pages"),
                ("interwikiprefix", "librarybase"),
                ("wpXml", &blob),
                ("wpFilename", &filename),
            ],
            None,
        )
        .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert!(done.body.contains("<h2>Imported</h2>"), "{}", done.body);
    assert!(
        done.body.contains(
            "33 pages created, 0 merged into their existing history, 140 revisions written"
        ),
        "{}",
        done.body
    );
    assert!(
        done.body
            .contains("4 talk pages converted into 4 discussions"),
        "{}",
        done.body
    );
    assert!(done.body.contains("Special:Jobs/"), "{}", done.body);
    assert!(done.body.contains("18 revisions imported"), "{}", done.body);
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.page WHERE tenant = 'wiki' AND ns IN (2, 4, 10)"
        )
        .await,
        33
    );
    // The imported page renders through the site with its content; its history rows and
    // the diff's header carry the imported chip, naming the upstream revision and the job
    // (0008 A50).
    let home = owner.get("/wiki/Project:Home").await;
    assert_eq!(home.status, StatusCode::OK, "{}", home.body);
    assert!(home.body.contains("Cool Wikibase Network"), "{}", home.body);
    let history = owner.get("/wiki/Project:Home?action=history").await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);
    assert!(
        history.body.contains("ts-chip--imported"),
        "{}",
        history.body
    );
    assert!(
        history.body.contains("title=\"Imported from revision ")
            && history.body.contains(" by job "),
        "{}",
        history.body
    );
    assert!(
        history
            .body
            .contains("class=\"ts-change__job\" href=\"/wiki/Special:Jobs/"),
        "the job is linked to its page (0104 §9): {}",
        history.body
    );
    let diff = owner.get("/wiki/Project:Home?diff=cur").await;
    assert_eq!(diff.status, StatusCode::OK, "{}", diff.body);
    assert!(diff.body.contains("ts-chip--imported"), "{}", diff.body);
    // `Special:SpecialPages` lists it.
    let index = owner.get("/wiki/Special:SpecialPages").await;
    assert!(index.body.contains("Special:Import"), "{}", index.body);
}

/// Wikipedia talk pages (`docs/samples/enwiki-talk`; 0008 §9.1, §9.6; 0090 §10): from
/// the current-revision export, the discussion pages in the Project namespace come as
/// pages and a talk page whose subject is not here is left out; from the full-history
/// export, Harej's user talk page becomes threads on his `enwiki>Harej` surrogate's talk
/// page, each post attributed from the history to a surrogate of the same kind.
#[tokio::test]
async fn wikipedia_talk_pages_are_attributed_from_their_history() {
    let Some((database, _)) = setup("tsimport_talk_test").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let args = |file: &str| import_cmd::Import {
        database: database.clone(),
        tenant: TENANT.into(),
        file: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/samples/enwiki-talk")
            .join(file),
        source: "enwiki".into(),
        issuer: None,
        subsidiary: None,
        census_only: false,
        skip_talk: false,
    };
    import_cmd::run(args("current.xml.bz2"))
        .await
        .expect("import current");
    // The Wikipedia: pages are Project pages here, one revision each; every talk page
    // in the file is an archive or an article's, with no subject here.
    let project = query(
        &database,
        "SELECT title, revisions FROM view.page WHERE tenant = 'wiki' AND ns = 4 ORDER BY title",
    )
    .await;
    assert_eq!(
        project.len(),
        3,
        "{:?}",
        project
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect::<Vec<_>>()
    );
    assert!(project.iter().all(|r| r.get::<_, i32>(1) == 1));
    assert_eq!(
        count(
            &database,
            "SELECT count(*)::bigint FROM view.thread WHERE tenant = 'wiki'"
        )
        .await,
        0
    );

    // Harej's talk page with its 4,677 revisions: `enwiki` is a registered issuer, so
    // the threads sit on his `enwiki:N` account's talk page, and each post is attributed
    // from the history's revisions to an `enwiki:N` account, or to a surrogate where a
    // signature names someone the file does not (0008 A50).
    import_cmd::run(args("history.xml.bz2"))
        .await
        .expect("import history");
    let harej = query(
        &database,
        "SELECT actor_key FROM view.actor WHERE tenant = 'wiki' AND kind = 'registered' AND name = 'Harej' AND actor_key LIKE 'enwiki:%'",
    )
    .await;
    assert_eq!(harej.len(), 1);
    let harej: String = harej[0].get(0);
    let threads = count(
        &database,
        &format!("SELECT count(*)::bigint FROM view.thread WHERE tenant = 'wiki' AND target_kind = 2 AND target_id = '{harej}'"),
    )
    .await;
    assert!(threads > 1, "{threads}");
    let signers = query(
        &database,
        "SELECT DISTINCT actor FROM view.post WHERE tenant = 'wiki'",
    )
    .await;
    assert!(signers.len() > 1, "{}", signers.len());
    // The accounts the file names; a post nobody can be credited for is the job's.
    assert!(
        signers
            .iter()
            .filter(|r| r.get::<_, String>(0).starts_with("enwiki:"))
            .count()
            > 1,
        "{:?}",
        signers
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect::<Vec<_>>()
    );
}

/// The XML export writes the model each revision had (0047 A37): a page imported as
/// wikitext and then edited to markdown exports with `wikitext` on its first revision and
/// `markdown` on its second, and imports back the same.
#[tokio::test]
async fn an_export_walks_a_pages_model_changes() {
    let Some((database, key_file)) = setup("tsimport_model_test").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let revision = |id: u64, model: &str, text: &str| scatter_mediawiki_xml::Revision {
        id,
        parent_id: (id > 1).then(|| id - 1),
        timestamp: format!("2026-01-0{id}T00:00:00Z"),
        user: Some((7, "Jane".into())),
        model: Some(model.into()),
        text: text.into(),
        comment: Some(format!("edit {id}")),
        ..scatter_mediawiki_xml::Revision::default()
    };
    let page = DumpPage {
        title: "Project:Notes".into(),
        ns: 4,
        id: 1,
        redirect: None,
        revisions: vec![
            revision(1, "wikitext", "== Notes =="),
            revision(2, "markdown", "## Notes"),
        ],
    };
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(&database, &farm).unwrap();
    let accounts = talk_import::accounts_of(std::iter::once(&page));
    let authors = Authors::Imported {
        issuer: None,
        accounts: &accounts,
        prefix: "notes",
    };
    let (out, _) = import_pages(
        &store,
        &pipeline,
        TENANT,
        Attestation::by("local:1"),
        ImportJob {
            source: "notes".into(),
            version: None,
            history: true,
            summary: None,
            job_id: None,
        },
        &authors,
        vec![page],
        triplespace_cli::common::now(),
    )
    .await
    .unwrap();
    assert_eq!((out.counts.created, out.counts.revisions), (1, 2));
    let router = router(&database, &key_file).await;
    let (status, xml) = export_from(
        &router,
        HOST,
        serde_json::json!({"format": "xml", "pages": ["Project:Notes"], "history": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&xml));
    let exported: Vec<DumpPage> = XmlDump::new(xml.as_slice()).map(|p| p.unwrap()).collect();
    assert_eq!(exported.len(), 1);
    let models: Vec<(Option<String>, Option<String>)> = exported[0]
        .revisions
        .iter()
        .map(|r| (r.model.clone(), r.format.clone()))
        .collect();
    assert_eq!(
        models,
        [
            (Some("wikitext".into()), Some("text/x-wiki".into())),
            (Some("markdown".into()), Some("text/markdown".into())),
        ]
    );
}
