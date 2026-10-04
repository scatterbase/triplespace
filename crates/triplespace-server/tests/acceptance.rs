//! The milestone-1 acceptance test (`claude/milestone-1-critical-path.md`): a bot client
//! logs in with a bot password, reads `Item:Q6` and `Domain:wikipedia.org`, and saves an
//! edit — against the router the server runs, over a database the CLI set up from a
//! Librarybase-shaped XML dump and an internetdomains-shaped JSON dump. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::{Secret, keys};
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::forwarder::{Create, Forwarder, Revoke, Target};
use triplespace_cli::{accounts, adopt, instance, sync};
use triplespace_projections::Farm;

const DB: &str = "tsaccept_test";
const TENANT: &str = "librarybase";
const HOST: &str = "librarybase.org";

const DUMP_XML: &str = r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/" version="0.11" xml:lang="en">
  <siteinfo><sitename>Librarybase</sitename><namespaces>
    <namespace key="120" case="first-letter">Item</namespace>
    <namespace key="122" case="first-letter">Property</namespace>
  </namespaces></siteinfo>
  <page><title>Property:P12</title><ns>122</ns><id>40</id>
    <revision><id>903</id><timestamp>2024-01-03T09:15:40Z</timestamp>
      <contributor><username>Alice</username><id>7</id></contributor>
      <model>wikibase-property</model><format>application/json</format>
      <text xml:space="preserve">{"type":"property","id":"P12","datatype":"string","labels":{"en":{"language":"en","value":"note"}},"claims":{}}</text>
    </revision></page>
  <page><title>Item:Q6</title><ns>120</ns><id>12</id>
    <revision><id>41877</id><timestamp>2026-09-20T14:02:11Z</timestamp>
      <contributor><username>Bob</username><id>9</id></contributor>
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"type":"item","id":"Q6","labels":{"en":{"language":"en","value":"Six &amp; more"}},"descriptions":{"en":{"language":"en","value":"a number"}},"claims":{"P12":[{"mainsnak":{"snaktype":"value","property":"P12","datatype":"string","datavalue":{"value":"six","type":"string"}},"type":"statement","rank":"normal","id":"Q6$11111111-1111-1111-1111-111111111111"}]}}</text>
    </revision></page>
</mediawiki>
"#;

fn dump_json() -> String {
    let st = |p: &str, v: Value, guid: &str, dt: &str| {
        let dv = if dt == "string" {
            json!({"value": v, "type": "string"})
        } else {
            json!({"value": {"entity-type": "item", "id": v}, "type": "wikibase-entityid"})
        };
        json!({"mainsnak": {"snaktype": "value", "property": p, "datatype": dt, "datavalue": dv},
            "type": "statement", "rank": "normal", "id": guid})
    };
    let items = [
        json!({"type": "item", "id": "Q9", "labels": {"en": {"language": "en", "value": "example.org"}}, "descriptions": {}, "aliases": {},
            "claims": {"P1": [st("P1", json!("example.org"), "Q9$00000000-0000-0000-0000-0000000000A1", "string")]},
            "sitelinks": {}, "lastrevid": 55, "modified": "2026-10-01T00:00:00Z"}),
        json!({"type": "item", "id": "Q11", "labels": {"en": {"language": "en", "value": "Wikipedia"}},
            "descriptions": {"en": {"language": "en", "value": "the free encyclopedia"}}, "aliases": {},
            "claims": {"P1": [st("P1", json!("wikipedia.org"), "Q11$00000000-0000-0000-0000-0000000000B1", "string")],
                       "P8": [st("P8", json!("Q9"), "Q11$00000000-0000-0000-0000-0000000000B2", "wikibase-item")]},
            "sitelinks": {}, "lastrevid": 56, "modified": "2026-10-01T00:00:00Z"}),
        json!({"type": "property", "id": "P1", "datatype": "string", "labels": {"en": {"language": "en", "value": "domain name"}},
            "descriptions": {}, "aliases": {}, "claims": {}, "lastrevid": 3, "modified": "2026-10-01T00:00:00Z"}),
        json!({"type": "property", "id": "P8", "datatype": "wikibase-item", "labels": {"en": {"language": "en", "value": "related domain"}},
            "descriptions": {}, "aliases": {}, "claims": {}, "lastrevid": 4, "modified": "2026-10-01T00:00:00Z"}),
    ];
    let lines: Vec<String> = items.iter().map(ToString::to_string).collect();
    format!("[\n{}\n]\n", lines.join(",\n"))
}

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
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

/// A client with a cookie jar of one cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
    bearer: Option<String>,
}

impl Client {
    async fn call(&mut self, method: &str, query: &str, form: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/w/api.php?{query}"))
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(b) = &self.bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
        }
        let req = match form {
            Some(f) => req
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(f.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        if let Some(sc) = response.headers().get(header::SET_COOKIE) {
            let text = sc.to_str().unwrap();
            let value = text
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap()
                .to_string();
            self.cookie = if value.is_empty() { None } else { Some(value) };
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, body)
    }

    async fn get(&mut self, query: &str) -> Value {
        let (status, body) = self.call("GET", query, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn post(&mut self, query: &str, form: &str) -> Value {
        let (status, body) = self.call("POST", query, Some(form)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn a_bot_logs_in_reads_and_edits() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsaccept-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key_file: PathBuf = dir.join("triplespace.key");
    let xml = dir.join("librarybase.xml");
    std::fs::write(&xml, DUMP_XML).unwrap();
    let xd = dir.join("internetdomains.json");
    std::fs::write(&xd, dump_json()).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();

    // 1. The instance, the adoption, the mirror sync: what the operator runs.
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        farm_slug: None,
        farm_base: None,
        providers: vec!["internetdomains".into()],
        adopt: Some(format!("https://{HOST}/")),
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    adopt::run(adopt::Adopt {
        database: database.clone(),
        tenant: TENANT.into(),
        dump: xml,
        source: format!("https://{HOST}/"),
        version: Some("test".into()),
        frozen: true,
        counters: Some("item=6,property=12".into()),
        log_floor: 0,
        entity_sources: vec![],
        subsidiary: None,
        batch: 200,
        no_accounts: false,
        farm_slug: None,
        farm_base: None,
    })
    .await
    .expect("adopt");
    sync::run(sync::Sync {
        database: database.clone(),
        provider: "internetdomains".into(),
        dump: xd,
        version: Some("20261001".into()),
        snapshot: false,
        threshold: 1000,
        batch: 200,
        farm_slug: None,
        farm_base: None,
    })
    .await
    .expect("sync");

    // 2. A subsidiary with a key.
    let farm = Farm {
        slug: TENANT.into(),
        base: format!("https://{HOST}"),
    };
    let (store, pipeline) = triplespace_cli::common::store(&database, &farm).unwrap();
    let mut cx = scatter_projection::Backend::begin(&store).await.unwrap();
    let bot_key = accounts::create_subsidiary(
        &store,
        &pipeline,
        &mut cx,
        TENANT,
        "Alice bot",
        "librarybase:7",
        &["bot".into()],
        "librarybase:7",
        triplespace_cli::common::now(),
    )
    .await
    .unwrap();
    scatter_projection::Backend::commit(&store, cx)
        .await
        .unwrap();
    assert_eq!(bot_key, "librarybase:10", "after the adopted users 7 and 9");
    let conn = triplespace_cli::common::connect(&database).await.unwrap();
    let issued = keys::issue(
        &conn,
        &bot_key,
        "laptop",
        &["editentity".into(), "highvolume".into()],
        None,
    )
    .await
    .unwrap();
    let (_, secret) = issued.bearer.split_once('.').unwrap();

    // A forwarder key (0057 §10): the hash in `private`, the act in the instance config,
    // and its revocation a null record that retires the registry row.
    let target = || Target {
        database: database.clone(),
        farm_slug: None,
        farm_base: None,
    };
    triplespace_cli::forwarder::run(Forwarder::Create(Create {
        target: target(),
        label: "edge".into(),
    }))
    .await
    .unwrap();
    let key_id: String = conn
        .query_one(
            "SELECT key_id FROM private.forwarder_key WHERE label = 'edge'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let row = conn
        .query_one(
            "SELECT config FROM view.registry WHERE tenant = '' AND kind = 'forwarder' AND code = $1",
            &[&key_id],
        )
        .await
        .unwrap();
    let entry: Value = row.get(0);
    assert_eq!(entry["label"], "edge");
    assert!(entry.get("hash").is_none() && entry.get("secret").is_none());
    triplespace_cli::forwarder::run(Forwarder::Revoke(Revoke {
        target: target(),
        key_id: key_id.clone(),
    }))
    .await
    .unwrap();
    assert!(
        conn.query_opt(
            "SELECT 1 FROM view.registry WHERE kind = 'forwarder' AND code = $1",
            &[&key_id],
        )
        .await
        .unwrap()
        .is_none()
    );

    // 3. The server.
    let signing = triplespace_cli::common::signing_key(&key_file).unwrap();
    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&signing.to_bytes()),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm,
            generator: "Triplespace test".into(),
        },
    )
    .unwrap();
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
        bearer: None,
    };

    // An unregistered host is 421.
    let req = Request::builder()
        .uri("/w/api.php?action=query&meta=siteinfo&format=json")
        .header(header::HOST, "evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        c.router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::MISDIRECTED_REQUEST
    );

    // siteinfo, as a client's first call.
    let si = c
        .get("action=query&meta=siteinfo&siprop=general|namespaces|triplespace&format=json")
        .await;
    assert_eq!(
        si["query"]["general"]["wikibase-conceptbaseuri"],
        json!(format!("https://{HOST}/entity/"))
    );
    assert_eq!(
        si["query"]["namespaces"]["120"]["defaultcontentmodel"],
        "wikibase-item"
    );
    assert_eq!(si["query"]["namespaces"]["210"]["name"], "Domain");
    assert!(si["query"]["triplespace"].get("insecure").is_none());

    // Anonymous read of both entities.
    let r = c
        .get("action=wbgetentities&ids=Q6|Domain:wikipedia.org&format=json")
        .await;
    assert_eq!(r["success"], 1);
    let q6 = &r["entities"]["Q6"];
    assert_eq!(q6["labels"]["en"]["value"], "Six & more");
    assert_eq!(q6["title"], "Item:Q6");
    assert_eq!(q6["pageid"], 12);
    let first_revid = q6["lastrevid"].as_u64().unwrap();
    assert!(first_revid > 41_877, "fresh IDs above the floor (0035 §3)");
    let wp = &r["entities"]["domain:wikipedia.org"];
    assert_eq!(wp["type"], "domain");
    assert_eq!(wp["title"], "Domain:wikipedia.org");
    assert_eq!(wp["labels"]["en"]["value"], "Wikipedia");
    assert_eq!(
        wp["labels"]["mul"]["value"], "wikipedia.org",
        "the derived label (0009 §5)"
    );
    assert_eq!(
        wp["claims"]["XDP8"][0]["mainsnak"]["datavalue"]["value"]["id"], "domain:example.org",
        "a value pointing at a mapped item is a domain value (0009 §9)"
    );
    assert!(
        wp["claims"].get("XDP1").is_none(),
        "the identity property is dropped"
    );
    let empty = c
        .get("action=wbgetentities&ids=domain:nothing-here.example&format=json")
        .await;
    assert_eq!(
        empty["entities"]["domain:nothing-here.example"]["labels"]["mul"]["value"],
        "nothing-here.example"
    );
    assert!(
        empty["entities"]["domain:nothing-here.example"]
            .get("missing")
            .is_none(),
        "a Domain is never missing (0009 §4)"
    );
    let missing = c.get("action=wbgetentities&ids=Q999&format=json").await;
    assert_eq!(missing["entities"]["Q999"]["missing"], "");

    // An anonymous write is refused.
    let (_, denied) = c
        .call(
            "POST",
            "action=wbsetlabel&format=json",
            Some(&format!("id=Q6&language=fr&value=Six&token={}", enc(r"+\"))),
        )
        .await;
    assert_eq!(denied["error"]["code"], "permissiondenied", "{denied}");

    // 4. Bot-password login, as WikibaseIntegrator does it.
    let t = c
        .get("action=query&meta=tokens&type=login&format=json")
        .await;
    let login_token = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(c.cookie.is_some(), "an anonymous session for the token");
    let bad = c
        .post(
            "action=login&format=json",
            &format!(
                "lgname={}&lgpassword=wrong&lgtoken={}",
                enc("Alice bot@laptop"),
                enc(&login_token)
            ),
        )
        .await;
    assert_eq!(bad["login"]["result"], "Failed");
    let primary = c
        .post(
            "action=login&format=json",
            &format!("lgname=Alice&lgpassword=x&lgtoken={}", enc(&login_token)),
        )
        .await;
    assert_eq!(primary["login"]["code"], "ts-use-oauth", "{primary}");
    let ok = c
        .post(
            "action=login&format=json",
            &format!(
                "lgname={}&lgpassword={}&lgtoken={}",
                enc("Alice bot@laptop"),
                enc(secret),
                enc(&login_token)
            ),
        )
        .await;
    assert_eq!(ok["login"]["result"], "Success", "{ok}");
    assert_eq!(ok["login"]["lgusername"], "Alice bot");
    let ui = c
        .get("action=query&meta=userinfo&uiprop=groups|rights&format=json")
        .await;
    assert_eq!(ui["query"]["userinfo"]["name"], "Alice bot");
    assert!(
        ui["query"]["userinfo"]["groups"]
            .as_array()
            .unwrap()
            .contains(&json!("bot"))
    );
    assert!(
        ui["query"]["userinfo"]["rights"]
            .as_array()
            .unwrap()
            .contains(&json!("edit"))
    );
    assert_eq!(ui["query"]["userinfo"]["operator"], "librarybase:7");
    let csrf = c.get("action=query&meta=tokens&format=json").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(csrf.ends_with(r"+\"));

    // 5. Edits: a label on Q6 through wbeditentity, a statement on the Domain.
    let (_, wrong) = c
        .call(
            "POST",
            "action=wbeditentity&format=json",
            Some(&format!(
                "id=Q6&data={}&token={}",
                enc(r#"{"labels":{"fr":{"language":"fr","value":"Six"}}}"#),
                enc(r"+\")
            )),
        )
        .await;
    assert_eq!(wrong["error"]["code"], "badtoken");
    let edited = c
        .post(
            "action=wbeditentity&format=json",
            &format!(
                "id=Q6&data={}&summary=fr+label&token={}",
                enc(r#"{"labels":{"fr":{"language":"fr","value":"Six"}}}"#),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(edited["success"], 1, "{edited}");
    assert_eq!(edited["entity"]["labels"]["fr"]["value"], "Six");
    assert_eq!(
        edited["entity"]["labels"]["en"]["value"], "Six & more",
        "untouched terms stay"
    );
    let second_revid = edited["entity"]["lastrevid"].as_u64().unwrap();
    assert!(second_revid > first_revid);

    let created = c
        .post(
            "action=wbcreateclaim&format=json",
            &format!(
                "entity={}&property=P12&snaktype=value&value={}&token={}",
                enc("domain:wikipedia.org"),
                enc("\"an assertion of ours\""),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(created["success"], 1, "{created}");
    let guid = created["claim"]["id"].as_str().unwrap().to_string();
    assert!(guid.starts_with("domain:wikipedia.org$"), "{guid}");
    let r = c
        .get("action=wbgetentities&ids=domain:wikipedia.org&format=json")
        .await;
    let wp = &r["entities"]["domain:wikipedia.org"];
    assert_eq!(
        wp["claims"]["P12"][0]["mainsnak"]["datavalue"]["value"],
        "an assertion of ours"
    );
    assert_eq!(
        wp["claims"]["XDP8"][0]["mainsnak"]["datavalue"]["value"]["id"], "domain:example.org",
        "the mirrored statement is still there"
    );
    assert_eq!(wp["labels"]["en"]["value"], "Wikipedia");

    // A stale base revision is an edit conflict (0006 §8).
    let (_, conflict) = c
        .call(
            "POST",
            "action=wbsetdescription&format=json",
            Some(&format!(
                "id=Q6&language=fr&value=nombre&baserevid={first_revid}&token={}",
                enc(&csrf)
            )),
        )
        .await;
    assert_eq!(conflict["error"]["code"], "editconflict", "{conflict}");
    let fine = c
        .post(
            "action=wbsetdescription&format=json",
            &format!(
                "id=Q6&language=fr&value=nombre&baserevid={second_revid}&token={}",
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(fine["success"], 1, "{fine}");
    let third_revid = fine["entity"]["lastrevid"].as_u64().unwrap();

    // Changing and removing a statement: remove + add under the same GUID, then remove.
    let changed = c.post("action=wbsetclaim&format=json", &format!("claim={}&token={}", enc(r#"{"id":"Q6$11111111-1111-1111-1111-111111111111","mainsnak":{"snaktype":"value","property":"P12","datavalue":{"value":"SIX","type":"string"}},"type":"statement","rank":"preferred"}"#), enc(&csrf))).await;
    assert_eq!(changed["success"], 1, "{changed}");
    assert_eq!(changed["claim"]["rank"], "preferred");
    assert_eq!(changed["claim"]["mainsnak"]["datavalue"]["value"], "SIX");
    let r = c.get("action=wbgetentities&ids=Q6&format=json").await;
    assert_eq!(
        r["entities"]["Q6"]["claims"]["P12"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "replaced, not duplicated: {}",
        r["entities"]["Q6"]["claims"]
    );
    let removed = c
        .post(
            "action=wbremoveclaims&format=json",
            &format!(
                "claim={}&token={}",
                enc("Q6$11111111-1111-1111-1111-111111111111"),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(removed["success"], 1, "{removed}");
    let r = c.get("action=wbgetentities&ids=Q6&format=json").await;
    assert!(
        r["entities"]["Q6"]["claims"].get("P12").is_none(),
        "{}",
        r["entities"]["Q6"]
    );
    assert!(r["entities"]["Q6"]["lastrevid"].as_u64().unwrap() > third_revid);

    // A new item.
    let new = c.post("action=wbeditentity&format=json", &format!("new=item&data={}&token={}", enc(r#"{"labels":{"en":{"language":"en","value":"Seven"}},"claims":{"P12":[{"mainsnak":{"snaktype":"value","property":"P12","datavalue":{"value":"seven","type":"string"}},"type":"statement","rank":"normal"}]}}"#), enc(&csrf))).await;
    assert_eq!(new["success"], 1, "{new}");
    assert_eq!(new["entity"]["id"], "Q7", "above the item floor");
    assert!(
        new["entity"]["claims"]["P12"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("Q7$")
    );
    let s = c
        .get("action=wbsearchentities&search=sev&language=en&format=json")
        .await;
    assert_eq!(s["search"][0]["id"], "Q7", "{s}");
    assert_eq!(s["search"][0]["label"], "Seven");

    // 6. The same key as a bearer credential, stateless.
    let mut b = Client {
        router: c.router.clone(),
        cookie: None,
        bearer: Some(issued.bearer.clone()),
    };
    let ui = b
        .get("action=query&meta=userinfo&uiprop=groups&format=json")
        .await;
    assert_eq!(ui["query"]["userinfo"]["name"], "Alice bot");
    assert!(b.cookie.is_none(), "no session for a bearer request");
    let btoken =
        b.get("action=query&meta=tokens&format=json").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string();
    let aliased = b
        .post(
            "action=wbsetaliases&format=json",
            &format!("id=Q6&language=en&add=VI&token={}", enc(&btoken)),
        )
        .await;
    assert_eq!(aliased["success"], 1, "{aliased}");
    assert_eq!(aliased["entity"]["aliases"]["en"][0]["value"], "VI");

    // 7. The data document and the concept URI.
    let req = Request::builder()
        .uri("/wiki/Special:EntityData/Q6.json")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let resp = c.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["entities"]["Q6"]["labels"]["fr"]["value"], "Six");
    let req = Request::builder()
        .uri("/entity/Q6")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let resp = c.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers()[header::LOCATION],
        "/wiki/Special:EntityData/Q6.json"
    );

    // 8. Logout ends the session.
    let out = c
        .post(
            "action=logout&format=json",
            &format!("token={}", enc(&csrf)),
        )
        .await;
    assert!(out.get("logout").is_some(), "{out}");
    let ui = c.get("action=query&meta=userinfo&format=json").await;
    assert_eq!(ui["query"]["userinfo"]["anon"], "");

    // 9. The owner's password works for clientlogin.
    let t = c
        .get("action=query&meta=tokens&type=login&format=json")
        .await;
    let login_token = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = c.post("action=clientlogin&format=json", &format!("username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/", enc(&login_token))).await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let ui = c
        .get("action=query&meta=userinfo&uiprop=groups&format=json")
        .await;
    assert!(
        ui["query"]["userinfo"]["groups"]
            .as_array()
            .unwrap()
            .contains(&json!("owner")),
        "{ui}"
    );

    // What the site reads in its first phase (0057 §5–6, §9; 0012 §5).
    what_the_site_reads(&c.router, &database).await;

    // 10. Everything the API wrote replays from the log.
    let status = triplespace_cli::status::Target {
        database: database.clone(),
        farm_slug: None,
        farm_base: None,
    };
    triplespace_cli::status::rebuild(status)
        .await
        .expect("rebuild");
    let r = c
        .get("action=wbgetentities&ids=Q6|Q7|domain:wikipedia.org&format=json")
        .await;
    assert_eq!(r["entities"]["Q6"]["aliases"]["en"][0]["value"], "VI");
    assert_eq!(r["entities"]["Q7"]["labels"]["en"]["value"], "Seven");
    assert_eq!(
        r["entities"]["domain:wikipedia.org"]["claims"]["P12"][0]["mainsnak"]["datavalue"]["value"],
        "an assertion of ours"
    );
}

/// Sends a request with no credentials, or with the headers given, and returns the
/// status, the headers and the body.
async fn send(
    router: &Router,
    uri: &str,
    extra: &[(&str, String)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut req = Request::builder().uri(uri).header(header::HOST, HOST);
    for (k, v) in extra {
        req = req.header(*k, v.as_str());
    }
    let response = router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}

#[allow(clippy::too_many_lines)]
async fn what_the_site_reads(router: &Router, database: &str) {
    // siteinfo: the API level, and no theme until one is set.
    let siteinfo = "/w/api.php?action=query&meta=siteinfo&siprop=triplespace&format=json";
    let (status, h, body) = send(router, siteinfo, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["query"]["triplespace"]["api_version"],
        json!(triplespace_api_action::API_VERSION)
    );
    assert!(body["query"]["triplespace"].get("theme").is_none());
    assert!(
        h["cache-control"]
            .to_str()
            .unwrap()
            .starts_with("public, max-age=60"),
        "siteinfo alone is stable: {h:?}"
    );

    // A tenant theme set in its config is what siteinfo reports.
    let farm = Farm {
        slug: TENANT.into(),
        base: format!("https://{HOST}"),
    };
    let (store, pipeline) = triplespace_cli::common::store(database, &farm).unwrap();
    let mut cx = scatter_projection::Backend::begin(&store).await.unwrap();
    let config = scatter_ingest::store::IngestStore::partition(&store, &mut cx, TENANT, "config")
        .await
        .unwrap()
        .expect("the tenant has a config partition");
    let theme =
        json!({"color-progressive": "#2b559e", "font-family-heading-main": "Newsreader, serif"});
    let d = triplespace_cli::common::draft(
        scatter_log::registry::PAYLOAD_CONFIG,
        "site:ui.theme",
        &json!({"kind": "site", "name": "ui.theme", "value": theme}),
        "librarybase:7",
        None,
        triplespace_cli::common::now(),
    )
    .unwrap();
    scatter_ingest::write::append_projected(
        &store,
        &pipeline,
        &mut cx,
        config,
        d,
        scatter_projection::Budget::NONE,
    )
    .await
    .unwrap();
    scatter_projection::Backend::commit(&store, cx)
        .await
        .unwrap();
    let (_, _, body) = send(router, siteinfo, &[]).await;
    assert_eq!(body["query"]["triplespace"]["theme"], theme);

    // An anonymous entity read is public, tagged and revalidates to 304.
    let get = "/w/api.php?action=wbgetentities&ids=Q6|domain:wikipedia.org&format=json";
    let (status, h, _) = send(router, get, &[]).await;
    assert_eq!(status, StatusCode::OK);
    let cc = h["cache-control"].to_str().unwrap();
    assert!(cc.starts_with("public, max-age=0, s-maxage=60"), "{cc}");
    assert_eq!(h["cache-tag"], "entity:Q6, entity:domain:wikipedia.org");
    let etag = h["etag"].to_str().unwrap().to_string();
    let (status, h2, _) = send(router, get, &[("if-none-match", etag.clone())]).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(h2["etag"].to_str().unwrap(), etag);

    // With credentials it is private, and carries no tags.
    let (_, h, _) = send(
        router,
        get,
        &[("authorization", "Bearer nonsense.key".into())],
    )
    .await;
    assert_eq!(h["cache-control"], "private, no-cache");
    assert!(h.get("cache-tag").is_none());

    // The data document caches the same way.
    let doc = "/wiki/Special:EntityData/Q6.json";
    let (status, h, _) = send(router, doc, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(h["cache-control"].to_str().unwrap().contains("s-maxage=60"));
    let etag = h["etag"].to_str().unwrap().to_string();
    let (status, _, _) = send(router, doc, &[("if-none-match", etag)]).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);

    // Provenance: a local item adopted from Librarybase.
    let (status, h, p) = send(
        router,
        "/w/rest.php/triplespace/v0/entity/Q6/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["id"], "Q6");
    assert_eq!(p["type"], "item");
    assert_eq!(p["minted_by"]["kind"], "tenant");
    assert_eq!(p["minted_by"]["adopted_from"], "https://librarybase.org/");
    assert_eq!(p["graphs"][0]["graph"], "local");
    assert_eq!(p["graphs"][0]["history"], "full");
    // The test removed Q6's one statement earlier, so no graph dominates.
    assert_eq!(p["graphs"][0]["statements"], 0);
    assert!(p.get("dominant").is_none());
    assert_eq!(h["cache-tag"], "entity:Q6");

    // A Domain: the mirror's record and the tenant's assertion, side by side.
    let (status, _, p) = send(
        router,
        "/rest.php/triplespace/v0/entity/domain:wikipedia.org/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["minted_by"], json!({"kind": "keyed", "type": "domain"}));
    let graphs: Vec<&str> = p["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["graph"].as_str().unwrap())
        .collect();
    assert_eq!(graphs, ["local", "mirror/internetdomains"]);
    let mirror = &p["graphs"][1];
    assert!(mirror.get("upstream_version").is_some(), "{mirror}");
    assert_eq!(mirror["history"], "latest");
    assert!(mirror.get("job").is_some(), "the sync job: {mirror}");
    // One statement each; a tie goes to the earlier graph, local first.
    assert_eq!(p["dominant"], "local");
    assert!(
        p["statements"]
            .as_object()
            .unwrap()
            .values()
            .any(|s| s["graphs"] == json!(["local"])),
        "the tenant's own statement on the Domain: {p}"
    );

    // Missing and malformed IDs.
    let (status, _, e) = send(
        router,
        "/w/rest.php/triplespace/v0/entity/Q999999/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(e["code"], "entity-not-found");
    let (status, _, e) = send(
        router,
        "/w/rest.php/triplespace/v0/entity/not-an-id/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(e["code"], "invalid-entity-id");
}
