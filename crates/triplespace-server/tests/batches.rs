//! Batches (0104 §1, §2, §3, §5, §6, §7, §9; 16 §2.2; 05 §3.3; 19 §3.10): writes that name
//! a batch value are grouped under one job, keyed by the value's hash bound to the
//! operator, so the same value under two accounts is two batches; a rejected batch
//! refuses further writes and tells its owner; a batch is closed and reopened; a job is
//! reverted record by record under the runner, skipping what changed since, and
//! reverting it again writes nothing. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous
//! without it.
//!
//! 0104 §1, §2, §3, §5, §6, §7, §9

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode as ServerMode, router_with};
use triplespace_api_ingest::jobs;
use triplespace_cli::{instance, settings};

const DB: &str = "tsbatches_test";
const TENANT: &str = "wiki";
const HOST: &str = "wiki.example";
const AUTH: &str = "auth.example";
const REST: &str = "/w/rest.php/triplespace/v0";

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

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// An instance with one tenant and its owner; registration made cheap for Alice.
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
    for (key, value) in [("challenge.bits", "8"), ("challenge.max_bits", "10")] {
        settings::run(settings::Settings::Set(settings::Set {
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
        }))
        .await
        .expect("settings set");
    }
    Some((database, key_file))
}

async fn query(database: &str, sql: &str) -> Vec<tokio_postgres::Row> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client.query(sql, &[]).await.unwrap()
}

async fn app(database: &str, key_file: &std::path::Path) -> (App, Router) {
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(key_file).unwrap();
    let app = App::new(
        triplespace_db::pool::pool(database, 4).unwrap(),
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
    async fn send(&mut self, method: &str, path: &str, body: Option<(&str, Vec<u8>)>) -> Got {
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
                "application/x-www-form-urlencoded",
                form.as_bytes().to_vec(),
            )),
        )
        .await
        .json()
    }

    async fn rest(&mut self, method: &str, path: &str, body: Option<Value>) -> Got {
        let body = body.map(|b| ("application/json", b.to_string().into_bytes()));
        self.send(method, &format!("{REST}{path}"), body).await
    }

    async fn token(&mut self, kind: &str) -> String {
        self.api(&format!("action=query&meta=tokens&type={kind}"))
            .await["query"]["tokens"][format!("{kind}token")]
        .as_str()
        .unwrap()
        .to_string()
    }

    async fn login(&mut self, name: &str, password: &str) {
        let token = self.token("login").await;
        let r = self
            .post(
                "action=clientlogin",
                &format!(
                    "username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
                    enc(name),
                    enc(password),
                    enc(&token)
                ),
            )
            .await;
        assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    }

    /// Registers and logs in.
    async fn register(&mut self, name: &str, password: &str) {
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
                    enc(password),
                    enc(password),
                    enc(&token)
                ),
            )
            .await;
        assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
        self.login(name, password).await;
    }

    /// `wbeditentity` with a batch value; the entity ID and the batch job.
    async fn edit_entity(&mut self, id: Option<&str>, data: &str, batch: &str) -> Value {
        let csrf = self.token("csrf").await;
        let subject = match id {
            Some(id) => format!("id={id}"),
            None => "new=item".to_string(),
        };
        self.post(
            "action=wbeditentity",
            &format!(
                "{subject}&data={}&token={}&batch={}",
                enc(data),
                enc(&csrf),
                enc(batch)
            ),
        )
        .await
    }

    /// `action=edit` with a batch value.
    async fn edit_page(&mut self, title: &str, text: &str, batch: &str) -> Value {
        let csrf = self.token("csrf").await;
        self.post(
            "action=edit",
            &format!(
                "title={}&text={}&token={}{}",
                enc(title),
                enc(text),
                enc(&csrf),
                if batch.is_empty() {
                    String::new()
                } else {
                    format!("&batch={}", enc(batch))
                }
            ),
        )
        .await
    }
}

async fn run_runner(app: &App) -> jobs::Ran {
    jobs::run_once(
        app.store(),
        app.pipeline(),
        "runner test",
        triplespace_cli::common::now,
    )
    .await
    .expect("the pass")
    .expect("a job waited")
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn batches_are_keyed_rejected_closed_and_reverted() {
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
    let mut owner = Client {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner", "owner-secret-1234").await;
    let mut alice = Client {
        router: site.clone(),
        cookie: None,
    };
    alice.register("Alice", "alice-secret-1234").await;

    // The first write naming a value opens the batch; the next ones join it (0104 §1).
    let created = owner
        .edit_entity(
            None,
            r#"{"labels":{"en":{"language":"en","value":"Six"}}}"#,
            "run-2026-11-08",
        )
        .await;
    assert_eq!(created["error"], Value::Null, "{created}");
    let q = created["entity"]["id"].as_str().unwrap().to_string();
    let j1 = created["batch"].as_u64().expect("the batch's job");
    let described = owner
        .edit_entity(
            Some(&q),
            r#"{"descriptions":{"en":{"language":"en","value":"a number"}}}"#,
            "run-2026-11-08",
        )
        .await;
    assert_eq!(described["batch"], j1, "{described}");
    let page = owner
        .edit_page("Project:Thing", "one", "run-2026-11-08")
        .await;
    assert_eq!(page["batch"], j1, "{page}");
    let page = owner
        .edit_page("Project:Thing", "two", "run-2026-11-08")
        .await;
    assert_eq!(page["batch"], j1, "{page}");
    let rows = query(
        &database,
        &format!("SELECT count(*) FROM view.activity WHERE tenant = 'wiki' AND job_id = {j1} AND kind = 'edit'"),
    )
    .await;
    assert_eq!(rows[0].get::<_, i64>(0), 4, "every write carries the job");
    let job = query(
        &database,
        &format!("SELECT mode, status, batch, params->>'operator' FROM view.job WHERE tenant = 'wiki' AND job_id = {j1}"),
    )
    .await;
    assert_eq!(job[0].get::<_, String>(0), "batch");
    assert_eq!(job[0].get::<_, String>(1), "running");
    let hash: String = job[0].get(2);
    assert_eq!(hash.len(), 32, "the hash, never the value: {hash}");
    let operator: String = job[0].get(3);
    assert_eq!(
        hash,
        triplespace_api_ingest::batch::hash("wiki", &operator, "run-2026-11-08")
    );

    // The same value under another operator is another batch (0104 §1).
    let theirs = alice
        .edit_page("Project:Alice", "hers", "run-2026-11-08")
        .await;
    assert_eq!(theirs["error"], Value::Null, "{theirs}");
    let j2 = theirs["batch"].as_u64().unwrap();
    assert_ne!(j2, j1);

    // The job's page shows the hash and the operator, not the value (0104 §6, §9).
    let shown = owner.rest("GET", &format!("/jobs/{j1}"), None).await.json();
    assert_eq!(shown["mode"], "batch", "{shown}");
    assert_eq!(shown["batch"], hash);
    assert_eq!(shown["operator"], operator);
    assert!(!shown.to_string().contains("run-2026-11-08"));
    let html = owner
        .send("GET", &format!("/wiki/Special:Jobs/{j1}"), None)
        .await
        .text();
    assert!(html.contains(&hash), "{html}");
    assert!(!html.contains("run-2026-11-08"));
    assert!(
        html.contains("name=\"wpAction\" value=\"close\""),
        "the owner may close it: {html}"
    );
    assert!(
        html.contains("name=\"wpAction\" value=\"reject\""),
        "and reject it: {html}"
    );

    // Alice cannot reject; the owner rejects Alice's batch, and her next write under it is
    // refused in the write path (0104 §2, §5).
    let csrf_a = alice.token("csrf").await;
    let refused = alice
        .rest(
            "POST",
            &format!("/jobs/{j2}/reject"),
            Some(json!({"reason": "no", "token": csrf_a})),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.text());
    let csrf = owner.token("csrf").await;
    let rejected = owner
        .rest(
            "POST",
            &format!("/jobs/{j2}/reject"),
            Some(json!({"reason": "unsourced additions", "token": csrf})),
        )
        .await;
    assert_eq!(rejected.status, StatusCode::OK, "{}", rejected.text());
    assert_eq!(rejected.json()["status"], "rejected");
    let blocked = alice
        .edit_page("Project:Alice", "more", "run-2026-11-08")
        .await;
    assert_eq!(blocked["error"]["code"], "ts-batch-rejected", "{blocked}");
    assert_eq!(blocked["error"]["job"], j2);
    let another = alice.edit_page("Project:Alice", "more", "run-2").await;
    assert_eq!(another["error"], Value::Null, "{another}");
    assert_ne!(another["batch"], j2);
    let shown = owner.rest("GET", &format!("/jobs/{j2}"), None).await.json();
    assert_eq!(shown["status"], "rejected");
    assert_eq!(shown["reason"], "unsourced additions");
    // Alice is told (16 §5.2).
    app.catch_up_async().await.expect("addressing");
    let inbox = alice
        .api("action=query&meta=notifications&notprop=list&notlimit=50")
        .await;
    let item = inbox["query"]["notifications"]["list"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|i| i["ts-reason"] == "job")
        .unwrap_or_else(|| panic!("a job notification in {inbox}"));
    assert_eq!(item["ts-job"], j2);
    let page = alice
        .send("GET", "/wiki/Special:Notifications", None)
        .await
        .text();
    assert!(page.contains("was rejected by"), "{page}");

    // Closing: the owner closes their batch; the next write under the value opens a new
    // one (0104 §1).
    let csrf = owner.token("csrf").await;
    let closed = owner
        .rest(
            "POST",
            &format!("/jobs/{j1}/close"),
            Some(json!({"token": csrf})),
        )
        .await;
    assert_eq!(closed.status, StatusCode::OK, "{}", closed.text());
    let reopened = owner
        .edit_page("Project:Other", "x", "run-2026-11-08")
        .await;
    let j4 = reopened["batch"].as_u64().unwrap();
    assert_ne!(j4, j1);
    let csrf = owner.token("csrf").await;
    let twice = owner
        .rest(
            "POST",
            &format!("/jobs/{j1}/close"),
            Some(json!({"token": csrf})),
        )
        .await;
    assert_eq!(twice.status, StatusCode::CONFLICT);

    // A change since the batch: the page edited outside it. The revert then restores
    // the entity (its description removed, then the entity deleted as the inverse of
    // its creation) and leaves the page as it is, listing it (0104 §3).
    let later = owner.edit_page("Project:Thing", "three", "").await;
    assert_eq!(later["error"], Value::Null, "{later}");
    assert_eq!(later["batch"], Value::Null);
    let csrf = owner.token("csrf").await;
    let revert = owner
        .rest(
            "POST",
            &format!("/jobs/{j1}/revert"),
            Some(json!({"reason": "test data", "token": csrf})),
        )
        .await;
    assert_eq!(revert.status, StatusCode::OK, "{}", revert.text());
    let r = revert.json()["job"].as_u64().expect("the revert job");
    assert_eq!(revert.json()["status"], "queued");
    let ran = run_runner(&app).await;
    assert_eq!(ran.job_id, r);
    assert_eq!(ran.error, None, "{ran:?}");
    let reverted = owner.rest("GET", &format!("/jobs/{r}"), None).await.json();
    assert_eq!(reverted["status"], "finished", "{reverted}");
    assert_eq!(reverted["mode"], "revert");
    assert_eq!(reverted["counts"]["reverts"], j1);
    assert_eq!(reverted["counts"]["reverted"], 2, "{reverted}");
    let skipped = reverted["counts"]["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2, "both page records: {skipped:?}");
    assert!(
        skipped
            .iter()
            .all(|s| s["reason"] == "changed since the job")
    );
    let original = owner.rest("GET", &format!("/jobs/{j1}"), None).await.json();
    assert_eq!(original["status"], "reverted");
    assert_eq!(original["reverted_by"], r);
    let entity = query(
        &database,
        &format!("SELECT deleted FROM view.entity WHERE tenant = 'wiki' AND id = '{q}'"),
    )
    .await;
    assert!(entity[0].get::<_, bool>(0), "the created entity is deleted");
    let text = query(
        &database,
        "SELECT r.state FROM view.page p JOIN view.entity_revision r ON r.tenant = p.tenant AND r.entity_id = p.page_id::text WHERE p.tenant = 'wiki' AND p.ns = 4 AND p.title = 'Thing' ORDER BY r.\"offset\" DESC LIMIT 1",
    )
    .await;
    assert_eq!(
        String::from_utf8_lossy(&text[0].get::<_, Vec<u8>>(0)),
        "three",
        "the later edit is kept"
    );
    let html = owner
        .send("GET", &format!("/wiki/Special:Jobs/{r}"), None)
        .await
        .text();
    assert!(html.contains("Left as it was"), "{html}");
    assert!(html.contains("changed since the job"), "{html}");

    // Reverting again writes nothing: every key's newest record is the revert's own, or
    // the later edit's.
    let csrf = owner.token("csrf").await;
    let again = owner
        .rest(
            "POST",
            &format!("/jobs/{j1}/revert"),
            Some(json!({"token": csrf})),
        )
        .await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.text());
    let r2 = again.json()["job"].as_u64().unwrap();
    let ran = run_runner(&app).await;
    assert_eq!(ran.job_id, r2);
    let second = owner.rest("GET", &format!("/jobs/{r2}"), None).await.json();
    assert_eq!(second["counts"]["reverted"], 0, "{second}");
    assert_eq!(second["counts"]["other"], Value::Null);

    // Rejecting with a revert in one step: Alice's second batch, through the site's form.
    let j3 = another["batch"].as_u64().unwrap();
    let page = owner
        .send("GET", &format!("/wiki/Special:Jobs/{j3}"), None)
        .await
        .text();
    let token = {
        let marker = "name=\"wpEditToken\" value=\"";
        let at = page.find(marker).expect("a token in the form");
        page[at + marker.len()..]
            .split('"')
            .next()
            .unwrap()
            .to_string()
    };
    let posted = owner
        .send(
            "POST",
            &format!("/wiki/Special:Jobs/{j3}"),
            Some((
                "application/x-www-form-urlencoded",
                format!(
                    "wpEditToken={}&wpAction=reject&wpReason={}&wpRevert=1",
                    enc(&token),
                    enc("test data")
                )
                .into_bytes(),
            )),
        )
        .await;
    assert_eq!(posted.status, StatusCode::SEE_OTHER, "{}", posted.text());
    let to = posted
        .json()
        .as_str()
        .map(str::to_string)
        .unwrap_or_default();
    let _ = to;
    let rejected = owner.rest("GET", &format!("/jobs/{j3}"), None).await.json();
    assert_eq!(rejected["status"], "rejected", "{rejected}");
    let r3 = rejected["revert"]
        .as_u64()
        .expect("the revert the rejection started");
    let ran = run_runner(&app).await;
    assert_eq!(ran.job_id, r3);
    let done = owner.rest("GET", &format!("/jobs/{r3}"), None).await.json();
    assert_eq!(done["counts"]["reverted"], 1, "{done}");
}
