//! The export job (21 §4.2; 0104 A9): a selection over `export.sync_revisions` asked for
//! by a signed-in account is queued and answered `202` with its job, run by a runner
//! pass that streams the file into the blob store, and fetched from `GET /export/{id}`
//! by the account that asked and nobody else; the file is removed after
//! `export.keep_days`. An anonymous caller, and a server without a blob store, are still
//! refused `413`.
//!
//! Needs `TRIPLESPACE_TEST_DATABASE_URL`; without it the test prints why and passes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_blob::BlobStore;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode as ServerMode, router_with};
use triplespace_api_ingest::jobs;
use triplespace_api_rest::export_job::export_work;
use triplespace_cli::{instance, settings};

const DB: &str = "tsexport_job";
const TENANT: &str = "wiki";
const HOST: &str = "wiki.example";
const AUTH: &str = "auth.example";
const PASSWORD: &str = "owner-secret-1234";

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

/// Librarybase's own export, as raw XML, for the upload.
fn dump_xml() -> Vec<u8> {
    use std::io::Read as _;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples/librarybase/dump/librarybase-wikitext.xml.gz");
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(std::fs::File::open(fixture).unwrap())
        .read_to_end(&mut out)
        .unwrap();
    out
}

async fn set_setting(database: &str, key: &str, value: &str) {
    settings::run(settings::Settings::Set(settings::Set {
        at: settings::Where {
            database: database.to_string(),
            tenant: Some(TENANT.into()),
            instance: false,
        },
        key: key.into(),
        value: value.into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await
    .unwrap_or_else(|e| panic!("set {key}: {e}"));
}

/// An instance with one tenant and its owner; registration made cheap for Alice. The
/// database, the key file and a directory of the test's own.
async fn setup(name: &str) -> Option<(String, PathBuf, PathBuf)> {
    admin_config()?;
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, format!("{PASSWORD}\n")).unwrap();
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
    for (key, value) in [("challenge.bits", "8"), ("challenge.max_bits", "10")] {
        set_setting(&database, key, value).await;
    }
    Some((database, key_file, dir))
}

async fn query(database: &str, sql: &str) -> Vec<tokio_postgres::Row> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client.query(sql, &[]).await.unwrap()
}

/// The app over the database, with a blob store in `blobs` where one is given, and the
/// Action API and REST routes.
async fn app(database: &str, key_file: &Path, blobs: Option<&Path>) -> (App, Router) {
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(key_file).unwrap();
    let app = App::new(
        triplespace_db::scratch::pools(database, 4),
        Secret::derive(&signing.to_bytes()),
        ServerConfig {
            mode: ServerMode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm,
            auth_base: Some(format!("https://{AUTH}")),
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    if let Some(dir) = blobs {
        app.set_blobs(BlobStore::filesystem(dir).unwrap());
    }
    let router = router_with(app.clone(), triplespace_api_rest::routes());
    (app, router)
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A client of the tenant's API: one session cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
}

/// What was answered.
struct Got {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Got {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }
}

impl Client {
    fn new(router: &Router) -> Self {
        Self {
            router: router.clone(),
            cookie: None,
        }
    }

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
        let headers = r.headers().clone();
        for sc in headers.get_all(header::SET_COOKIE) {
            if let Some(v) = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
            {
                self.cookie = (!v.is_empty()).then(|| v.to_string());
            }
        }
        let body = r.into_body().collect().await.unwrap().to_bytes().to_vec();
        Got {
            status,
            headers,
            body,
        }
    }

    async fn api(&mut self, query: &str) -> Value {
        self.send(
            "GET",
            &format!("/w/api.php?{query}&format=json&formatversion=2"),
            None,
        )
        .await
        .json()
    }

    async fn post(&mut self, query: &str, form: &str) -> Value {
        self.send(
            "POST",
            &format!("/w/api.php?{query}&format=json&formatversion=2"),
            Some((
                "application/x-www-form-urlencoded".into(),
                form.as_bytes().to_vec(),
            )),
        )
        .await
        .json()
    }

    /// `POST /export`.
    async fn export(&mut self, body: &Value) -> Got {
        self.send(
            "POST",
            "/w/rest.php/triplespace/v0/export",
            Some(("application/json".into(), body.to_string().into_bytes())),
        )
        .await
    }

    /// `GET /export/{id}`.
    async fn fetch(&mut self, job: u64) -> Got {
        self.send(
            "GET",
            &format!("/w/rest.php/triplespace/v0/export/{job}"),
            None,
        )
        .await
    }

    /// `action=import` of a file, on the request path.
    async fn import(&mut self, xml: &[u8]) -> Value {
        const B: &str = "----triplespace-export-job-test";
        let csrf = self.token("csrf").await;
        let mut body = Vec::new();
        for (k, v) in [("token", csrf.as_str()), ("interwikiprefix", "librarybase")] {
            body.extend_from_slice(
                format!("--{B}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                    .as_bytes(),
            );
        }
        body.extend_from_slice(
            format!("--{B}\r\nContent-Disposition: form-data; name=\"xml\"; filename=\"export.xml\"\r\nContent-Type: application/xml\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(xml);
        body.extend_from_slice(format!("\r\n--{B}--\r\n").as_bytes());
        self.send(
            "POST",
            "/w/api.php?action=import&format=json&formatversion=2",
            Some((format!("multipart/form-data; boundary={B}"), body)),
        )
        .await
        .json()
    }

    async fn token(&mut self, kind: &str) -> String {
        self.api(&format!("action=query&meta=tokens&type={kind}"))
            .await["query"]["tokens"][format!("{kind}token")]
        .as_str()
        .unwrap()
        .to_string()
    }

    async fn login(&mut self, name: &str) {
        let token = self.token("login").await;
        let r = self
            .post(
                "action=clientlogin",
                &format!(
                    "username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
                    enc(name),
                    enc(PASSWORD),
                    enc(&token)
                ),
            )
            .await;
        assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    }

    /// Registers and logs in.
    async fn register(&mut self, name: &str) {
        let v = self
            .api("action=query&meta=authmanagerinfo&amirequestsfor=create&amimergerequestfields=1")
            .await;
        let challenge = v["query"]["authmanagerinfo"]["fields"]["tschallengeid"]["value"]
            .as_str()
            .map(|id| {
                let bits = scatter_challenge::Challenge::parse(id).unwrap().bits;
                format!(
                    "&tschallengeid={}&tschallengesolution={}",
                    enc(id),
                    scatter_challenge::solve(id, bits)
                )
            })
            .unwrap_or_default();
        let token = self.token("createaccount").await;
        let r = self
            .post(
                "action=createaccount",
                &format!(
                    "username={}&password={}&retype={}&createtoken={}&createreturnurl=https://{HOST}/{challenge}",
                    enc(name),
                    enc(PASSWORD),
                    enc(PASSWORD),
                    enc(&token)
                ),
            )
            .await;
        assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
        self.login(name).await;
    }
}

fn now() -> u64 {
    triplespace_api_action::edit::now_micros()
}

/// One pass of a runner that runs exports: the job it ran.
async fn run_pass(app: &App) -> jobs::Ran {
    let runner = jobs::Runner::new("t").with_work(jobs::Kind::Export, export_work(app.clone()));
    Box::pin(jobs::run_once(app.store(), app.pipeline(), &runner, now))
        .await
        .expect("the pass")
        .expect("a job waited")
}

/// Asks for an export over the limit as `client`: the job it was queued as.
async fn queue_export(client: &mut Client, body: &Value) -> u64 {
    let got = client.export(body).await;
    assert_eq!(got.status, StatusCode::ACCEPTED, "{}", got.text());
    let v = got.json();
    assert_eq!(v["status"], "queued", "{v}");
    let job = v["job"].as_u64().expect("a job ID");
    let url = format!("https://{HOST}/w/rest.php/triplespace/v0/export/{job}");
    assert_eq!(v["url"], url, "{v}");
    assert_eq!(got.header(header::LOCATION), url);
    job
}

/// `(status, source, counts)` of a job in `view.job`.
async fn job_row(database: &str, id: u64) -> (String, String, Value) {
    let rows = query(
        database,
        &format!(
            "SELECT status, source, coalesce(counts, '{{}}'::jsonb) FROM view.job WHERE tenant = '{TENANT}' AND job_id = {id}"
        ),
    )
    .await;
    let r = rows.first().expect("the job has a record");
    (r.get(0), r.get(1), r.get(2))
}

/// The file of a job in the `fs` blob store under `blobs`.
fn file_of(blobs: &Path, job: u64, name: &str) -> PathBuf {
    blobs
        .join(TENANT)
        .join("job")
        .join(job.to_string())
        .join(name)
}

/// Makes a file look written `days` days ago.
fn backdate(file: &Path, days: u64) {
    let then = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(then)
        .unwrap();
}

fn pages_in(xml: &[u8]) -> (usize, usize) {
    let pages: Vec<scatter_mediawiki_xml::DumpPage> = scatter_mediawiki_xml::XmlDump::new(xml)
        .map(|p| p.expect("a well-formed page"))
        .collect();
    (
        pages.len(),
        pages.iter().map(|p| p.revisions.len()).sum::<usize>(),
    )
}

/// A selection over `export.sync_revisions` is queued for a signed-in account, run, and
/// fetched by that account alone; anonymous callers are refused as before.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_large_export_runs_as_a_job() {
    let Some((database, key_file, dir)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let blobs = dir.join("blobs");
    let (app, api) = app(&database, &key_file, Some(&blobs)).await;
    let mut owner = Client::new(&api);
    owner.login("Owner").await;
    let imported = owner.import(&dump_xml()).await;
    assert_eq!(imported["error"], Value::Null, "{imported}");
    let mut alice = Client::new(&api);
    alice.register("Alice").await;
    let mut anon = Client::new(&api);

    // Two pages with their history, and a title that names nothing.
    let selection = json!({
        "pages": ["Project:Home", "Project:Scope", "No such page"],
        "history": true,
    });
    let whole = owner.export(&selection).await;
    assert_eq!(whole.status, StatusCode::OK, "{}", whole.text());
    let (pages, revisions) = pages_in(&whole.body);
    assert_eq!(pages, 2);
    assert!(revisions > 2, "{revisions} revisions");
    assert_eq!(whole.headers["x-export-missing"], "1");

    // Over the limit: an anonymous caller is refused, as before.
    set_setting(&database, "export.sync_revisions", "1").await;
    let refused = anon.export(&selection).await;
    assert_eq!(refused.status, StatusCode::PAYLOAD_TOO_LARGE);
    let refusal = refused.json();
    assert_eq!(refusal["code"], "export-too-large", "{refusal}");
    assert!(
        refusal["message"].as_str().unwrap().contains("sign in"),
        "{refusal}"
    );
    // So is anyone on a server that keeps no blob store.
    let (_, bare) = Box::pin(self::app(&database, &key_file, None)).await;
    let mut owner_bare = Client {
        router: bare,
        cookie: owner.cookie.clone(),
    };
    let refused = owner_bare.export(&selection).await;
    assert_eq!(refused.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(refused.text().contains("keeps no job output"));
    assert_eq!(owner_bare.fetch(1).await.status, StatusCode::NOT_FOUND);

    // A signed-in caller's is queued (21 §4.2): 202 with the job and where to fetch it.
    let job = queue_export(&mut owner, &selection).await;
    let params: Value = query(
        &database,
        &format!("SELECT params FROM ops.job_queue WHERE job_id = {job} AND kind = 'export'"),
    )
    .await[0]
        .get(0);
    assert_eq!(params["format"], "xml", "the format is written out");
    assert_eq!(params["pages"], selection["pages"]);
    let waiting = owner.fetch(job).await;
    assert_eq!(waiting.status, StatusCode::ACCEPTED);
    assert_eq!(waiting.json(), json!({"job": job, "status": "queued"}));
    // Nobody else learns that it exists.
    for other in [&mut alice, &mut anon] {
        let got = other.fetch(job).await;
        assert_eq!(got.status, StatusCode::NOT_FOUND);
        assert_eq!(got.json()["code"], "export-not-found");
    }

    // A runner pass writes the file into the blob store and finishes the job.
    let ran = run_pass(&app).await;
    assert_eq!((ran.job_id, ran.kind), (job, jobs::Kind::Export));
    assert_eq!(ran.error, None);
    let (status, source, counts) = job_row(&database, job).await;
    assert_eq!((status.as_str(), source.as_str()), ("finished", "export"));
    let file = file_of(&blobs, job, "export.xml");
    let on_disk = std::fs::read(&file).expect("the file is in the blob store");
    assert_eq!(counts["export"]["format"], "xml", "{counts}");
    assert_eq!(counts["export"]["file"], "export.xml");
    assert_eq!(counts["export"]["bytes"], on_disk.len());
    assert_eq!(counts["export"]["missing"], 1);

    // The submitter fetches what an export in place would have sent.
    let fetched = owner.fetch(job).await;
    assert_eq!(fetched.status, StatusCode::OK, "{}", fetched.text());
    assert_eq!(
        fetched.header(header::CONTENT_TYPE),
        "application/xml; charset=utf-8"
    );
    assert_eq!(fetched.header(header::CACHE_CONTROL), "private, no-store");
    assert_eq!(
        fetched.header(header::CONTENT_DISPOSITION),
        format!("attachment; filename=\"{TENANT}-{job}.xml\"")
    );
    assert!(fetched.text().trim_end().ends_with("</mediawiki>"));
    assert_eq!(pages_in(&fetched.body), (pages, revisions));
    assert_eq!(fetched.text(), whole.text(), "the same document");
    // The site serves it too (21 §4.2): `Special:Export?job=N` is the submitter's download,
    // passed on from the API as it arrives, and nobody else's.
    let site =
        api.clone()
            .fallback_service(triplespace_ui::router(triplespace_client::Client::new(
                triplespace_client::ServiceTransport::new(api.clone()),
            )));
    let mut on_site = Client {
        router: site.clone(),
        cookie: owner.cookie.clone(),
    };
    let page = on_site
        .send("GET", &format!("/wiki/Special:Export?job={job}"), None)
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(page.body, fetched.body, "the file, through the site");
    assert!(page.header(header::CONTENT_DISPOSITION).contains(".xml"));
    let mut others = Client {
        router: site,
        cookie: alice.cookie.clone(),
    };
    let page = others
        .send("GET", &format!("/wiki/Special:Export?job={job}"), None)
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(
        page.text().contains("There is no such export of yours."),
        "another reader gets the form and a refusal"
    );
    for other in [&mut alice, &mut anon] {
        assert_eq!(other.fetch(job).await.status, StatusCode::NOT_FOUND);
    }
    // An ID that is no export job, and one that is no ID, are the same refusal.
    assert_eq!(owner.fetch(job + 1000).await.status, StatusCode::NOT_FOUND);

    // The `records` format goes the same way.
    let records = queue_export(
        &mut owner,
        &json!({"format": "records", "pages": ["Project:Home"], "history": true}),
    )
    .await;
    assert_eq!(run_pass(&app).await.error, None);
    let fetched = owner.fetch(records).await;
    assert_eq!(fetched.status, StatusCode::OK, "{}", fetched.text());
    assert_eq!(
        fetched.header(header::CONTENT_TYPE),
        scatter_integrity::records::MEDIA_TYPE
    );
    assert_eq!(
        fetched.header(header::CONTENT_DISPOSITION),
        format!("attachment; filename=\"{TENANT}-{records}.cbor\"")
    );
    assert_eq!(
        fetched.body,
        std::fs::read(file_of(&blobs, records, "export.cbor")).unwrap()
    );
    let (_, _, counts) = job_row(&database, records).await;
    assert_eq!(counts["export"]["format"], "records", "{counts}");
    assert_eq!(alice.fetch(records).await.status, StatusCode::NOT_FOUND);

    // A job that fails says so to its submitter: here, a request that names nothing.
    let actor: String = query(
        &database,
        &format!("SELECT actor_key FROM view.job WHERE tenant = '{TENANT}' AND job_id = {job}"),
    )
    .await[0]
        .get(0);
    let broken = jobs::submit(
        app.store(),
        &jobs::Submission {
            tenant: TENANT.into(),
            kind: jobs::Kind::Export,
            params: json!({"format": "xml", "pages": []}),
            payload: None,
            actor,
        },
    )
    .await
    .unwrap();
    assert!(run_pass(&app).await.error.is_some());
    let failed = owner.fetch(broken).await;
    assert_eq!(failed.status, StatusCode::OK);
    assert_eq!(failed.json()["status"], "failed", "{}", failed.text());
    assert!(
        failed.json()["error"]
            .as_str()
            .unwrap()
            .contains("no-pages"),
        "{}",
        failed.text()
    );
    assert_eq!(alice.fetch(broken).await.status, StatusCode::NOT_FOUND);

    // `export.keep_days` (7): the next export job's sweep removes a file older than it,
    // and leaves the others.
    backdate(&file, 8);
    let later = queue_export(&mut owner, &selection).await;
    assert_eq!(run_pass(&app).await.error, None);
    assert!(!file.exists(), "the sweep removed the old file");
    assert!(file_of(&blobs, records, "export.cbor").exists());
    let gone = owner.fetch(job).await;
    assert_eq!(gone.status, StatusCode::GONE);
    assert_eq!(gone.json()["code"], "export-expired");
    assert_eq!(alice.fetch(job).await.status, StatusCode::NOT_FOUND);
    // A file past its keeping is gone to a fetch even where no sweep has met it.
    assert_eq!(owner.fetch(later).await.status, StatusCode::OK);
    set_setting(&database, "export.keep_days", "2").await;
    assert_eq!(owner.fetch(later).await.status, StatusCode::OK);
    backdate(&file_of(&blobs, later, "export.xml"), 3);
    assert_eq!(owner.fetch(later).await.status, StatusCode::GONE);
    assert!(!file_of(&blobs, later, "export.xml").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
