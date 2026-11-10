//! `Special:CreateEntities` (0104 §11; 21 §6.6): a table pasted or uploaded, its rows
//! checked against the properties' data types in a preview, then created as one batch,
//! each row's new ID reported; the batch's job on `Special:Jobs`. And what the table's
//! text names beyond new items (0106 §2, §4, §5, §8): the item a row adds to, properties
//! to create first, other rows, and references. Needs `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.
//!
//! 0104 §11; 0106 §2; 0106 §8

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
        html.contains("2 items will be created, 0 added to and 1 row left out"),
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
    assert!(
        html.contains("2 items created, 0 added to, 0 not written."),
        "{html}"
    );
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

/// Creates an entity through the API and returns its ID.
async fn make(owner: &mut Browser, csrf: &str, kind: &str, data: &str) -> String {
    let r = owner
        .post_api(
            "action=wbeditentity",
            &format!("new={kind}&data={}&token={}", enc(data), enc(csrf)),
        )
        .await;
    r["entity"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no entity: {r}"))
        .to_string()
}

// 0106 §2, §4, §5, §8: the text names the item a row adds to, a property to create first,
// another row, and a statement's own reference; the preview fills in what it finds.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn the_text_names_ids_new_properties_references_and_rows() {
    let Some((database, key_file)) = setup("tscreateentities_text_test").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let site = site(&database, &key_file).await;
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner", "owner-secret-1234").await;
    let form = owner.get("/wiki/Special:CreateEntities").await;
    assert!(form.body.contains("name=\"reference\""), "{}", form.body);
    let token = Browser::field(&form.body, "token");
    let csrf =
        owner.api("action=query&meta=tokens&type=csrf").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string();
    let prop = |label: &str, dt: &str| {
        format!(r#"{{"labels":{{"en":{{"language":"en","value":"{label}"}}}},"datatype":"{dt}"}}"#)
    };
    let p_kind = make(
        &mut owner,
        &csrf,
        "property",
        &prop("instance of", "wikibase-item"),
    )
    .await;
    let p_date = make(&mut owner, &csrf, "property", &prop("published", "time")).await;
    let p_url = make(&mut owner, &csrf, "property", &prop("reference URL", "url")).await;
    let p_pub = make(
        &mut owner,
        &csrf,
        "property",
        &prop("publisher", "wikibase-item"),
    )
    .await;
    let paper = make(
        &mut owner,
        &csrf,
        "item",
        r#"{"labels":{"en":{"language":"en","value":"paper"}}}"#,
    )
    .await;
    let acme = make(
        &mut owner,
        &csrf,
        "item",
        r#"{"labels":{"en":{"language":"en","value":"Acme Press"}},"descriptions":{"en":{"language":"en","value":"a publisher"}}}"#,
    )
    .await;

    // Row 1 names row 2, which is Acme Press as it stands; row 3 gives the item another
    // label; row 4 names a row that will not be written. A new property has no data type.
    let table = format!(
        "label,description,{p_date},{p_date}/ref:{p_url},{p_pub},new:Founded,source\n\
         Paper A,a paper,1969,https://example.org/own,row:2,,https://example.org/a\n\
         Acme Press,a publisher,,,,1901,https://example.org/b\n\
         Paper B,another paper,1970,,row:4,,\n\
         ,,,,,,\n"
    );
    let shared = format!("{p_kind}:{paper}");
    let reference = format!("{p_url}:{{source}}");
    let fields = |rows: &str| {
        vec![
            ("token", token.clone()),
            ("wpStep", "preview".to_string()),
            ("rows", rows.to_string()),
            ("shared", shared.clone()),
            ("reference", reference.clone()),
            ("batch", "papers".to_string()),
        ]
    };
    let (router, cookie) = (owner.router.clone(), owner.cookie.clone());
    let post = |f: Vec<(&'static str, String)>| {
        let mut browser = Browser {
            router: router.clone(),
            cookie: cookie.clone(),
        };
        async move {
            let f: Vec<(&str, &str)> = f.iter().map(|(k, v)| (*k, v.as_str())).collect();
            browser.post_form("/wiki/Special:CreateEntities", &f).await
        }
    };
    let first = post(fields(&table)).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    let html = &first.body;
    // The data type is proposed and written into the header, the match into `id`; nothing
    // can be confirmed until the rows are checked again.
    assert!(html.contains("new:Founded@time"), "{html}");
    assert!(html.contains(&format!("matches {acme}")), "{html}");
    assert!(
        html.contains(&format!("{acme},Acme Press,a publisher")),
        "{html}"
    );
    assert!(
        html.contains("which is not a row that will be written"),
        "{html}"
    );
    assert!(
        !html.contains(">Create 1 item, add to 1, skip 1<"),
        "{html}"
    );

    // Checked again as the preview wrote it, with a term that differs on a third item.
    let table = format!(
        "id,label,description,{p_date},{p_date}/ref:{p_url},{p_pub},new:Founded@time,source\n\
         ,Paper A,a paper,1969,https://example.org/own,row:2,,https://example.org/a\n\
         {acme},Acme Press,a publisher,,,,1901,https://example.org/b\n\
         {paper},Papers,,,,,,\n\
         skip,Paper C,skipped,,,,,\n"
    );
    let second = post(fields(&table)).await;
    let html = &second.body;
    assert!(
        html.contains("1 item will be created, 1 added to and 2 rows left out"),
        "{html}"
    );
    assert!(html.contains("term conflict (1)"), "{html}");
    assert!(html.contains("skipped"), "{html}");
    assert!(html.contains("Create 1 item, add to 1, skip 2"), "{html}");
    let plans = Browser::field(html, "wpPlans");

    let report = owner
        .post_form(
            "/wiki/Special:CreateEntities",
            &[
                ("token", &token),
                ("wpStep", "create"),
                ("batch", "papers"),
                ("wpPlans", &plans),
            ],
        )
        .await;
    let html = &report.body;
    assert!(
        html.contains("1 item created, 1 added to, 0 not written."),
        "{html}"
    );
    assert!(html.contains("Property “Founded” created as "), "{html}");
    let jobs = query(
        &database,
        "SELECT job_id FROM view.job WHERE tenant = 'wiki' AND mode = 'batch'",
    )
    .await;
    assert_eq!(jobs.len(), 1, "the property and both rows are one job");

    // The new property, and the publisher with what the row added under its reference.
    let founded = owner
        .api("action=wbsearchentities&search=Founded&language=en&type=property")
        .await;
    let founded = founded["search"][0]["id"]
        .as_str()
        .expect("Founded")
        .to_string();
    let e = owner
        .api(&format!("action=wbgetentities&ids={acme}|{founded}"))
        .await;
    assert_eq!(e["entities"][&founded]["datatype"], "time");
    let a = &e["entities"][&acme];
    assert_eq!(
        a["claims"][&founded][0]["mainsnak"]["datavalue"]["value"]["time"],
        "+1901-00-00T00:00:00Z"
    );
    assert_eq!(
        a["claims"][&p_kind][0]["references"][0]["snaks"][&p_url][0]["datavalue"]["value"],
        "https://example.org/b"
    );
    // The paper names the publisher's item; its date carries its own reference only.
    let found = owner
        .api("action=wbsearchentities&search=Paper%20A&language=en&type=item")
        .await;
    let id = found["search"][0]["id"]
        .as_str()
        .expect("Paper A")
        .to_string();
    let e = owner.api(&format!("action=wbgetentities&ids={id}")).await;
    let e = &e["entities"][&id];
    assert_eq!(
        e["claims"][&p_pub][0]["mainsnak"]["datavalue"]["value"]["id"],
        acme
    );
    let refs = e["claims"][&p_date][0]["references"].as_array().unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(
        refs[0]["snaks"][&p_url][0]["datavalue"]["value"],
        "https://example.org/own"
    );
    assert_eq!(
        e["claims"][&p_kind][0]["references"][0]["snaks"][&p_url][0]["datavalue"]["value"],
        "https://example.org/a"
    );

    // Run again, the paper matches itself and nothing is left to add.
    let again = post(fields(&table)).await;
    assert!(
        again.body.contains("0 items will be created, 2 added to"),
        "{}",
        again.body
    );
    let plans = Browser::field(&again.body, "wpPlans");
    let report = owner
        .post_form(
            "/wiki/Special:CreateEntities",
            &[
                ("token", &token),
                ("wpStep", "create"),
                ("batch", "papers again"),
                ("wpPlans", &plans),
            ],
        )
        .await;
    assert!(
        report
            .body
            .contains("already holds everything the row gives"),
        "{}",
        report.body
    );
}

// 0045 §3, §5; 0106 §9: a Table page's definition is held to its schema when saved and
// stored canonically; the page is drawn as its grid; a table being prepared is kept as a
// draft whose staged rows an import turns into listed rows.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_table_is_validated_drawn_and_kept_as_a_draft() {
    let Some((database, key_file)) = setup("tscreateentities_draft_test").await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let site = site(&database, &key_file).await;
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner", "owner-secret-1234").await;
    let csrf =
        owner.api("action=query&meta=tokens&type=csrf").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string();
    let p_kind = make(
        &mut owner,
        &csrf,
        "property",
        r#"{"labels":{"en":{"language":"en","value":"instance of"}},"datatype":"wikibase-item"}"#,
    )
    .await;
    let p_url = make(
        &mut owner,
        &csrf,
        "property",
        r#"{"labels":{"en":{"language":"en","value":"reference URL"}},"datatype":"url"}"#,
    )
    .await;
    let journal = make(
        &mut owner,
        &csrf,
        "item",
        r#"{"labels":{"en":{"language":"en","value":"journal"}}}"#,
    )
    .await;
    let nature = make(
        &mut owner,
        &csrf,
        "item",
        &format!(
            r#"{{"labels":{{"en":{{"language":"en","value":"Nature"}}}},"claims":[{{"mainsnak":{{"snaktype":"value","property":"{p_kind}","datavalue":{{"type":"wikibase-entityid","value":{{"entity-type":"item","id":"{journal}"}}}}}},"type":"statement","rank":"normal"}}]}}"#
        ),
    )
    .await;

    // A definition that is not one is refused, each problem with its place.
    let edit = |text: String| {
        format!(
            "title=Table:Journals&text={}&token={}",
            enc(&text),
            enc(&csrf)
        )
    };
    let refused = owner
        .post_api(
            "action=edit",
            &edit(r#"{"version": 1, "rows": {"ids": ["5"]}, "columns": []}"#.to_string()),
        )
        .await;
    assert_eq!(refused["error"]["code"], "ts-invalid-table", "{refused}");
    let info = refused["error"]["info"].as_str().unwrap_or_default();
    assert!(
        info.contains("rows.ids[0]") && info.contains("columns"),
        "{info}"
    );
    // One that is, is stored canonically: tenant-relative IDs in their stored form, one
    // row to a line.
    let saved = owner
        .post_api(
            "action=edit",
            &edit(format!(
                r#"{{"columns": [{{"language": "en", "field": "label"}}, {{"field": "statements", "property": "{p_kind}"}}], "rows": {{"ids": ["qqq{}", "Q999"]}}, "version": 1}}"#,
                &nature[1..]
            )),
        )
        .await;
    assert_eq!(saved["edit"]["result"], "Success", "{saved}");
    let stored = owner
        .api("action=query&prop=revisions&titles=Table:Journals&rvprop=content&rvslots=main")
        .await;
    let stored = stored["query"]["pages"][0]["revisions"][0]["slots"]["main"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        stored.contains(&format!(
            "    \"ids\": [\n      \"{nature}\",\n      \"Q999\"\n    ]"
        )),
        "{stored}"
    );

    // The page is its grid: the entity's label, its best value by its label, and a row
    // for the entity that is not there.
    let page = owner.get("/wiki/Table:Journals").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("ts-table__grid"), "{}", page.body);
    assert!(page.body.contains(">Nature<"), "{}", page.body);
    assert!(page.body.contains(">journal</a>"), "{}", page.body);
    assert!(page.body.contains("instance of"), "{}", page.body);
    assert!(
        page.body.contains("There is no such entity."),
        "{}",
        page.body
    );

    // A table being prepared is saved as a draft under the editor's name.
    let form = owner.get("/wiki/Special:CreateEntities").await;
    let token = Browser::field(&form.body, "token");
    let table = format!(
        "label,description,{p_kind},source,notes\n\
         Science,a journal,{journal},https://example.org/s,x\n\
         Nature,,row:1,https://example.org/n,\n\
         ,,nonsense,,\n"
    );
    let reference = format!("{p_url}:{{source}}");
    let saved = owner
        .post_form(
            "/wiki/Special:CreateEntities",
            &[
                ("token", &token),
                ("wpStep", "preview"),
                ("wpSave", "1"),
                ("rows", &table),
                ("shared", ""),
                ("reference", &reference),
                ("batch", ""),
                ("draftname", "journals"),
            ],
        )
        .await;
    assert_eq!(saved.status, StatusCode::SEE_OTHER, "{}", saved.body);
    let draft = "Table:Drafts/Owner/journals";
    // The draft's page shows its staged rows, and the way back.
    let page = owner.get(&format!("/wiki/{draft}")).await;
    assert!(page.body.contains("3 staged rows"), "{}", page.body);
    assert!(
        page.body
            .contains(&format!("href=\"/wiki/Special:CreateEntities/{draft}\"")),
        "{}",
        page.body
    );
    // Opened again, the form holds the table as text, without the column nothing reads.
    let opened = owner
        .get(&format!("/wiki/Special:CreateEntities/{draft}"))
        .await;
    assert_eq!(opened.status, StatusCode::OK, "{}", opened.body);
    assert!(
        opened
            .body
            .contains(&format!("id,label,description,{p_kind},source\n")),
        "{}",
        opened.body
    );
    assert!(
        opened.body.contains(",Nature,,row:1,https://example.org/n"),
        "{}",
        opened.body
    );
    assert!(
        opened.body.contains(&format!("{p_url}:{{source}}")),
        "{}",
        opened.body
    );
    assert_eq!(Browser::field(&opened.body, "draft"), draft);

    // Checked and imported from the draft: the first row is created, the second matched
    // by hand to the existing item, the third cannot be written and stays staged.
    let rows = format!(
        "id,label,description,{p_kind},source\n\
         ,Science,a journal,{journal},https://example.org/s\n\
         {nature},Nature,,row:1,https://example.org/n\n\
         ,Third,,nonsense,\n"
    );
    let preview = owner
        .post_form(
            "/wiki/Special:CreateEntities",
            &[
                ("token", &token),
                ("wpStep", "preview"),
                ("rows", &rows),
                ("shared", ""),
                ("reference", &reference),
                ("batch", "journals"),
                ("draft", draft),
            ],
        )
        .await;
    let html = &preview.body;
    assert!(html.contains("Create 1 item, add to 1, skip 1"), "{html}");
    let plans = Browser::field(html, "wpPlans");
    let report = owner
        .post_form(
            "/wiki/Special:CreateEntities",
            &[
                ("token", &token),
                ("wpStep", "create"),
                ("batch", "journals"),
                ("wpPlans", &plans),
                ("draft", draft),
                ("rows", &rows),
                ("shared", ""),
                ("reference", &reference),
            ],
        )
        .await;
    let html = &report.body;
    assert!(
        html.contains("1 item created, 1 added to, 0 not written."),
        "{html}"
    );
    assert!(
        html.contains("1 row is still staged in the table"),
        "{html}"
    );

    // The draft now lists the two items and keeps the row that was not written.
    let stored = owner
        .api(&format!(
            "action=query&prop=revisions&titles={draft}&rvprop=content&rvslots=main"
        ))
        .await;
    let stored: Value = serde_json::from_str(
        stored["query"]["pages"][0]["revisions"][0]["slots"]["main"]["content"]
            .as_str()
            .unwrap_or_default(),
    )
    .expect("the draft is JSON");
    let ids = stored["rows"]["ids"].as_array().expect("ids");
    assert_eq!(ids.len(), 2, "{stored}");
    assert!(ids.contains(&Value::String(nature.clone())), "{stored}");
    let staged = stored["staged"]["rows"].as_array().expect("staged rows");
    assert_eq!(staged.len(), 1, "{stored}");
    assert_eq!(staged[0]["cells"][0], "Third");
    // Nature's statement names the item the first row became.
    let science = ids
        .iter()
        .filter_map(Value::as_str)
        .find(|id| *id != nature)
        .unwrap()
        .to_string();
    let e = owner
        .api(&format!("action=wbgetentities&ids={nature}"))
        .await;
    let kinds = e["entities"][&nature]["claims"][&p_kind]
        .as_array()
        .unwrap();
    assert!(
        kinds
            .iter()
            .any(|c| c["mainsnak"]["datavalue"]["value"]["id"] == science.as_str()),
        "{kinds:?}"
    );
    // And the page draws both as rows, with the row left staged below them.
    let page = owner.get(&format!("/wiki/{draft}")).await;
    assert!(
        page.body.contains(">Science<") && page.body.contains(">Nature<"),
        "{}",
        page.body
    );
    assert!(page.body.contains("1 staged row"), "{}", page.body);
}
