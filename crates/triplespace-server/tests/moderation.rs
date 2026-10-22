//! The moderation modules end to end (0023 §8; 09 §6): `action=protect`, `action=delete`,
//! `action=undelete` and `action=revisiondelete` on document pages and threads, what they
//! do to the views (deleted pages leave the title index, hidden parts set the visibility
//! bits) and the log events they project as. Needs `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

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

const DB: &str = "tsmoderation_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "lbwiki";
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
        Reply { status, body }
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
async fn protect_delete_undelete_and_hide() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsmoderation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: Some("Library Base".into()),
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
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
    };

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
    let t = enc(&token);
    let mut anon = Client {
        router: c.router.clone(),
        cookie: None,
    };

    // A page with two revisions.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=First.&token={t}"
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().unwrap();
    c.api_post(&format!(
        "action=edit&title=Project:Scope&text=Second.&token={t}"
    ))
    .await;

    // The modules need a token and the right; an anonymous caller has neither.
    let r = anon
        .send(
            "POST",
            "/w/api.php",
            Some("application/x-www-form-urlencoded"),
            Some(
                "action=delete&title=Project:Scope&format=json&formatversion=2&token=%2B%5C".into(),
            ),
            &[],
        )
        .await;
    assert!(r.body["error"]["code"].is_string(), "{}", r.body);

    // Protect, then change the protection.
    let p = c
        .api_post(&format!("action=protect&title=Project:Scope&protections=edit%3Dsysop%7Cmove%3Dsysop&expiry=infinite&reason=Vandalism&token={t}"))
        .await;
    assert_eq!(p["protect"]["protections"][0]["edit"], "sysop", "{p}");
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Scope&protections=edit%3Dautoconfirmed&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Scope&protections=edit%3Dsysop&cascade=1&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-no-cascade");

    // Delete: the page is missing for everyone, its title is free, and the protection
    // stays beside the deletion.
    let d = c
        .api_post(&format!(
            "action=delete&title=Project:Scope&reason=Spam&token={t}"
        ))
        .await;
    assert_eq!(d["delete"]["title"], "Library Base:Scope", "{d}");
    assert!(d["delete"]["logid"].is_u64());
    let q = anon
        .api("action=query&prop=info&titles=Project:Scope")
        .await;
    assert_eq!(q["query"]["pages"][0]["missing"], true, "{q}");
    let r = c
        .api_post(&format!("action=delete&title=Project:Scope&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle", "{r}");

    // A new page takes the title; undeleting into it is refused.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Fresh.&createonly=1&token={t}"
        ))
        .await;
    let fresh = e["edit"]["pageid"].as_u64().unwrap();
    assert_ne!(fresh, page);
    let r = c
        .api_post(&format!("action=undelete&title=Project:Scope&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "ts-title-taken", "{r}");

    // Delete the new one, undelete the newest deleted, which is it.
    c.api_post(&format!("action=delete&title=Project:Scope&token={t}"))
        .await;
    let u = c
        .api_post(&format!(
            "action=undelete&title=Project:Scope&reason=Mistake&token={t}"
        ))
        .await;
    assert_eq!(u["undelete"]["title"], "Library Base:Scope", "{u}");
    let q = anon
        .api("action=query&prop=info&titles=Project:Scope")
        .await;
    assert_eq!(q["query"]["pages"][0]["pageid"], fresh, "{q}");

    // The log events.
    let l = anon.api("action=query&list=logevents&lelimit=50").await;
    let events: Vec<String> = l["query"]["logevents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            format!(
                "{}/{}",
                e["type"].as_str().unwrap_or("?"),
                e["action"].as_str().unwrap_or("?")
            )
        })
        .collect();
    for want in [
        "protect/protect",
        "protect/modify",
        "delete/delete",
        "delete/restore",
    ] {
        assert!(events.iter().any(|e| e == want), "{want} in {events:?}");
    }

    // Hide a revision's text and summary: `texthidden` and `commenthidden` in the page's
    // history, for everyone.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Libel.&token={t}"
        ))
        .await;
    let libel = e["edit"]["newrevid"].as_u64().unwrap();
    let h = c
        .api_post(&format!("action=revisiondelete&type=revision&ids={libel}&hide=content%7Ccomment&reason=Libel&token={t}"))
        .await;
    assert_eq!(h["revisiondelete"]["items"][0]["texthidden"], true, "{h}");
    assert_eq!(h["revisiondelete"]["items"][0]["commenthidden"], true);
    let r = anon
        .api(&format!(
            "action=query&prop=revisions&revids={libel}&rvprop=content%7Ccomment%7Cids"
        ))
        .await;
    let rev = &r["query"]["pages"][0]["revisions"][0];
    assert_eq!(rev["texthidden"], true, "{r}");
    assert_eq!(rev["commenthidden"], true);
    assert!(
        rev.get("content").is_none() && rev.get("slots").is_none(),
        "{rev}"
    );
    // Showing the comment again leaves the text hidden.
    let h = c
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={libel}&show=comment&token={t}"
        ))
        .await;
    assert!(
        h["revisiondelete"]["items"][0]
            .get("commenthidden")
            .is_none(),
        "{h}"
    );
    assert_eq!(h["revisiondelete"]["items"][0]["texthidden"], true);
    let l = anon
        .api("action=query&list=logevents&letype=delete&lelimit=50")
        .await;
    assert!(
        l["query"]["logevents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "revision"),
        "{l}"
    );

    // A thread deleted through action=delete answers 410 on the thread route.
    let r = c
        .rest("POST", "/thread", Some(json!({"subject": "Q", "target": {"kind": "page", "id": fresh}, "text": "Hi", "token": token})))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread = r.body["thread"].as_u64().unwrap();
    let title = r.body["title"].as_str().unwrap().to_string();
    let d = c
        .api_post(&format!(
            "action=delete&title={}&token={t}",
            enc(&format!("Thread:{title}"))
        ))
        .await;
    assert!(d.get("error").is_none(), "{d}");
    assert_eq!(
        anon.rest("GET", &format!("/thread/{thread}"), None)
            .await
            .status,
        StatusCode::GONE
    );
}
