//! Strict names end to end (ADR 0102): the reserved prefixes, enacting `login.names =
//! provider` and what it waits on, the derived name on the first visit, one identity
//! per login, and the rename that follows the provider's. The harness is that of
//! `external_identity.rs`. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

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

const DB: &str = "tsstrict_test";
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
async fn strict_names() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsstrict-{}", std::process::id()));
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
            ("code-bob", ("5001", "Bob")),
            ("code-bob-2", ("5001", "Bob")),
            ("code-bob-renamed", ("5001", "Bobby")),
            ("code-carol", ("7001", "Carol")),
            ("code-carol-2", ("7001", "Carol")),
            ("code-eve", ("9001", "Eve")),
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
    let name_of = |key: String| {
        let db = pool.server.clone();
        async move {
            let c = db.get().await.unwrap();
            c.query_one(
                "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2",
                &[&TENANT, &key],
            )
            .await
            .unwrap()
            .get::<_, String>(0)
        }
    };

    let mut anon = Browser::new(&router, [203, 0, 113, 9]);
    let si = anon
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(si["query"]["triplespace"]["account"]["names"], "chosen");

    // The prefix is reserved with strict names off (0102 §2): for a chosen name with a
    // password, for one on a pending identity, and for a subsidiary.
    let token = anon.token("createaccount").await;
    let r = anon
        .post(&format!(
            "action=createaccount&username=Wikimedia-Zed&password=zed-secret-1&retype=zed-secret-1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "ts-name-reserved", "{r}");
    let mut eve = Browser::new(&router, [203, 0, 113, 20]);
    let r = eve.sign_in("login", "code-eve").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let p = eve
        .send("GET", &format!("{REST}/auth/pending"), None, None)
        .await;
    assert_eq!(p.body["username"], "Eve");
    assert!(
        p.body.get("name").is_none(),
        "no derived name yet: {}",
        p.body
    );
    let token = eve.token("createaccount").await;
    let r = eve
        .post(&format!(
            "action=createaccount&username=Wikimedia-Eve&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "ts-name-reserved", "{r}");
    let r = eve
        .post(&format!(
            "action=createaccount&username=Eve&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    assert_eq!(eve.whoami().await.as_deref(), Some("Eve"));
    {
        let store = app.store();
        let mut cx = scatter_projection::Backend::begin(store).await.unwrap();
        let alice_key = format!(
            "{}:7",
            triplespace_cli::common::issuer_at(&database, TENANT)
                .await
                .unwrap()
        );
        let made = triplespace_cli::accounts::create_subsidiary(
            store,
            app.pipeline(),
            &mut cx,
            TENANT,
            "Wikimedia-bot",
            &alice_key,
            &[],
            &alice_key,
            1,
        )
        .await;
        scatter_projection::Backend::rollback(store, cx)
            .await
            .unwrap();
        let err = made.expect_err("reserved for the issuer").to_string();
        assert!(err.contains("reserved"), "{err}");
    }

    // Enacting strict names (0102 §5) waits on the requirement.
    let mut alice = Browser::new(&router, [198, 51, 100, 1]);
    let lt = alice.token("login").await;
    let r = alice
        .post(&format!(
            "action=clientlogin&username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(&lt)
        ))
        .await;
    assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    let r = alice.set_instance(json!({"login.names": "provider"})).await;
    assert_ne!(r.status, StatusCode::OK, "{}", r.body);
    let body = r.body.to_string();
    assert!(body.contains("ts-strict-names-unmet"), "{body}");
    assert!(body.contains("`login.external_required` is off"), "{body}");
    let r = alice.sign_in("link", "code-alice").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:Account?tsnotice=ts-linked#sign-in-methods")
    );
    let r = alice
        .set_instance(json!({"login.external_required": true}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // Then on the names: Alice and Eve are not named after their provider usernames.
    let r = alice.set_instance(json!({"login.names": "provider"})).await;
    assert_ne!(r.status, StatusCode::OK, "{}", r.body);
    let body = r.body.to_string();
    assert!(
        body.contains("2 accounts are not named after their provider usernames"),
        "{body}"
    );
    let ready = triplespace_api_ingest::accounts::strict_readiness(&**db, registry)
        .await
        .unwrap();
    assert!(ready.external_required);
    assert_eq!(ready.several_identities, Vec::<String>::new());
    assert_eq!(ready.unknown, vec![]);
    assert_eq!(
        ready.differing,
        vec![
            (
                TENANT.to_string(),
                "Alice".to_string(),
                "Wikimedia-AliceWM".to_string()
            ),
            (
                TENANT.to_string(),
                "Eve".to_string(),
                "Wikimedia-Eve".to_string()
            ),
        ]
    );
    // A login with a second identity would block too.
    let two = triplespace_api_ingest::accounts::StrictReadiness {
        external_required: true,
        several_identities: vec!["Alice".into()],
        ..Default::default()
    };
    assert_eq!(
        two.blockers(),
        vec!["1 login holds more than one external identity".to_string()]
    );
    // The bureaucrat renames them (0102 §5), with the primitive `renameuser` will use.
    let issuer = triplespace_cli::common::issuer_at(&database, TENANT)
        .await
        .unwrap();
    let alice_key = format!("{issuer}:7");
    let eve_key: String = db
        .query_one(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = 'Eve'",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0);
    for (key, to) in [
        (alice_key.as_str(), "Wikimedia-AliceWM"),
        (eve_key.as_str(), "Wikimedia-Eve"),
    ] {
        // A rename is the accounts service's: it writes name tombstones (0013 §4, A99).
        let store = app.accounts_store();
        let mut cx = scatter_projection::Backend::begin(store).await.unwrap();
        triplespace_api_ingest::accounts::rename(
            store,
            app.pipeline(),
            &mut cx,
            &secret,
            TENANT,
            key,
            to,
            &alice_key,
            "renameuser",
            triplespace_api_action::edit::now_micros(),
        )
        .await
        .unwrap();
        scatter_projection::Backend::commit(store, cx)
            .await
            .unwrap();
    }
    assert_eq!(alice.whoami().await.as_deref(), Some("Wikimedia-AliceWM"));
    let r = alice.set_instance(json!({"login.names": "provider"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let si = anon
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert_eq!(
        si["query"]["triplespace"]["account"]["names"], "provider",
        "reported (0102 §6)"
    );
    assert!(
        triplespace_api_ingest::accounts::strict_readiness(&**db, registry)
            .await
            .unwrap()
            .blockers()
            .is_empty()
    );
    // The requirement cannot go while strict names hold.
    let r = alice
        .set_instance(json!({"login.external_required": false}))
        .await;
    assert_ne!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body.to_string().contains("ts-strict-names-unmet"));

    // The first visit (0102 §1): the derived name is shown, taken or empty; nothing else.
    let mut bob = Browser::new(&router, [203, 0, 113, 30]);
    let r = bob.sign_in("login", "code-bob").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let p = bob
        .send("GET", &format!("{REST}/auth/pending"), None, None)
        .await;
    assert_eq!(p.body["username"], "Bob");
    assert_eq!(p.body["name"], "Wikimedia-Bob", "{}", p.body);
    let token = bob.token("createaccount").await;
    let r = bob
        .post(&format!(
            "action=createaccount&username=Bob&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["messagecode"], "ts-strict-name", "{r}");
    let r = bob
        .post(&format!(
            "action=createaccount&createpreservestate=1&createtoken={}&createreturnurl=https://{HOST}/",
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    assert_eq!(r["createaccount"]["username"], "Wikimedia-Bob", "{r}");
    assert_eq!(bob.whoami().await.as_deref(), Some("Wikimedia-Bob"));
    let bob_key: String = db
        .query_one(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = 'Wikimedia-Bob'",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0);
    let username: Option<String> = db
        .query_one(
            "SELECT username FROM private.binding WHERE tenant = '' AND subject = '5001'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        username.as_deref(),
        Some("Bob"),
        "the username is kept on the identity"
    );

    // One identity per login (0102 §4): the Action API's door and the site's.
    let csrf = bob.token("csrf").await;
    let r = bob
        .post(&format!(
            "action=linkaccount&tsissuer-{ISSUER}=1&linktoken={}&linkreturnurl=https://{HOST}/",
            enc(&csrf)
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-strict-identity", "{r}");
    let r = bob.sign_in("link", "code-carol").await;
    assert!(
        r.location
            .as_deref()
            .unwrap()
            .contains("tserror=ts-strict-identity"),
        "{:?}",
        r.location
    );

    // Upstream is canonical (0102 §3): renamed there, renamed here at the next login.
    let csrf = bob.token("csrf").await;
    bob.post(&format!("action=logout&token={}", enc(&csrf)))
        .await;
    let r = bob.sign_in("login", "code-bob-renamed").await;
    assert_eq!(r.location.as_deref(), Some("/wiki/Main_Page"), "{}", r.body);
    assert_eq!(bob.whoami().await.as_deref(), Some("Wikimedia-Bobby"));
    assert_eq!(name_of(bob_key.clone()).await, "Wikimedia-Bobby");
    // The rename is an instance act whose comment says the provider renamed it.
    let (partition, offset): (i64, i64) = db
        .query_one(
            "SELECT \"partition\", \"offset\" FROM log.record WHERE key = $1 ORDER BY \"offset\" DESC LIMIT 1",
            &[&bob_key],
        )
        .await
        .map(|r| (r.get(0), r.get(1)))
        .unwrap();
    #[allow(clippy::cast_sign_loss)]
    let slot = scatter_log_postgres::log::read(&**db, partition as u64, offset as u64)
        .await
        .unwrap();
    let scatter_log::store::Slot::Record(record) = slot else {
        panic!("a record");
    };
    let comment = record.body().comment().value().unwrap().unwrap();
    assert_eq!(
        comment,
        scatter_log::cbor::Value::text("renameuser: renamed by the provider")
    );
    let attestation = record.body().attestation().value().unwrap().unwrap();
    assert_eq!(
        attestation,
        scatter_log::cbor::Value::map(vec![(
            scatter_log::cbor::Value::text("actor"),
            scatter_log::cbor::Value::text(&farm.operator().to_string())
        )]),
        "an instance act"
    );
    let username: Option<String> = db
        .query_one(
            "SELECT username FROM private.binding WHERE tenant = '' AND subject = '5001'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(username.as_deref(), Some("Bobby"));

    // Off again: nothing is renamed, and names are chosen once more (0102 §5).
    let r = alice.set_instance(json!({"login.names": "chosen"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(name_of(bob_key).await, "Wikimedia-Bobby");
    let mut carol = Browser::new(&router, [203, 0, 113, 40]);
    let r = carol.sign_in("login", "code-carol-2").await;
    assert_eq!(
        r.location.as_deref(),
        Some("/wiki/Special:CreateAccount?returnto=Main_Page")
    );
    let p = carol
        .send("GET", &format!("{REST}/auth/pending"), None, None)
        .await;
    assert!(p.body.get("name").is_none(), "{}", p.body);
}
