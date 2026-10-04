//! Credentials, sessions and permissions over Postgres. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use tokio_postgres::{Client, Config, NoTls};
use triplespace_accounts::identity::{Auth, resolve};
use triplespace_accounts::{LoginError, Secret, keys, login, password, session};

const TENANT: &str = "librarybase";
const OWNER: &str = "librarybase:7";
const BOT: &str = "librarybase:12";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn fresh() -> (Client, String) {
    let admin = admin_config().expect("checked by the caller");
    let name = format!("tsaccounts_test_{}", std::process::id());
    let a = connect(&admin).await;
    let _ = a
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await;
    a.batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    let mut config = admin;
    config.dbname(&name);
    let mut client = connect(&config).await;
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .expect("migrate");
    // The actors the accounts refer to, as the projection would write them.
    client
        .batch_execute(&format!(
            "INSERT INTO view.actor (tenant, actor_key, issuer, subject, kind, name, status, iri, \"offset\") VALUES
               ('{TENANT}', '{OWNER}', '{TENANT}', '7', 'registered', 'Alice', 'active', 'x', 1),
               ('{TENANT}', '{BOT}', '{TENANT}', '12', 'bot', 'Alice-bot', 'active', 'x', 2);
             UPDATE view.actor SET operator = '{OWNER}' WHERE actor_key = '{BOT}';
             INSERT INTO view.membership (tenant, actor_key, \"group\", layer, \"offset\") VALUES
               ('{TENANT}', '{OWNER}', 'owner', 'tenant', 1);"
        ))
        .await
        .expect("seed");
    (client, name)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn passwords_keys_sessions_and_permissions() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (client, _name) = fresh().await;
    let secret = Secret::derive(b"test seed");

    // Anonymous.
    let anon = resolve(&client, TENANT, Auth::None).await.unwrap();
    assert!(!anon.identity.is_authenticated());
    assert!(anon.effective.holds("read"));
    assert!(!anon.effective.holds("edit"));

    // The owner's password and clientlogin.
    assert!(password::set(&client, OWNER, "short").await.is_err());
    password::set(&client, OWNER, "correct horse battery staple")
        .await
        .unwrap();
    assert!(password::has(&client, OWNER).await.unwrap());
    assert!(matches!(
        login::password_login(&client, TENANT, "Alice", "wrong", None).await,
        Err(LoginError::Failed)
    ));
    assert!(matches!(
        login::password_login(&client, TENANT, "Alice-bot", "x", None).await,
        Err(LoginError::SubsidiaryPassword(_))
    ));
    let s = login::password_login(
        &client,
        TENANT,
        "alice",
        "correct horse battery staple",
        None,
    )
    .await
    .expect("name is normalized");
    let owner = resolve(&client, TENANT, Auth::Cookie(&s.id)).await.unwrap();
    assert_eq!(owner.identity.actor_key.as_deref(), Some(OWNER));
    assert_eq!(owner.identity.name.as_deref(), Some("Alice"));
    assert!(owner.effective.in_group("owner"));
    assert!(owner.effective.holds("edit") && owner.effective.holds("ts-keys"));
    assert!(owner.identity.credential.is_none());
    let csrf = secret.token("csrf", owner.identity.token_binding().unwrap());
    assert!(secret.check("csrf", &s.id, &csrf));
    assert!(!secret.check("csrf", &s.id, r"+\"));

    // The bot's key.
    assert!(
        keys::issue(&client, BOT, "bad label!", &[], None)
            .await
            .is_err()
    );
    assert!(
        keys::issue(&client, BOT, "laptop", &["nonsense".into()], None)
            .await
            .is_err()
    );
    let issued = keys::issue(&client, BOT, "laptop", &["editentity".into()], None)
        .await
        .unwrap();
    assert!(issued.bearer.starts_with(&format!("{}.", issued.key_id)));
    assert!(
        keys::issue(&client, BOT, "laptop", &[], None)
            .await
            .is_err(),
        "labels are unique per account"
    );
    let listed = keys::list(&client, BOT).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].grants, vec!["basic", "editentity"]);

    // Bearer: stateless, grants intersect the account's permissions.
    let bearer = resolve(&client, TENANT, Auth::Bearer(&issued.bearer))
        .await
        .unwrap();
    assert_eq!(bearer.identity.actor_key.as_deref(), Some(BOT));
    assert_eq!(bearer.identity.operator.as_deref(), Some(OWNER));
    assert!(bearer.effective.holds("read"));
    assert!(
        bearer.effective.holds("edit"),
        "user holds edit; editentity covers it"
    );
    assert!(!bearer.effective.holds("bot"), "no highvolume grant");
    assert!(!bearer.effective.holds("ts-keys"), "a bot is not the owner");
    assert_eq!(
        bearer.identity.token_binding(),
        Some(issued.key_id.as_str())
    );
    let bad = resolve(
        &client,
        TENANT,
        Auth::Bearer(&format!("{}.wrong", issued.key_id)),
    )
    .await
    .unwrap();
    assert!(!bad.identity.is_authenticated());

    // Bot-password login.
    assert!(matches!(
        login::bot_login(&client, TENANT, "Alice@laptop", "x", None).await,
        Err(LoginError::UseOauth(_))
    ));
    assert!(matches!(
        login::bot_login(&client, TENANT, "Alice-bot@laptop", "wrong", None).await,
        Err(LoginError::Failed)
    ));
    assert!(matches!(
        login::bot_login(&client, TENANT, "Alice-bot@desk", &issued.bearer, None).await,
        Err(LoginError::Failed)
    ));
    let (_, secret_part) = issued.bearer.split_once('.').unwrap();
    let anon_session = session::create(&client, TENANT, None, None).await.unwrap();
    let (bot_session, key) = login::bot_login(
        &client,
        TENANT,
        "Alice-bot@laptop",
        secret_part,
        Some(&anon_session.id),
    )
    .await
    .unwrap();
    assert_eq!(key.key_id, issued.key_id);
    assert!(
        session::load(&client, TENANT, &anon_session.id)
            .await
            .unwrap()
            .is_none(),
        "the anonymous session ended with the login"
    );
    let bot = resolve(&client, TENANT, Auth::Cookie(&bot_session.id))
        .await
        .unwrap();
    assert_eq!(bot.identity.actor_key.as_deref(), Some(BOT));
    assert_eq!(
        bot.identity.credential.as_ref().map(|k| k.key_id.as_str()),
        Some(issued.key_id.as_str())
    );
    assert!(bot.effective.holds("edit"));
    assert!(
        resolve(&client, "other", Auth::Cookie(&bot_session.id))
            .await
            .unwrap()
            .identity
            .actor_key
            .is_none(),
        "a session is per tenant"
    );

    // Revocation ends the key and its sessions.
    assert!(keys::revoke(&client, BOT, &issued.key_id).await.unwrap());
    assert!(
        !resolve(&client, TENANT, Auth::Bearer(&issued.bearer))
            .await
            .unwrap()
            .identity
            .is_authenticated()
    );
    assert!(
        !resolve(&client, TENANT, Auth::Cookie(&bot_session.id))
            .await
            .unwrap()
            .identity
            .is_authenticated()
    );
}
