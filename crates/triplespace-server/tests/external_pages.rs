//! Logging in through an external issuer on the site (ADR 0088 §5–§7), over the real API
//! with a fake provider: the buttons on `Special:UserLogin`, the round trip through the
//! API's start route and callback, `Special:CreateAccount` for a pending identity with
//! the provider's username offered, and `Special:Account`'s Sign-in methods: the last
//! one's Remove disabled, adding one and removing it. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_actors::issuer::OAuthEndpoints;
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_accounts::oauth::{
    self, BoxFuture, Client as OAuthClient, Profile, SourceUser, Upstream, UpstreamError,
};
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;
use triplespace_client::{Client, ServiceTransport};
use triplespace_projections::Farm;

const DB: &str = "tsexternalpages_test";
const HOST: &str = "librarybase.org";
const START: &str = "/w/rest.php/triplespace/v0/auth/wikimedia-central/start";

struct Fake;

impl Upstream for Fake {
    fn identify<'a>(
        &'a self,
        _endpoints: &'a OAuthEndpoints,
        _client: &'a OAuthClient,
        code: &'a str,
        _verifier: &'a str,
    ) -> BoxFuture<'a, Result<Profile, UpstreamError>> {
        Box::pin(async move {
            let (sub, username) = match code {
                "code-bob" => ("5001", "Bob"),
                "code-alice" => ("6001", "AliceWM"),
                _ => return Err(UpstreamError("invalid_grant".into())),
            };
            Ok(Profile {
                sub: sub.into(),
                username: username.into(),
            })
        })
    }

    fn source_user<'a>(
        &'a self,
        _users_api: &'a str,
        _username: &'a str,
    ) -> BoxFuture<'a, Result<Option<SourceUser>, UpstreamError>> {
        Box::pin(async { Ok(None) })
    }
}

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

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
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

    /// The value of the first input of a name.
    fn field(&self, name: &str) -> String {
        let marker = format!("name=\"{name}\" value=\"");
        let at = self
            .body
            .find(&marker)
            .unwrap_or_else(|| panic!("no {name} field in {}", self.body));
        let rest = &self.body[at + marker.len()..];
        rest[..rest.find('"').unwrap()]
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }
}

/// A browser: the site's router, one cookie, one address.
struct Browser {
    router: Router,
    cookie: Option<String>,
    address: [u8; 4],
}

impl Browser {
    async fn send(
        &mut self,
        method: &str,
        path: &str,
        body: Option<String>,
        json_body: bool,
    ) -> Got {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if body.is_some() {
            req = req.header(
                header::CONTENT_TYPE,
                if json_body {
                    "application/json"
                } else {
                    "application/x-www-form-urlencoded"
                },
            );
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((self.address, 50000))));
        let r = self.router.clone().oneshot(req).await.unwrap();
        let status = r.status();
        let headers = r.headers().clone();
        for sc in headers.get_all(header::SET_COOKIE) {
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
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&mut self, path: &str) -> Got {
        self.send("GET", path, None, false).await
    }

    async fn post(&mut self, path: &str, pairs: &[(&str, &str)]) -> Got {
        self.send("POST", path, Some(form(pairs)), false).await
    }

    async fn api(&mut self, query: &str) -> Value {
        let r = self
            .get(&format!("/w/api.php?{query}&format=json&formatversion=2"))
            .await;
        serde_json::from_str(&r.body).unwrap()
    }
}

/// The form fields of the issuer button on a page.
fn button_fields(page: &Got) -> Vec<(String, String)> {
    let at = page
        .body
        .find(&format!("action=\"{START}\""))
        .unwrap_or_else(|| panic!("no issuer button in {}", page.body));
    let form = &page.body[at..at + page.body[at..].find("</form>").unwrap()];
    let mut out = Vec::new();
    for part in form.split("<input type=\"hidden\" name=\"").skip(1) {
        let (name, rest) = part.split_once("\" value=\"").unwrap();
        let value = &rest[..rest.find('"').unwrap()];
        out.push((
            name.to_string(),
            value.replace("&amp;", "&").replace("&#39;", "'"),
        ));
    }
    out
}

/// Presses a page's issuer button and follows the provider back with a code; the
/// callback's answer.
async fn round_trip(b: &mut Browser, page: &Got, code: &str) -> Got {
    let fields = button_fields(page);
    let pairs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let r = b.post(START, &pairs).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let to = url::Url::parse(r.location()).unwrap();
    assert_eq!(to.host_str(), Some("meta.wikimedia.org"));
    let q: BTreeMap<String, String> = to.query_pairs().into_owned().collect();
    b.get(&format!(
        "/w/rest.php/triplespace/v0/auth/wikimedia-central/callback?state={}&code={code}",
        enc(&q["state"])
    ))
    .await
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn external_login_through_the_site() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsexternalpages-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: "librarybase".into(),
        base: format!("https://{HOST}"),
        name: Some("Librarybase".into()),
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
    let secret = Secret::derive(&[7u8; 32]);
    let pool = triplespace_db::pool::pool(&database, 4).unwrap();
    let app = App::with_upstream(
        pool.clone(),
        secret.clone(),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: Farm {
                slug: "librarybase".into(),
                base: format!("https://{HOST}"),
            },
            generator: "Triplespace test".into(),
            version: None,
        },
        Arc::new(Fake),
    )
    .unwrap();
    let db = pool.get().await.unwrap();
    oauth::set_client(
        &**db,
        &secret,
        scatter_actors::issuer::IssuerRegistry::default_registry(),
        "",
        "wikimedia-central",
        "client-123",
        "s3cret",
        &oauth::callback_url(&format!("https://{HOST}"), "wikimedia-central"),
    )
    .await
    .unwrap();
    let api = router_with(app, triplespace_api_rest::routes());
    let site = api
        .clone()
        .fallback_service(triplespace_ui::router(Client::new(ServiceTransport::new(
            api.clone(),
        ))));

    // The login page offers the issuer, with what the wiki learns.
    let mut bob = Browser {
        router: site.clone(),
        cookie: None,
        address: [203, 0, 113, 5],
    };
    let login = bob.get("/wiki/Special:UserLogin").await;
    assert_eq!(login.status, StatusCode::OK);
    assert!(
        login.body.contains("Log in with Wikimedia"),
        "{}",
        login.body
    );
    assert!(
        login
            .body
            .contains("This wiki learns which account you hold there")
    );
    // The start route redirects to the provider, which `form-action` must allow.
    let csp = login.headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    assert!(
        csp.contains("form-action 'self' https://meta.wikimedia.org"),
        "{csp}"
    );
    // The provider sends Bob back: no account yet, so on to CreateAccount.
    let r = round_trip(&mut bob, &login, "code-bob").await;
    assert!(
        r.location()
            .starts_with("/wiki/Special:CreateAccount?returnto="),
        "{}",
        r.location()
    );
    let page = bob.get(r.location()).await;
    assert!(
        page.body.contains("You signed in with Wikimedia as Bob"),
        "{}",
        page.body
    );
    assert!(page.body.contains("Use your Wikimedia name, Bob"));
    assert!(
        page.body
            .contains("Using the same name makes your two accounts easy to connect.")
    );
    assert!(
        !page.body.contains("name=\"password\""),
        "no password asked"
    );
    assert!(!page.body.contains("tschallengeid"), "no challenge");
    let created = bob
        .post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("createtoken", &page.field("createtoken")),
                ("returnto", "Main Page"),
                ("tspending", "1"),
                ("username", ""),
                ("tssuggested", "Bob"),
            ],
        )
        .await;
    assert_eq!(created.status, StatusCode::SEE_OTHER, "{}", created.body);
    assert_eq!(created.location(), "/wiki/Main_Page");
    let me = bob.api("action=query&meta=userinfo").await;
    assert_eq!(me["query"]["userinfo"]["name"], "Bob");

    // Bob's account page: one way to log in, which cannot be removed.
    let account = bob.get("/wiki/Special:Account").await;
    assert_eq!(account.status, StatusCode::OK);
    assert!(account.body.contains("Sign-in methods"), "{}", account.body);
    assert!(account.body.contains("Wikimedia"));
    assert!(
        account
            .body
            .contains(" disabled aria-describedby=\"ts-signin-last\"")
    );
    assert!(
        account
            .body
            .contains("Your only way to log in cannot be removed.")
    );

    // Alice, with a password, adds Wikimedia and removes it again.
    let mut alice = Browser {
        router: site.clone(),
        cookie: None,
        address: [198, 51, 100, 1],
    };
    let login = alice.get("/wiki/Special:UserLogin").await;
    let r = alice
        .post(
            "/w/index.php?title=Special:UserLogin",
            &[
                ("logintoken", &login.field("logintoken")),
                ("returnto", "Main Page"),
                ("username", "Alice"),
                ("password", "owner-secret-1234"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let account = alice.get("/wiki/Special:Account").await;
    assert!(
        account.body.contains("Password on this wiki"),
        "{}",
        account.body
    );
    assert!(account.body.contains("Add Wikimedia"));
    let r = round_trip(&mut alice, &account, "code-alice").await;
    assert_eq!(
        r.location(),
        "/wiki/Special:Account?tsnotice=ts-linked#sign-in-methods"
    );
    let account = alice.get("/wiki/Special:Account?tsnotice=ts-linked").await;
    assert!(
        account.body.contains("Sign-in method added."),
        "{}",
        account.body
    );
    assert!(!account.body.contains("Add Wikimedia"), "added already");
    let removed = alice
        .post(
            "/w/index.php?title=Special:Account",
            &[
                ("issuer", "wikimedia-central"),
                ("token", &account.field("token")),
            ],
        )
        .await;
    assert_eq!(removed.status, StatusCode::SEE_OTHER, "{}", removed.body);
    let account = alice.get(removed.location()).await;
    assert!(
        account.body.contains("Sign-in method removed."),
        "{}",
        account.body
    );
    assert!(account.body.contains("Add Wikimedia"));

    // Answers the API sends back are shown; unknown ones are not.
    let mut anon = Browser {
        router: site.clone(),
        cookie: None,
        address: [203, 0, 113, 6],
    };
    let page = anon
        .get("/wiki/Special:UserLogin?tserror=ts-oauth-denied")
        .await;
    assert!(
        page.body
            .contains("You did not allow this wiki to learn which account you hold there")
    );
    let page = anon.get("/wiki/Special:Account").await;
    assert!(page.body.contains("Log in to see your account."));
}
