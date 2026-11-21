//! Value IRIs stored as entity IDs (0004 A20, A23): a local edit that writes a quantity's
//! unit and a time's calendar model as IRIs stores the IDs they name, serves the IRIs
//! back, and is unchanged when made again; a Wikidata entity synced from a dump stores
//! `WDQ…` and keeps no upstream hash, since Wikidata's hash is recomputed over the IRI.
//! Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

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

const DB: &str = "tsvalueiris_test";
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

/// An entity's state as stored, from the view.
async fn stored(database: &str, tenant: &str, id: &str) -> Value {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    let registry = scatter_providers::Registry::default_registry();
    let slugs: Vec<&str> = registry
        .providers()
        .iter()
        .map(|p| p.slug.as_str())
        .collect();
    let order = triplespace_projections::entities::EntityProjection::new(&slugs, registry);
    let id = scatter_wikibase_model::id::EntityId::parse(id).unwrap();
    let current = triplespace_projections::read::current(&client, &order, tenant, &id)
        .await
        .unwrap()
        .expect("held");
    serde_json::to_value(&current.resolved.entity).unwrap()
}

fn claim(e: &Value, p: &str) -> Value {
    e["claims"][p][0]["mainsnak"]["datavalue"]["value"].clone()
}

// 0004 A20, A23.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn value_iris_are_stored_as_ids_and_served_as_iris() {
    let Some((database, key_file)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let site = site(&database, &key_file).await;
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner", "owner-secret-1234").await;
    let csrf = owner.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();
    let edit = |data: Value, target: &str| {
        format!(
            "{target}&data={}&token={}",
            enc(&data.to_string()),
            enc(&csrf)
        )
    };

    // A unit item and two properties of the tenant's own.
    let mut ids = Vec::new();
    for data in [json!({"labels": {"en": {"language": "en", "value": "metre"}}})] {
        let r = owner
            .post_api("action=wbeditentity", &edit(data, "new=item"))
            .await;
        ids.push(
            r["entity"]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("{r}"))
                .to_string(),
        );
    }
    let metre = ids[0].clone();
    let mut props = Vec::new();
    for (dt, label) in [("quantity", "length"), ("time", "date")] {
        let r = owner
            .post_api(
                "action=wbeditentity",
                &edit(
                    json!({"datatype": dt, "labels": {"en": {"language": "en", "value": label}}}),
                    "new=property",
                ),
            )
            .await;
        props.push(
            r["entity"]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("{r}"))
                .to_string(),
        );
    }
    let (length, when) = (props[0].clone(), props[1].clone());
    let metre_iri = format!("https://{HOST}/entity/{metre}");
    let gregorian = "http://www.wikidata.org/entity/Q1985727";
    let snak = |p: &str, value: Value, ty: &str| {
        json!({"mainsnak": {"snaktype": "value", "property": p,
            "datavalue": {"value": value, "type": ty}}, "type": "statement", "rank": "normal"})
    };
    let data = json!({
        "labels": {"en": {"language": "en", "value": "a bridge"}},
        "claims": [
            snak(&length, json!({"amount": "+12", "unit": metre_iri}), "quantity"),
            snak(&when, json!({"time": "+1900-01-01T00:00:00Z", "timezone": 0, "before": 0,
                "after": 0, "precision": 11, "calendarmodel": gregorian}), "time"),
        ],
    });
    let r = owner
        .post_api("action=wbeditentity", &edit(data.clone(), "new=item"))
        .await;
    let item = r["entity"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_string();

    // Stored as the IDs the IRIs name.
    let e = stored(&database, TENANT, &item).await;
    assert_eq!(claim(&e, &length)["unit"], metre.as_str(), "{e}");
    assert_eq!(claim(&e, &when)["calendarmodel"], "WDQ1985727", "{e}");
    // Served as the IRIs.
    let got = owner.api(&format!("action=wbgetentities&ids={item}")).await;
    let served = &got["entities"][item.as_str()];
    assert_eq!(claim(served, &length)["unit"], metre_iri.as_str(), "{got}");
    assert_eq!(claim(served, &when)["calendarmodel"], gregorian, "{got}");
    // The served claims sent back change nothing: their IRIs compare as the IDs stored.
    let lastrevid = served["lastrevid"].clone();
    let claims: Vec<Value> = served["claims"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|g| g.as_array().unwrap().clone())
        .collect();
    let again = owner
        .post_api(
            "action=wbeditentity",
            &edit(json!({"claims": claims}), &format!("id={item}")),
        )
        .await;
    assert!(again.get("error").is_none(), "{again}");
    let after = owner.api(&format!("action=wbgetentities&ids={item}")).await;
    assert_eq!(
        after["entities"][item.as_str()]["lastrevid"],
        lastrevid,
        "an unchanged edit writes nothing: {again}"
    );

    // A Wikidata entity from a dump: the unit stored as `WDQ…`, served as Wikidata's IRI,
    // and no upstream hash kept, since the recomputed one is Wikidata's.
    let unit = "http://www.wikidata.org/entity/Q11573";
    let value = json!({"amount": "+3", "unit": unit});
    let upstream_snak: scatter_wikibase_model::statement::Snak = serde_json::from_value(json!({
        "snaktype": "value", "property": "P2048",
        "datavalue": {"value": value, "type": "quantity"}, "datatype": "quantity"}))
    .unwrap();
    let hash = scatter_wikibase_model::Hasher::mirrored_from("WD").snak(&upstream_snak);
    let entity = json!({"type": "item", "id": "Q99", "lastrevid": 5, "modified": "2026-11-21T00:00:00Z",
        "labels": {"en": {"language": "en", "value": "a rod"}}, "descriptions": {}, "aliases": {},
        "sitelinks": {}, "claims": {"P2048": [{"mainsnak": {"snaktype": "value", "property": "P2048",
            "hash": hash, "datavalue": {"value": value, "type": "quantity"}, "datatype": "quantity"},
            "type": "statement", "rank": "normal", "id": "Q99$00000000-0000-0000-0000-000000000001"}]}});
    let dir = std::env::temp_dir().join(format!("tsvalueiris-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dump: PathBuf = dir.join("wd.json");
    std::fs::write(&dump, format!("[\n{entity}\n]\n")).unwrap();
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
    let e = stored(&database, "", "WDQ99").await;
    let mainsnak = &e["claims"]["WDP2048"][0]["mainsnak"];
    assert_eq!(mainsnak["datavalue"]["value"]["unit"], "WDQ11573", "{e}");
    assert!(
        mainsnak.get("hash").is_none(),
        "the upstream hash matched and was dropped: {e}"
    );
    let got = owner.api("action=wbgetentities&ids=WDQ99").await;
    let served = &got["entities"]["WDQ99"]["claims"]["WDP2048"][0]["mainsnak"];
    assert_eq!(served["datavalue"]["value"]["unit"], unit, "{got}");
    assert_eq!(served["hash"], hash.as_str(), "{got}");
    let _ = std::fs::remove_dir_all(&dir);
}
