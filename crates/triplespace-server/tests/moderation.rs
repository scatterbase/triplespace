//! The moderation modules end to end (0023 §8; 09 §6): `action=protect`, `action=delete`,
//! `action=undelete` and `action=revisiondelete` on document pages and threads, what they
//! do to the views (deleted pages leave the title index, hidden parts set the visibility
//! bits) and the log events they project as; protection enforced on `action=edit` and on
//! thread writes; reserved titles (09 §4.8). Needs `TRIPLESPACE_TEST_DATABASE_URL`;
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

async fn fresh_database(name: &str) -> String {
    let db = format!("{DB}_{name}");
    let (client, connection) = admin_config()
        .unwrap()
        .connect(NoTls)
        .await
        .expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {db}"))
        .await
        .expect("create database");
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{db}"))
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

/// A fresh instance (database `{DB}_{name}`) with Alice, its owner, logged in.
async fn instance(name: &str) -> (String, App, Client) {
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("tsmoderation-{}-{name}", std::process::id()));
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
        router: router_with(app.clone(), triplespace_api_rest::routes()),
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
    (database, app, c)
}

/// Registers an account, solving the challenge when one is offered, and logs it in.
async fn account(router: &Router, name: &str, password: &str) -> Client {
    let mut a = Client {
        router: router.clone(),
        cookie: None,
    };
    let v = a
        .api("action=query&meta=authmanagerinfo&amirequestsfor=create&amimergerequestfields=1")
        .await;
    let challenge = v["query"]["authmanagerinfo"]["fields"]["tschallengeid"]["value"]
        .as_str()
        .map(|id| {
            let bits = scatter_challenge::Challenge::parse(id).unwrap().bits;
            format!(
                "&tschallengeid={}&tschallengesolution={}",
                enc(id),
                scatter_challenge::solve(id, bits)
            )
        })
        .unwrap_or_default();
    let token = a.api("action=query&meta=tokens&type=createaccount").await["query"]["tokens"]
        ["createaccounttoken"]
        .as_str()
        .unwrap()
        .to_string();
    let r = a
        .api_post(&format!(
            "action=createaccount&username={}&password={}&retype={}&createtoken={}&createreturnurl=https://{HOST}/{challenge}",
            enc(name),
            enc(password),
            enc(password),
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    let token = a.api("action=query&meta=tokens&type=login").await["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = a
        .api_post(&format!(
            "action=clientlogin&username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(name),
            enc(password),
            enc(&token)
        ))
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    a
}

/// Adds an account to a group with a membership record, as `userrights` will.
async fn grant(app: &App, name: &str, group: &str) {
    use scatter_ingest::store::{IngestStore as _, Sequence};
    use scatter_projection::Backend as _;
    let store = app.store();
    let mut cx = store.begin().await.unwrap();
    let key: String = cx
        .conn()
        .query_one(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = $2",
            &[&TENANT, &name],
        )
        .await
        .unwrap()
        .get(0);
    let actors = store
        .partition(&mut cx, TENANT, "actors")
        .await
        .unwrap()
        .unwrap();
    let mut d = triplespace_cli::common::draft(
        "scatter:v0/membership",
        &key,
        &serde_json::to_value(scatter_actors::membership::Membership::add(group, None)).unwrap(),
        &key,
        Some("test"),
        triplespace_api_action::edit::now_micros(),
    )
    .unwrap();
    d.logid = Some(
        store
            .next_id(&mut cx, TENANT, &Sequence::Log)
            .await
            .unwrap(),
    );
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

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn protect_delete_undelete_and_hide() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, _app, mut c) = instance("main").await;
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

    // Delete the new one: two deleted pages had the title, so undelete needs `pageid`.
    c.api_post(&format!("action=delete&title=Project:Scope&token={t}"))
        .await;
    let r = c
        .api_post(&format!("action=undelete&title=Project:Scope&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "ts-ambiguous-undelete", "{r}");
    assert_eq!(r["error"]["pageids"], json!([fresh, page]), "{r}");
    let u = c
        .api_post(&format!(
            "action=undelete&title=Project:Scope&pageid={fresh}&reason=Mistake&token={t}"
        ))
        .await;
    assert_eq!(u["undelete"]["title"], "Library Base:Scope", "{u}");
    assert_eq!(u["undelete"]["pageid"], fresh, "{u}");
    let q = anon
        .api("action=query&prop=info&titles=Project:Scope")
        .await;
    assert_eq!(q["query"]["pages"][0]["pageid"], fresh, "{q}");

    // Protection is enforced: a group Alice is not in may edit, she may not.
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Scope&protections=edit%3Dbot&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let r = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Blocked.&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "protectedpage", "{r}");
    c.api_post(&format!(
        "action=protect&title=Project:Scope&protections=edit%3Dall&token={t}"
    ))
    .await;
    let r = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Open.&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");

    // Create-protection reserves the title under a page ID; a create takes it once the
    // creator is admitted (Alice is in `owner`, not `bot`).
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Dbot&token={t}"
        ))
        .await;
    let reserved = p["protect"]["pageid"].as_u64().expect("a reserved ID");
    let q = anon
        .api("action=query&prop=info&titles=Project:Reserved")
        .await;
    assert_eq!(q["query"]["pages"][0]["missing"], true, "{q}");
    let r = c
        .api_post(&format!(
            "action=edit&title=Project:Reserved&text=Mine.&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-restricted-title", "{r}");
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=edit%3Dsysop&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle-createonly", "{r}");
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Downer&token={t}"
        ))
        .await;
    assert_eq!(p["protect"]["pageid"], reserved, "{p}");
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Reserved&text=Mine.&token={t}"
        ))
        .await;
    assert_eq!(e["edit"]["pageid"], reserved, "{e}");
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Downer&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "create-titleexists", "{r}");

    // Read protection before a page exists: a confidential reservation, which may name
    // only a group the protector is in.
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Secret&protections=read%3Dbot&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-locked-out", "{r}");
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Secret&protections=read%3Downer&token={t}"
        ))
        .await;
    let secret = p["protect"]["pageid"].as_u64().expect("a reserved ID");
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Secret&text=Hush.&token={t}"
        ))
        .await;
    assert_eq!(e["edit"]["pageid"], secret, "{e}");

    // The confidential page is absent for a principal outside the group, on every read
    // that looks it up, lists it, links to it or transcludes it (0056 §5; 09 §5.3), and
    // readable for the group.
    let i = anon
        .api("action=query&prop=info&titles=Project:Secret")
        .await;
    assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
    let i = anon
        .api(&format!(
            "action=query&prop=revisions&pageids={secret}&rvprop=content"
        ))
        .await;
    assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
    let i = c
        .api(&format!(
            "action=query&prop=revisions&pageids={secret}&rvprop=content"
        ))
        .await;
    assert_eq!(i["query"]["pages"][0]["pageid"], secret, "{i}");
    let p = anon.api("action=parse&page=Project:Secret").await;
    assert_eq!(p["error"]["code"], "missingtitle", "{p}");
    let p = c.api("action=parse&page=Project:Secret&prop=text").await;
    assert!(
        p["parse"]["text"].as_str().unwrap().contains("Hush."),
        "{p}"
    );
    let titles = |r: &Value, list: &str| -> Vec<String> {
        r["query"][list]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["title"].as_str().map(str::to_owned))
            .collect()
    };
    let a = anon.api("action=query&list=allpages&apnamespace=4").await;
    assert!(
        !titles(&a, "allpages").iter().any(|t| t.ends_with("Secret")),
        "{a}"
    );
    let a = c.api("action=query&list=allpages&apnamespace=4").await;
    assert!(
        titles(&a, "allpages").iter().any(|t| t.ends_with("Secret")),
        "{a}"
    );
    let rc = anon
        .api("action=query&list=recentchanges&rcprop=title%7Cids&rclimit=500")
        .await;
    assert!(
        !titles(&rc, "recentchanges")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{rc}"
    );
    let rc = c
        .api("action=query&list=recentchanges&rcprop=title%7Cids&rclimit=500")
        .await;
    assert!(
        titles(&rc, "recentchanges")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{rc}"
    );
    let l = anon.api("action=query&list=logevents&lelimit=500").await;
    assert!(
        !titles(&l, "logevents")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{l}"
    );
    let l = c.api("action=query&list=logevents&lelimit=500").await;
    assert!(
        titles(&l, "logevents")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{l}"
    );
    let r = anon.rest("GET", "/activity?limit=500", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!r.body.to_string().contains("Secret"), "{}", r.body);
    // A public page that links to it and transcludes it: a red link and nothing of its
    // text for the outsider.
    c.api_post(&format!(
        "action=edit&title=Project:Index&text={}&token={t}",
        enc("See [[Project:Secret]]. {{Project:Secret}}")
    ))
    .await;
    let p = anon.api("action=parse&page=Project:Index&prop=text").await;
    let html = p["parse"]["text"].as_str().unwrap();
    assert!(!html.contains("Hush."), "{html}");
    assert!(html.contains("redlink=1"), "{html}");
    let p = c.api("action=parse&page=Project:Index&prop=text").await;
    let html = p["parse"]["text"].as_str().unwrap();
    assert!(html.contains("Hush."), "{html}");
    assert!(!html.contains("redlink=1"), "{html}");

    // A confidential `namespace` or `tenant` restriction hides every page under it, even
    // from the owner when the owner is not in the group. Nothing writes those targets
    // yet, so the rows go into view.acl directly.
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    for target in [
        "acl:namespace:4".to_string(),
        format!("acl:tenant:{TENANT}"),
    ] {
        db.execute(
            "INSERT INTO view.acl (tenant, target, restrictions, read_kind, extra, \"offset\")
             VALUES ($1, $2, $3, 'confidential', NULL, 0)",
            &[&TENANT, &target, &json!({"read": {"group": "board"}})],
        )
        .await
        .unwrap();
        for who in [&mut anon, &mut c] {
            let i = who.api("action=query&prop=info&titles=Project:Index").await;
            assert_eq!(i["query"]["pages"][0]["missing"], true, "{target}: {i}");
            let a = who.api("action=query&list=allpages&apnamespace=4").await;
            assert!(titles(&a, "allpages").is_empty(), "{target}: {a}");
            let rc = who
                .api("action=query&list=recentchanges&rcprop=title&rclimit=500")
                .await;
            assert!(
                !titles(&rc, "recentchanges")
                    .iter()
                    .any(|t| t.ends_with("Index")),
                "{target}: {rc}"
            );
        }
        db.execute(
            "DELETE FROM view.acl WHERE tenant = $1 AND target = $2",
            &[&TENANT, &target],
        )
        .await
        .unwrap();
    }
    let i = anon
        .api("action=query&prop=info&titles=Project:Index")
        .await;
    assert!(i["query"]["pages"][0].get("missing").is_none(), "{i}");

    // Lifting a reservation before anyone creates the page frees the title.
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Free&protections=create%3Dbot&token={t}"
        ))
        .await;
    let held = p["protect"]["pageid"].as_u64().unwrap();
    c.api_post(&format!(
        "action=protect&title=Project:Free&protections=create%3Dall&token={t}"
    ))
    .await;
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Free&text=Open.&token={t}"
        ))
        .await;
    assert_ne!(e["edit"]["pageid"], held, "{e}");

    // A deleted page takes no read protection; its title can be reserved, and the
    // reservation then keeps the page from being undeleted into it.
    c.api_post(&format!(
        "action=edit&title=Project:Gone&text=Bye.&token={t}"
    ))
    .await;
    c.api_post(&format!("action=delete&title=Project:Gone&token={t}"))
        .await;
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Gone&protections=read%3Downer&token={t}"
        ))
        .await;
    assert!(p["protect"]["pageid"].is_u64(), "{p}");
    let r = c
        .api_post(&format!("action=undelete&title=Project:Gone&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "ts-title-taken", "{r}");

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

    // Suppressing: the writer marks the record, so the bit and the `suppress/revision`
    // event follow the tenant's suppression group without the projections reading it.
    let h = c
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={libel}&hide=user&suppress=yes&token={t}"
        ))
        .await;
    assert!(h.get("error").is_none(), "{h}");
    let r = c
        .api(&format!(
            "action=query&prop=revisions&revids={libel}&rvprop=user%7Cids"
        ))
        .await;
    assert_eq!(
        r["query"]["pages"][0]["revisions"][0]["suppressed"], true,
        "{r}"
    );
    let l = c
        .api("action=query&list=logevents&letype=suppress&lelimit=50")
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
    let base = r.body["base"].as_u64().unwrap();
    let read = c.rest("GET", &format!("/thread/{thread}"), None).await;
    let first = read.body["posts"][0]["id"].as_u64().unwrap();

    // Protecting the home talk page stops posting there.
    let home = read.body["home"]["talk_page_id"]
        .as_u64()
        .unwrap_or_else(|| panic!("a home talk page in {}", read.body));
    let p = c
        .api_post(&format!(
            "action=protect&pageid={home}&protections=edit%3Dbot&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/post"),
            Some(json!({"inReplyTo": first, "text": "Me too", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["code"], "protectedpage", "{}", r.body);
    c.api_post(&format!(
        "action=protect&pageid={home}&protections=edit%3Dall&token={t}"
    ))
    .await;
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/post"),
            Some(json!({"inReplyTo": first, "text": "Me too", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
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

/// Confidentiality holds against administrators outside the group (0056 A18; 09 §4.5,
/// §5.2, §5.3): a sysop outside `board` can neither find, delete, unprotect nor
/// revision-delete a board page; deleting it keeps `board` beside the deletion group, so
/// the deletion is invisible outside `board` and undeleting restores the restriction; and
/// `security.restrictions = none` refuses a new confidential restriction.
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn confidentiality_holds_against_administrators_outside_the_group() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, app, mut alice) = instance("outsiders").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Alice", "board").await;
    grant(&app, "Carol", "board").await;
    let t = enc(&alice.csrf().await);
    let bt = enc(&bob.csrf().await);

    let e = alice
        .api_post(&format!(
            "action=edit&title=Project:Minutes&text=Secret+minutes.&token={t}"
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().unwrap();
    let revid = e["edit"]["newrevid"].as_u64().unwrap();
    let p = alice
        .api_post(&format!(
            "action=protect&title=Project:Minutes&protections=read%3Dboard&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let missing = |r: &Value| r["query"]["pages"][0]["missing"] == true;
    assert!(missing(
        &bob.api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));
    assert!(!missing(
        &carol
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));

    // Bob holds delete, protect and deleterevision, and none of them reaches the page.
    let r = bob
        .api_post(&format!("action=delete&title=Project:Minutes&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle", "{r}");
    let r = bob
        .api_post(&format!("action=delete&pageid={page}&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "nosuchpageid", "{r}");
    let r = bob
        .api_post(&format!(
            "action=protect&title=Project:Minutes&protections=read%3Dall&token={bt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-restricted-title", "{r}");
    let r = bob
        .api_post(&format!(
            "action=protect&pageid={page}&protections=read%3Dall&token={bt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "nosuchpageid", "{r}");
    let r = bob
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={revid}&hide=content&token={bt}"
        ))
        .await;
    assert!(r.to_string().contains("nosuchrevid"), "{r}");
    assert!(!missing(
        &carol
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));

    // Alice deletes it. The deletion is not shown outside board, Bob cannot undelete
    // what he cannot see, and undeleting brings the restriction back.
    let d = alice
        .api_post(&format!("action=delete&title=Project:Minutes&token={t}"))
        .await;
    assert!(d.get("error").is_none(), "{d}");
    let logs = |r: &Value| r["query"]["logevents"].to_string();
    for who in [&mut anon, &mut bob] {
        let l = who.api("action=query&list=logevents&lelimit=500").await;
        assert!(!logs(&l).contains("Minutes"), "{l}");
    }
    let l = carol.api("action=query&list=logevents&lelimit=500").await;
    assert!(logs(&l).contains("Minutes"), "{l}");
    let r = bob
        .api_post(&format!("action=undelete&title=Project:Minutes&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "cantundelete", "{r}");
    let u = alice
        .api_post(&format!("action=undelete&title=Project:Minutes&token={t}"))
        .await;
    assert!(u.get("error").is_none(), "{u}");
    for who in [&mut anon, &mut bob] {
        assert!(missing(
            &who.api("action=query&prop=info&titles=Project:Minutes")
                .await
        ));
    }
    assert!(!missing(
        &carol
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));

    // security.restrictions = none: no new confidential restriction, but protecting
    // `edit` on a page that already has one keeps it and is allowed.
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let set = |v: &'static str| {
        let db = &db;
        async move {
            db.execute(
                "INSERT INTO view.registry (tenant, kind, code, config, \"offset\")
                 VALUES ('', 'tenancy', 'security.restrictions', $1, 0)
                 ON CONFLICT (tenant, kind, code) DO UPDATE SET config = EXCLUDED.config",
                &[&json!({"kind": "tenancy", "value": v})],
            )
            .await
            .unwrap();
        }
    };
    set("none").await;
    alice
        .api_post(&format!(
            "action=edit&title=Project:Agenda&text=Next+time.&token={t}"
        ))
        .await;
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Agenda&protections=read%3Dboard&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-policy", "{r}");
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Minutes&protections=edit%3Dsysop&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    assert!(missing(
        &anon
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));
    set("any").await;
}
