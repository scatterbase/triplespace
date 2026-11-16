//! Tenant requests from the site (ADR 0111 §5, §7, §8): `Special:Tenants` at the farm base
//! lists the wikis and, for a holder of `ts-createtenant`, files a request and shows the
//! requests; `POST /tenancy/requests` files one, refused to a reader; the provisioner
//! carries it out; `POST /tenancy/requests/{id}/invitation` issues the owner's invitation,
//! which `Special:CreateAccount?invite=` on the new wiki redeems with a password, after which
//! the owner logs in there. Then the other acts (0111 §10): the row buttons freeze a wiki,
//! whose writes answer `readonly`, and the API unfreezes it; the primary role is offered,
//! withdrawn and offered again (0046 §5), and accepted on the offered wiki's own
//! `Special:Tenants`. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

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
use triplespace_cli::provision::{Outcome, Provisioner};
use triplespace_cli::{common, instance, tenant};
use triplespace_client::{Client as SiteClient, ServiceTransport};

const DB: &str = "tsrequestsite_test";
const ALPHA: &str = "alpha.example";
const BETA: &str = "beta.example";
const AUTH: &str = "auth.example";
const REST: &str = "/w/rest.php/triplespace/v0";
const PASSWORD: &str = "owner-secret-1234";

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

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A client on one host, with one session cookie.
struct Client {
    router: Router,
    host: &'static str,
    cookie: Option<String>,
}

struct Reply {
    status: StatusCode,
    body: Value,
    text: String,
    location: Option<String>,
}

impl Client {
    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Option<String>,
    ) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, self.host)
            .header(header::ORIGIN, format!("https://{}", self.host));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
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
        let location = headers
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            body,
            text,
            location,
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

    async fn login(&mut self, name: &str, password: &str) -> Value {
        let t = self.api("action=query&meta=tokens&type=login").await;
        let token = t["query"]["tokens"]["logintoken"]
            .as_str()
            .unwrap()
            .to_string();
        let r = self
            .send(
                "POST",
                "/w/api.php",
                Some("application/x-www-form-urlencoded"),
                Some(format!(
                    "action=clientlogin&username={}&password={}&logintoken={}&loginreturnurl=https://{}/&format=json&formatversion=2",
                    enc(name),
                    enc(password),
                    enc(&token),
                    self.host
                )),
            )
            .await;
        r.body
    }

    async fn csrf(&mut self) -> String {
        self.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn rest(&mut self, method: &str, path: &str, body: Option<Value>) -> Reply {
        let body = body.map(|b| b.to_string());
        let ct = body.as_ref().map(|_| "application/json");
        self.send(method, &format!("{REST}{path}"), ct, body).await
    }

    async fn page(&mut self, path: &str) -> Reply {
        self.send("GET", path, None, None).await
    }

    async fn page_post(&mut self, path: &str, pairs: &[(&str, &str)]) -> Reply {
        let form = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
            .collect::<Vec<_>>()
            .join("&");
        self.send(
            "POST",
            path,
            Some("application/x-www-form-urlencoded"),
            Some(form),
        )
        .await
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn tenant_requests_from_the_site() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsrequestsite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key_file = dir.join("triplespace.key");
    let _ = std::fs::remove_file(&key_file);
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, format!("{PASSWORD}\n")).unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        farm_slug: "acme".into(),
        farm_base: None,
        tenancy: "isolated".into(),
        tenant: "alpha".into(),
        base: format!("https://{ALPHA}"),
        name: Some("Alpha".into()),
        providers: vec![],
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    let db = common::connect(&database).await.unwrap();
    let key = tenant::current_key(&db, &key_file).await.unwrap();

    let app = App::new(
        triplespace_db::scratch::pools(&database, 4),
        Secret::derive(&key.to_bytes()),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: common::farm_at(&database).await.unwrap(),
            auth_base: Some(format!("https://{AUTH}")),
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let api = router_with(app, triplespace_api_rest::routes());
    let site = api
        .clone()
        .fallback_service(triplespace_ui::router(SiteClient::new(
            ServiceTransport::new(api.clone()),
        )));
    let mut farm = Client {
        router: site.clone(),
        host: ALPHA,
        cookie: None,
    };

    // 0111 §8: a reader may see the list, but files nothing and reads no requests.
    let r = farm.page("/wiki/Special:Tenants").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.text.contains("Alpha") && r.text.contains("alpha.example"),
        "{}",
        r.text
    );
    assert!(
        !r.text.contains("ts-tenants__create"),
        "no form for a reader"
    );
    let r = farm
        .rest(
            "POST",
            "/tenancy/requests",
            Some(json!({"slug": "beta", "base": format!("https://{BETA}"), "owner_name": "Bob"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    let r = farm.rest("GET", "/tenancy/requests", None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);

    // The owner, who holds `ts-createtenant` through `owner` (0111 §8, A1).
    let l = farm.login("Owner", PASSWORD).await;
    assert_eq!(l["clientlogin"]["status"], "PASS", "{l}");
    let r = farm.page("/wiki/Special:Tenants").await;
    assert!(r.text.contains("ts-tenants__create"), "{}", r.text);

    // 0111 §7: the API files a request, pending, with no owner login.
    let token = farm.csrf().await;
    let r = farm
        .rest(
            "POST",
            "/tenancy/requests",
            Some(json!({"slug": "beta", "base": format!("https://{BETA}"),
                        "name": "Beta", "owner_name": "Bob", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    assert_eq!(r.body["request"]["state"], "pending");
    let id = r.body["request"]["id"].as_u64().unwrap();

    // A request filed through the page's form, which the API refuses for its shape.
    let token = farm.csrf().await;
    let r = farm
        .page_post(
            "/w/index.php?title=Special:Tenants",
            &[
                ("token", &token),
                ("slug", ""),
                ("base", ""),
                ("owner_name", ""),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains("cdx-message--error"), "{}", r.text);

    // The provisioner carries the request out.
    db.batch_execute("GRANT ts_provision, ts_accounts TO CURRENT_USER")
        .await
        .unwrap();
    let provisioner = Provisioner {
        database: database.clone(),
        accounts_database: database.clone(),
        key,
        auth_base: Some(format!("https://{AUTH}")),
    };
    let done = provisioner.pass().await.unwrap();
    assert!(
        matches!(&done[..], [(slug, Outcome::Created(_))] if slug == "beta"),
        "{done:?}"
    );
    let r = farm
        .rest("GET", &format!("/tenancy/requests/{id}"), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["request"]["state"], "created");
    let r = farm.page("/wiki/Special:Tenants").await;
    assert!(r.text.contains("Invite the owner"), "{}", r.text);

    // 0111 §5: the owner's invitation, issued through the page, its address shown once.
    let token = farm.csrf().await;
    let r = farm
        .page_post(
            "/w/index.php?title=Special:Tenants",
            &[("token", &token), ("invite", &id.to_string())],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let at = r
        .text
        .find("https://beta.example/wiki/Special:CreateAccount?invite=")
        .unwrap_or_else(|| panic!("{}", r.text));
    let url = &r.text[at..];
    let code: String = url["https://beta.example/wiki/Special:CreateAccount?invite=".len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    assert_eq!(code.len(), 43, "32 bytes in URL-safe base64");

    // On the new wiki: the invitation page, then redeeming it, then the owner logs in.
    let mut beta = Client {
        router: site.clone(),
        host: BETA,
        cookie: None,
    };
    let r = beta
        .page(&format!("/wiki/Special:CreateAccount?invite={code}"))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.text.contains("Beta") && r.text.contains("Bob"),
        "{}",
        r.text
    );
    let r = beta
        .page_post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("invite", &code),
                ("password", "short"),
                ("retype", "short"),
            ],
        )
        .await;
    assert!(r.text.contains("at least 8"), "{}", r.text);
    let r = beta
        .page_post(
            "/w/index.php?title=Special:CreateAccount",
            &[
                ("invite", &code),
                ("password", "bob-secret-5678"),
                ("retype", "bob-secret-5678"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.text);
    assert_eq!(r.location.as_deref(), Some("/"));
    let who = beta.api("action=query&meta=userinfo").await;
    assert_eq!(who["query"]["userinfo"]["name"], "Bob", "{who}");

    // Spent: the code no longer opens anything.
    let mut stranger = Client {
        router: site,
        host: BETA,
        cookie: None,
    };
    let r = stranger
        .rest("GET", &format!("/auth/invitation?code={code}"), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    let l = stranger.login("Bob", "bob-secret-5678").await;
    assert_eq!(l["clientlogin"]["status"], "PASS", "{l}");

    // A second invitation is refused now that the owner has a login.
    let token = farm.csrf().await;
    let r = farm
        .rest(
            "POST",
            &format!("/tenancy/requests/{id}/invitation"),
            Some(json!({"token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.body["code"], "ts-invitation-bound");

    // 0111 §10: the row's Freeze button files a freeze, which the provisioner carries out.
    let r = farm.page("/wiki/Special:Tenants").await;
    assert!(
        r.text.contains("name=\"act\" value=\"freeze\""),
        "{}",
        r.text
    );
    let token = farm.csrf().await;
    let r = farm
        .page_post(
            "/w/index.php?title=Special:Tenants",
            &[("token", &token), ("act", "freeze"), ("slug", "beta")],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains("cdx-message--success"), "{}", r.text);
    let done = provisioner.pass().await.unwrap();
    assert!(
        matches!(&done[..], [(slug, Outcome::Carried(act))] if slug == "beta" && act == "freeze"),
        "{done:?}"
    );
    let r = farm.page("/wiki/Special:Tenants").await;
    assert!(r.text.contains("Frozen: read-only"), "{}", r.text);
    assert!(r.text.contains("value=\"unfreeze\""), "{}", r.text);

    // 08 §6.3: the frozen wiki reports it, and a write answers `readonly`.
    let info = beta.api("action=query&meta=siteinfo").await;
    assert_eq!(info["query"]["general"]["readonly"], true, "{info}");
    let token = beta.csrf().await;
    let r = beta
        .send(
            "POST",
            "/w/api.php",
            Some("application/x-www-form-urlencoded"),
            Some(format!(
                "action=edit&title=Sandbox&text=hello&token={}&format=json&formatversion=2",
                enc(&token)
            )),
        )
        .await;
    assert_eq!(r.body["error"]["code"], "readonly", "{}", r.body);

    // The API unfreezes it.
    let token = farm.csrf().await;
    let r = farm
        .rest(
            "POST",
            "/tenancy/requests",
            Some(json!({"act": "unfreeze", "slug": "beta", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    assert_eq!(r.body["request"]["act"], "unfreeze");
    let id = r.body["request"]["id"].as_u64().unwrap();
    provisioner.pass().await.unwrap();
    let r = farm
        .rest("GET", &format!("/tenancy/requests/{id}"), None)
        .await;
    assert_eq!(r.body["request"]["state"], "done", "{}", r.body);
    let info = beta.api("action=query&meta=siteinfo").await;
    assert_eq!(info["query"]["general"]["readonly"], false, "{info}");

    // 0046 §5: an offer through the page, withdrawn through the API.
    let token = farm.csrf().await;
    let r = farm
        .page_post(
            "/w/index.php?title=Special:Tenants",
            &[("token", &token), ("offer_to", "beta")],
        )
        .await;
    assert!(r.text.contains("Offered to beta"), "{}", r.text);
    let r = farm.rest("GET", "/tenancy", None).await;
    let offer = r.body["offers"][0]["id"].as_u64().unwrap();
    assert_eq!(r.body["offers"][0]["tenant"], "beta", "{}", r.body);
    let token = farm.csrf().await;
    let r = farm
        .rest(
            "POST",
            "/tenancy/primary/withdraw",
            Some(json!({"offer": offer, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = beta.rest("GET", "/tenancy/acceptance", None).await;
    assert_eq!(r.body["offers"], json!([]), "{}", r.body);
    // Only a holder of `ts-primary` offers; Bob is an owner of beta, not of the primary.
    let token = beta.csrf().await;
    let r = beta
        .rest(
            "POST",
            "/tenancy/primary/offer",
            Some(json!({"tenant": "beta", "token": token})),
        )
        .await;
    assert_ne!(r.status, StatusCode::OK, "{}", r.body);

    // Offered again, and accepted on beta's own Special:Tenants by its owner.
    let token = farm.csrf().await;
    let r = farm
        .rest(
            "POST",
            "/tenancy/primary/offer",
            Some(json!({"tenant": "beta", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let offer = r.body["offer"]["id"].as_u64().unwrap();
    let r = beta.page("/wiki/Special:Tenants").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains("Accept the primary role"), "{}", r.text);
    let token = beta.csrf().await;
    let r = beta
        .page_post(
            "/w/index.php?title=Special:Tenants",
            &[("token", &token), ("accept", &offer.to_string())],
        )
        .await;
    assert!(r.text.contains("cdx-message--success"), "{}", r.text);
    let done = provisioner.pass().await.unwrap();
    assert!(
        matches!(&done[..], [(_, Outcome::Carried(act))] if act == "accept-primary"),
        "{done:?}"
    );
    let r = farm.rest("GET", "/tenancy", None).await;
    assert_eq!(r.body["primary"], "beta", "{}", r.body);
    assert_eq!(r.body["offers"], json!([]), "{}", r.body);
}
