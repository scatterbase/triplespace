//! `Special:CreateEntities` (0104 §11; 21 §6.6): a table pasted or uploaded, its rows
//! checked against the properties' data types in a preview, then created as one batch,
//! each row's new ID reported; the batch's job on `Special:Jobs`. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.
//!
//! 0104 §11

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode as ServerMode, router_with};
use triplespace_cli::instance;

const DB: &str = "tscreateentities_test";
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

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

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

async fn site(database: &str, key_file: &std::path::Path) -> Router {
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
    let api = router_with(app, triplespace_api_rest::routes());
    api.clone()
        .fallback_service(triplespace_ui::router(triplespace_client::Client::new(
            triplespace_client::ServiceTransport::new(api),
        )))
}

struct Browser {
    router: Router,
    cookie: Option<String>,
}

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

    async fn api(&mut self, query: &str) -> Value {
        let g = self
            .get(&format!("/w/api.php?{query}&format=json&formatversion=2"))
            .await;
        serde_json::from_str(&g.body).unwrap_or(Value::Null)
    }

    async fn post_api(&mut self, query: &str, form: &str) -> Value {
        let g = self
            .send(
                "POST",
                &format!("/w/api.php?{query}&format=json&formatversion=2"),
                Some((
                    "application/x-www-form-urlencoded".into(),
                    form.as_bytes().to_vec(),
                )),
            )
            .await;
        serde_json::from_str(&g.body).unwrap_or(Value::Null)
    }

    async fn post_form(&mut self, path: &str, fields: &[(&str, &str)]) -> Got {
        let body = fields
            .iter()
            .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
            .collect::<Vec<_>>()
            .join("&");
        self.send(
            "POST",
            path,
            Some((
                "application/x-www-form-urlencoded".into(),
                body.into_bytes(),
            )),
        )
        .await
    }

    /// A multipart post: the fields, then a file as `file`.
    async fn post_multipart(
        &mut self,
        path: &str,
        fields: &[(&str, &str)],
        file: Option<&[u8]>,
    ) -> Got {
        const B: &str = "----triplespace-create-test";
        let mut body = Vec::new();
        for (k, v) in fields {
            body.extend_from_slice(
                format!("--{B}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                    .as_bytes(),
            );
        }
        if let Some(f) = file {
            body.extend_from_slice(
                format!("--{B}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"items.csv\"\r\nContent-Type: text/csv\r\n\r\n")
                    .as_bytes(),
            );
            body.extend_from_slice(f);
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

    fn field(body: &str, name: &str) -> String {
        let marker = format!("name=\"{name}\" value=\"");
        let at = body
            .find(&marker)
            .unwrap_or_else(|| panic!("no field {name} in {body}"));
        body[at + marker.len()..]
            .split('"')
            .next()
            .unwrap_or_default()
            .to_string()
    }

    async fn login(&mut self, name: &str, password: &str) {
        let token =
            self.api("action=query&meta=tokens&type=login").await["query"]["tokens"]["logintoken"]
                .as_str()
                .unwrap()
                .to_string();
        let r = self
            .post_api(
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
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_table_becomes_items_under_one_batch() {
    let Some((database, key_file)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let site = site(&database, &key_file).await;

    // A reader is asked to log in; the owner sees the form.
    let mut reader = Browser {
        router: site.clone(),
        cookie: None,
    };
    let asked = reader.get("/wiki/Special:CreateEntities").await;
    assert_eq!(asked.status, StatusCode::OK);
    assert!(asked.body.contains("Log in to create"), "{}", asked.body);
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner", "owner-secret-1234").await;
    let form = owner.get("/wiki/Special:CreateEntities").await;
    assert_eq!(form.status, StatusCode::OK, "{}", form.body);
    assert!(form.body.contains("name=\"rows\""), "{}", form.body);
    assert!(form.body.contains("type=\"file\"") && form.body.contains("name=\"file\""));
    let token = Browser::field(&form.body, "token");

    // Two properties to fill: an item one and a quantity.
    let csrf =
        owner.api("action=query&meta=tokens&type=csrf").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string();
    let p_kind = owner
        .post_api(
            "action=wbeditentity",
            &format!(
                "new=property&data={}&token={}",
                enc(r#"{"labels":{"en":{"language":"en","value":"instance of"}},"datatype":"wikibase-item"}"#),
                enc(&csrf)
            ),
        )
        .await;
    let p_kind = p_kind["entity"]["id"]
        .as_str()
        .expect("a property")
        .to_string();
    let p_pop = owner
        .post_api(
            "action=wbeditentity",
            &format!(
                "new=property&data={}&token={}",
                enc(r#"{"labels":{"en":{"language":"en","value":"population"}},"datatype":"quantity"}"#),
                enc(&csrf)
            ),
        )
        .await;
    let p_pop = p_pop["entity"]["id"]
        .as_str()
        .expect("a property")
        .to_string();
    let city = owner
        .post_api(
            "action=wbeditentity",
            &format!(
                "new=item&data={}&token={}",
                enc(r#"{"labels":{"en":{"language":"en","value":"city"}}}"#),
                enc(&csrf)
            ),
        )
        .await;
    let city = city["entity"]["id"].as_str().expect("an item").to_string();

    // The preview: a CSV with a bad row, a column nobody knows and a shared statement.
    let csv = format!(
        "label,description:de,aliases,{p_pop},notes\nPortland,\"Stadt in Oregon, USA\",PDX|Rose City,650000,x\n,,,twelve,\nSalem,Hauptstadt von Oregon,,175000,\n"
    );
    let preview = owner
        .post_multipart(
            "/wiki/Special:CreateEntities",
            &[
                ("token", &token),
                ("wpStep", "preview"),
                ("rows", ""),
                ("shared", &format!("{p_kind}:{city}")),
                ("batch", "cities 2026"),
            ],
            Some(csv.as_bytes()),
        )
        .await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    let html = &preview.body;
    assert!(
        html.contains("2 rows will be created and 1 cannot be"),
        "{html}"
    );
    assert!(html.contains("population"), "{html}");
    assert!(html.contains("not a column name this page knows"), "{html}");
    assert!(html.contains("is not a value of type"), "{html}");
    assert!(
        html.contains("Portland") && html.contains("Salem"),
        "{html}"
    );
    assert!(html.contains("2 statements"), "{html}");
    assert!(html.contains("name=\"wpPlans\""), "{html}");
    let plans = Browser::field(html, "wpPlans");
    let batch = Browser::field(html, "batch");
    assert_eq!(batch, "cities 2026");

    // The creation: two items, one batch job, the report naming each ID.
    let report = owner
        .post_form(
            "/wiki/Special:CreateEntities",
            &[
                ("token", &token),
                ("wpStep", "create"),
                ("batch", &batch),
                ("wpPlans", &plans),
            ],
        )
        .await;
    assert_eq!(report.status, StatusCode::OK, "{}", report.body);
    let html = &report.body;
    assert!(html.contains("2 items created, 0 not."), "{html}");
    assert!(html.contains("The run is job "), "{html}");
    let rows = query(
        &database,
        "SELECT job_id, mode, status, batch FROM view.job WHERE tenant = 'wiki' AND mode = 'batch'",
    )
    .await;
    assert_eq!(rows.len(), 1, "one batch job");
    let job: i64 = rows[0].get(0);
    assert_eq!(rows[0].get::<_, String>(2), "running");
    assert!(
        html.contains(&format!("href=\"/wiki/Special:Jobs/{job}\"")),
        "{html}"
    );
    let written = query(
        &database,
        &format!("SELECT count(*) FROM view.activity WHERE tenant = 'wiki' AND job_id = {job} AND kind = 'edit'"),
    )
    .await;
    assert_eq!(written[0].get::<_, i64>(0), 2);
    let items = query(
        &database,
        "SELECT t.text FROM view.term t WHERE t.tenant = 'wiki' AND t.kind = 1 AND t.lang = 'en'
           AND t.entity_id LIKE 'Q%'",
    )
    .await;
    // Sorted here, not by the database: its collation differs between hosts.
    let mut labels: Vec<String> = items.iter().map(|r| r.get(0)).collect();
    labels.sort();
    assert_eq!(labels, ["Portland", "Salem", "city"]);
    let found = owner
        .api("action=wbsearchentities&search=Portland&language=en&type=item")
        .await;
    let id = found["search"][0]["id"]
        .as_str()
        .expect("Portland found")
        .to_string();
    let e = owner
        .api(&format!(
            "action=wbgetentities&ids={id}&props=claims|aliases|descriptions"
        ))
        .await;
    let e = &e["entities"][&id];
    assert_eq!(e["descriptions"]["de"]["value"], "Stadt in Oregon, USA");
    assert_eq!(e["aliases"]["en"].as_array().unwrap().len(), 2);
    assert_eq!(
        e["claims"][&p_pop][0]["mainsnak"]["datavalue"]["value"]["amount"],
        "+650000"
    );
    assert_eq!(
        e["claims"][&p_kind][0]["mainsnak"]["datavalue"]["value"]["id"],
        city
    );
}
