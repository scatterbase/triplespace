//! `Special:Version` end to end (ADR 0077): `GET /version` over the API, the page and its
//! subpages through the embedded site, and what `meta=siteinfo` adds. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;
use triplespace_client::{Client, ServiceTransport};
use triplespace_projections::Farm;

const DB: &str = "tsversion_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "librarybase";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let (client, connection) = admin_config()
        .unwrap()
        .connect(NoTls)
        .await
        .expect("connect");
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
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{DB}"))
        .unwrap()
}

async fn get(router: &Router, path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut req = Request::get(path)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
    let r = router.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
}

/// Sets a tenant `site` setting, as `ts-config` writes one.
async fn set(database: &str, key: &str, value: Value) {
    let farm = Farm {
        slug: TENANT.into(),
        base: format!("https://{HOST}"),
    };
    let (store, pipeline) = triplespace_cli::common::store(database, &farm).unwrap();
    let mut cx = scatter_projection::Backend::begin(&store).await.unwrap();
    let config = scatter_ingest::store::IngestStore::partition(&store, &mut cx, TENANT, "config")
        .await
        .unwrap()
        .expect("the tenant has a config partition");
    let d = triplespace_cli::common::draft(
        scatter_log::registry::PAYLOAD_CONFIG,
        &format!("site:{key}"),
        &json!({"kind": "site", "name": key, "value": value}),
        "librarybase:7",
        None,
        triplespace_cli::common::now(),
    )
    .unwrap();
    scatter_ingest::write::append_projected(
        &store,
        &pipeline,
        &mut cx,
        config,
        d,
        scatter_projection::Budget::NONE,
    )
    .await
    .unwrap();
    scatter_projection::Backend::commit(&store, cx)
        .await
        .unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn special_version_and_its_api() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsversion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        farm_slug: None,
        farm_base: None,
        providers: vec!["internetdomains".into()],
        adopt: None,
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: None,
    })
    .await
    .expect("instance create");

    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: Farm {
                slug: TENANT.into(),
                base: format!("https://{HOST}"),
            },
            generator: "Triplespace test".into(),
            version: Some(include_str!(concat!(env!("OUT_DIR"), "/version.json"))),
        },
    )
    .unwrap();
    triplespace_api_action::version::refresh(&app).await;
    let api = router_with(app, triplespace_api_rest::routes());
    let site = api
        .clone()
        .fallback_service(triplespace_ui::router(Client::new(ServiceTransport::new(
            api.clone(),
        ))));

    // The API: credits, the build, PostgreSQL connected with its version and nothing
    // else, since nothing else is configured (0077 A2).
    let (status, h, body) = get(&api, "/w/rest.php/triplespace/v0/version").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["developer"]["name"], "Scatter LLC");
    assert_eq!(v["license"]["spdx"], "GPL-3.0-or-later");
    let services = v["services"].as_array().unwrap();
    assert_eq!(services.len(), 1, "{services:?}");
    assert_eq!(services[0]["name"], "PostgreSQL");
    assert_eq!(services[0]["state"], "connected");
    assert!(
        services[0]["version"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(
        v["features"]["tenant"].is_array() && v["features"]["instance"].is_array(),
        "one host for both forms"
    );
    assert!(
        v["wikitext"].as_array().unwrap().is_empty(),
        "expansion is off"
    );
    assert_eq!(v["inspired_by"][0]["name"], "MediaWiki");
    assert!(
        v["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "Claude Opus")
    );
    assert!(!body.contains("\"adr\""), "the page names no ADRs");

    // A tenant setting shows, and turns the wikitext list on.
    set(&database, "wikitext.expansion", json!("on")).await;
    let (_, _, body) = get(&api, "/w/rest.php/triplespace/v0/version").await;
    let v: Value = serde_json::from_str(&body).unwrap();
    let exp = v["features"]["tenant"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "wikitext.expansion")
        .unwrap()
        .clone();
    assert_eq!(
        (exp["value"].clone(), exp["set"].clone()),
        (json!("on"), json!(true))
    );
    assert!(
        v["wikitext"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["name"] == "#if")
    );

    // A component this build does not have.
    let (status, _, _) = get(
        &api,
        "/w/rest.php/triplespace/v0/version/licenses/nothing@0",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // One it has, when the build was made with a manifest.
    if v["components_listed"] == json!(true) {
        let id = v["crates"][0]["id"].as_str().unwrap().to_string();
        let (status, _, body) = get(
            &api,
            &format!("/w/rest.php/triplespace/v0/version/licenses/{id}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let c: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(c["id"], id.as_str());
    }

    // siteinfo: libraries, PostgreSQL's version, the build.
    let (_, _, body) = get(
        &api,
        "/w/api.php?action=query&meta=siteinfo&siprop=general%7Clibraries%7Cextensions%7Ctriplespace&format=json&formatversion=2",
    )
    .await;
    let si: Value = serde_json::from_str(&body).unwrap();
    assert!(
        si["query"]["general"]["dbversion"].as_str().is_some(),
        "{body}"
    );
    assert!(si["query"]["libraries"].is_array());
    assert!(si.get("warnings").is_none(), "{body}");
    assert!(si["query"]["triplespace"]["build"]["version"].is_string());
    assert!(
        si["query"]["triplespace"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "version")
    );
    let ext: Vec<&str> = si["query"]["extensions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str())
        .collect();
    assert_eq!(
        ext,
        ["WikibaseRepository", "Triplespace"],
        "inspired-by is not installed"
    );

    // The page, through the site.
    let (status, _, html) = get(&site, "/wiki/Special:Version").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("This wiki is powered by"), "{html}");
    assert!(html.contains("developed by Scatter LLC in Portland, Oregon."));
    assert!(html.contains("PostgreSQL"));
    assert!(
        !html.contains("OpenSearch") && !html.contains("Valkey"),
        "not configured: not shown"
    );
    assert!(html.contains("Inspired by"));
    assert!(html.contains("Claude Fable"));
    assert!(!html.contains("⧼"), "a message is missing");
    let (status, _, html) = get(&site, "/wiki/Special:Version/License").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("GNU GENERAL PUBLIC LICENSE"));
    assert!(html.contains("Version 3, 29 June 2007"));
    let (status, _, html) = get(&site, "/wiki/Special:Version/Credits").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("There are no other contributors yet."),
        "{html}"
    );
    let (status, _, _) = get(&site, "/wiki/Special:Version/License/nothing@0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = get(&site, "/wiki/Special:Version/Nonsense").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, _, html) = get(&site, "/wiki/Special:SpecialPages").await;
    assert!(
        html.contains("/wiki/Special:Version"),
        "listed on Special:SpecialPages"
    );
}
