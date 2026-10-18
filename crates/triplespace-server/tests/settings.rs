//! Settings and preferences end to end (ADR 0085): the wiki's name from `instance create`,
//! `GET /settings` and `PATCH /settings` with their refusals, instance defaults and locks
//! at the farm base, `siteinfo` from the resolved settings, `settings/change` in the log,
//! `action=options` and `uiprop=options`, the preference routes, and `triplespace
//! settings`. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

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
use triplespace_cli::{instance, settings};
use triplespace_projections::Farm;

const DB: &str = "tssettings_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "librarybase";
const REST: &str = "/w/rest.php/triplespace/v0";

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

/// A client with one session cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl Client {
    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Option<String>,
        extra: &[(header::HeaderName, &str)],
    ) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        for (name, value) in extra {
            req = req.header(name, *value);
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        if let Some(sc) = headers.get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap()
                .to_string();
            self.cookie = if value.is_empty() { None } else { Some(value) };
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn api(&mut self, query: &str) -> Value {
        let r = self
            .send(
                "GET",
                &format!("/w/api.php?{query}&format=json&formatversion=2"),
                None,
                None,
                &[],
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn api_post(&mut self, form: &str) -> Value {
        let r = self
            .send(
                "POST",
                "/w/api.php",
                Some("application/x-www-form-urlencoded"),
                Some(format!("{form}&format=json&formatversion=2")),
                &[],
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn rest(&mut self, method: &str, path: &str, body: Option<Value>) -> Reply {
        let body = body.map(|b| b.to_string());
        let ct = body.as_ref().map(|_| "application/json");
        self.send(method, &format!("{REST}{path}"), ct, body, &[])
            .await
    }

    async fn csrf(&mut self) -> String {
        self.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn settings_and_preferences() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssettings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: Some("Library Base".into()),
        farm_slug: None,
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
            farm: Farm {
                slug: TENANT.into(),
                base: format!("https://{HOST}"),
            },
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
    };

    // The name `instance create` gave, in siteinfo and the settings; the wiki ID stays
    // the slug, and the Project namespace takes the name.
    let si = c
        .api("action=query&meta=siteinfo&siprop=general|namespaces")
        .await;
    assert_eq!(si["query"]["general"]["sitename"], "Library Base");
    assert_eq!(si["query"]["general"]["wikiid"], "librarybase");
    assert_eq!(si["query"]["general"]["lang"], "en");
    assert_eq!(
        si["query"]["namespaces"]["4"]["name"], "Library Base",
        "{si}"
    );

    // Anyone may read the settings; each carries its origin.
    let r = c.rest("GET", "/settings", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(
        r.headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    let s = &r.body["settings"];
    assert_eq!(s["site.name"]["value"], "Library Base");
    assert_eq!(s["site.name"]["origin"], "tenant");
    assert_eq!(s["site.name"]["can_change"], false);
    assert_eq!(s["site.language"]["origin"], "registry");
    assert!(
        s.get("version.services").is_none(),
        "instance settings are apart"
    );
    let r = c.rest("GET", "/settings/schema", None).await;
    let names: Vec<&str> = r.body["settings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["key"].as_str())
        .collect();
    assert!(names.contains(&"site.logo"));
    let lang_def = r.body["settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["key"] == "site.language")
        .unwrap();
    assert_eq!(lang_def["label"], "ts-setting-site-language");
    // The farm base is this tenant's base, so `scope=instance` is served here.
    let r = c.rest("GET", "/settings?scope=instance", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["settings"]["version.services"]["value"], "full");
    let r = c.rest("GET", "/settings?keys=no.such", None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["code"], "ts-unknown-setting");

    // A reader without an account may not write.
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"site.language": "de"}})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["code"], "permissiondenied");

    // The owner logs in.
    let t = c.api("action=query&meta=tokens&type=login").await;
    let login = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = c
        .api_post(&format!(
            "action=clientlogin&username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(&login)
        ))
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let token = c.csrf().await;

    // A session's write needs its token.
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"site.language": "de"}})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["code"], "badtoken");

    // Read, then save against the base read.
    let read = c.rest("GET", "/settings?group=site", None).await;
    assert_eq!(read.body["settings"]["site.language"]["can_change"], true);
    assert!(
        read.body["settings"].get("account.approval").is_none(),
        "group= narrows"
    );
    let base = read.body["base"].as_u64().unwrap();
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"site.language": "de"}, "summary": "German wiki", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["changes"][0]["key"], "site.language");
    assert_eq!(r.body["changes"][0]["old"], Value::Null);
    assert_eq!(r.body["changes"][0]["new"], "de");
    assert_eq!(
        r.body["records"].as_array().unwrap().len(),
        2,
        "the record and the log event"
    );
    // The same base again is now a conflict on that key.
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"site.language": "fr"}, "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.body["code"], "editconflict");
    // Every reason a save is refused, and nothing written.
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"no.such": 1, "account.approval": "yes", "site.name": "Talk", "version.services": "off"}, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    let mut codes: Vec<&str> = r.body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["code"].as_str())
        .collect();
    codes.sort_unstable();
    assert_eq!(
        codes,
        vec![
            "ts-instance-policy",
            "ts-invalid-setting",
            "ts-invalid-setting",
            "ts-unknown-setting"
        ]
    );
    let si = c.api("action=query&meta=siteinfo").await;
    assert_eq!(si["query"]["general"]["lang"], "de");
    assert_eq!(si["query"]["general"]["sitename"], "Library Base");

    // The save is one `settings/change` event in the log.
    let log = c.api("action=query&list=logevents&letype=settings").await;
    let events = log["query"]["logevents"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{log}");
    assert_eq!(events[0]["action"], "change");

    // Unset returns it to the registry default.
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"unset": ["site.language"], "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c.rest("GET", "/settings?keys=site.language", None).await;
    assert_eq!(r.body["settings"]["site.language"]["origin"], "registry");
    assert_eq!(r.body["settings"]["site.language"]["value"], "en");

    // At the farm base: defaults need `config.template` other than `none`; turning it on
    // in the same save allows them, and the tenant inherits at once.
    let r = c
        .rest(
            "PATCH",
            "/settings?scope=instance",
            Some(json!({"defaults": {"site.language": "fr"}, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.body["code"], "ts-instance-policy");
    let r = c
        .rest(
            "PATCH",
            "/settings?scope=instance",
            Some(json!({"tenancy": {"config.template": "defaults"}, "defaults": {"site.language": "fr"}, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c.rest("GET", "/settings?keys=site.language", None).await;
    assert_eq!(r.body["settings"]["site.language"]["origin"], "instance");
    assert_eq!(r.body["settings"]["site.language"]["value"], "fr");
    let si = c.api("action=query&meta=siteinfo").await;
    assert_eq!(si["query"]["general"]["lang"], "fr");
    // The tenant's own value wins over the default …
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"site.language": "de"}, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // … until the instance locks the key.
    let r = c
        .rest(
            "PATCH",
            "/settings?scope=instance",
            Some(json!({"tenancy": {"config.template": "locks"}, "locks": {"site.language": true}, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c.rest("GET", "/settings?keys=site.language", None).await;
    assert_eq!(r.body["settings"]["site.language"]["origin"], "locked");
    assert_eq!(r.body["settings"]["site.language"]["value"], "fr");
    assert_eq!(r.body["settings"]["site.language"]["can_change"], false);
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"site.language": "nl"}, "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-locked", "{}", r.body);
    let r = c.rest("GET", "/settings/schema?scope=instance", None).await;
    assert_eq!(r.body["template"], "locks");
    assert_eq!(r.body["tenant_settings"]["site.language"]["locked"], true);
    assert_eq!(
        r.body["tenant_settings"]["site.language"]["instance_default"],
        "fr"
    );

    // Preferences: the holder's alone.
    let mut anon = Client {
        router: c.router.clone(),
        cookie: None,
    };
    let r = anon.rest("GET", "/account/preferences", None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // A reader who is not logged in is still served the site's language.
    let ui = anon.api("action=query&meta=userinfo&uiprop=options").await;
    assert_eq!(ui["query"]["userinfo"]["options"]["language"], "fr");
    let r = c.rest("GET", "/account/preferences", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["preferences"]["ui.mode"]["value"], "auto");
    assert_eq!(r.body["preferences"]["ui.mode"]["origin"], "registry");
    assert_eq!(r.body["preferences"]["language"]["origin"], "tenant");
    assert!(
        r.headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("private")
    );

    // action=options, MediaWiki's way: unknown keys and bad values warn and are skipped.
    let out = c
        .api_post(&format!(
            "action=options&change={}&token={}",
            enc("ui.mode=dark|rc.limit=250|no.such=1|ui.contrast=purple"),
            enc(&token)
        ))
        .await;
    assert_eq!(out["options"], "success", "{out}");
    assert!(out["warnings"].to_string().contains("no.such"), "{out}");
    let ui = c.api("action=query&meta=userinfo&uiprop=options").await;
    let o = &ui["query"]["userinfo"]["options"];
    assert_eq!(o["ui.mode"], "dark");
    assert_eq!(o["rc.limit"], 250);
    assert_eq!(o["ui.contrast"], "auto");
    // uselang overrides the language for one request.
    let ui = c
        .api("action=query&meta=userinfo&uiprop=options&uselang=nl")
        .await;
    assert_eq!(ui["query"]["userinfo"]["options"]["language"], "nl");
    // The REST routes.
    let r = c
        .rest(
            "PATCH",
            "/account/preferences",
            Some(json!({"set": {"timezone": "Europe/Amsterdam"}, "reset": ["rc.limit"], "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        r.body["preferences"]["timezone"]["value"],
        "Europe/Amsterdam"
    );
    assert_eq!(r.body["preferences"]["timezone"]["own"], true);
    assert_eq!(r.body["preferences"]["rc.limit"]["value"], 50);
    let r = c
        .rest(
            "PATCH",
            "/account/preferences",
            Some(json!({"set": {"ui.mode": "purple"}, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["code"], "ts-invalid-preference");
    let r = c
        .rest(
            "DELETE",
            &format!("/account/preferences/ui.mode?token={}", enc(&token)),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["preferences"]["ui.mode"]["value"], "auto");
    let out = c
        .api_post(&format!("action=options&reset=1&token={}", enc(&token)))
        .await;
    assert_eq!(out["options"], "success");
    let r = c.rest("GET", "/account/preferences", None).await;
    assert_eq!(r.body["preferences"]["timezone"]["own"], false);

    // The command line writes through the same validator.
    let at = || settings::Where {
        database: database.clone(),
        tenant: None,
        instance: false,
    };
    settings::run(settings::Settings::Set(settings::Set {
        at: at(),
        key: "site.name".into(),
        value: "Librarybase".into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: Some("rename".into()),
    }))
    .await
    .expect("settings set");
    let refused = settings::run(settings::Settings::Set(settings::Set {
        at: at(),
        key: "site.name".into(),
        value: "User".into(),
        default: false,
        lock: false,
        tenancy: false,
        summary: None,
    }))
    .await;
    assert!(refused.is_err(), "the CLI refuses what the API refuses");
    settings::run(settings::Settings::Unset(settings::Unset {
        at: at(),
        key: "site.language".into(),
        default: false,
        lock: true,
        summary: None,
    }))
    .await
    .expect("settings unset --lock");
    let si = c.api("action=query&meta=siteinfo").await;
    assert_eq!(si["query"]["general"]["sitename"], "Librarybase");
    assert_eq!(
        si["query"]["general"]["lang"], "de",
        "unlocked: the tenant's own value is back"
    );
}
