//! The site both ways (0057 §1.4, §2): the web tier over HTTP to the API on a real TCP
//! listener, and the server's embedded site in process, serve byte-identical pages; and
//! the embedded site never answers for the API's paths. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::{adopt, instance, sync};
use triplespace_client::{Client, HttpTransport, ServiceTransport};
use triplespace_projections::Farm;

const DB: &str = "tssite_test";
const HOST: &str = "librarybase.org";

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

/// What the browser gets.
struct Got {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

async fn get(router: &Router, path: &str, cookie: Option<&str>) -> Got {
    let browser: SocketAddr = "198.51.100.7:50000".parse().unwrap();
    let mut req = Request::get(path).header(header::HOST, HOST);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let mut req = req.body(Body::empty()).unwrap();
    req.extensions_mut().insert(ConnectInfo(browser));
    let r = router.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    Got {
        status,
        headers,
        body: String::from_utf8(bytes.to_vec()).unwrap(),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn the_web_tier_and_the_embedded_site_serve_the_same_pages() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: "librarybase".into(),
        base: format!("https://{HOST}"),
        farm_slug: None,
        farm_base: None,
        providers: vec!["internetdomains".into()],
        adopt: Some(format!("https://{HOST}/")),
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: None,
    })
    .await
    .expect("instance create");
    // The entities of the site's fixtures: every data type, ranks, qualifiers and a
    // reference, terms in three languages, and a Domain from the mirror.
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    adopt::run(adopt::Adopt {
        database: database.clone(),
        tenant: "librarybase".into(),
        dump: fixtures.join("site-entities.xml"),
        source: format!("https://{HOST}/"),
        version: Some("test".into()),
        frozen: true,
        counters: Some("item=6,property=12".into()),
        log_floor: 0,
        entity_sources: vec![],
        subsidiary: None,
        batch: 200,
        no_accounts: false,
        farm_slug: None,
        farm_base: None,
    })
    .await
    .expect("adopt");
    sync::run(sync::Sync {
        database: database.clone(),
        provider: "internetdomains".into(),
        dump: fixtures.join("internetdomains.json"),
        version: Some("20261001".into()),
        snapshot: false,
        threshold: 1000,
        batch: 200,
        farm_slug: None,
        farm_base: None,
    })
    .await
    .expect("sync");

    // The API, trusting the web tier on loopback by address (0057 §10).
    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec!["127.0.0.1".into()],
            dev_tenant: None,
            farm: Farm {
                slug: "librarybase".into(),
                base: format!("https://{HOST}"),
            },
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let api = router_with(app, triplespace_api_rest::routes());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = api.clone();
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    // The web tier, over HTTP; the server, with the site embedded.
    let web = triplespace_ui::router(Client::new(
        HttpTransport::new(&format!("http://127.0.0.1:{port}"), None)
            .await
            .unwrap(),
    ));
    let embedded = api
        .clone()
        .fallback_service(triplespace_ui::router(Client::new(ServiceTransport::new(
            api.clone(),
        ))));

    for path in [
        "/wiki/Project:Home",
        "/wiki/Special:SpecialPages",
        "/wiki/Project:About",
        "/wiki/Item:Q6",
        "/wiki/Item:Q6?tab=identifiers",
        "/wiki/Item:Q6?tab=sitelinks",
        "/wiki/Item:Q6?tab=labels",
        "/wiki/Item:Q6?uselang=ar",
        "/wiki/Item:Q6?action=render&region=statements/P3",
        "/wiki/Property:P3",
        "/wiki/Domain:wikipedia.org",
        "/wiki/Item:Q404",
        "/w/index.php?title=Item:Q6&action=history",
        "/w/index.php?title=Special:Search&search=six&fulltext=1",
        "/nothing/here",
    ] {
        let a = get(&web, path, None).await;
        let b = get(&embedded, path, None).await;
        assert!(
            a.status == StatusCode::OK || a.status == StatusCode::NOT_FOUND,
            "{path}: {} {}",
            a.status,
            a.body
        );
        assert_eq!(a.status, b.status, "{path}");
        assert_eq!(a.body, b.body, "{path}: the HTML differs");
        for h in [header::ETAG, header::CACHE_CONTROL, header::VARY] {
            assert_eq!(a.headers.get(&h), b.headers.get(&h), "{path}: {h}");
        }
        assert!(
            a.headers[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .starts_with("public"),
            "{path}: an anonymous page is public"
        );
    }
    let page = get(&web, "/wiki/Project:Home", None).await;
    assert!(
        page.body
            .contains("<title>Project:Home – librarybase</title>"),
        "{}",
        page.body
    );
    assert!(page.body.contains("This page is generated"));
    let main = get(&web, "/wiki/Main_Page", None).await;
    assert_eq!(
        main.status,
        StatusCode::FOUND,
        "the old main page redirects"
    );

    // A session cookie the API does not know: the site asks who the viewer is, and the
    // API answers with the anonymous, public form, so the page is the anonymous page.
    let a = get(
        &web,
        "/wiki/Project:Home",
        Some("triplespace_session=nonsense"),
    )
    .await;
    let b = get(
        &embedded,
        "/wiki/Project:Home",
        Some("triplespace_session=nonsense"),
    )
    .await;
    assert_eq!(a.body, b.body);
    assert_eq!(a.body, page.body);
    assert_eq!(
        a.headers.get(header::CACHE_CONTROL),
        b.headers.get(header::CACHE_CONTROL)
    );

    // The theme the pages link to.
    let href = page
        .body
        .split("href=\"/ui/theme/")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap();
    let css = get(&embedded, &format!("/ui/theme/{href}"), None).await;
    assert_eq!(css.status, StatusCode::OK);
    assert!(
        css.headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("immutable")
    );

    // The embedded site leaves the API's paths to the API.
    let api_call = get(
        &embedded,
        "/w/api.php?action=query&meta=siteinfo&format=json&formatversion=2",
        None,
    )
    .await;
    assert_eq!(api_call.status, StatusCode::OK);
    assert!(api_call.body.contains("\"sitename\""));
    for path in [
        "/w/rest.php/triplespace/v0/nothing",
        "/wiki/Special:EntityData/",
        "/oauth/x",
    ] {
        let r = get(&embedded, path, None).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
        assert!(!r.body.contains("<html"), "{path} reached the site");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
