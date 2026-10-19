//! Two tenants on one instance, end to end (ADR 0018; 0028 §1; 0046; 0079; 0089 §1):
//! `instance create` with a farm of its own and the primary tenant, the auth-host gate on
//! a second tenant, `tenant create`'s refusals, the founding records and derived issuer
//! codes, the attestations of the records creation writes, the routing of each host
//! (tenant, farm, auth, unknown), `GET /tenancy` and `siprop=triplespace`, and isolation:
//! sessions, entity numbering, entities and attribution stay per tenant. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use scatter_actors::issuer::{derived_code, is_derived_code};
use scatter_integrity::attest::Attestation;
use scatter_log::header::CONFIG_PARTITION;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::{instance, tenant};

const DB: &str = "tstenants_test";
const FARM: &str = "farm.example";
const ALPHA: &str = "alpha.example";
const BETA: &str = "beta.example";
const AUTH: &str = "auth.example";
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
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{DB}"))
        .unwrap()
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A browser on one host: one session cookie.
struct Client {
    router: Router,
    host: &'static str,
    cookie: Option<String>,
}

impl Client {
    fn new(router: &Router, host: &'static str) -> Self {
        Self {
            router: router.clone(),
            host,
            cookie: None,
        }
    }

    async fn send(&mut self, method: &str, uri: &str, form: Option<String>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, self.host);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if form.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        }
        let mut req = req.body(form.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([198, 51, 100, 7], 50000))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        if let Some(sc) = response.headers().get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap_or_default()
                .to_string();
            self.cookie = if value.is_empty() { None } else { Some(value) };
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, body)
    }

    async fn api(&mut self, query: &str) -> Value {
        let (status, body) = self
            .send(
                "GET",
                &format!("/w/api.php?{query}&format=json&formatversion=2"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn post(&mut self, form: &str) -> Value {
        let (status, body) = self
            .send(
                "POST",
                "/w/api.php",
                Some(format!("{form}&format=json&formatversion=2")),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn token(&mut self, kind: &str) -> String {
        self.api(&format!("action=query&meta=tokens&type={kind}"))
            .await["query"]["tokens"][format!("{kind}token")]
        .as_str()
        .unwrap()
        .to_string()
    }

    async fn login(&mut self, name: &str) -> Value {
        let token = self.token("login").await;
        self.post(&format!(
            "action=clientlogin&username={}&password={}&logintoken={}&loginreturnurl=https://{}/",
            enc(name),
            enc(PASSWORD),
            enc(&token),
            self.host
        ))
        .await
    }

    async fn whoami(&mut self) -> Value {
        self.api("action=query&meta=userinfo").await["query"]["userinfo"].clone()
    }

    async fn new_item(&mut self, label: &str) -> Value {
        let csrf = self.token("csrf").await;
        let data = json!({"labels": {"en": {"language": "en", "value": label}}}).to_string();
        self.post(&format!(
            "action=wbeditentity&new=item&data={}&token={}",
            enc(&data),
            enc(&csrf)
        ))
        .await
    }

    async fn siteinfo(&mut self) -> Value {
        self.api("action=query&meta=siteinfo&siprop=general|triplespace")
            .await["query"]
            .clone()
    }
}

fn tenant_create(
    database: &str,
    key_file: &std::path::Path,
    slug: &str,
    host: &str,
) -> tenant::Create {
    tenant::Create {
        database: database.to_string(),
        key_file: key_file.to_path_buf(),
        tenant: slug.into(),
        base: format!("https://{host}"),
        name: None,
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: None,
        operator: None,
        auth_base: Some(format!("https://{AUTH}")),
    }
}

/// The record at `offset` of `partition`.
async fn record(database: &str, partition: u64, offset: u64) -> scatter_log::record::Record {
    let store =
        scatter_log_postgres::PgLog::new(triplespace_cli::common::connect(database).await.unwrap());
    let slot = scatter_log::store::LogStore::read(&store, partition, offset)
        .await
        .unwrap();
    slot.record().cloned().expect("present")
}

async fn partition_of(db: &tokio_postgres::Client, tenant: &str, name: &str) -> u64 {
    let id: i64 = db
        .query_one(
            "SELECT partition FROM log.partition WHERE tenant = $1 AND name = $2",
            &[&tenant, &name],
        )
        .await
        .unwrap()
        .get(0);
    scatter_log_postgres::ids::partition_from_db(id)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn two_tenants_on_one_instance() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tstenants-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key_file = dir.join("triplespace.key");
    let _ = std::fs::remove_file(&key_file);
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, format!("{PASSWORD}\n")).unwrap();

    // The farm slug is required and differs from the primary tenant's; the presets that
    // give the farm identity are refused until farm accounts exist.
    let create = |farm_slug: &str, tenancy: &str| instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        farm_slug: farm_slug.into(),
        farm_base: Some(format!("https://{FARM}")),
        tenancy: tenancy.into(),
        tenant: "alpha".into(),
        base: format!("https://{ALPHA}"),
        name: Some("Alpha".into()),
        providers: vec![],
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: Some(pw.clone()),
    };
    let err = instance::run(create("alpha", "isolated"))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("must differ"), "{err:#}");
    let err = instance::run(create("acme", "community"))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("farm identity"), "{err:#}");
    let err = instance::run(create("wikidatawiki", "isolated"))
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("registered issuer code"),
        "{err:#}"
    );
    instance::run(create("acme", "isolated"))
        .await
        .expect("instance create");

    // A second tenant needs the auth host (0089 §1), which is no tenant's or farm's host.
    let mut no_auth = tenant_create(&database, &key_file, "beta", BETA);
    no_auth.auth_base = None;
    let err = tenant::run(tenant::TenantCommand::Create(no_auth))
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("ts-auth-base-required"),
        "{err:#}"
    );
    let mut auth_clash = tenant_create(&database, &key_file, "beta", BETA);
    auth_clash.auth_base = Some(format!("https://{ALPHA}"));
    let err = tenant::run(tenant::TenantCommand::Create(auth_clash))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("auth base"), "{err:#}");
    for (slug, host, why) in [
        ("alpha", BETA, "already exists"),
        ("acme", BETA, "farm slug"),
        ("beta", ALPHA, "already the host"),
        ("beta", FARM, "farm base"),
        ("beta", AUTH, "auth base"),
        ("internetdomains", BETA, "registered"),
    ] {
        let err = tenant::run(tenant::TenantCommand::Create(tenant_create(
            &database, &key_file, slug, host,
        )))
        .await
        .unwrap_err();
        assert!(format!("{err:#}").contains(why), "{slug} {host}: {err:#}");
    }
    // The operator must be an owner of the primary tenant.
    let mut stranger = tenant_create(&database, &key_file, "beta", BETA);
    stranger.operator = Some(99);
    let err = tenant::run(tenant::TenantCommand::Create(stranger))
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("not a member of `owner`"),
        "{err:#}"
    );
    // A key that is not the instance's is refused.
    let mut wrong_key = tenant_create(&database, &key_file, "beta", BETA);
    let other = dir.join("other.key");
    let _ = std::fs::remove_file(&other);
    triplespace_cli::common::signing_key(&other).unwrap();
    wrong_key.key_file = other;
    let err = tenant::run(tenant::TenantCommand::Create(wrong_key))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("current key"), "{err:#}");

    let mut beta = tenant_create(&database, &key_file, "beta", BETA);
    beta.name = Some("Beta".into());
    beta.owner_password_file = Some(pw.clone());
    tenant::run(tenant::TenantCommand::Create(beta))
        .await
        .expect("tenant create");

    // The registry: the farm, two tenants with derived codes, alpha primary.
    let db = triplespace_cli::common::connect(&database).await.unwrap();
    let farm = triplespace_cli::common::farm_of(&db).await.unwrap();
    assert_eq!(farm.slug, "acme");
    assert!(is_derived_code(&farm.code), "{}", farm.code);
    let tenants = triplespace_projections::tenants::tenants(&db)
        .await
        .unwrap();
    assert_eq!(
        tenants.iter().map(|t| t.slug.as_str()).collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    let (alpha, beta) = (tenants[0].clone(), tenants[1].clone());
    for t in [&alpha, &beta] {
        assert!(is_derived_code(&t.issuer), "{}", t.issuer);
    }
    assert_ne!(alpha.issuer, beta.issuer);
    assert_ne!(alpha.issuer, farm.code);
    assert_eq!(
        triplespace_projections::tenants::primary(&db)
            .await
            .unwrap()
            .as_deref(),
        Some("alpha")
    );

    // Founding records: the instance's cites nothing and derives the farm code; each
    // tenant's cites the instance's and derives its issuer code (0079 §1–4).
    let instance_founding = record(&database, CONFIG_PARTITION, 0).await;
    assert_eq!(derived_code(&instance_founding.leaf()), farm.code);
    let key = triplespace_cli::common::signing_key(&key_file).unwrap();
    let public = key.verifying_key().to_bytes();
    let key_id = scatter_log::registry::key_id(&public);
    let lookup = |k: &str| (k == key_id).then_some(public);
    let att = Attestation::of(&instance_founding).unwrap();
    assert!(
        matches!(
            att,
            Attestation::Founding {
                authority: None,
                ..
            }
        ),
        "{att:?}"
    );
    assert_eq!(att.verify(&instance_founding, lookup), Ok(true));
    for t in [&alpha, &beta] {
        let config = partition_of(&db, &t.slug, "config").await;
        let founding = record(&database, config, 0).await;
        assert_eq!(derived_code(&founding.leaf()), t.issuer, "{}", t.slug);
        let att = Attestation::of(&founding).unwrap();
        assert!(matches!(
            att,
            Attestation::Founding {
                authority: Some(_),
                ..
            }
        ));
        assert_eq!(att.verify(&founding, lookup), Ok(true));
        // The owner's account is a provision: an instance act citing the `tenant:`
        // record, not binding.
        let actors = partition_of(&db, &t.slug, "actors").await;
        let owner = record(&database, actors, 0).await;
        assert_eq!(
            owner.header().key.as_deref(),
            Some(format!("{}:1", t.issuer).as_str())
        );
        let att = Attestation::of(&owner).unwrap();
        assert!(matches!(att, Attestation::Instance { .. }), "{att:?}");
        assert_eq!(att.verify(&owner, lookup), Ok(true));
    }
    // `verify` at level 2 over the instance `config` and each tenant's partitions: the
    // founding records, the provisions and the key chain all check, and no founding
    // attestation sits anywhere but offset 0.
    let store = scatter_log_postgres::PgLog::new(
        triplespace_cli::common::connect(&database).await.unwrap(),
    );
    let chain = scatter_integrity::KeyChain::from_records([&instance_founding]).unwrap();
    let checkpoints = scatter_integrity::MemoryCheckpoints::new();
    // (partition, whether it is a `config`, whether creation signed records into it)
    let mut checked = vec![(CONFIG_PARTITION, true, true)];
    for t in [&alpha, &beta] {
        for name in ["config", "actors", "local", "log"] {
            let signed = matches!(name, "config" | "actors");
            checked.push((
                partition_of(&db, &t.slug, name).await,
                name == "config",
                signed,
            ));
        }
    }
    for (partition, is_config, signed) in checked {
        let report = scatter_integrity::verify(
            &store,
            &checkpoints,
            &scatter_integrity::Verify {
                partition,
                origin: "test",
                chain: &chain,
                level: scatter_integrity::Level::Bodies,
                actor_keys: None,
                part_names: None,
                is_config,
            },
        )
        .await;
        let bad: Vec<_> = report
            .problems
            .iter()
            .filter(|p| {
                matches!(
                    p,
                    scatter_integrity::Problem::Signature { .. }
                        | scatter_integrity::Problem::MisplacedFounding { .. }
                        | scatter_integrity::Problem::StaleInstanceKey { .. }
                )
            })
            .collect();
        assert!(bad.is_empty(), "{partition:#x}: {bad:?}");
        assert!(
            !signed || report.signatures > 0,
            "{partition:#x}: {report:?}"
        );
    }

    // Beta's `tenant:` record is attested by the operator who created it, alpha's owner.
    let row = db
        .query_one(
            "SELECT actor, instance_act FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = 'beta'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        row.get::<_, Option<String>>(0),
        Some(format!("{}:1", alpha.issuer))
    );
    assert!(!row.get::<_, bool>(1));

    // The server: a second tenant needs the auth base in its configuration too.
    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&key.to_bytes()),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: farm.clone(),
            auth_base: Some(format!("https://{AUTH}")),
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let router = router_with(app, triplespace_api_rest::routes());

    // Each host: its tenant; the farm base serves the primary tenant; the auth base is a
    // registered host that serves no wiki; any other host is 421.
    let mut on_alpha = Client::new(&router, ALPHA);
    let mut on_beta = Client::new(&router, BETA);
    let mut on_farm = Client::new(&router, FARM);
    let si = on_alpha.siteinfo().await;
    assert_eq!(si["general"]["sitename"], "Alpha");
    assert_eq!(si["triplespace"]["tenant"], "alpha");
    assert_eq!(si["triplespace"]["issuer"], alpha.issuer.as_str());
    assert_eq!(si["triplespace"]["farm"], "acme");
    assert_eq!(si["triplespace"]["farm_code"], farm.code.as_str());
    assert_eq!(si["triplespace"]["at_farm"], false);
    assert!(si["triplespace"].get("primary").is_none());
    assert_eq!(si["triplespace"]["tenancy"]["preset"], "isolated");
    assert_eq!(
        si["triplespace"]["tenancy"]["switches"]["providers.between_tenants"],
        "operator"
    );
    let si = on_beta.siteinfo().await;
    assert_eq!(si["general"]["sitename"], "Beta");
    assert_eq!(si["triplespace"]["issuer"], beta.issuer.as_str());
    let si = on_farm.siteinfo().await;
    assert_eq!(si["triplespace"]["tenant"], "alpha");
    assert_eq!(si["triplespace"]["at_farm"], true);
    assert_eq!(si["triplespace"]["primary"], "alpha");
    let mut auth = Client::new(&router, AUTH);
    let (status, body) = auth.send("GET", "/w/api.php?action=query", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _) = auth.send("GET", "/healthz", None).await;
    assert_eq!(status, StatusCode::OK);
    let mut evil = Client::new(&router, "evil.example");
    let (status, _) = evil.send("GET", "/w/api.php?action=query", None).await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);

    // GET /tenancy: at the farm base only.
    let (status, tenancy) = on_farm
        .send("GET", "/w/rest.php/triplespace/v0/tenancy", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{tenancy}");
    assert_eq!(
        tenancy["farm"],
        json!({"slug": "acme", "code": farm.code, "base": format!("https://{FARM}")})
    );
    assert_eq!(tenancy["primary"], "alpha");
    assert_eq!(tenancy["policy"]["preset"], "isolated");
    assert_eq!(
        tenancy["tenants"],
        json!([
            {"slug": "alpha", "issuer": alpha.issuer, "host": ALPHA, "base": format!("https://{ALPHA}"), "name": "Alpha", "status": "active"},
            {"slug": "beta", "issuer": beta.issuer, "host": BETA, "base": format!("https://{BETA}"), "name": "Beta", "status": "active"},
        ])
    );
    let (status, err) = on_beta
        .send("GET", "/w/rest.php/triplespace/v0/tenancy", None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["code"], "ts-instance-policy");

    // Isolation. Both owners are "Owner", each on their own tenant; a session belongs to
    // the host it was opened on.
    assert_eq!(
        on_alpha.login("Owner").await["clientlogin"]["status"],
        "PASS"
    );
    assert_eq!(on_alpha.whoami().await["name"], "Owner");
    let mut b_with_alphas_cookie = Client::new(&router, BETA);
    b_with_alphas_cookie.cookie = on_alpha.cookie.clone();
    assert!(
        b_with_alphas_cookie.whoami().await.get("anon").is_some(),
        "alpha's session is nothing on beta"
    );
    assert_eq!(
        on_beta.login("Owner").await["clientlogin"]["status"],
        "PASS"
    );

    // Entity numbering is per tenant, and so are the entities.
    let made = on_alpha.new_item("Alpha's first").await;
    assert_eq!(made["success"], 1, "{made}");
    assert_eq!(made["entity"]["id"], "Q1");
    let made = on_beta.new_item("Beta's first").await;
    assert_eq!(made["entity"]["id"], "Q1", "beta numbers its own items");
    let got = on_alpha.api("action=wbgetentities&ids=Q1").await;
    assert_eq!(
        got["entities"]["Q1"]["labels"]["en"]["value"],
        "Alpha's first"
    );
    let got = on_beta.api("action=wbgetentities&ids=Q1").await;
    assert_eq!(
        got["entities"]["Q1"]["labels"]["en"]["value"],
        "Beta's first"
    );

    // Attribution carries each tenant's issuer code.
    for t in [&alpha, &beta] {
        let actors: Vec<String> = db
            .query(
                "SELECT DISTINCT actor_key FROM view.activity WHERE tenant = $1 AND kind = 'edit'",
                &[&t.slug],
            )
            .await
            .unwrap()
            .iter()
            .map(|r| r.get(0))
            .collect();
        assert_eq!(actors, [format!("{}:1", t.issuer)], "{}", t.slug);
    }
}
