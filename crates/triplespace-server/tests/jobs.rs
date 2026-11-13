//! The job runner (0104 §4, §8; 16 §2.2; 03 §4.12): an import over `import.sync_pages`
//! submitted through `action=import` is queued with its job ID, listed by `GET /jobs`
//! and shown `queued` on `Special:Jobs/{id}`; a runner pass claims and runs it, after
//! which the job is `finished` with its counts, its owner has a `job` notification
//! (16 §5.2), and the feed links the job's page; a run whose runner died is found by
//! the next claim, marked `job/interrupt` and run again under a new ID. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.
//!
//! 0104 §4, §6, §8, §9

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_ingest::job::Job;
use scatter_ingest::write::Attestation;
use scatter_projection::Backend as _;
use scatter_wikibase_changeset::{JobHeader, Mode};
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode as ServerMode, router_with};
use triplespace_api_ingest::jobs;
use triplespace_cli::{instance, settings};

const DB: &str = "tsjobs_test";
const TENANT: &str = "wiki";
const HOST: &str = "wiki.example";
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

/// The app over the database, and the Action API and REST routes.
async fn app(database: &str, key_file: &std::path::Path) -> (App, Router) {
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
    let router = router_with(app.clone(), triplespace_api_rest::routes());
    (app, router)
}

/// A client of the tenant's API and site: one session cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
}

/// What was answered.
struct Got {
    status: StatusCode,
    body: Vec<u8>,
}

impl Got {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

impl Client {
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
        let body = r.into_body().collect().await.unwrap().to_bytes().to_vec();
        Got { status, body }
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

    async fn rest(&mut self, path: &str) -> Got {
        self.send("GET", &format!("/w/rest.php/triplespace/v0{path}"), None)
            .await
    }

    /// `action=import` with a multipart body: the fields, then the file as `xml`.
    async fn import(&mut self, fields: &[(&str, &str)], xml: &[u8]) -> Value {
        const B: &str = "----triplespace-jobs-test";
        let mut body = Vec::new();
        for (k, v) in fields {
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
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{B}--\r\n").as_bytes());
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
        let enc =
            |s: &str| -> String { url::form_urlencoded::byte_serialize(s.as_bytes()).collect() };
        let r = self
            .post(
                "action=clientlogin",
                &format!(
                    "username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
                    enc(name),
                    enc("owner-secret-1234"),
                    enc(&token)
                ),
            )
            .await;
        assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    }
}

/// An import over the threshold is queued with its ID, run by a runner pass, reported on
/// its page and in its owner's inbox; a dead run is found and run again.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_large_import_runs_through_the_queue() {
    let Some((database, key_file)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let (app, api) = app(&database, &key_file).await;
    let site =
        api.clone()
            .fallback_service(triplespace_ui::router(triplespace_client::Client::new(
                triplespace_client::ServiceTransport::new(api.clone()),
            )));
    let xml = dump_xml();

    // Ten pages run while the caller waits; this file's 39 do not (0104 §4).
    settings::run(settings::Settings::Set(settings::Set {
        at: settings::Where {
            database: database.clone(),
            tenant: Some(TENANT.into()),
            instance: false,
        },
        key: "import.sync_pages".into(),
        value: "10".into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await
    .expect("set import.sync_pages");
    let mut owner = Client {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner").await;
    let si = owner
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(si["query"]["triplespace"]["import"]["sync_pages"], 10);
    let csrf = owner.token("csrf").await;
    let queued = owner
        .import(
            &[
                ("token", &csrf),
                ("interwikiprefix", "librarybase"),
                ("summary", "the library's pages"),
            ],
            &xml,
        )
        .await;
    assert_eq!(queued["error"], Value::Null, "{queued}");
    assert_eq!(queued["job"]["status"], "queued", "{queued}");
    assert_eq!(queued["job"]["pages"], 39);
    let id = queued["job"]["id"].as_u64().expect("a job ID");
    assert_eq!(queued["job"]["page"], format!("Special:Jobs/{id}"));
    assert!(queued["import"].as_array().unwrap().is_empty());
    assert_eq!(
        query(
            &database,
            "SELECT count(*) FROM view.page WHERE tenant = 'wiki'"
        )
        .await[0]
            .get::<_, i64>(0),
        0,
        "nothing is written at submission"
    );

    // The queue, as the API and the pages show it (0104 §6, §9).
    let listed = owner.rest("/jobs").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text());
    let jobs = listed.json()["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0]["id"], id);
    assert_eq!(jobs[0]["status"], "queued");
    assert_eq!(jobs[0]["position"], 1);
    assert_eq!(jobs[0]["kind"], "import");
    assert_eq!(jobs[0]["actor"]["name"], "Owner");
    let one = owner.rest(&format!("/jobs/{id}")).await.json();
    assert_eq!(one["status"], "queued", "{one}");
    assert_eq!(one["params"]["source"], "upload");
    assert_eq!(one["links"]["page"], format!("/wiki/Special:Jobs/{id}"));
    let missing = owner.rest("/jobs/999999").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.json()["code"], "job-not-found");
    let page = owner
        .send("GET", &format!("/wiki/Special:Jobs/{id}"), None)
        .await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.text());
    let html = page.text();
    assert!(html.contains(&format!("Job {id}")), "{html}");
    assert!(html.contains("Queued"), "{html}");
    assert!(html.contains("1 in the queue"), "{html}");
    let list = owner.send("GET", "/wiki/Special:Jobs", None).await.text();
    assert!(
        list.contains(&format!("href=\"/wiki/Special:Jobs/{id}\"")),
        "{list}"
    );
    let gone = owner.send("GET", "/wiki/Special:Jobs/999999", None).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);

    // A runner pass: the job is claimed, run under its ID, and finished (0104 §4).
    let ran = jobs::run_once(
        app.store(),
        app.pipeline(),
        &jobs::Runner::new("runner test"),
        triplespace_cli::common::now,
    )
    .await
    .expect("the pass")
    .expect("a job waited");
    assert_eq!(ran.job_id, id);
    assert_eq!(ran.error, None);
    assert_eq!(
        jobs::run_once(
            app.store(),
            app.pipeline(),
            &jobs::Runner::new("runner test"),
            triplespace_cli::common::now
        )
        .await
        .unwrap()
        .map(|r| r.job_id),
        None,
        "the queue is empty"
    );
    let job = query(
        &database,
        &format!(
            "SELECT status, mode, source, (counts->>'created')::int, params->>'summary' FROM view.job WHERE tenant = 'wiki' AND job_id = {id}"
        ),
    )
    .await;
    assert_eq!(job.len(), 1);
    assert_eq!(job[0].get::<_, String>(0), "finished");
    assert_eq!(job[0].get::<_, String>(1), "import");
    assert_eq!(job[0].get::<_, String>(2), "upload");
    assert_eq!(job[0].get::<_, i32>(3), 33);
    assert_eq!(job[0].get::<_, String>(4), "the library's pages");
    assert_eq!(
        query(
            &database,
            "SELECT count(*) FROM view.page WHERE tenant = 'wiki' AND NOT deleted"
        )
        .await[0]
            .get::<_, i64>(0),
        38, // the 33 pages and the talk pages the conversion made
    );
    let done = owner.rest(&format!("/jobs/{id}")).await.json();
    assert_eq!(done["status"], "finished", "{done}");
    assert_eq!(done["counts"]["created"], 33);
    assert_eq!(done["counts"]["import"]["revisions"], 140);
    assert_eq!(done["graph"], "pages");
    assert!(done["started"].as_str().is_some_and(|s| s.ends_with('Z')));
    let html = owner
        .send("GET", &format!("/wiki/Special:Jobs/{id}"), None)
        .await
        .text();
    assert!(html.contains("Finished"), "{html}");
    assert!(html.contains("<dt>Created</dt><dd>33</dd>"), "{html}");
    assert!(html.contains("Changes in this job"), "{html}");

    // The feed links the job's page: its row, and the Imported chip's "Job n" on an
    // imported revision (0104 §9).
    let changes = owner
        .send(
            "GET",
            "/w/index.php?title=Special:RecentChanges&filtered=1&kind=job&bots=1&days=30",
            None,
        )
        .await
        .text();
    assert!(
        changes.contains(&format!("href=\"/wiki/Special:Jobs/{id}\"")),
        "{changes}"
    );

    // The owner is told the job finished (16 §5.2, reason `job`): the addressing step
    // runs behind the write, as the server runs it.
    app.catch_up_async().await.expect("addressing");
    let inbox = owner
        .api("action=query&meta=notifications&notprop=list&notlimit=50")
        .await;
    let items = inbox["query"]["notifications"]["list"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let job_item = items
        .iter()
        .find(|i| i["ts-reason"] == "job")
        .unwrap_or_else(|| panic!("a job notification in {inbox}"));
    assert_eq!(job_item["title"]["full"], format!("Special:Jobs/{id}"));
    assert_eq!(job_item["ts-job"], id);
    let page = owner
        .send("GET", "/wiki/Special:Notifications", None)
        .await
        .text();
    assert!(
        page.contains(&format!(
            "<a href=\"/wiki/Special:Jobs/{id}\">Job {id}</a> finished."
        )),
        "{page}"
    );

    // A run whose runner died: the row is claimed, the start record written, and then
    // nothing. The next claim finds the stale heartbeat, marks the dead run interrupted
    // and runs the job again under a new ID, which finds every page present (0104 §4).
    let store = app.store();
    let dead = jobs::submit(
        store,
        &jobs::Submission {
            tenant: TENANT.into(),
            kind: jobs::Kind::Import,
            params: serde_json::to_value(triplespace_api_ingest::import::ImportRequest {
                source: "upload".into(),
                version: None,
                prefix: "librarybase".into(),
                issuer: Some("librarybase".into()),
                summary: None,
                tags: Vec::new(),
                skip_talk: false,
            })
            .unwrap(),
            payload: Some(xml.clone()),
            actor: "local:1".into(),
        },
    )
    .await
    .expect("submit");
    let claimed = jobs::claim(
        store,
        app.pipeline(),
        &jobs::Runner::new("runner dead"),
        triplespace_cli::common::now(),
    )
    .await
    .expect("claim")
    .expect("the dead job waited");
    assert_eq!(claimed.job_id, dead);
    assert_eq!(claimed.resumes, None);
    {
        let mut cx = store.begin().await.unwrap();
        let mut header = JobHeader::local("upload");
        header.mode = Some(Mode::Import);
        header.graph = "pages".into();
        Job::start_as(
            store,
            app.pipeline(),
            &mut cx,
            TENANT,
            dead,
            header,
            Attestation::by("local:1"),
            triplespace_cli::common::now(),
        )
        .await
        .expect("the dead run's start record");
        store.commit(cx).await.unwrap();
    }
    query(
        &database,
        &format!(
            "UPDATE ops.job_queue SET heartbeat = now() - interval '10 minutes', claimed_at = now() - interval '10 minutes' WHERE job_id = {dead}"
        ),
    )
    .await;
    let shown = owner.rest(&format!("/jobs/{dead}")).await.json();
    assert_eq!(
        shown["status"], "queued",
        "a stale claim waits again: {shown}"
    );
    let again = jobs::run_once(
        app.store(),
        app.pipeline(),
        &jobs::Runner::new("runner test"),
        triplespace_cli::common::now,
    )
    .await
    .expect("the pass")
    .expect("the dead job waited");
    assert_ne!(again.job_id, dead, "run again under a new ID");
    assert_eq!(again.error, None);
    let rows = query(
        &database,
        &format!(
            "SELECT job_id, status, counts FROM view.job WHERE tenant = 'wiki' AND job_id IN ({dead}, {}) ORDER BY job_id",
            again.job_id
        ),
    )
    .await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<_, String>(1), "interrupted");
    assert_eq!(
        rows[0].get::<_, Value>(2)["by"].as_u64(),
        Some(again.job_id),
        "the interrupt names the run that noticed"
    );
    assert_eq!(rows[1].get::<_, String>(1), "finished");
    assert_eq!(
        rows[1].get::<_, Value>(2)["import"]["present"],
        140,
        "nothing written twice"
    );
    assert_eq!(
        query(&database, "SELECT count(*) FROM ops.job_queue").await[0].get::<_, i64>(0),
        0,
        "the queue is empty again"
    );
    let interrupted = owner.rest(&format!("/jobs/{dead}")).await.json();
    assert_eq!(interrupted["status"], "interrupted");
    assert_eq!(interrupted["interrupted_by"], again.job_id);

    // Cancelling: a queued job is removed; a finished one is not in the queue.
    let waiting = jobs::submit(
        store,
        &jobs::Submission {
            tenant: TENANT.into(),
            kind: jobs::Kind::Import,
            params: serde_json::json!({"source": "upload", "prefix": "imported"}),
            payload: Some(b"<mediawiki></mediawiki>".to_vec()),
            actor: "local:1".into(),
        },
    )
    .await
    .unwrap();
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    assert!(jobs::cancel(&client, TENANT, waiting).await.unwrap());
    assert!(!jobs::cancel(&client, TENANT, id).await.unwrap());
    assert!(!jobs::cancel(&client, TENANT, waiting).await.unwrap());

    // A job that fails on its input: its row goes, and its failure is its own record.
    // A file that is not XML fails before the import's start record, and gets both
    // records, so the ID the submitter holds is a failed job (0104 §4, A7).
    let broken = jobs::submit(
        store,
        &jobs::Submission {
            tenant: TENANT.into(),
            kind: jobs::Kind::Import,
            params: serde_json::json!({"source": "upload", "prefix": "imported"}),
            payload: Some(b"not xml at all".to_vec()),
            actor: "local:1".into(),
        },
    )
    .await
    .unwrap();
    let failed = jobs::run_once(
        app.store(),
        app.pipeline(),
        &jobs::Runner::new("runner test"),
        triplespace_cli::common::now,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(failed.job_id, broken);
    assert!(failed.error.is_some(), "{failed:?}");
    assert_eq!(
        query(&database, "SELECT count(*) FROM ops.job_queue").await[0].get::<_, i64>(0),
        0
    );
    let record = owner.rest(&format!("/jobs/{broken}")).await;
    assert_eq!(
        record.status,
        StatusCode::OK,
        "a job that failed before it started is a failed job: {}",
        record.text()
    );
    let record = record.json();
    assert_eq!(record["status"], "failed", "{record}");
    let job = query(
        &database,
        &format!(
            "SELECT mode, source, counts->>'error' FROM view.job WHERE tenant = 'wiki' AND job_id = {broken}"
        ),
    )
    .await;
    assert_eq!(job[0].get::<_, String>(0), "import");
    assert_eq!(job[0].get::<_, String>(1), "upload");
    assert_eq!(job[0].get::<_, Option<String>>(2), failed.error);
    let page = owner
        .send("GET", &format!("/wiki/Special:Jobs/{broken}"), None)
        .await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.text());

    // Paused: nothing is claimed.
    settings::run(settings::Settings::Set(settings::Set {
        at: settings::Where {
            database: database.clone(),
            tenant: None,
            instance: true,
        },
        key: "jobs.paused".into(),
        value: "true".into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await
    .expect("pause");
    assert!(jobs::paused(&client).await.unwrap());
}

// 0104 §4 (A6): a dump is a job the runner carries, and a runner claims only its kinds.
#[tokio::test]
async fn a_dump_runs_through_the_queue() {
    let Some((database, key_file)) = setup("tsjobs_dump").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let (app, _api) = app(&database, &key_file).await;
    let now = triplespace_cli::common::now;
    let dumps = std::env::temp_dir().join(format!("tsjobs-dumps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dumps);

    // A row of a kind no runner here runs, older than every other: it waits, and holds
    // nothing back.
    let client = triplespace_cli::common::connect(&database).await.unwrap();
    client
        .execute(
            "INSERT INTO ops.job_queue (tenant, job_id, partition, kind, params, actor_key, submitted)
             SELECT $1, 900000, partition, 'adopt', '{}', 'local:1', now() - interval '1 hour'
             FROM log.partition WHERE tenant = $1 AND name = 'pages'",
            &[&TENANT],
        )
        .await
        .unwrap();
    let id = jobs::submit(
        app.store(),
        &jobs::Submission {
            tenant: TENANT.into(),
            kind: jobs::Kind::Dump,
            params: serde_json::json!({"keep": 2}),
            payload: None,
            actor: "local:1".into(),
        },
    )
    .await
    .expect("submit");

    // A runner without a dumps directory does not claim it.
    let plain = jobs::Runner::new("runner plain");
    assert!(
        jobs::run_once(app.store(), app.pipeline(), &plain, now)
            .await
            .unwrap()
            .is_none()
    );
    // One with a directory runs it under its ID: the start record, the files, the finish.
    let runner = jobs::Runner::new("runner dumps").with_dumps(&dumps);
    let ran = jobs::run_once(app.store(), app.pipeline(), &runner, now)
        .await
        .expect("the pass")
        .expect("the dump waited");
    assert_eq!((ran.job_id, ran.kind), (id, jobs::Kind::Dump));
    assert_eq!(ran.error, None);
    let job = query(
        &database,
        &format!("SELECT status, source FROM view.job WHERE tenant = '{TENANT}' AND job_id = {id}"),
    )
    .await;
    assert_eq!(job.len(), 1);
    assert_eq!(job[0].get::<_, String>(0), "finished");
    assert_eq!(job[0].get::<_, String>(1), "dump");
    let written: Vec<_> = std::fs::read_dir(dumps.join(TENANT))
        .expect("the tenant's dump directory")
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(written[0].join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["tenant"], TENANT);
    // The dump's row is gone; the row of the other kind still waits.
    let left = query(&database, "SELECT kind FROM ops.job_queue").await;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].get::<_, String>(0), "adopt");
    let _ = std::fs::remove_dir_all(&dumps);
}

/// A job whose file is not XML: an import of it fails as soon as it runs.
fn broken_job(tenant: &str, kind: jobs::Kind) -> jobs::Submission {
    jobs::Submission {
        tenant: tenant.into(),
        kind,
        params: serde_json::json!({"source": "upload", "prefix": "imported", "reverts": 1}),
        payload: Some(b"not xml at all".to_vec()),
        actor: "local:1".into(),
    }
}

/// `(status, counts)` of a job in `view.job`.
async fn job_row(database: &str, tenant: &str, id: u64) -> Option<(String, Value)> {
    query(
        database,
        &format!(
            "SELECT status, coalesce(counts, '{{}}'::jsonb) FROM view.job WHERE tenant = '{tenant}' AND job_id = {id}"
        ),
    )
    .await
    .first()
    .map(|r| (r.get(0), r.get(1)))
}

/// Makes a row a dead runner's: claimed `attempts` times, its heartbeat long stale.
async fn stale(database: &str, id: u64, attempts: u32) {
    query(
        database,
        &format!(
            "UPDATE ops.job_queue SET claimed_by = 'runner dead', attempts = {attempts},
                    heartbeat = now() - interval '10 minutes', claimed_at = now() - interval '10 minutes'
             WHERE job_id = {id}"
        ),
    )
    .await;
}

async fn queue_len(database: &str) -> i64 {
    query(database, "SELECT count(*) FROM ops.job_queue").await[0].get::<_, i64>(0)
}

// 0104 §4 (A7): one running job per partition holds between concurrent claimers, and a
// claimer that finds a partition taken moves on; a job claimed `jobs.max_attempts` times
// is failed, not run again; a runner whose claim was taken writes nothing about the row.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn claims_are_one_per_partition_and_capped() {
    let Some((database, key_file)) = setup("tsjobs_claims").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let (app, _api) = app(&database, &key_file).await;
    let now = triplespace_cli::common::now;
    let (store, pipeline) = (app.store(), app.pipeline());
    // A second runner with a store of its own, as the server gives each.
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let (store_b, pipeline_b) = triplespace_cli::common::store(&database, &farm).unwrap();
    let (a, b) = (jobs::Runner::new("runner a"), jobs::Runner::new("runner b"));
    let import = broken_job(TENANT, jobs::Kind::Import);

    // Two jobs of one tenant and, younger than both, one of the instance.
    let first = jobs::submit(store, &import).await.unwrap();
    let second = jobs::submit(store, &import).await.unwrap();
    let other = jobs::submit(store, &broken_job("", jobs::Kind::Revert))
        .await
        .unwrap();

    // Runner A, stopped in the middle of its claim of the first: the partition's claim
    // lock taken, the row locked and written as claimed, and nothing committed.
    let first_id = i64::try_from(first).unwrap();
    let mut conn_a = triplespace_cli::common::connect(&database).await.unwrap();
    let tx = conn_a.transaction().await.unwrap();
    let partition: i64 = tx
        .query_one(
            "SELECT partition FROM ops.job_queue WHERE job_id = $1 FOR UPDATE",
            &[&first_id],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        tx.query_one(jobs::PARTITION_LOCK, &[&partition])
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    tx.execute(
        "UPDATE ops.job_queue SET claimed_by = 'runner a', claimed_at = now(), heartbeat = now(), attempts = attempts + 1
         WHERE job_id = $1",
        &[&first_id],
    )
    .await
    .unwrap();
    // Runner B meanwhile: not the second job of that partition, which it cannot yet see
    // has a running job, but the job of the other partition; and then nothing.
    let moved_on = jobs::claim(&store_b, &pipeline_b, &b, now())
        .await
        .unwrap()
        .expect("a claimer that loses a partition moves on to another");
    assert_eq!(
        (moved_on.tenant.as_str(), moved_on.job_id),
        ("", other),
        "not the second job of a partition being claimed"
    );
    assert!(
        jobs::claim(&store_b, &pipeline_b, &b, now())
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    // A's claim is committed: the partition has its running job, and the second waits.
    assert!(
        jobs::claim(&store_b, &pipeline_b, &b, now())
            .await
            .unwrap()
            .is_none()
    );
    // Until the first has finished.
    query(
        &database,
        &format!("DELETE FROM ops.job_queue WHERE job_id IN ({first}, {other})"),
    )
    .await;
    let next = jobs::claim(&store_b, &pipeline_b, &b, now())
        .await
        .unwrap()
        .expect("the second, once the first is done");
    assert_eq!(next.job_id, second);
    query(&database, "DELETE FROM ops.job_queue").await;

    // Two claimers at once, over and over: never both.
    for round in 0..20 {
        let one = jobs::submit(store, &import).await.unwrap();
        let two = jobs::submit(store, &import).await.unwrap();
        let (now_a, now_b) = (now(), now());
        let (got_a, got_b) = tokio::join!(
            jobs::claim(store, pipeline, &a, now_a),
            jobs::claim(&store_b, &pipeline_b, &b, now_b),
        );
        let got: Vec<u64> = [got_a.unwrap(), got_b.unwrap()]
            .into_iter()
            .flatten()
            .map(|c| c.job_id)
            .collect();
        assert!(
            got.len() <= 1,
            "round {round}: both of {one} and {two} were claimed"
        );
        // Where each found the partition taken by the other, the next pass claims.
        if got.is_empty() {
            let c = jobs::claim(store, pipeline, &a, now()).await.unwrap();
            assert_eq!(c.map(|c| c.job_id), Some(one));
        } else {
            assert_eq!(got, vec![one]);
        }
        assert!(
            jobs::claim(&store_b, &pipeline_b, &b, now())
                .await
                .unwrap()
                .is_none()
        );
        query(&database, "DELETE FROM ops.job_queue").await;
    }

    // The cap (A7). A job whose runs die before their start record, claimed three times:
    // the next claim fails it under the ID it was submitted with, and removes its row.
    let crash = jobs::submit(store, &import).await.unwrap();
    stale(&database, crash, 3).await;
    assert!(
        jobs::claim(store, pipeline, &a, now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(queue_len(&database).await, 0, "its row is removed");
    let (status, counts) = job_row(&database, TENANT, crash)
        .await
        .expect("the job exists");
    assert_eq!(status, "failed");
    assert!(
        counts["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("given up after 3 runs")),
        "{counts}"
    );

    // One whose last run had started: that run is interrupted as ever, by the job that
    // is failed in its place.
    let crash = jobs::submit(store, &import).await.unwrap();
    let claimed = jobs::claim(store, pipeline, &a, now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.job_id, crash);
    {
        let mut cx = store.begin().await.unwrap();
        let mut header = JobHeader::local("upload");
        header.mode = Some(Mode::Import);
        header.graph = "pages".into();
        Job::start_as(
            store,
            pipeline,
            &mut cx,
            TENANT,
            crash,
            header,
            Attestation::by("local:1"),
            now(),
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
    }
    stale(&database, crash, 3).await;
    assert!(
        jobs::claim(store, pipeline, &a, now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(queue_len(&database).await, 0);
    let (status, counts) = job_row(&database, TENANT, crash).await.unwrap();
    assert_eq!(status, "interrupted");
    let by = counts["by"].as_u64().expect("the job that noticed");
    let (status, counts) = job_row(&database, TENANT, by).await.unwrap();
    assert_eq!(status, "failed", "{counts}");

    // Under the cap, and under a cap the instance raised, a stale claim runs again.
    let again = jobs::submit(store, &import).await.unwrap();
    stale(&database, again, 2).await;
    let claimed = jobs::claim(store, pipeline, &a, now())
        .await
        .unwrap()
        .expect("two claims are under the cap");
    assert_eq!(claimed.resumes, Some(again));
    settings::run(settings::Settings::Set(settings::Set {
        at: settings::Where {
            database: database.clone(),
            tenant: None,
            instance: true,
        },
        key: "jobs.max_attempts".into(),
        value: "5".into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await
    .expect("set jobs.max_attempts");
    stale(&database, claimed.job_id, 4).await;
    let claimed = jobs::claim(store, pipeline, &a, now())
        .await
        .unwrap()
        .expect("four claims are under a cap of five");
    assert_eq!(
        query(&database, "SELECT attempts FROM ops.job_queue").await[0].get::<_, i32>(0),
        5
    );

    // A claim taken while the job ran (its heartbeat had stalled, and another runner
    // claimed the row, which gives it a new ID): the first runner writes nothing about
    // the row, neither a record of the job's end nor the row's removal.
    let lost = claimed.job_id;
    let taken = lost + 1_000_000;
    query(
        &database,
        &format!(
            "UPDATE ops.job_queue SET job_id = {taken}, claimed_by = 'runner b' WHERE job_id = {lost}"
        ),
    )
    .await;
    let ran = jobs::run_claimed(store, pipeline, &a, claimed, now)
        .await
        .unwrap();
    assert!(
        ran.error
            .as_deref()
            .is_some_and(|e| e.starts_with("the claim was lost")),
        "{ran:?}"
    );
    assert_eq!(
        queue_len(&database).await,
        1,
        "the row is the other runner's"
    );
    assert_eq!(job_row(&database, TENANT, lost).await, None);
    assert_eq!(job_row(&database, TENANT, taken).await, None);
}

// 0104 §4 (A8): a job whose claim is taken while it runs is stopped, not left to write
// beside the run that took it; and the unit of work it was in is rolled back, its
// connection closed and not handed to the next unit of work.
#[tokio::test]
async fn a_job_that_loses_its_claim_is_stopped() {
    let Some((database, key_file)) = setup("tsjobs_stop").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let (app, _api) = app(&database, &key_file).await;
    let (store, pipeline) = (app.store(), app.pipeline());
    let now = triplespace_cli::common::now;
    let count = async |sql: &str| query(&database, sql).await[0].get::<_, i64>(0);

    // A unit of work dropped part-way leaves nothing behind, in the database or the pool.
    let cx = store.begin().await.unwrap();
    cx.conn()
        .execute(
            "INSERT INTO ops.visibility_epoch (tenant, epoch) VALUES ('dropped', 1)",
            &[],
        )
        .await
        .unwrap();
    drop(cx);
    for _ in 0..8 {
        let cx = store.begin().await.unwrap();
        let seen: i64 = cx
            .conn()
            .query_one(
                "SELECT count(*) FROM ops.visibility_epoch WHERE tenant = 'dropped'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(seen, 0, "no unit of work runs inside the abandoned one");
        store.commit(cx).await.unwrap();
    }

    // An import of 33 pages, its claim taken before its first heartbeat.
    let pages_before = count("SELECT count(*) FROM view.page").await;
    let id = jobs::submit(
        store,
        &jobs::Submission {
            tenant: TENANT.into(),
            kind: jobs::Kind::Import,
            params: serde_json::to_value(triplespace_api_ingest::import::ImportRequest {
                source: "upload".into(),
                version: None,
                prefix: "imported".into(),
                issuer: None,
                summary: None,
                tags: Vec::new(),
                skip_talk: false,
            })
            .unwrap(),
            payload: Some(dump_xml()),
            actor: "local:1".into(),
        },
    )
    .await
    .unwrap();
    let mut runner = jobs::Runner::new("runner slow");
    runner.heartbeat = std::time::Duration::from_millis(5);
    let claimed = jobs::claim(store, pipeline, &runner, now())
        .await
        .unwrap()
        .expect("the import waited");
    assert_eq!(claimed.job_id, id);
    query(
        &database,
        &format!(
            "UPDATE ops.job_queue SET job_id = {}, claimed_by = 'runner b' WHERE job_id = {id}",
            id + 1_000_000
        ),
    )
    .await;
    let ran = jobs::run_claimed(store, pipeline, &runner, claimed, now)
        .await
        .unwrap();
    let error = ran.error.expect("the run did not end as a job's does");
    assert!(error.contains("stopped"), "{error}");
    let written = count("SELECT count(*) FROM view.page").await - pages_before;
    assert!(
        written < 33,
        "the import was stopped part-way, at {written} pages"
    );
    // Nothing more comes of it: the count stands, and the row is the other runner's.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        count("SELECT count(*) FROM view.page").await - pages_before,
        written
    );
    assert_eq!(count("SELECT count(*) FROM ops.job_queue").await, 1);
    assert_eq!(
        count(&format!(
            "SELECT count(*) FROM view.job WHERE job_id = {id} AND status <> 'running'"
        ))
        .await,
        0,
        "the stopped run wrote no end for the job"
    );
}
