//! Login through external issuers end to end (ADR 0088), with a fake provider behind
//! [`Upstream`]: the client credential and `siprop=triplespace`'s `login`; the site's
//! door (`/auth/{issuer}/start`, the callback, `/auth/pending`) with PKCE and `state`;
//! the first login's pending identity and `createaccount&createpreservestate`; logging in
//! again; the Action API's door (`REDIRECT`, the callback, `logincontinue`); adding and
//! removing sign-in methods, the last refused; approval; `login.issuers` and
//! `ts-no-login-method`; and reclaiming an adopted account by vouching. Needs
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
use triplespace_accounts::Secret;
use triplespace_accounts::oauth::{
    self, BoxFuture, Client as OAuthClient, Profile, SourceUser, Upstream, UpstreamError,
};
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;

const DB: &str = "tsexternal_test";
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

    async fn set(&mut self, settings: Value) -> Reply {
        let token = self.token("csrf").await;
        let base = self
            .send("GET", &format!("{REST}/settings"), None, None)
            .await
            .body["base"]
            .clone();
        self.send(
            "PATCH",
            &format!("{REST}/settings"),
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
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::similar_names
)]
async fn external_identity_providers() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsexternal-{}", std::process::id()));
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
            ("code-bob", ("mw:CentralAuth:5001", "Bob")),
            ("code-bob-2", ("5001", "Bob")),
            ("code-bob-3", ("mw:CentralAuth:5001", "Bob")),
            ("code-bob-4", ("5001", "Bob")),
            ("code-alice", ("6001", "AliceWM")),
            ("code-carol", ("7001", "Carol")),
            ("code-carol-2", ("7001", "Carol")),
            ("code-carol-3", ("7001", "Carol")),
            ("code-dana", ("8001", "Dana")),
            ("code-eve", ("9001", "Dana")),
            ("code-local", ("mw:local:5001", "Bob")),
            ("code-unreachable", ("9101", "Unreachable")),
        ]
        .into_iter()
        .map(|(c, (s, u))| (c.to_string(), (s.to_string(), u.to_string())))
        .collect(),
        source: [(
            "Dana".to_string(),
            SourceUser {
                userid: 42,
                central: Some(8001),
            },
        )]
        .into_iter()
        .collect(),
        seen: Mutex::default(),
    });
    let secret = Secret::derive(&[7u8; 32]);
    let pool = triplespace_db::pool::pool(&database, 4).unwrap();
    let app = App::with_upstream(
        pool.clone(),
        secret.clone(),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: triplespace_cli::common::farm_at(&database).await.unwrap(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
        fake.clone(),
    )
    .unwrap();
    let router = router_with(app.clone(), triplespace_api_rest::routes());
    let db = pool.get().await.unwrap();

    // No credential: nothing offered.
    let mut anon = Browser::new(&router, [203, 0, 113, 9]);
    let si = anon
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(si["query"]["triplespace"]["login"], json!([]));
    let r = anon.start("login").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert!(
        r.location.unwrap().contains("tserror=ts-oauth-unavailable"),
        "not offered without a credential"
    );

    // The instance default credential, registered for the primary tenant's callback.
    let callback = oauth::callback_url(&format!("https://{HOST}"), ISSUER);
    let registry = scatter_actors::issuer::IssuerRegistry::default_registry();
    assert!(
        oauth::set_client(
            &**db, &secret, registry, "", "openalex", "x", "y", &callback
        )
        .await
        .is_err(),
        "only an issuer with an oauth table"
    );
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
    let sealed: Vec<u8> = db
        .query_one("SELECT secret FROM private.oauth_client", &[])
        .await
        .unwrap()
        .get(0);
    assert!(
        !sealed.windows(6).any(|w| w == b"s3cret"),
        "the secret is sealed"
    );
    let si = anon
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(
        si["query"]["triplespace"]["login"],
        json!([{"code": ISSUER, "name": "Wikimedia central accounts (CentralAuth)", "label": "Wikimedia", "origin": "https://meta.wikimedia.org"}])
    );
    let v = anon
        .api("action=query&meta=authmanagerinfo&amirequestsfor=login&amimergerequestfields=1")
        .await;
    assert_eq!(
        v["query"]["authmanagerinfo"]["fields"][format!("tsissuer-{ISSUER}")]["type"],
        "button"
    );

    // The site's door: a bad token goes back to the login page.
    let r = anon
        .send(
            "POST",
            &format!("{REST}/auth/{ISSUER}/start"),
            Some("application/x-www-form-urlencoded"),
            Some("purpose=login&logintoken=nope&returnto=Main_Page".into()),
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert!(r.location.unwrap().contains("tserror=ts-oauth-state"));

    // A good one goes to the provider, with PKCE and a state.
    let r = anon.start("login").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let to = r.location.unwrap();
    assert!(
        to.starts_with("https://meta.wikimedia.org/w/rest.php/oauth2/authorize?"),
        "{to}"
    );
    let q = query_of(&to);
    assert_eq!(q["client_id"], "client-123");
    assert_eq!(q["redirect_uri"], callback);
    assert_eq!(q["code_challenge_method"], "S256");
    // The person declines.
    let r = anon.callback(&q["state"], "error=access_denied").await;
    assert!(
        r.location.unwrap().contains("tserror=ts-oauth-denied"),
        "declined"
    );
    // A state that is not this tenant's, or a replay, is refused.
    let r = anon.callback("not-a-state", "code=code-bob").await;
    assert!(r.location.unwrap().contains("tserror=ts-oauth-state"));
    let r = anon.callback(&q["state"], "code=code-bob").await;
    assert!(
        r.location.unwrap().contains("tserror=ts-oauth-state"),
        "replay"
    );

    // First login: no account yet, so a pending identity and on to CreateAccount.
    let r = anon.start("login").await;
    let q = query_of(r.location.as_deref().unwrap());
    let r = anon.callback(&q["state"], "code=code-bob").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    {
        let seen = fake.seen.lock().unwrap();
        let (code, verifier, client_id, secret_seen, redirect) = seen.last().unwrap();
        assert_eq!(code, "code-bob");
        assert_eq!(
            oauth::pkce_challenge(verifier),
            q["code_challenge"],
            "the verifier matches the challenge sent"
        );
        assert_eq!(
            (client_id.as_str(), secret_seen.as_str()),
            ("client-123", "s3cret")
        );
        assert_eq!(redirect, &callback);
    }
    assert_eq!(anon.whoami().await, None, "not logged in yet");
    let p = anon
        .send("GET", &format!("{REST}/auth/pending"), None, None)
        .await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.body);
    assert_eq!(p.body["issuer"], ISSUER);
    assert_eq!(p.body["username"], "Bob");
    let mut stranger = Browser::new(&router, [203, 0, 113, 10]);
    let _ = stranger.token("login").await;
    assert_eq!(
        stranger
            .send("GET", &format!("{REST}/auth/pending"), None, None)
            .await
            .status,
        StatusCode::NOT_FOUND,
        "another session cannot read it"
    );

    // The account for it: a name, no password, no challenge.
    let token = anon.token("createaccount").await;
    let r = anon
        .post(&format!(
            "action=createaccount&username=Alice&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "userexists", "{r}");
    let r = anon
        .post(&format!(
            "action=createaccount&username=Bob&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    assert_eq!(anon.whoami().await.as_deref(), Some("Bob"));
    let bob: String = db
        .query_one(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = 'Bob'",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        !triplespace_accounts::password::has(&**db, &bob)
            .await
            .unwrap(),
        "no password"
    );
    let a = anon
        .send("GET", &format!("{REST}/account"), None, None)
        .await;
    assert_eq!(a.body["bindings"][0]["issuer"], ISSUER, "{}", a.body);
    assert_eq!(a.body["ways"], 1);
    assert_eq!(a.body["can_add"], json!([]));
    // The only way to log in cannot go.
    let csrf = anon.token("csrf").await;
    let r = anon
        .post(&format!(
            "action=unlinkaccount&request=TriplespaceIssuerAuthenticationRequest:{ISSUER}&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-last-binding", "{r}");

    // Logging in again, as the same subject in the other `sub` form.
    let csrf = anon.token("csrf").await;
    anon.post(&format!("action=logout&token={}", enc(&csrf)))
        .await;
    assert_eq!(anon.whoami().await, None);
    let r = anon.sign_in("login", "code-bob-2").await;
    assert_eq!(r.location.as_deref(), Some("/wiki/Main_Page"), "{}", r.body);
    assert_eq!(anon.whoami().await.as_deref(), Some("Bob"));
    // A `sub` in another numbering is not this subject.
    let mut other = Browser::new(&router, [203, 0, 113, 11]);
    let r = other.sign_in("login", "code-local").await;
    assert!(r.location.unwrap().contains("tserror=ts-oauth-failed"));

    // The Action API's door: REDIRECT, the callback, logincontinue.
    let mut tool = Browser::new(&router, [203, 0, 113, 12]);
    let lt = tool.token("login").await;
    let r = tool
        .post(&format!(
            "action=clientlogin&tsissuer-{ISSUER}=1&logintoken={}&loginreturnurl={}",
            enc(&lt),
            enc("https://evil.example/")
        ))
        .await;
    assert_eq!(r["clientlogin"]["messagecode"], "ts-oauth-returnurl", "{r}");
    let r = tool
        .post(&format!(
            "action=clientlogin&tsissuer-{ISSUER}=1&logintoken={}&loginreturnurl={}",
            enc(&lt),
            enc(&format!("https://{HOST}/app?x=1"))
        ))
        .await;
    assert_eq!(r["clientlogin"]["status"], "REDIRECT", "{r}");
    let target = r["clientlogin"]["redirecttarget"]
        .as_str()
        .unwrap()
        .to_string();
    let state = query_of(&target)["state"].clone();
    // The browser the tool opened follows the provider back, without the tool's cookie.
    let mut opened = Browser::new(&router, [203, 0, 113, 13]);
    let back = opened.callback(&state, "code=code-bob-3").await;
    let back = back.location.unwrap();
    assert!(
        back.starts_with(&format!("https://{HOST}/app?x=1&state=")),
        "{back}"
    );
    assert_eq!(
        opened.whoami().await,
        None,
        "the outcome is the tool's session's"
    );
    let r = tool
        .post(&format!(
            "action=clientlogin&logincontinue=1&state={}&logintoken={}",
            enc(&query_of(&back)["state"]),
            enc(&lt)
        ))
        .await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    assert_eq!(r["clientlogin"]["username"], "Bob");
    assert_eq!(tool.whoami().await.as_deref(), Some("Bob"));

    // Alice, with a password, adds a sign-in method, and removes it.
    let mut alice = Browser::new(&router, [198, 51, 100, 1]);
    let lt = alice.token("login").await;
    let r = alice
        .post(&format!(
            "action=clientlogin&username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(&lt)
        ))
        .await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    let a = alice
        .send("GET", &format!("{REST}/account"), None, None)
        .await;
    assert_eq!(a.body["password"], true);
    assert_eq!(a.body["can_add"][0]["issuer"], ISSUER);
    let r = alice.sign_in("link", "code-bob-4").await;
    assert!(
        r.location
            .unwrap()
            .contains("tserror=ts-oauth-bound-elsewhere"),
        "Bob's identity is Bob's"
    );
    let r = alice.sign_in("link", "code-alice").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:Account?tsnotice=ts-linked#sign-in-methods")
    );
    let a = alice
        .send("GET", &format!("{REST}/account"), None, None)
        .await;
    assert_eq!(a.body["ways"], 2, "{}", a.body);
    let (partition, offset): (i64, i64) = db
        .query_one(
            "SELECT \"partition\", \"offset\" FROM private.binding WHERE subject = '6001'",
            &[],
        )
        .await
        .map(|r| (r.get(0), r.get(1)))
        .unwrap();
    let csrf = alice.token("csrf").await;
    let r = alice
        .post(&format!(
            "action=unlinkaccount&request=TriplespaceIssuerAuthenticationRequest:{ISSUER}&token={}",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["unlinkaccount"], json!({}), "{r}");
    let erased: i16 = db
        .query_one(
            "SELECT erased FROM log.record WHERE partition = $1 AND \"offset\" = $2",
            &[&partition, &offset],
        )
        .await
        .unwrap()
        .get(0);
    assert_ne!(erased, 0, "the binding record is erased");
    let a = alice
        .send("GET", &format!("{REST}/account"), None, None)
        .await;
    assert_eq!(a.body["ways"], 1);

    // Approval: a first login files a request, and learns how it stands by logging in.
    assert_eq!(
        alice.set(json!({"account.approval": true})).await.status,
        StatusCode::OK
    );
    let mut carol = Browser::new(&router, [192, 0, 2, 30]);
    let r = carol.sign_in("login", "code-carol").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let token = carol.token("createaccount").await;
    let r = carol
        .post(&format!(
            "action=createaccount&username=Carol&createpreservestate=1&tsstatement=Hello&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(
        r["createaccount"]["messagecode"], "ts-account-requested",
        "{r}"
    );
    let r = carol.sign_in("login", "code-carol-2").await;
    assert!(r.location.unwrap().contains("tserror=ts-account-pending"));
    let list = alice
        .send("GET", &format!("{REST}/account/requests"), None, None)
        .await;
    let id = list.body["requests"][0]["id"].as_str().unwrap().to_string();
    let csrf = alice.token("csrf").await;
    let r = alice
        .send(
            "POST",
            &format!("{REST}/account/requests/{id}/approve"),
            Some("application/json"),
            Some(json!({"token": csrf}).to_string()),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = carol.sign_in("login", "code-carol-3").await;
    assert_eq!(r.location.as_deref(), Some("/wiki/Main_Page"));
    assert_eq!(carol.whoami().await.as_deref(), Some("Carol"));
    alice.set(json!({"account.approval": false})).await;

    // login.issuers narrows what is offered; no way to log in at all is refused.
    let r = alice.set(json!({"login.issuers": []})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let si = anon
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(si["query"]["triplespace"]["login"], json!([]));
    let r = alice.set(json!({"login.password": false})).await;
    assert_ne!(r.status, StatusCode::OK);
    assert!(
        r.body.to_string().contains("ts-no-login-method"),
        "{}",
        r.body
    );
    alice.set(json!({"login.issuers": ["*"]})).await;
    let r = alice.set(json!({"login.password": false})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    alice.set(json!({"login.password": true})).await;

    // Vouching: an adopted account, reclaimed by the source's central ID.
    {
        use scatter_ingest::store::IngestStore as _;
        use scatter_projection::Backend as _;
        let store = app.store();
        let mut cx = store.begin().await.unwrap();
        let actors = store
            .partition(&mut cx, TENANT, "actors")
            .await
            .unwrap()
            .unwrap();
        let record = scatter_actors::actor::ActorRecord::registered("Dana");
        let dana = format!(
            "{}:42",
            triplespace_cli::common::issuer_at(&database, TENANT)
                .await
                .unwrap()
        );
        let d = scatter_log::store::Draft {
            appended_at: 1,
            payload_type: "scatter:v0/actor".into(),
            key: Some(dana.clone()),
            revid: None,
            logid: None,
            page_id: None,
            body: scatter_log::body::Body::core(
                &scatter_log::cbor::Value::from_json(&serde_json::to_value(&record).unwrap()),
                &scatter_log::cbor::Value::Null,
                &scatter_log::cbor::Value::map(vec![(
                    scatter_log::cbor::Value::text("actor"),
                    scatter_log::cbor::Value::text(&dana),
                )]),
            )
            .unwrap(),
        };
        scatter_ingest::write::append_projected(
            store,
            app.pipeline(),
            &mut cx,
            actors,
            d,
            scatter_projection::Budget::NONE,
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
    }
    let mut dana = Browser::new(&router, [192, 0, 2, 40]);
    // Not vouching yet: an unbound login is a pending identity.
    let r = dana.sign_in("login", "code-dana").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    assert_eq!(
        alice.set(json!({"reclaim.vouching": ISSUER})).await.status,
        StatusCode::OK
    );
    let mut eve = Browser::new(&router, [192, 0, 2, 41]);
    let r = eve.sign_in("login", "code-eve").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page"),
        "the source's central ID is not Eve's"
    );
    let mut lost = Browser::new(&router, [192, 0, 2, 42]);
    let r = lost.sign_in("login", "code-unreachable").await;
    assert!(
        r.location
            .unwrap()
            .contains("tserror=ts-reclaim-unavailable")
    );
    let mut dana = Browser::new(&router, [192, 0, 2, 43]);
    let r = dana.sign_in("login", "code-dana").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:UserLogin?tsnotice=ts-reclaimed&returnto=Main_Page")
    );
    assert_eq!(dana.whoami().await.as_deref(), Some("Dana"));
    let v = anon.api("action=query&list=logevents&letype=reclaim").await;
    let e = &v["query"]["logevents"][0];
    assert_eq!(e["type"], "reclaim", "{v}");
    assert_eq!(e["action"], "reclaim");
    assert_eq!(e["params"]["method"], "vouched");
    // Reclaimed once: Dana's account is held now.
    let mut again = Browser::new(&router, [192, 0, 2, 44]);
    let r = again.sign_in("login", "code-dana").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Main_Page"),
        "a bound login now"
    );

    // The instance default does not serve a tenant on another host.
    assert!(
        oauth::resolve_row(&**db, "other", "https://example.wiki", ISSUER)
            .await
            .unwrap()
            .is_none()
    );
}
