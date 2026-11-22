//! Badges (0114; 06 §6, 19 §1.6): a Wikidata property that is an instance of *obsolete
//! Wikidata property*, a subclass of *obsolete predicate*, carries the `class` badge;
//! `wbgetentities` reports it in `ts-badges` when asked, the page bundle gives it for
//! the properties an entity uses, the entity page draws it beside the property, and a
//! local sitelink may carry only a `stored` badge. Needs `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

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

const DB: &str = "tsbadges_test";
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

fn entity(kind: &str, id: &str, label: &str, claims: &[(&str, &str)]) -> Value {
    let mut c = serde_json::Map::new();
    for (n, (p, v)) in claims.iter().enumerate() {
        c.insert(
            (*p).to_string(),
            json!([{"mainsnak": {"snaktype": "value", "property": p,
            "datavalue": {"value": {"entity-type": "item", "id": v}, "type": "wikibase-entityid"},
            "datatype": "wikibase-item"}, "type": "statement", "rank": "normal",
            "id": format!("{id}$00000000-0000-0000-0000-00000000000{n}")}]),
        );
    }
    let mut e = json!({"type": kind, "id": id, "lastrevid": 5, "modified": "2026-11-22T00:00:00Z",
        "labels": {"en": {"language": "en", "value": label}}, "descriptions": {}, "aliases": {},
        "claims": c});
    if kind == "property" {
        e["datatype"] = json!("wikibase-item");
    } else {
        e["sitelinks"] = json!({});
    }
    e
}

// 0114 §1, §2, §4, §5 (A1).
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_retired_property_carries_its_badge_everywhere() {
    let Some((database, key_file)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    // Wikidata's classes, as a shallow mirror would hold them, and a retired property
    // that one item still uses.
    let dump_entities = [
        entity("property", "P31", "instance of", &[]),
        entity("property", "P279", "subclass of", &[]),
        entity("item", "Q142241643", "obsolete predicate", &[]),
        entity(
            "item",
            "Q18644427",
            "obsolete Wikidata property",
            &[("P279", "Q142241643")],
        ),
        entity("property", "P1", "old link", &[("P31", "Q18644427")]),
        entity("property", "P2", "current link", &[]),
        entity("item", "Q7", "a retired thing", &[("P31", "Q142241643")]),
        entity("item", "Q5", "a thing", &[("P1", "Q5"), ("P2", "Q7")]),
    ];
    let dir = std::env::temp_dir().join(format!("tsbadges-{}", std::process::id()));
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
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };

    // `wbgetentities` reports the class badge when `props` asks for `ts-badges`: on the
    // retired property, reached through the subclass, and on nothing else.
    let got = owner
        .api("action=wbgetentities&ids=WDP1|WDP2|WDQ5&props=labels|ts-badges")
        .await;
    assert_eq!(got["ts-badges"], json!({"WDP1": ["WDQ142241643"]}), "{got}");
    assert!(got["entities"]["WDP1"]["labels"]["en"].is_object(), "{got}");
    let plain = owner.api("action=wbgetentities&ids=WDP1").await;
    assert!(plain.get("ts-badges").is_none(), "{plain}");

    // The page bundle gives the badges of the properties the entity uses.
    let bundle = owner
        .get("/w/rest.php/triplespace/v0/entity/WDQ5/page")
        .await;
    assert_eq!(bundle.status, StatusCode::OK, "{}", bundle.body);
    let bundle: Value = serde_json::from_str(&bundle.body).unwrap();
    assert_eq!(
        bundle["badges"],
        json!({"WDP1": ["WDQ142241643"], "WDQ7": ["WDQ142241643"]}),
        "{bundle}"
    );
    // The retired property's statement is still there.
    assert!(bundle["entity"]["claims"]["WDP1"].is_array(), "{bundle}");

    // The entity page draws the badge beside the retired property and after the value
    // that is an instance of the class, and nowhere else: one component, with its icon,
    // its tooltip and a link to its item (19 §1.6).
    let page = owner.get("/wiki/Item:WDQ5").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert_eq!(
        page.body.matches("ts-chip--badge").count(),
        2,
        "{}",
        page.body
    );
    assert!(page.body.contains("deprecated</a>"), "{}", page.body);
    assert!(
        page.body.contains(r#"href="/wiki/Item:WDQ142241643""#),
        "{}",
        page.body
    );
    assert!(page.body.contains("ts-chip__icon"), "{}", page.body);
    assert!(
        page.body
            .contains(r#"title="Its source has retired this property"#),
        "{}",
        page.body
    );
    // The property's own page draws it in the identity line.
    let property = owner.get("/wiki/Property:WDP1").await;
    assert!(
        property.body.contains("ts-chip--badge"),
        "{}",
        property.body
    );

    // A local sitelink may carry a stored badge, and no other item.
    owner.login("Owner", "owner-secret-1234").await;
    let csrf = owner.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();
    let edit = |badge: &str, title: &str| {
        let data = json!({"labels": {"en": {"language": "en", "value": title}},
            "sitelinks": {"enwiki": {"site": "enwiki", "title": title, "badges": [badge]}}});
        format!(
            "new=item&data={}&token={}",
            enc(&data.to_string()),
            enc(&csrf)
        )
    };
    let refused = owner
        .post_api("action=wbeditentity", &edit("WDQ5", "Refused"))
        .await;
    assert_eq!(refused["error"]["code"], "not-badge", "{refused}");
    let ok = owner
        .post_api("action=wbeditentity", &edit("WDQ17437796", "Featured"))
        .await;
    assert_eq!(
        ok["entity"]["sitelinks"]["enwiki"]["badges"],
        json!(["WDQ17437796"]),
        "{ok}"
    );
    // A stored badge is drawn on its sitelink, named by its item and linked to it.
    let featured = ok["entity"]["id"].as_str().unwrap().to_string();
    let sitelinks = owner
        .get(&format!("/wiki/Item:{featured}?tab=sitelinks"))
        .await;
    assert!(
        sitelinks.body.contains(r#"href="/wiki/Item:WDQ17437796""#),
        "{}",
        sitelinks.body
    );
    assert!(
        sitelinks.body.contains("ts-chip--badge"),
        "{}",
        sitelinks.body
    );

    // Search results carry the badges of the entities found.
    let found = owner
        .get("/wiki/Special:Search?search=old+link&fulltext=1")
        .await;
    assert_eq!(found.status, StatusCode::OK, "{}", found.body);
    assert!(found.body.contains("ts-chip--badge"), "{}", found.body);
    let _ = std::fs::remove_dir_all(&dir);
}
