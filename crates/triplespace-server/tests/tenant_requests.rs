//! Creating a tenant by request (ADR 0111 §1, §2, §3, §4, §5, §6, §8): requests filed in
//! the instance `log` by a holder of `ts-createtenant` (§8),
//! carried out by the provisioner running as `ts_provision`, which may not create a record
//! table itself and does so through `log.create_record_table` (§3); the four checks and
//! their refusals (§4); a new owner bound to an existing login (§5); and the records each
//! writes. Needs `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use tokio_postgres::{Config, NoTls};
use triplespace_cli::provision::{Ask, Outcome, Provisioner, file, instance_log, pending};
use triplespace_cli::{common, instance, tenant};

const DB: &str = "tsrequests_test";
const ALPHA: &str = "alpha.example";
const BETA: &str = "beta.example";
const AUTH: &str = "auth.example";

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

/// `url` with the session's role set to `role` at connection, as a login granted that role
/// would have it: row-level security and the role's grants apply, not the superuser's.
fn as_role(url: &str, role: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}options=-c%20role%3D{role}")
}

fn ask(slug: &str, host: &str, source: Option<&str>) -> Ask {
    Ask {
        slug: slug.into(),
        base: format!("https://{host}"),
        name: Some(format!("Wiki {slug}")),
        owner_name: "Owner".into(),
        owner_login_source: source.map(str::to_owned),
        adopt: None,
        owner_id: None,
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn tenants_by_request() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsrequests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key_file = dir.join("triplespace.key");
    let _ = std::fs::remove_file(&key_file);
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
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
        // The password gives alpha's owner a login, which a request may name (§5).
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");

    let db = common::connect(&database).await.unwrap();
    let farm = common::farm_of(&db).await.unwrap();
    let alpha_issuer = common::issuer_of(&db, "alpha").await.unwrap();
    let alpha_owner = format!("{alpha_issuer}:1");
    let key = tenant::current_key(&db, &key_file).await.unwrap();

    // The test's own login acts as the two roles, as a deployment's logins granted them
    // would. A superuser needs no grant; a `CREATEROLE` user, which made the roles, may give
    // itself them.
    db.batch_execute("GRANT ts_provision, ts_accounts TO CURRENT_USER")
        .await
        .unwrap();

    // 0111 §3: `ts_provision` may not create a partition of `log.record` itself.
    let prov_url = as_role(&database, "ts_provision");
    let prov = common::connect(&prov_url).await.unwrap();
    let role: String = prov
        .query_one("SELECT current_user::text", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(role, "ts_provision");
    let err = prov
        .batch_execute(
            "CREATE TABLE log.record_00000000deadbeef PARTITION OF log.record FOR VALUES IN (3735928559)",
        )
        .await
        .unwrap_err();
    // Refused for want of privilege: no `CREATE` on schema `log`, and not the owner of
    // `log.record`.
    assert_eq!(
        err.code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE),
        "{err:?}"
    );

    let provisioner = Provisioner {
        database: prov_url.clone(),
        accounts_database: as_role(&database, "ts_accounts"),
        key: key.clone(),
        auth_base: Some(format!("https://{AUTH}")),
    };

    // 0111 §2: a request is a log event in the instance `log`, keyed by the slug, and it
    // is pending until the provisioner answers it.
    let beta = file(
        &db,
        &alpha_owner,
        &ask("beta", BETA, Some(&alpha_owner)),
        None,
    )
    .await
    .unwrap();
    let ilog = instance_log(&db).await.unwrap();
    assert_eq!(beta.partition, ilog);
    assert_eq!(pending(&db).await.unwrap().len(), 1);

    // 0111 §4: carried out by the provisioner as `ts_provision`.
    let done = provisioner.pass().await.unwrap();
    assert_eq!(done.len(), 1);
    let Outcome::Created(created) = &done[0].1 else {
        panic!("{done:?}");
    };
    assert_eq!(created.entry.slug, "beta");
    assert!(pending(&db).await.unwrap().is_empty());

    // The `tenant:` record is the instance's, citing the request by its offset.
    let row = db
        .query_one(
            "SELECT actor, instance_act, config->>'request' FROM view.registry
             WHERE tenant = '' AND kind = 'tenant' AND code = 'beta'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        row.get::<_, Option<String>>(0),
        Some(farm.operator().to_string())
    );
    assert!(row.get::<_, bool>(1));
    assert_eq!(
        row.get::<_, Option<String>>(2),
        Some(beta.offset.to_string())
    );

    // 0111 §3: every partition of beta has the record table `create_partition` would make:
    // its autovacuum options, its key index, and the grants, the accounts partition hidden
    // from the public roles.
    let tables = db
        .query(
            "SELECT p.name, c.oid::regclass::text, c.reloptions::text[]
             FROM log.partition p
             JOIN pg_class c ON c.relname = 'record_' || lpad(to_hex(p.partition), 16, '0')
             WHERE p.tenant = 'beta'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(tables.len(), 6, "config and the five source partitions");
    for t in &tables {
        let name: String = t.get(0);
        let table: String = t.get(1);
        let options: Vec<String> = t.get(2);
        for o in scatter_log_postgres::log::RECORD_TABLE_OPTIONS.split(", ") {
            let o = o.replace(' ', "");
            assert!(options.contains(&o), "{table}: {options:?}");
        }
        let indexes: i64 = db
            .query_one(
                "SELECT count(*) FROM pg_index WHERE indrelid = $1::text::regclass",
                &[&table],
            )
            .await
            .unwrap()
            .get(0);
        assert!(indexes >= 1, "{table}");
        let server_reads: bool = db
            .query_one(
                "SELECT has_table_privilege('ts_server', $1::text, 'SELECT')",
                &[&table],
            )
            .await
            .unwrap()
            .get(0);
        let accounts_reads: bool = db
            .query_one(
                "SELECT has_table_privilege('ts_accounts', $1::text, 'SELECT')",
                &[&table],
            )
            .await
            .unwrap()
            .get(0);
        assert!(accounts_reads, "{table}");
        assert_eq!(server_reads, name != "accounts", "{name}: {table}");
    }

    // 0111 §5: beta's owner is bound to the login of alpha's owner.
    let beta_issuer = common::issuer_of(&db, "beta").await.unwrap();
    let alpha_login = triplespace_accounts::logins::login_of(&db, "alpha", &alpha_owner)
        .await
        .unwrap();
    assert!(alpha_login.is_some());
    assert_eq!(
        triplespace_accounts::logins::login_of(&db, "beta", &format!("{beta_issuer}:1"))
            .await
            .unwrap(),
        alpha_login
    );

    // 0111 §4: the refusals, each a `tenant/refuse` naming the request, attested by the
    // instance.
    let refused = [
        // The slug is taken.
        (
            alpha_owner.clone(),
            ask("beta", "beta2.example", None),
            "ts-tenant-invalid",
        ),
        // The filer holds no `ts-createtenant` on the primary tenant.
        (
            format!("{alpha_issuer}:99"),
            ask("gamma", "gamma.example", None),
            "ts-not-entitled",
        ),
        // The owner login source has no login.
        (
            alpha_owner.clone(),
            ask("delta", "delta.example", Some(&format!("{beta_issuer}:7"))),
            "ts-owner-no-login",
        ),
        // The host is the auth base's.
        (
            alpha_owner.clone(),
            ask("epsilon", AUTH, None),
            "ts-tenant-invalid",
        ),
    ];
    for (filer, a, _) in &refused {
        file(&db, filer, a, Some("a test")).await.unwrap();
    }
    let done = provisioner.pass().await.unwrap();
    assert_eq!(done.len(), refused.len(), "{done:?}");
    for ((slug, outcome), (_, a, code)) in done.iter().zip(&refused) {
        assert_eq!(slug, &a.slug);
        match outcome {
            Outcome::Refused(c, _) => assert_eq!(c, code, "{slug}"),
            other => panic!("{slug}: {other:?}"),
        }
    }
    assert!(pending(&db).await.unwrap().is_empty());
    let refusals = db
        .query(
            "SELECT \"offset\" FROM log.record WHERE partition = $1 AND key = 'tenant:gamma' ORDER BY \"offset\"",
            &[&scatter_log_postgres::ids::partition_to_db(ilog)],
        )
        .await
        .unwrap();
    assert_eq!(refusals.len(), 2, "the request and its refusal");

    // A third tenant with no auth base configured is refused (0089 §1).
    let no_auth = Provisioner {
        auth_base: None,
        ..provisioner.clone()
    };
    file(&db, &alpha_owner, &ask("zeta", "zeta.example", None), None)
        .await
        .unwrap();
    let done = no_auth.pass().await.unwrap();
    assert!(
        matches!(&done[0].1, Outcome::Refused(c, _) if c == "ts-auth-base-required"),
        "{done:?}"
    );

    // 0111 §6: `tenant create` files a request and carries it out at once.
    tenant::run(tenant::TenantCommand::Create(tenant::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: "eta".into(),
        base: "https://eta.example".into(),
        name: Some("Eta".into()),
        adopt: None,
        owner: None,
        owner_name: "Owner".into(),
        owner_password_file: None,
        operator: None,
        auth_base: Some(format!("https://{AUTH}")),
        owner_login_of: Some(alpha_owner.clone()),
    }))
    .await
    .expect("tenant create");
    let eta_request: Option<String> = db
        .query_one(
            "SELECT config->>'request' FROM view.registry WHERE tenant = '' AND kind = 'tenant' AND code = 'eta'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(eta_request.is_some());
    assert!(pending(&db).await.unwrap().is_empty());
}
