//! The settings pages and the Appearance menu end to end (ADR 0085 §4–6), through the
//! server's embedded site over the real API: Special:WikiSettings read and saved without
//! JavaScript, a refused save shown again with its reason, Reset, Special:Tenancy,
//! Special:InstanceSettings with the tenancy switches, defaults and locks, and
//! Special:Preferences with the Appearance menu's form. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;
use triplespace_client::{Client, ServiceTransport};

const DB: &str = "tssettingspages_test";
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

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

struct Got {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Got {
    fn location(&self) -> &str {
        self.headers
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }

    fn cache_control(&self) -> &str {
        self.headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }

    /// The value of the first hidden input of a name.
    fn hidden(&self, name: &str) -> String {
        let marker = format!("name=\"{name}\" value=\"");
        let at = self
            .body
            .find(&marker)
            .unwrap_or_else(|| panic!("no {name} field"));
        let rest = &self.body[at + marker.len()..];
        rest[..rest.find('"').unwrap()]
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }
}

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    form: Option<String>,
    extra: &[(&str, &str)],
) -> Got {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, format!("triplespace_session={c}"));
    }
    if form.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let mut req = req.body(form.map_or_else(Body::empty, Body::from)).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([198, 51, 100, 7], 50000))));
    let r = router.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    Got {
        status,
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Logs the owner in through the API and returns the session cookie's value.
async fn log_in(router: &Router) -> String {
    let r = send(
        router,
        "GET",
        "/w/api.php?action=query&meta=tokens&type=login&format=json",
        None,
        None,
        &[],
    )
    .await;
    let cookie = r.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .strip_prefix("triplespace_session=")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let v: Value = serde_json::from_str(&r.body).unwrap();
    let token = v["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let r = send(
        router,
        "POST",
        "/w/api.php",
        Some(&cookie),
        Some(form(&[
            ("action", "clientlogin"),
            ("format", "json"),
            ("username", "Alice"),
            ("password", "owner-secret-1234"),
            ("logintoken", &token),
            ("loginreturnurl", &format!("https://{HOST}/")),
        ])),
        &[],
    )
    .await;
    assert!(r.body.contains("PASS"), "{}", r.body);
    r.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .strip_prefix("triplespace_session=")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn sitename(router: &Router) -> String {
    let r = send(
        router,
        "GET",
        "/w/api.php?action=query&meta=siteinfo&format=json&formatversion=2",
        None,
        None,
        &[],
    )
    .await;
    let v: Value = serde_json::from_str(&r.body).unwrap();
    v["query"]["general"]["sitename"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn settings_pages_and_the_appearance_menu() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssettingspages-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: "lbwiki".into(),
        base: format!("https://{HOST}"),
        name: Some("Librarybase".into()),
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
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
            farm: triplespace_cli::common::farm_at(&database).await.unwrap(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let api = router_with(app, triplespace_api_rest::routes());
    let site = api
        .clone()
        .fallback_service(triplespace_ui::router(Client::new(ServiceTransport::new(
            api.clone(),
        ))));

    // A reader without an account: the values, no controls, a public page, and an
    // Appearance menu that says the page follows the device.
    let r = send(&site, "GET", "/wiki/Special:WikiSettings", None, None, &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(
        r.cache_control().starts_with("public"),
        "{}",
        r.cache_control()
    );
    assert!(
        r.body
            .contains("<h1 class=\"ts-page__title\">Wiki settings</h1>")
    );
    assert!(r.body.contains("id=\"site\""));
    assert!(r.body.contains("id=\"tenancy\""));
    assert!(r.body.contains("href=\"#tenancy\""));
    assert!(
        !r.body.contains("ts-settings__form"),
        "no controls without the right"
    );
    assert!(r.body.contains(">Wiki name</span>"));
    assert!(r.body.contains("Set on this wiki"));
    assert!(r.body.contains("class=\"ts-appearance__nojs\""));
    assert!(r.body.contains("skin-theme-clientpref-os"));
    assert!(!r.body.contains("data-appearance"));
    let r = send(&site, "GET", "/wiki/Special:Preferences", None, None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.body
            .contains("Special:UserLogin?returnto=Special:Preferences")
    );
    assert!(r.body.contains("Appearance menu"));
    // The farm base is this tenant's base: Special:Tenancy goes to the instance's page.
    let r = send(&site, "GET", "/wiki/Special:Tenancy", None, None, &[]).await;
    assert_eq!(r.status, StatusCode::FOUND);
    assert_eq!(r.location(), "/wiki/Special:InstanceSettings#tenancy");
    let r = send(&site, "GET", "/wiki/Special:SpecialPages", None, None, &[]).await;
    assert!(r.body.contains("/wiki/Special:WikiSettings"));
    assert!(r.body.contains("/wiki/Special:InstanceSettings"));

    // The owner: the form, with its token and base, private.
    let cookie = log_in(&api).await;
    let c = Some(cookie.as_str());
    let page = send(&site, "GET", "/wiki/Special:WikiSettings", c, None, &[]).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.cache_control().starts_with("private"));
    assert!(page.body.contains("ts-settings__form"));
    assert!(
        page.body.contains("/wiki/Special:Preferences"),
        "the account menu links Preferences"
    );
    let token = page.hidden("token");
    let base = page.hidden("base");

    // Save the site section: the name changes, the language is left as it is.
    let save = |name: &str| {
        form(&[
            ("section", "site"),
            ("base", &base),
            ("token", &token),
            ("keys", "site.name site.language"),
            ("v.site.name", name),
            ("v.site.language", "en"),
            ("summary", "rename"),
        ])
    };
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:WikiSettings",
        c,
        Some(save("Library Base")),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert_eq!(r.location(), "/wiki/Special:WikiSettings?saved=site#site");
    assert_eq!(sitename(&api).await, "Library Base");
    let r = send(
        &site,
        "GET",
        "/wiki/Special:WikiSettings?saved=site",
        c,
        None,
        &[],
    )
    .await;
    assert!(r.body.contains("Saved."));
    assert!(
        r.body
            .contains("<a class=\"ts-header__site\" href=\"/wiki/Project:Home\">Library Base</a>")
    );

    // The wiki's palette is chosen from the palettes it offers (0086 §4), and every
    // page links its stylesheet.
    assert!(
        r.body
            .contains("<option value=\"parchment-by\">parchment-by</option>")
    );
    let palette = form(&[
        ("section", "site"),
        ("base", &r.hidden("base")),
        ("token", &r.hidden("token")),
        ("keys", "theme.palette"),
        ("v.theme.palette", "parchment-by"),
        ("summary", "palette"),
    ]);
    let p = send(
        &site,
        "POST",
        "/w/index.php?title=Special:WikiSettings",
        c,
        Some(palette),
        &[],
    )
    .await;
    assert_eq!(p.status, StatusCode::SEE_OTHER, "{}", p.body);
    let anon = send(&site, "GET", "/wiki/Special:SpecialPages", None, None, &[]).await;
    assert!(
        anon.body
            .contains("id=\"ts-palette\" href=\"/ui/theme/p-parchment-by-"),
        "{}",
        &anon.body[..900]
    );
    let r = send(
        &site,
        "GET",
        "/wiki/Special:WikiSettings?saved=site",
        c,
        None,
        &[],
    )
    .await;

    // A refused save: the page again, with the reason and what the reader typed.
    let token = r.hidden("token");
    let base = r.hidden("base");
    let bad = form(&[
        ("section", "site"),
        ("base", &base),
        ("token", &token),
        ("keys", "site.name"),
        ("v.site.name", "User"),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:WikiSettings",
        c,
        Some(bad),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(
        r.body.contains("already a namespace"),
        "{}",
        &r.body[..r.body.len().min(2000)]
    );
    assert!(
        r.body.contains("name=\"v.site.name\" value=\"User\""),
        "the input kept"
    );
    assert_eq!(sitename(&api).await, "Library Base");
    // A stale token is refused in the reader's words, not saved.
    let stale = form(&[
        ("section", "site"),
        ("token", "nonsense"),
        ("keys", "site.name"),
        ("v.site.name", "X"),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:WikiSettings",
        c,
        Some(stale),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(sitename(&api).await, "Library Base");

    // Reset returns the name to inheriting: the slug.
    let page = send(&site, "GET", "/wiki/Special:WikiSettings", c, None, &[]).await;
    let reset = form(&[
        ("section", "site"),
        ("base", &page.hidden("base")),
        ("token", &page.hidden("token")),
        ("keys", "site.name"),
        ("v.site.name", "Library Base"),
        ("reset", "site.name"),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:WikiSettings",
        c,
        Some(reset),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(sitename(&api).await, "lbwiki");

    // The instance's page: switch to locks, then give and lock a default.
    let page = send(&site, "GET", "/wiki/Special:InstanceSettings", c, None, &[]).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("id=\"tenancy\""));
    assert!(
        !page.body.contains("id=\"defaults\""),
        "no defaults under `none`"
    );
    let tenancy = form(&[
        ("section", "tenancy"),
        ("base", &page.hidden("base")),
        ("token", &page.hidden("token")),
        ("keys", "config.template wikitext.ceiling"),
        ("t.config.template", "locks"),
        ("t.wikitext.ceiling", "lua"),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:InstanceSettings",
        c,
        Some(tenancy),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let page = send(&site, "GET", "/wiki/Special:InstanceSettings", c, None, &[]).await;
    assert!(page.body.contains("id=\"defaults\""));
    assert!(
        page.body.contains("One wiki uses this default"),
        "the inheriting count"
    );
    let defaults = form(&[
        ("section", "defaults"),
        ("base", &page.hidden("base")),
        ("token", &page.hidden("token")),
        ("keys", "site.language"),
        ("d.site.language", "de"),
        ("l.site.language", "1"),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:InstanceSettings",
        c,
        Some(defaults),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let r = send(&site, "GET", "/wiki/Special:WikiSettings", c, None, &[]).await;
    assert!(r.body.contains("Locked by the instance"));
    assert!(
        r.body
            .contains("<html class=\"client-nojs skin-theme-clientpref-os\" lang=\"de\""),
        "the content language follows the default"
    );

    // Preferences: a section saved, and the reader's choices on <html>.
    let page = send(&site, "GET", "/wiki/Special:Preferences", c, None, &[]).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("id=\"appearance\""));
    // A palette and a type set are chosen from what the wiki offers (0086 §4), the
    // wiki's own as the empty choice.
    assert!(
        page.body
            .contains("<select class=\"cdx-select\" id=\"ts-p-ui.palette\" name=\"v.ui.palette\"")
    );
    assert!(page.body.contains(
        "<option value=\"\" selected>Wiki default (Parchment (tritanopia))</option>"
    ));
    assert!(
        page.body
            .contains("<option value=\"opendyslexic\">OpenDyslexic (accessibility)</option>")
    );
    let prefs = form(&[
        ("section", "appearance"),
        ("token", &page.hidden("token")),
        (
            "keys",
            "ui.mode ui.contrast ui.text-size ui.palette ui.typeset",
        ),
        ("v.ui.mode", "dark"),
        ("v.ui.contrast", "auto"),
        ("v.ui.text-size", "medium"),
        ("v.ui.palette", "parchment-rg"),
        ("v.ui.typeset", ""),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:Preferences",
        c,
        Some(prefs),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert_eq!(
        r.location(),
        "/wiki/Special:Preferences?saved=appearance#appearance"
    );
    let r = send(&site, "GET", "/wiki/Special:RecentChanges", c, None, &[]).await;
    assert!(
        r.body.contains("skin-theme-clientpref-night"),
        "{}",
        &r.body[..300]
    );
    assert!(
        r.body
            .contains("data-appearance=\"account\" data-mode=\"dark\" data-text-size=\"medium\"")
    );
    // The server links a signed-in reader's own palette, and the wiki's type set.
    assert!(
        r.body.contains(
            "<link rel=\"stylesheet\" id=\"ts-palette\" href=\"/ui/theme/p-parchment-rg-"
        )
    );
    assert!(
        r.body
            .contains("<link rel=\"stylesheet\" id=\"ts-typeset\" href=\"/ui/theme/t-default26-")
    );

    // The Appearance menu without JavaScript: only from the site's own pages.
    let menu = form(&[
        ("returnto", "Special:RecentChanges"),
        ("ui.text-size", "large"),
        ("ui.mode", "dark"),
        ("ui.palette", ""),
        ("ui.typeset", "hyperlegible"),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:Preferences&appearance=1",
        c,
        Some(menu.clone()),
        &[],
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::FORBIDDEN,
        "no Origin, no Sec-Fetch-Site"
    );
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:Preferences&appearance=1",
        c,
        Some(menu.clone()),
        &[("origin", "https://evil.example")],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:Preferences&appearance=1",
        c,
        Some(menu),
        &[("origin", &format!("https://{HOST}"))],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert_eq!(r.location(), "/wiki/Special:RecentChanges");
    let r = send(&site, "GET", "/wiki/Special:RecentChanges", c, None, &[]).await;
    assert!(r.body.contains("data-text-size=\"large\""));
    assert!(
        r.body
            .contains("id=\"ts-palette\" href=\"/ui/theme/p-parchment-by-"),
        "the empty choice resets to the wiki's"
    );
    assert!(
        r.body
            .contains("id=\"ts-typeset\" href=\"/ui/theme/t-hyperlegible-")
    );
    // The sheet the page names is served, cached as immutable.
    let at = r.body.find("/ui/theme/t-hyperlegible-").unwrap();
    let href = &r.body[at..at + r.body[at..].find('"').unwrap()];
    let css = send(&site, "GET", href, None, None, &[]).await;
    assert_eq!(css.status, StatusCode::OK);
    assert!(
        css.body
            .contains("--font-family-base: 'Atkinson Hyperlegible'")
    );

    // Restore all defaults.
    let page = send(&site, "GET", "/wiki/Special:Preferences", c, None, &[]).await;
    let restore = form(&[
        ("section", "restore"),
        ("token", &page.hidden("token")),
        ("reset_all", "1"),
    ]);
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:Preferences",
        c,
        Some(restore),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let r = send(
        &site,
        "GET",
        "/wiki/Special:Preferences?saved=restore",
        c,
        None,
        &[],
    )
    .await;
    assert!(
        r.body
            .contains("All your preferences are back to the defaults.")
    );
    assert!(!r.body.contains("data-mode="));

    // A reader without an account posting the menu keeps nothing and is sent back.
    let r = send(
        &site,
        "POST",
        "/w/index.php?title=Special:Preferences&appearance=1",
        None,
        Some(form(&[
            ("returnto", "Special:RecentChanges"),
            ("ui.mode", "dark"),
        ])),
        &[("origin", &format!("https://{HOST}"))],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
}
