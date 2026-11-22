//! Resolver pages (0029 §3, §6; 06 §3.3, 18 §3.3): a DOI written in capitals in a
//! Wikidata mirror is found by `GET /resolve` and by viewing `DOI:…`, in any case. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

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
use triplespace_cli::{instance, sync};

const DB: &str = "tsresolve_test";
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
        providers: vec!["wikidata".into()],
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    Some((database, key_file))
}

async fn site(database: &str, key_file: &std::path::Path) -> Router {
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
        assert_eq!(g.status, StatusCode::OK, "{}", g.body);
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

fn item(id: &str, label: &str, dois: &[&str]) -> Value {
    let claims: Vec<Value> = dois
        .iter()
        .enumerate()
        .map(|(n, d)| {
            json!({"mainsnak": {"snaktype": "value", "property": "P356",
                "datavalue": {"value": d, "type": "string"}, "datatype": "external-id"},
                "type": "statement", "rank": "normal",
                "id": format!("{id}$00000000-0000-0000-0000-00000000000{n}")})
        })
        .collect();
    let mut e = json!({"type": "item", "id": id, "lastrevid": 5,
        "modified": "2026-11-22T00:00:00Z",
        "labels": {"en": {"language": "en", "value": label}},
        "descriptions": {"en": {"language": "en", "value": format!("about {label}")}},
        "aliases": {}, "claims": {}, "sitelinks": {}});
    if !claims.is_empty() {
        e["claims"] = json!({"P356": claims});
    }
    e
}

// 0029 §3, §6 (06 §3.3; 18 §3.3): a DOI that Wikidata writes in capitals is found by
// any case of it; one item redirects, several or several values are listed, none offers
// the external link and, to a signed-in reader, the new-item form.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_doi_reaches_the_item_that_carries_it() {
    let Some((database, key_file)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let mut doi = json!({"type": "property", "id": "P356", "lastrevid": 5,
        "modified": "2026-11-22T00:00:00Z", "datatype": "external-id",
        "labels": {"en": {"language": "en", "value": "DOI"}}, "descriptions": {},
        "aliases": {}, "claims": {}});
    doi["datatype"] = json!("external-id");
    let dump_entities = [
        doi,
        item("Q5", "A paper", &["10.1000/ABC"]),
        item("Q6", "A duplicate", &["10.1000/DUP"]),
        item("Q7", "Its twin", &["10.1000/dup"]),
        item("Q8", "Two DOIs", &["10.1000/M1", "10.1000/M2"]),
    ];
    let dir = std::env::temp_dir().join(format!("tsresolve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dump: PathBuf = dir.join("wd.json");
    let lines: Vec<String> = dump_entities.iter().map(Value::to_string).collect();
    std::fs::write(&dump, format!("[\n{}\n]\n", lines.join(",\n"))).unwrap();
    sync::run(sync::Sync {
        job_id: None,
        database: database.clone(),
        provider: Some("wikidata".into()),
        tenant: None,
        source: None,
        dump: Some(dump),
        version: Some("test".into()),
        snapshot: false,
        threshold: 10,
        batch: 10,
        rejects: None,
        no_bootstrap: true,
        writers: 1,
        no_gap_check: true,
    })
    .await
    .expect("sync");

    let site = site(&database, &key_file).await;
    let mut reader = Browser {
        router: site.clone(),
        cookie: None,
    };
    let resolve = |q: &str| format!("/w/rest.php/triplespace/v0/resolve?q={}", enc(q));
    let json_of = |g: &Got| serde_json::from_str::<Value>(&g.body).unwrap_or(Value::Null);

    // The API: one item, by any case and with or without the prefix.
    for q in [
        "doi:10.1000/abc",
        "DOI:10.1000/ABC",
        "10.1000/ABC",
        "https://doi.org/10.1000/abc",
    ] {
        let g = reader.get(&resolve(q)).await;
        assert_eq!(g.status, StatusCode::OK, "{q}: {}", g.body);
        let r = json_of(&g);
        assert_eq!(r["kind"], "resolver", "{q}: {r}");
        assert_eq!(r["key"], "10.1000/abc", "{q}: {r}");
        assert_eq!(r["properties"], json!(["WDP356"]), "{r}");
        assert_eq!(
            r["target"],
            json!({"id": "WDQ5", "title": "Item:WDQ5"}),
            "{q}: {r}"
        );
        assert_eq!(r["external"], "https://doi.org/10.1000/abc");
    }
    // Several items; one item with several values; none; not a DOI.
    let r = json_of(&reader.get(&resolve("doi:10.1000/Dup")).await);
    let ids: Vec<&str> = r["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["WDQ6", "WDQ7"], "{r}");
    assert!(r.get("target").is_none(), "{r}");
    let r = json_of(&reader.get(&resolve("doi:10.1000/m1")).await);
    assert_eq!(
        r["candidates"],
        json!([{"id": "WDQ8", "title": "Item:WDQ8", "values": ["10.1000/m1", "10.1000/m2"]}]),
        "{r}"
    );
    let r = json_of(&reader.get(&resolve("doi:10.1000/none")).await);
    assert_eq!(r["candidates"], json!([]), "{r}");
    let r = json_of(&reader.get(&resolve("DOI:11.1/x")).await);
    assert_eq!(r["resolver"], "doi", "{r}");
    assert!(r["invalid"].is_string(), "{r}");
    // An entity ID comes first; a bare domain is a suggestion, not an answer.
    let r = json_of(&reader.get(&resolve("wdq5")).await);
    assert_eq!(r["kind"], "entity", "{r}");
    assert_eq!(r["id"], "WDQ5", "{r}");
    let g = reader.get(&resolve("example.org")).await;
    assert_eq!(g.status, StatusCode::NOT_FOUND, "{}", g.body);
    assert_eq!(
        json_of(&g)["suggestions"],
        json!(["domain:example.org"]),
        "{}",
        g.body
    );
    let g = reader.get(&resolve("")).await;
    assert_eq!(g.status, StatusCode::BAD_REQUEST);

    // The site: one item is a 303, by the path or by index.php.
    let g = site
        .clone()
        .oneshot(
            Request::builder()
                .uri("/wiki/DOI:10.1000/aBc")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(g.status(), StatusCode::SEE_OTHER);
    assert_eq!(g.headers()[header::LOCATION], "/wiki/Item:WDQ5");
    let g = reader.get("/w/index.php?title=DOI:10.1000/ABC").await;
    assert_eq!(g.status, StatusCode::SEE_OTHER, "{}", g.body);
    // Several items: a disambiguation page with labels and descriptions.
    let g = reader.get("/wiki/DOI:10.1000/dup").await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    for want in [
        "A duplicate",
        "Its twin",
        "about Its twin",
        "/wiki/Item:WDQ6",
        "duplicates to merge",
    ] {
        assert!(g.body.contains(want), "{want}: {}", g.body);
    }
    // One item with two values: the item and each value in the resolver form.
    let g = reader.get("/wiki/DOI:10.1000/m2").await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    for want in [
        "Two DOIs",
        "/wiki/DOI:10.1000/m1",
        "/wiki/DOI:10.1000/m2",
        "has more than one DOI",
    ] {
        assert!(g.body.contains(want), "{want}: {}", g.body);
    }
    // None: the external link; the new-item form only for a signed-in reader.
    let g = reader.get("/wiki/DOI:10.1000/none").await;
    assert_eq!(g.status, StatusCode::NOT_FOUND, "{}", g.body);
    assert!(
        g.body.contains("No item has the DOI 10.1000/none"),
        "{}",
        g.body
    );
    assert!(
        g.body.contains("https://doi.org/10.1000/none"),
        "{}",
        g.body
    );
    assert!(!g.body.contains("Special:NewItem?statement="), "{}", g.body);
    let g = reader.get("/wiki/DOI:11.1/x").await;
    assert_eq!(g.status, StatusCode::NOT_FOUND, "{}", g.body);
    assert!(g.body.contains("This is not a valid DOI"), "{}", g.body);
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner", "owner-secret-1234").await;
    let g = owner.get("/wiki/DOI:10.1000/none").await;
    assert!(
        g.body
            .contains("/wiki/Special:NewItem?statement=WDP356%3A10%2E1000%2Fnone"),
        "{}",
        g.body
    );
    assert!(
        g.body.contains("Create an item with this DOI"),
        "{}",
        g.body
    );
}
