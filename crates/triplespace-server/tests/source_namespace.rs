//! An entity source's namespace (05 §6.5, §6.6; 0078 §5, A10): declaring a source gives it
//! a resolver namespace from 3000, named by the tenant and always by the source's name;
//! `MHC:Q9` resolves through `GET /resolve` and the site, queues the fetch, offers *Create
//! an item for this*, and once the item is minted with `ts-sameas`, redirects to it. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode as ServerMode, router_with};
use triplespace_cli::{instance, source};

const DB: &str = "tssourcens_test";
const TENANT: &str = "wiki";
const HOST: &str = "wiki.example";
const AUTH: &str = "auth.example";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database(name: &str) -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    triplespace_db::scratch::drop_at_thread_end(name);
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap()
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

async fn setup(name: &str) -> Option<(String, PathBuf)> {
    admin_config()?;
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    let key_file = dir.join("triplespace.key");
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    Some((database, key_file))
}

async fn site(database: &str, key_file: &std::path::Path) -> Router {
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(key_file).unwrap();
    let app = App::new(
        triplespace_db::scratch::pools(database, 4),
        Secret::derive(&signing.to_bytes()),
        ServerConfig {
            mode: ServerMode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm,
            auth_base: Some(format!("https://{AUTH}")),
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let api = router_with(app, triplespace_api_rest::routes());
    api.clone()
        .fallback_service(triplespace_ui::router(triplespace_client::Client::new(
            triplespace_client::ServiceTransport::new(api),
        )))
}

struct Browser {
    router: Router,
    cookie: Option<String>,
}

struct Got {
    status: StatusCode,
    body: String,
}

impl Browser {
    async fn send(&mut self, method: &str, path: &str, body: Option<(String, Vec<u8>)>) -> Got {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let req = match body {
            Some((ct, b)) => req
                .header(header::CONTENT_TYPE, ct)
                .body(Body::from(b))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let r = self.router.clone().oneshot(req).await.unwrap();
        let status = r.status();
        for sc in r.headers().get_all(header::SET_COOKIE) {
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
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&mut self, path: &str) -> Got {
        self.send("GET", path, None).await
    }

    async fn api(&mut self, query: &str) -> Value {
        let g = self
            .get(&format!("/w/api.php?{query}&format=json&formatversion=2"))
            .await;
        assert_eq!(g.status, StatusCode::OK, "{}", g.body);
        serde_json::from_str(&g.body).unwrap_or(Value::Null)
    }

    async fn post_api(&mut self, query: &str, form: &str) -> Value {
        let g = self
            .send(
                "POST",
                &format!("/w/api.php?{query}&format=json&formatversion=2"),
                Some((
                    "application/x-www-form-urlencoded".into(),
                    form.as_bytes().to_vec(),
                )),
            )
            .await;
        serde_json::from_str(&g.body).unwrap_or(Value::Null)
    }

    async fn login(&mut self, name: &str, password: &str) {
        let token =
            self.api("action=query&meta=tokens&type=login").await["query"]["tokens"]["logintoken"]
                .as_str()
                .unwrap()
                .to_string();
        let r = self
            .post_api(
                "action=clientlogin",
                &format!(
                    "username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
                    enc(name),
                    enc(password),
                    enc(&token)
                ),
            )
            .await;
        assert_eq!(r["clientlogin"]["status"], "PASS", "{r}");
    }
}

/// What the stub wiki was asked: each request's line.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A wiki whose `wbgetentities` knows `Q9`, labelled "Nine" in English, and reports any
/// other ID missing.
async fn wiki() -> (String, Asked) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let asked = Asked::default();
    let noted = asked.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let noted = noted.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    request.extend_from_slice(&chunk[..n]);
                    if n == 0 || request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let line = text.lines().next().unwrap_or_default().to_string();
                noted.lock().unwrap().push(line.clone());
                let ids = line
                    .split(['?', '&', ' '])
                    .find_map(|p| p.strip_prefix("ids="))
                    .unwrap_or_default()
                    .replace("%7C", "|");
                let mut entities = serde_json::Map::new();
                for id in ids.split('|').filter(|i| !i.is_empty()) {
                    let e = if id == "Q9" {
                        json!({"type": "item", "id": "Q9",
                            "labels": {"en": {"language": "en", "value": "Nine"}}})
                    } else {
                        json!({"id": id, "missing": ""})
                    };
                    entities.insert(id.to_string(), e);
                }
                let answer = json!({"success": 1, "entities": entities}).to_string();
                let _ = socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                             content-length: {}\r\nconnection: close\r\n\r\n{answer}",
                            answer.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    (format!("http://{address}/w/api.php"), asked)
}

async fn declare(
    database: &str,
    name: &str,
    api: Option<&str>,
    namespace: Option<&str>,
    aliases: &[&str],
) -> anyhow::Result<()> {
    source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: source::At {
                database: database.into(),
                tenant: TENANT.into(),
            },
            name: name.into(),
            reason: "the test".into(),
        },
        label: if name == "mhc" {
            "Miraheze Communities".into()
        } else {
            name.to_uppercase()
        },
        types: vec!["Q=item".into(), "P=property".into()],
        iri: format!("https://{name}.example/entity/{{upstream_id}}"),
        api: api.map(str::to_string),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
        namespace: namespace.map(str::to_string),
        namespace_aliases: aliases.iter().map(|a| (*a).to_string()).collect(),
    }))
    .await
}

// 05 §6.5 and §6.6; 0078 A10.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_sources_namespace_resolves_and_mints() {
    let Some((database, key_file)) = setup(DB).await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let (api, _asked) = wiki().await;

    // Declaring: the first namespace is 3000, the next 3002; the source's name is an alias.
    declare(
        &database,
        "mhc",
        Some(&api),
        Some("Miraheze Communities"),
        &[],
    )
    .await
    .expect("mhc");
    declare(&database, "oc", None, None, &[]).await.expect("oc");
    let refused = declare(&database, "itm", None, Some("Item"), &[]).await;
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("ts-namespace-name-taken")),
        "{refused:?}"
    );
    // A later record may rename the namespace and keeps its number.
    declare(&database, "oc", None, Some("Open Credit"), &["OC"])
        .await
        .expect("oc renamed");

    let site = site(&database, &key_file).await;
    let mut reader = Browser {
        router: site.clone(),
        cookie: None,
    };
    let g = reader
        .get("/w/rest.php/triplespace/v0/entity-sources")
        .await;
    let v: Value = serde_json::from_str(&g.body).unwrap();
    let sources = v["sources"].as_array().unwrap();
    let ns = |name: &str| sources.iter().find(|s| s["name"] == name).unwrap()["namespace"].clone();
    assert_eq!(
        ns("mhc"),
        json!({"id": 3000, "canonical": "Miraheze Communities", "aliases": ["mhc"]})
    );
    assert_eq!(ns("oc")["id"], 3002, "{v}");
    assert_eq!(ns("oc")["canonical"], "Open Credit", "{v}");
    assert_eq!(
        ns("oc")["aliases"],
        json!(["OC"]),
        "OC names oc already: {v}"
    );
    assert!(sources.iter().all(|s| s["name"] != "itm"));
    let info = reader
        .api("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    let own = info["query"]["triplespace"]["own_namespaces"].clone();
    assert!(
        own.as_array().unwrap().iter().any(|n| n["id"] == 3000
            && n["kind"] == "resolver"
            && n["canonical"] == "Miraheze Communities"),
        "{own}"
    );

    // Resolving: by the source's name or any name of its namespace, in any case.
    for q in ["MHC:Q9", "Miraheze_Communities:q9", "mhc:Q9"] {
        let g = reader
            .get(&format!("/w/rest.php/triplespace/v0/resolve?q={}", enc(q)))
            .await;
        assert_eq!(g.status, StatusCode::OK, "{q}: {}", g.body);
        let r: Value = serde_json::from_str(&g.body).unwrap();
        assert_eq!(r["kind"], "entity-source", "{r}");
        assert_eq!(r["source"], "mhc", "{r}");
        assert_eq!(r["id"], "mhc:Q9", "{r}");
        assert_eq!(r["external"], "https://mhc.example/entity/Q9", "{r}");
        assert_eq!(r["fetching"], true, "queued, then still queued: {r}");
        assert!(r.get("target").is_none(), "{r}");
    }
    let g = reader
        .get("/w/rest.php/triplespace/v0/resolve?q=MHC:X1")
        .await;
    let r: Value = serde_json::from_str(&g.body).unwrap();
    assert!(r["invalid"].is_string(), "{r}");
    // A source without an API is not fetched.
    let g = reader
        .get("/w/rest.php/triplespace/v0/resolve?q=OC:Q1")
        .await;
    let r: Value = serde_json::from_str(&g.body).unwrap();
    assert_eq!(r["fetching"], false, "{r}");

    // The site: a 404 page with the link out and the fetch, and no create link for a reader.
    let g = reader.get("/wiki/MHC:Q9").await;
    assert_eq!(g.status, StatusCode::NOT_FOUND, "{}", g.body);
    assert!(
        g.body
            .contains("mhc:Q9 from Miraheze Communities is not on this wiki yet"),
        "{}",
        g.body
    );
    assert!(
        g.body.contains("https://mhc.example/entity/Q9"),
        "{}",
        g.body
    );
    assert!(
        g.body
            .contains("It is queued to be read from Miraheze Communities")
    );
    assert!(!g.body.contains("ts-sameas="), "{}", g.body);
    let g = reader.get("/wiki/MHC:X1").await;
    assert_eq!(g.status, StatusCode::NOT_FOUND);
    assert!(g.body.contains("This is not an ID of Miraheze Communities"));

    // Signed in: Create an item for this, with the label from the label cache.
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };
    owner.login("Owner", "owner-secret-1234").await;
    let g = owner.get("/wiki/MHC:Q9").await;
    assert!(
        g.body
            .contains("/wiki/Special:NewItem?ts-sameas=mhc%3AQ9&amp;label=Nine"),
        "{}",
        g.body
    );
    let g = owner
        .get("/wiki/Special:NewItem?ts-sameas=mhc%3AQ9&label=Nine")
        .await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    assert!(g.body.contains("name=\"ts-sameas\""), "{}", g.body);
    assert!(g.body.contains("The same thing as mhc:Q9"), "{}", g.body);

    // Minting: one write, the item and its same-as.
    let token = owner.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();
    let data = json!({"labels": {"en": {"language": "en", "value": "Nine"}}}).to_string();
    let bad = owner
        .post_api(
            "action=wbeditentity",
            &format!(
                "new=property&data={}&ts-sameas=mhc:Q9&token={}",
                enc(&json!({"datatype": "string", "labels": {"en": {"language": "en", "value": "P"}}}).to_string()),
                enc(&token)
            ),
        )
        .await;
    assert_eq!(bad["error"]["code"], "param-illegal", "{bad}");
    let bad = owner
        .post_api(
            "action=wbeditentity",
            &format!(
                "new=item&data={}&ts-sameas=nope:Q9&token={}",
                enc(&data),
                enc(&token)
            ),
        )
        .await;
    assert_eq!(bad["error"]["code"], "ts-sameas-invalid", "{bad}");
    let made = owner
        .post_api(
            "action=wbeditentity",
            &format!(
                "new=item&data={}&ts-sameas=mhc:Q9&token={}",
                enc(&data),
                enc(&token)
            ),
        )
        .await;
    let id = made["entity"]["id"].as_str().expect("made").to_string();

    // Now MHC:Q9 reaches the local item.
    let g = reader
        .get("/w/rest.php/triplespace/v0/resolve?q=MHC:Q9")
        .await;
    let r: Value = serde_json::from_str(&g.body).unwrap();
    assert_eq!(r["target"]["id"], id, "{r}");
    assert_eq!(r["target"]["title"], format!("Item:{id}"), "{r}");
    let g = reader.get("/wiki/MHC:Q9").await;
    assert_eq!(g.status, StatusCode::SEE_OTHER, "{}", g.body);
}
