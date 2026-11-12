//! Name tombstones and `renameuser` end to end (ADR 0010 §10; 19 §2.3; 0102 §3): a rename
//! by hand through `action=renameuser`, its log event, the old name refused to every
//! newcomer, and under strict names the tombstone that yields to the issuer that named
//! it and holds against everyone else. The harness is that of `strict_names.rs`. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_actors::issuer::OAuthEndpoints;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::oauth::{
    self, BoxFuture, Client as OAuthClient, Profile, SourceUser, Upstream, UpstreamError,
};
use triplespace_accounts::{Secret, tombstones};
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;

const DB: &str = "tstombstone_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "librarybase";
const REST: &str = "/w/rest.php/triplespace/v0";
const ISSUER: &str = "wikimedia-central";

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
    triplespace_db::scratch::drop_at_thread_end(DB);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{DB}"))
        .unwrap()
}

/// What an exchange was sent: code, verifier, client ID, secret, redirect URI.
type Exchange = (String, String, String, String, String);

/// A provider and a source wiki that answer from tables, and remember what they were
/// sent.
#[derive(Default)]
struct Fake {
    /// code → (sub, username)
    codes: BTreeMap<String, (String, String)>,
    /// username → user on the source wiki
    source: BTreeMap<String, SourceUser>,
    /// The (code, verifier, client_id, secret, redirect_uri) of each exchange.
    seen: Mutex<Vec<Exchange>>,
}

impl Upstream for Fake {
    fn identify<'a>(
        &'a self,
        _endpoints: &'a OAuthEndpoints,
        client: &'a OAuthClient,
        code: &'a str,
        verifier: &'a str,
    ) -> BoxFuture<'a, Result<Profile, UpstreamError>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push((
                code.into(),
                verifier.into(),
                client.client_id.clone(),
                client.secret.clone(),
                client.redirect_uri.clone(),
            ));
            let (sub, username) = self
                .codes
                .get(code)
                .ok_or_else(|| UpstreamError("invalid_grant".into()))?;
            Ok(Profile {
                sub: sub.clone(),
                username: username.clone(),
            })
        })
    }

    fn source_user<'a>(
        &'a self,
        users_api: &'a str,
        username: &'a str,
    ) -> BoxFuture<'a, Result<Option<SourceUser>, UpstreamError>> {
        Box::pin(async move {
            assert_eq!(users_api, "https://librarybase.org/w/api.php");
            if username == "Unreachable" {
                return Err(UpstreamError("timeout".into()));
            }
            Ok(self.source.get(username).cloned())
        })
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A browser: one session cookie, one address.
struct Browser {
    router: Router,
    cookie: Option<String>,
    address: SocketAddr,
}

struct Reply {
    status: StatusCode,
    location: Option<String>,
    body: Value,
}

impl Browser {
    fn new(router: &Router, address: [u8; 4]) -> Self {
        Self {
            router: router.clone(),
            cookie: None,
            address: SocketAddr::from((address, 50000)),
        }
    }

    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        ct: Option<&str>,
        body: Option<String>,
    ) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(ct) = ct {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut().insert(ConnectInfo(self.address));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        if let Some(sc) = response.headers().get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap()
                .to_string();
            self.cookie = if value.is_empty() { None } else { Some(value) };
        }
        let location = response
            .headers()
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Reply {
            status,
            location,
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
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn post(&mut self, form: &str) -> Value {
        let r = self
            .send(
                "POST",
                "/w/api.php",
                Some("application/x-www-form-urlencoded"),
                Some(format!("{form}&format=json&formatversion=2")),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn token(&mut self, kind: &str) -> String {
        self.api(&format!("action=query&meta=tokens&type={kind}"))
            .await["query"]["tokens"][format!("{kind}token")]
        .as_str()
        .unwrap()
        .to_string()
    }

    async fn whoami(&mut self) -> Option<String> {
        let v = self.api("action=query&meta=userinfo").await;
        let u = &v["query"]["userinfo"];
        (u["anon"].as_bool() != Some(true) && u["id"].as_u64() != Some(0))
            .then(|| u["name"].as_str().unwrap_or_default().to_string())
    }

    /// The site's door: posts the login button and returns the provider's URL.
    async fn start(&mut self, purpose: &str) -> Reply {
        let (field, token) = if purpose == "link" {
            ("token", self.token("csrf").await)
        } else {
            ("logintoken", self.token("login").await)
        };
        self.send(
            "POST",
            &format!("{REST}/auth/{ISSUER}/start"),
            Some("application/x-www-form-urlencoded"),
            Some(format!(
                "purpose={purpose}&{field}={}&returnto=Main_Page",
                enc(&token)
            )),
        )
        .await
    }

    /// The provider's answer, as the browser follows it back.
    async fn callback(&mut self, state: &str, answer: &str) -> Reply {
        self.send(
            "GET",
            &format!(
                "{REST}/auth/{ISSUER}/callback?state={}&{answer}",
                enc(state)
            ),
            None,
            None,
        )
        .await
    }

    /// Logs in through the site's door with a code the fake knows; the callback's reply.
    async fn sign_in(&mut self, purpose: &str, code: &str) -> Reply {
        let r = self.start(purpose).await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
        let state = query_of(r.location.as_deref().unwrap())["state"].clone();
        self.callback(&state, &format!("code={code}")).await
    }
}

impl Browser {
    /// `PATCH /settings?scope=instance` with `set`, at the farm base.
    async fn set_instance(&mut self, settings: Value) -> Reply {
        let token = self.token("csrf").await;
        let base = self
            .send(
                "GET",
                &format!("{REST}/settings?scope=instance"),
                None,
                None,
            )
            .await
            .body["base"]
            .clone();
        self.send(
            "PATCH",
            &format!("{REST}/settings?scope=instance"),
            Some("application/json"),
            Some(json!({"set": settings, "base": base, "token": token}).to_string()),
        )
        .await
    }
}

fn query_of(url: &str) -> BTreeMap<String, String> {
    let u = url::Url::parse(url)
        .unwrap_or_else(|_| url::Url::parse(&format!("https://{HOST}{url}")).unwrap());
    u.query_pairs().into_owned().collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn name_tombstones() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tstombstone-{}", std::process::id()));
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
        adopt: Some(format!("https://{HOST}/")),
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    let fake = Arc::new(Fake {
        codes: [
            ("code-alice", ("6001", "AliceWM")),
            ("code-eve", ("9001", "Eve")),
            ("code-bob", ("5001", "Bob")),
            ("code-bob-renamed", ("5001", "Bobby")),
            ("code-usurper", ("5002", "Bob")),
            ("code-dan", ("5003", "Dan")),
        ]
        .into_iter()
        .map(|(c, (s, u))| (c.to_string(), (s.to_string(), u.to_string())))
        .collect(),
        source: BTreeMap::new(),
        seen: Mutex::default(),
    });
    let secret = Secret::derive(&[7u8; 32]);
    let pool = triplespace_db::scratch::pools(&database, 4);
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let app = App::with_upstream(
        pool.clone(),
        secret.clone(),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: farm.clone(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
        fake.clone(),
    )
    .unwrap();
    let router = router_with(app.clone(), triplespace_api_rest::routes());
    let db = pool.accounts.get().await.unwrap();
    let registry = scatter_actors::issuer::IssuerRegistry::default_registry();
    let callback = oauth::callback_url(&format!("https://{HOST}"), ISSUER);
    oauth::set_client(
        &**db,
        &secret,
        registry,
        "",
        ISSUER,
        "client-123",
        "s3cret",
        &callback,
    )
    .await
    .unwrap();
    let tombstone = |name: &'static str| {
        let db = pool.accounts.clone();
        let secret = secret.clone();
        async move {
            let c = db.get().await.unwrap();
            tombstones::held(&**c, &secret, TENANT, name).await.unwrap()
        }
    };

    // Eve registers with a chosen name.
    let mut eve = Browser::new(&router, [203, 0, 113, 20]);
    let r = eve.sign_in("login", "code-eve").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let token = eve.token("createaccount").await;
    let r = eve
        .post(&format!(
            "action=createaccount&username=Eve&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");

    // A rename by hand (19 §2.3): `action=renameuser`, restricted to `renameuser`.
    let mut alice = Browser::new(&router, [198, 51, 100, 1]);
    let lt = alice.token("login").await;
    let r = alice
        .post(&format!(
            "action=clientlogin&username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(&lt)
        ))
        .await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    let csrf = eve.token("csrf").await;
    let r = eve
        .post(&format!(
            "action=renameuser&user=Eve&newuser=Zed&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");
    let csrf = alice.token("csrf").await;
    let r = alice
        .post(&format!(
            "action=renameuser&user=Nobody&newuser=Zed&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["error"]["code"], "nosuchuser", "{r}");
    let r = alice
        .post(&format!(
            "action=renameuser&user=Eve&newuser=Evelyn&reason=asked&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["renameuser"]["from"], "Eve", "{r}");
    assert_eq!(r["renameuser"]["to"], "Evelyn", "{r}");
    assert_eq!(eve.whoami().await.as_deref(), Some("Evelyn"));
    // The rename is a public `renameuser/renameuser` event by its performer (16 §2.3).
    let r = alice
        .api("action=query&list=logevents&letype=renameuser&leprop=user|type|details|comment")
        .await;
    let events = r["query"]["logevents"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{r}");
    assert_eq!(events[0]["action"], "renameuser", "{r}");
    assert_eq!(events[0]["user"], "Alice", "{r}");
    assert_ne!(events[0]["params"]["instance"], true, "{r}");
    assert!(
        events[0]["comment"]
            .as_str()
            .is_some_and(|c| c.ends_with("renameuser: asked")),
        "{r}"
    );

    // A name is never reused (0010 §10): the old name is a tombstone of no issuer,
    // refused to a newcomer, in any case, and to a rename back.
    assert_eq!(
        tombstone("Eve").await,
        Some(tombstones::Tombstone { issuer: None })
    );
    let mut anon = Browser::new(&router, [203, 0, 113, 9]);
    for name in ["Eve", "eve", "EVE"] {
        let token = anon.token("createaccount").await;
        let r = anon
            .post(&format!(
                "action=createaccount&username={name}&password=zed-secret-1&retype=zed-secret-1&createtoken={}&createreturnurl=https://{HOST}/",
                enc(&token)
            ))
            .await;
        assert_eq!(r["createaccount"]["messagecode"], "ts-name-reserved", "{r}");
    }
    let token = anon.token("createaccount").await;
    let r = anon
        .post(&format!(
            "action=createaccount&username=Evelyn&password=zed-secret-1&retype=zed-secret-1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "userexists", "{r}");
    let r = alice
        .post(&format!(
            "action=renameuser&user=Evelyn&newuser=Eve&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-name-reserved", "{r}");
    let r = alice
        .post(&format!(
            "action=renameuser&user=Evelyn&newuser=Wikimedia-Zed&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-name-reserved", "a prefix: {r}");

    // Preparing strict names (0102 §5): a bureaucrat may rename an account to the one
    // prefixed name that is its own derived name.
    let r = alice.sign_in("link", "code-alice").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:Account?tsnotice=ts-linked#sign-in-methods")
    );
    let r = alice
        .set_instance(json!({"login.external_required": true}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let csrf = alice.token("csrf").await;
    let r = alice
        .post(&format!(
            "action=renameuser&user=Evelyn&newuser=Wikimedia-Eve&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["renameuser"]["to"], "Wikimedia-Eve", "{r}");
    let r = alice
        .post(&format!(
            "action=renameuser&user=Alice&newuser=Wikimedia-AliceWM&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["renameuser"]["to"], "Wikimedia-AliceWM", "{r}");
    assert_eq!(alice.whoami().await.as_deref(), Some("Wikimedia-AliceWM"));
    assert_eq!(
        tombstone("Evelyn").await,
        Some(tombstones::Tombstone { issuer: None })
    );
    let r = alice.set_instance(json!({"login.names": "provider"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    // Under strict names a rename by hand is refused (0102 §3, Q1).
    let csrf = alice.token("csrf").await;
    let r = alice
        .post(&format!(
            "action=renameuser&user=Wikimedia-Eve&newuser=Wikimedia-Evelyn&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-strict-name", "{r}");

    // The provider renames Bob: the released name is a tombstone of the issuer's own
    // naming, and the rename an instance act (0102 §3).
    let mut bob = Browser::new(&router, [203, 0, 113, 30]);
    let r = bob.sign_in("login", "code-bob").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let token = bob.token("createaccount").await;
    let r = bob
        .post(&format!(
            "action=createaccount&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["username"], "Wikimedia-Bob", "{r}");
    let csrf = bob.token("csrf").await;
    bob.post(&format!("action=logout&token={}", enc(&csrf)))
        .await;
    let r = bob.sign_in("login", "code-bob-renamed").await;
    assert_eq!(r.location.as_deref(), Some("/wiki/Main_Page"), "{}", r.body);
    assert_eq!(bob.whoami().await.as_deref(), Some("Wikimedia-Bobby"));
    assert_eq!(
        tombstone("Wikimedia-Bob").await,
        Some(tombstones::Tombstone {
            issuer: Some(ISSUER.into())
        })
    );
    let r = alice
        .api("action=query&list=logevents&letype=renameuser&leprop=user|type|details|comment&lelimit=1")
        .await;
    let events = r["query"]["logevents"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{r}");
    assert_eq!(events[0]["params"]["instance"], true, "{r}");
    assert!(
        events[0]["comment"]
            .as_str()
            .is_some_and(|c| c.ends_with("renameuser: renamed by the provider")),
        "{r}"
    );

    // The tombstone yields to the same issuer attesting another subject for the name
    // (0102 §3): the person Wikimedia now calls Bob is Wikimedia-Bob here.
    let mut usurper = Browser::new(&router, [203, 0, 113, 40]);
    let r = usurper.sign_in("login", "code-usurper").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let p = usurper
        .send("GET", &format!("{REST}/auth/pending"), None, None)
        .await;
    assert_eq!(p.body["name"], "Wikimedia-Bob", "{}", p.body);
    let token = usurper.token("createaccount").await;
    let r = usurper
        .post(&format!(
            "action=createaccount&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    assert_eq!(usurper.whoami().await.as_deref(), Some("Wikimedia-Bob"));

    // A tombstone of any other origin holds, even for a derived name.
    tombstones::reserve(&**db, &secret, TENANT, "Wikimedia-Dan", None)
        .await
        .unwrap();
    let mut dan = Browser::new(&router, [203, 0, 113, 50]);
    let r = dan.sign_in("login", "code-dan").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let token = dan.token("createaccount").await;
    let r = dan
        .post(&format!(
            "action=createaccount&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "ts-name-reserved", "{r}");
}
