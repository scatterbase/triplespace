//! `instance setup` and `tenant setup`: `instance create` and `tenant create` by question
//! and answer, for an operator at a terminal (0018 A36).
//!
//! Each asks what its command's flags would say, checks every answer as it is given with
//! the checks the command itself makes (slugs and the shared namespace, bases and the hosts
//! already taken, the wiki's name, the account name, the instance's current key), shows a
//! summary with the equivalent command line, and creates nothing until the operator
//! confirms. Then it runs the same code as the command: `instance setup` is
//! [`instance::create_with`], and `tenant setup` is [`tenant::create_with`].
//!
//! `instance setup` asks about the instance first and then about its first tenant, the
//! primary tenant, which `instance create` creates with it; afterwards it offers to go on
//! to `tenant setup` for more tenants. `tenant setup` runs alone against an existing
//! instance, and offers to create another tenant after each one.
//!
//! The questions are asked through [`Prompt`]: [`Terminal`] at a terminal, [`Scripted`]
//! from a list of answers in tests.

use std::collections::VecDeque;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::common::{connect, farm_of};
use crate::instance;
use crate::tenant::{self, Spec, check_base, check_name, host_of};

/// Asks the operator questions, one at a time.
pub trait Prompt {
    /// A line of text; `default` is taken when the answer is empty.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be read, or a script runs out.
    fn line(&mut self, prompt: &str, default: Option<&str>) -> Result<String>;
    /// A secret, not echoed; may be empty.
    ///
    /// # Errors
    ///
    /// As [`Prompt::line`].
    fn secret(&mut self, prompt: &str) -> Result<String>;
    /// Yes or no.
    ///
    /// # Errors
    ///
    /// As [`Prompt::line`].
    fn yes(&mut self, prompt: &str, default: bool) -> Result<bool>;
    /// One of `items`, by index.
    ///
    /// # Errors
    ///
    /// As [`Prompt::line`].
    fn choose(&mut self, prompt: &str, items: &[String], default: usize) -> Result<usize>;
    /// Any number of `items`, by index, none checked at first.
    ///
    /// # Errors
    ///
    /// As [`Prompt::line`].
    fn choose_many(&mut self, prompt: &str, items: &[String]) -> Result<Vec<usize>>;
    /// Tells the operator something.
    fn say(&mut self, text: &str);
    /// Tells the operator an answer was refused, and why.
    fn warn(&mut self, text: &str);
}

/// Asks until `check` accepts the answer, saying why each refused one was refused.
fn ask<T>(
    p: &mut dyn Prompt,
    prompt: &str,
    default: Option<&str>,
    check: impl Fn(&str) -> Result<T>,
) -> Result<T> {
    loop {
        let answer = p.line(prompt, default)?;
        match check(answer.trim()) {
            Ok(v) => return Ok(v),
            Err(e) => p.warn(&format!("{e:#}")),
        }
    }
}

/// A base as an operator may type it: `example.wiki` is taken as `https://example.wiki`,
/// and a trailing `/` is dropped. Then checked as `--base` is (0018 §1).
pub fn normalize_base(answer: &str) -> Result<String> {
    let mut base = answer.trim().to_string();
    if base.is_empty() {
        bail!("a base is required");
    }
    if !base.contains("://") {
        base = format!("https://{base}");
    }
    while base.ends_with('/') {
        base.pop();
    }
    check_base(&base)?;
    Ok(base)
}

/// What a new tenant may not take: slugs and hosts already in use on the instance.
#[derive(Debug, Clone, Default)]
pub struct Taken {
    /// Tenant slugs and the farm slug.
    pub slugs: Vec<String>,
    /// Hosts of tenants, the farm base and the auth base, each with why it is taken.
    pub hosts: Vec<(String, String)>,
}

impl Taken {
    fn host(&mut self, base: &str, what: String) {
        if let Some(h) = host_of(base) {
            self.hosts.push((h, what));
        }
    }
}

/// The answers about one tenant: what `tenant create` or the tenant half of `instance
/// create` takes.
#[derive(Debug, Clone)]
pub struct TenantAnswers {
    /// The tenant.
    pub spec: Spec,
    /// The owner's password; `None` to set it later.
    pub password: Option<String>,
}

/// Asks about a tenant, the primary tenant when `primary`.
///
/// # Errors
///
/// When the prompt fails.
pub fn ask_tenant(p: &mut dyn Prompt, taken: &Taken, primary: bool) -> Result<TenantAnswers> {
    let kinds = [
        "A new, empty wiki".to_string(),
        "An existing Wikibase or MediaWiki, adopted from its XML dump (0035)".to_string(),
    ];
    let adopt = if p.choose("What is this wiki?", &kinds, 0)? == 1 {
        p.say(
            "Adoption makes the source wiki become this tenant, keeping its IDs, users and \
             history. Freeze the source first; `triplespace adopt` loads its dump afterwards.",
        );
        Some(ask(p, "The source wiki's base URL", None, |a| {
            let url = url::Url::parse(a).with_context(|| format!("`{a}` is not a URL"))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                bail!("`{a}` is not an http or https URL");
            }
            Ok(a.to_string())
        })?)
    } else {
        None
    };

    p.say(
        "The slug names the wiki on this instance, in the command line and the API: \
         lowercase letters, digits and hyphens. It cannot be changed later.",
    );
    let slug = ask(p, "Slug", None, |s| {
        check_name(s, adopt.as_deref())?;
        if taken.slugs.iter().any(|t| t == s) {
            bail!("`{s}` is already taken on this instance");
        }
        Ok(s.to_string())
    })?;

    p.say(
        "The base URL is where the wiki is served and the root of its entity IRIs, such as \
         `https://example.wiki`: a scheme and a host, no path.",
    );
    let base = ask(p, "Base URL", None, |a| {
        let base = normalize_base(a)?;
        let host = host_of(&base).context("base host")?;
        if let Some((_, what)) = taken.hosts.iter().find(|(h, _)| *h == host) {
            bail!("`{host}` is already {what}");
        }
        Ok(base)
    })?;

    let name = ask(p, "The wiki's name, shown on its pages", Some(&slug), |n| {
        triplespace_accounts::settings::plan::check_site_name(n)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(n.to_string())
    })?;
    let name = (name != slug).then_some(name);

    let owner = if adopt.is_some() {
        p.say("The owner keeps their account from the source wiki, under its user ID there.");
        Some(ask(
            p,
            "The owner's user ID on the source wiki",
            None,
            |a| match a.parse::<u64>() {
                Ok(n) if n > 0 => Ok(n),
                _ => bail!("a user ID is a positive number"),
            },
        )?)
    } else {
        None
    };
    if primary {
        p.say(
            "The owner is the wiki's first account and a member of `owner`, which also \
             operates the instance.",
        );
    } else {
        p.say("The owner is the wiki's first account and a member of its `owner` group.");
    }
    let owner_name = ask(p, "The owner's account name", Some("Owner"), |n| {
        scatter_actors::actor::check_name(n)
            .map_err(|e| anyhow::anyhow!("`{n}` cannot be an account name: {e}"))?;
        Ok(n.to_string())
    })?;
    let password = ask_password(p)?;

    Ok(TenantAnswers {
        spec: Spec {
            slug,
            base,
            name,
            adopt,
            owner,
            owner_name,
        },
        password,
    })
}

/// The owner's password, typed twice; `None` when left empty.
fn ask_password(p: &mut dyn Prompt) -> Result<Option<String>> {
    loop {
        let first = p.secret("The owner's password (leave empty to set it later)")?;
        if first.is_empty() {
            p.say("The owner will have no password until `triplespace password set` gives one.");
            return Ok(None);
        }
        if p.secret("The password again")? == first {
            return Ok(Some(first));
        }
        p.warn("The passwords differ; again.");
    }
}

/// The answers of `instance setup`.
#[derive(Debug, Clone)]
pub struct InstanceAnswers {
    /// The farm slug.
    pub farm_slug: String,
    /// The farm base, `None` for the primary tenant's.
    pub farm_base: Option<String>,
    /// The providers to open mirror partitions for, by slug.
    pub providers: Vec<String>,
    /// The primary tenant.
    pub tenant: TenantAnswers,
}

/// Asks about the instance and then its primary tenant.
///
/// # Errors
///
/// When the prompt fails.
pub fn ask_instance(p: &mut dyn Prompt) -> Result<InstanceAnswers> {
    p.say(
        "The farm slug is the instance's own name. It differs from every wiki's slug and \
         never changes.",
    );
    let farm_slug = ask(p, "Farm slug", None, |s| {
        check_name(s, None)?;
        Ok(s.to_string())
    })?;

    p.say(
        "Mirrors hold a copy of another wiki's data, such as Wikidata's, which every wiki \
         here can use. Choose every provider you may ever mirror: they cannot be added \
         after the instance is created.",
    );
    let registry = scatter_providers::Registry::default_registry();
    let providers = registry.providers();
    let items: Vec<String> = providers
        .iter()
        .map(|pr| format!("{} ({})", pr.slug, pr.name))
        .collect();
    let providers: Vec<String> = p
        .choose_many(
            "Providers to mirror (space to select, enter to finish)",
            &items,
        )?
        .into_iter()
        .map(|i| providers[i].slug.clone())
        .collect();
    p.say("Tenancy: `isolated`, the only preset this version supports (0028 §1).");

    p.say("");
    p.say("Now the first wiki, the primary tenant: its accounts operate the instance.");
    let taken = Taken {
        slugs: vec![farm_slug.clone()],
        hosts: Vec::new(),
    };
    let tenant = ask_tenant(p, &taken, true)?;

    p.say("");
    p.say(
        "The farm base is where the instance answers for every wiki: its mirrors and its \
         own pages. One wiki's instance can share that wiki's base.",
    );
    let primary_host = host_of(&tenant.spec.base);
    let farm_base = ask(p, "Farm base URL", Some(&tenant.spec.base), |a| {
        let base = normalize_base(a)?;
        if base != tenant.spec.base && host_of(&base) == primary_host {
            bail!("the farm base is either the primary wiki's base or a host of its own");
        }
        Ok(base)
    })?;
    let farm_base = (farm_base != tenant.spec.base).then_some(farm_base);

    Ok(InstanceAnswers {
        farm_slug,
        farm_base,
        providers,
        tenant,
    })
}

/// A word quoted for a POSIX shell when it needs to be.
fn shell_quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=@,+%".contains(&b))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// The flags of a tenant's answers, as `instance create` and `tenant create` share them.
fn tenant_flags(t: &TenantAnswers, flags: &mut Vec<String>) {
    let s = &t.spec;
    flags.extend([
        "--tenant".into(),
        s.slug.clone(),
        "--base".into(),
        s.base.clone(),
    ]);
    if let Some(n) = &s.name {
        flags.extend(["--name".into(), n.clone()]);
    }
    if let Some(a) = &s.adopt {
        flags.extend(["--adopt".into(), a.clone()]);
    }
    if let Some(o) = s.owner {
        flags.extend(["--owner".into(), o.to_string()]);
    }
    flags.extend(["--owner-name".into(), s.owner_name.clone()]);
    if t.password.is_some() {
        flags.extend(["--owner-password-file".into(), "<file>".into()]);
    }
}

/// Joins a command line, quoting words, one flag and its value to a line.
fn command_line(command: &str, flags: &[String]) -> String {
    let mut out = command.to_string();
    for word in flags {
        if word.starts_with("--") {
            out.push_str(" \\\n    ");
        } else {
            out.push(' ');
        }
        if word == "<file>" {
            out.push_str(word);
        } else {
            out.push_str(&shell_quote(word));
        }
    }
    out
}

/// The `instance create` command the answers amount to, run with the same
/// `TRIPLESPACE_DATABASE_URL`.
#[must_use]
pub fn instance_command(a: &InstanceAnswers, key_file: &Path) -> String {
    let mut flags = vec!["--farm-slug".to_string(), a.farm_slug.clone()];
    if let Some(b) = &a.farm_base {
        flags.extend(["--farm-base".into(), b.clone()]);
    }
    if key_file != Path::new(DEFAULT_KEY_FILE) {
        flags.extend(["--key-file".into(), key_file.display().to_string()]);
    }
    for pr in &a.providers {
        flags.extend(["--provider".into(), pr.clone()]);
    }
    tenant_flags(&a.tenant, &mut flags);
    command_line("triplespace instance create", &flags)
}

/// The `tenant create` command the answers amount to.
#[must_use]
pub fn tenant_command(
    t: &TenantAnswers,
    key_file: &Path,
    auth_base: &str,
    operator: Option<u64>,
) -> String {
    let mut flags = Vec::new();
    tenant_flags(t, &mut flags);
    if key_file != Path::new(DEFAULT_KEY_FILE) {
        flags.extend(["--key-file".into(), key_file.display().to_string()]);
    }
    flags.extend(["--auth-base".into(), auth_base.to_string()]);
    if let Some(o) = operator {
        flags.extend(["--operator".into(), o.to_string()]);
    }
    command_line("triplespace tenant create", &flags)
}

/// Tells the operator what a tenant's answers are.
fn say_tenant(p: &mut dyn Prompt, t: &TenantAnswers) {
    let s = &t.spec;
    p.say(&format!("  wiki          {} ({})", s.slug, s.base));
    p.say(&format!(
        "  name          {}",
        s.name.as_deref().unwrap_or(&s.slug)
    ));
    if let Some(a) = &s.adopt {
        p.say(&format!("  adopts        {a}"));
    }
    let id = s
        .owner
        .map(|o| format!(", user ID {o}"))
        .unwrap_or_default();
    p.say(&format!("  owner         {}{id}", s.owner_name));
    p.say(&format!(
        "  password      {}",
        if t.password.is_some() {
            "set now"
        } else {
            "none yet"
        }
    ));
}

/// Tells the operator what the instance's answers are.
fn say_instance(p: &mut dyn Prompt, a: &InstanceAnswers, key_file: &Path) {
    p.say("The instance:");
    p.say(&format!("  farm          {}", a.farm_slug));
    p.say(&format!(
        "  farm base     {}",
        a.farm_base.as_deref().unwrap_or("the primary wiki's base")
    ));
    p.say(&format!(
        "  mirrors       {}",
        if a.providers.is_empty() {
            "none".to_string()
        } else {
            a.providers.join(", ")
        }
    ));
    p.say(&format!("  key file      {}", key_file.display()));
    p.say("Its primary wiki:");
    say_tenant(p, &a.tenant);
    p.say("");
    p.say("The same, as a command (with TRIPLESPACE_DATABASE_URL set):");
    p.say(&instance_command(a, key_file));
}

/// The key file `instance create` uses when none is named.
const DEFAULT_KEY_FILE: &str = "triplespace.key";

/// Refuses to ask questions where no one can answer them.
fn require_terminal() -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!(
            "setup asks questions and needs a terminal; without one, use `instance create` \
             or `tenant create` with flags"
        );
    }
    Ok(())
}

/// `instance setup`.
#[derive(Debug, Args)]
pub struct InstanceSetup {
    /// The Postgres URL, offered as the first answer.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: Option<String>,
    /// Where the instance signing key will live, offered as the answer.
    #[arg(long, default_value = DEFAULT_KEY_FILE)]
    pub key_file: PathBuf,
}

/// `tenant setup`.
#[derive(Debug, Args)]
pub struct TenantSetup {
    /// The Postgres URL, offered as the first answer.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: Option<String>,
    /// The instance signing key, offered as the answer.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = DEFAULT_KEY_FILE)]
    pub key_file: PathBuf,
    /// The auth host, `server.auth_base` (0089 §1), offered as the answer.
    #[arg(long, env = "TRIPLESPACE_AUTH_BASE")]
    pub auth_base: Option<String>,
}

/// Runs `instance setup` at the terminal.
///
/// # Errors
///
/// When there is no terminal, or the creation fails.
pub async fn run_instance(args: InstanceSetup) -> Result<()> {
    require_terminal()?;
    instance_setup(&mut Terminal::new(), args).await
}

/// Runs `tenant setup` at the terminal.
///
/// # Errors
///
/// As [`run_instance`].
pub async fn run_tenant(args: TenantSetup) -> Result<()> {
    require_terminal()?;
    tenant_setup(&mut Terminal::new(), args).await
}

/// Where the instance is to be, or is: asked until the database answers and holds an
/// instance or not, as `want_instance` says. `None` when `instance setup` finds an
/// instance and the operator goes on to add a tenant to it instead.
async fn ask_database(
    p: &mut dyn Prompt,
    default: Option<&str>,
    want_instance: bool,
) -> Result<Option<String>> {
    loop {
        let url = p.line("Postgres URL", default)?;
        let url = url.trim().to_string();
        if url.is_empty() {
            p.warn("a URL is required, `postgres://user@host/database`");
            continue;
        }
        let holds = match instance::exists(&url).await {
            Ok(h) => h,
            Err(e) => {
                p.warn(&format!("cannot use that database: {e:#}"));
                continue;
            }
        };
        match (holds, want_instance) {
            (false, false) | (true, true) => return Ok(Some(url)),
            (true, false) => {
                p.warn("this database already holds an instance");
                if p.yes("Add a wiki to it instead?", true)? {
                    return Ok(None);
                }
            }
            (false, true) => {
                p.warn("this database holds no instance; `triplespace instance setup` creates one");
            }
        }
    }
}

/// `instance setup`, asking through `p`.
///
/// # Errors
///
/// When the prompt fails, the operator declines, or the creation fails.
pub async fn instance_setup(p: &mut dyn Prompt, args: InstanceSetup) -> Result<()> {
    p.say("This creates a Triplespace instance and its first wiki. Nothing is written until you confirm.");
    p.say("");
    let Some(database) = ask_database(p, args.database.as_deref(), false).await? else {
        return tenant_setup(
            p,
            TenantSetup {
                database: args.database,
                key_file: args.key_file,
                auth_base: None,
            },
        )
        .await;
    };

    let default_key = args.key_file.display().to_string();
    let key_file = loop {
        let path = PathBuf::from(p.line("Instance key file", Some(&default_key))?.trim());
        if path.as_os_str().is_empty() {
            continue;
        }
        if path.exists() {
            if p.yes(
                &format!("{} exists; make it this instance's key?", path.display()),
                false,
            )? {
                break path;
            }
        } else {
            p.say(&format!(
                "{} will be created (mode 0600). It signs the instance's log: keep it safe and \
                 back it up.",
                path.display()
            ));
            break path;
        }
    };
    p.say("");

    let answers = ask_instance(p)?;

    p.say("");
    say_instance(p, &answers, &key_file);
    p.say("");
    if !p.yes("Create the instance?", true)? {
        bail!("nothing was created");
    }

    let InstanceAnswers {
        farm_slug,
        farm_base,
        providers,
        tenant: TenantAnswers { spec, password },
    } = answers;
    instance::create_with(
        instance::Create {
            database: database.clone(),
            key_file: key_file.clone(),
            farm_slug,
            farm_base,
            tenancy: "isolated".into(),
            tenant: spec.slug,
            base: spec.base,
            name: spec.name,
            providers,
            adopt: spec.adopt,
            owner: spec.owner,
            owner_name: spec.owner_name,
            owner_password_file: None,
        },
        password.as_deref(),
    )
    .await?;

    p.say("");
    if p.yes("Create another wiki on this instance now?", false)? {
        tenant_setup(
            p,
            TenantSetup {
                database: Some(database),
                key_file,
                auth_base: None,
            },
        )
        .await?;
    } else {
        p.say("More wikis can be added later with `triplespace tenant setup`.");
    }
    Ok(())
}

/// `tenant setup`, asking through `p`; offers another after each tenant.
///
/// # Errors
///
/// When the prompt fails, the operator declines, or the creation fails.
#[allow(clippy::too_many_lines)]
pub async fn tenant_setup(p: &mut dyn Prompt, args: TenantSetup) -> Result<()> {
    p.say("This adds a wiki to an existing Triplespace instance. Nothing is written until you confirm.");
    p.say("");
    let database = ask_database(p, args.database.as_deref(), true)
        .await?
        .context("no database")?;
    let client = connect(&database).await?;

    let default_key = args.key_file.display().to_string();
    let key_file = loop {
        let path = PathBuf::from(p.line("Instance key file", Some(&default_key))?.trim());
        match tenant::current_key(&client, &path).await {
            Ok(_) => break path,
            Err(e) => p.warn(&format!("{e:#}")),
        }
    };

    let mut auth_base = args.auth_base.filter(|a| !a.is_empty());
    loop {
        let farm = farm_of(&client).await?;
        let tenants = triplespace_projections::tenants::tenants(&client).await?;
        let (primary, _, owners) = tenant::primary_owners(&client).await?;
        p.say("");
        p.say(&format!("Instance `{}` ({}), with:", farm.slug, farm.base));
        for t in &tenants {
            let mark = if t.slug == primary { "  (primary)" } else { "" };
            p.say(&format!("  {:<20} {}{mark}", t.slug, t.base));
        }

        // Who creates it: an `owner` of the primary tenant (0046 §1).
        let operator = match owners.as_slice() {
            [] => bail!("the primary tenant `{primary}` has no `owner` to create a tenant"),
            [(id, key)] => {
                p.say(&format!("Created by {key}, user {id} of `{primary}`."));
                None
            }
            many => {
                let items: Vec<String> = many
                    .iter()
                    .map(|(id, key)| format!("user {id} of `{primary}` ({key})"))
                    .collect();
                let i = p.choose("Which owner is creating it?", &items, 0)?;
                (i > 0).then_some(many[i].0)
            }
        };

        let mut taken = Taken {
            slugs: tenants.iter().map(|t| t.slug.clone()).collect(),
            hosts: Vec::new(),
        };
        taken.slugs.push(farm.slug.clone());
        for t in &tenants {
            taken.host(&t.base, format!("the host of `{}`", t.slug));
        }
        taken.host(&farm.base, "the farm base's host".into());

        // An instance with more than one tenant needs the auth host (0089 §1).
        p.say("");
        p.say(
            "An instance with more than one wiki signs people in at its auth host, a host of \
             its own. The server must be given the same one (TRIPLESPACE_AUTH_BASE).",
        );
        let auth = ask(p, "Auth base URL", auth_base.as_deref(), |a| {
            let base = normalize_base(a)?;
            let host = host_of(&base).context("auth host")?;
            if let Some((_, what)) = taken.hosts.iter().find(|(h, _)| *h == host) {
                bail!("`{host}` is {what}; the auth host is a host of its own");
            }
            Ok(base)
        })?;
        taken.host(&auth, "the auth base's host".into());
        auth_base = Some(auth.clone());
        p.say("");

        let answers = ask_tenant(p, &taken, false)?;
        p.say("");
        p.say("The new wiki:");
        say_tenant(p, &answers);
        p.say(&format!("  auth base     {auth}"));
        p.say("");
        p.say("The same, as a command (with TRIPLESPACE_DATABASE_URL set):");
        p.say(&tenant_command(&answers, &key_file, &auth, operator));
        p.say("");
        if !p.yes("Create the wiki?", true)? {
            bail!("nothing was created");
        }
        let TenantAnswers { spec, password } = answers;
        tenant::create_with(
            tenant::Create {
                database: database.clone(),
                key_file: key_file.clone(),
                tenant: spec.slug,
                base: spec.base,
                name: spec.name,
                adopt: spec.adopt,
                owner: spec.owner,
                owner_name: spec.owner_name,
                owner_password_file: None,
                operator,
                auth_base: Some(auth.clone()),
            },
            password.as_deref(),
        )
        .await?;
        p.say("");
        p.say(&format!(
            "Start the server with TRIPLESPACE_AUTH_BASE={auth}, which it needs from now on."
        ));
        if !p.yes("Create another wiki?", false)? {
            return Ok(());
        }
    }
}

/// The questions asked at a terminal.
pub struct Terminal {
    theme: dialoguer::theme::ColorfulTheme,
}

impl Terminal {
    /// A prompt on the process's terminal.
    #[must_use]
    pub fn new() -> Self {
        Self {
            theme: dialoguer::theme::ColorfulTheme::default(),
        }
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Prompt for Terminal {
    fn line(&mut self, prompt: &str, default: Option<&str>) -> Result<String> {
        let mut input = dialoguer::Input::<String>::with_theme(&self.theme)
            .with_prompt(prompt)
            .allow_empty(true);
        if let Some(d) = default {
            input = input.default(d.to_string());
        }
        Ok(input.interact_text()?)
    }

    fn secret(&mut self, prompt: &str) -> Result<String> {
        Ok(dialoguer::Password::with_theme(&self.theme)
            .with_prompt(prompt)
            .allow_empty_password(true)
            .interact()?)
    }

    fn yes(&mut self, prompt: &str, default: bool) -> Result<bool> {
        Ok(dialoguer::Confirm::with_theme(&self.theme)
            .with_prompt(prompt)
            .default(default)
            .interact()?)
    }

    fn choose(&mut self, prompt: &str, items: &[String], default: usize) -> Result<usize> {
        Ok(dialoguer::Select::with_theme(&self.theme)
            .with_prompt(prompt)
            .items(items)
            .default(default)
            .interact()?)
    }

    fn choose_many(&mut self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
        Ok(dialoguer::MultiSelect::with_theme(&self.theme)
            .with_prompt(prompt)
            .items(items)
            .interact()?)
    }

    fn say(&mut self, text: &str) {
        println!("{text}");
    }

    fn warn(&mut self, text: &str) {
        eprintln!("  ! {text}");
    }
}

/// Answers from a list, for tests: each question takes the next answer. A line's empty
/// answer takes the default; a choice is answered by the start of an item's text, and a
/// multiple choice by such starts separated by commas; yes or no by `y` or `n`, or empty
/// for the default. Everything asked and said is kept in [`Scripted::transcript`].
#[derive(Debug, Default)]
pub struct Scripted {
    answers: VecDeque<String>,
    /// Every question with its answer, every remark and every warning, in order.
    pub transcript: Vec<String>,
}

impl Scripted {
    /// A script of `answers`.
    #[must_use]
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(answers: I) -> Self {
        Self {
            answers: answers.into_iter().map(Into::into).collect(),
            transcript: Vec::new(),
        }
    }

    fn next(&mut self, prompt: &str) -> Result<String> {
        let answer = self
            .answers
            .pop_front()
            .with_context(|| format!("the script has no answer for `{prompt}`"))?;
        self.transcript.push(format!("? {prompt}: {answer}"));
        Ok(answer)
    }

    /// The warnings given, in order.
    #[must_use]
    pub fn warnings(&self) -> Vec<&str> {
        self.transcript
            .iter()
            .filter_map(|l| l.strip_prefix("! "))
            .collect()
    }

    /// Whether every answer was used.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.answers.is_empty()
    }
}

impl Prompt for Scripted {
    fn line(&mut self, prompt: &str, default: Option<&str>) -> Result<String> {
        let a = self.next(prompt)?;
        Ok(match (a.is_empty(), default) {
            (true, Some(d)) => d.to_string(),
            _ => a,
        })
    }

    fn secret(&mut self, prompt: &str) -> Result<String> {
        self.next(prompt)
    }

    fn yes(&mut self, prompt: &str, default: bool) -> Result<bool> {
        match self.next(prompt)?.as_str() {
            "" => Ok(default),
            "y" => Ok(true),
            "n" => Ok(false),
            other => bail!("`{other}` is not y or n, for `{prompt}`"),
        }
    }

    fn choose(&mut self, prompt: &str, items: &[String], default: usize) -> Result<usize> {
        let a = self.next(prompt)?;
        if a.is_empty() {
            return Ok(default);
        }
        items
            .iter()
            .position(|i| i.starts_with(&a))
            .with_context(|| format!("no item of `{prompt}` starts `{a}`"))
    }

    fn choose_many(&mut self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
        let a = self.next(prompt)?;
        a.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                items
                    .iter()
                    .position(|i| i.starts_with(s))
                    .with_context(|| format!("no item of `{prompt}` starts `{s}`"))
            })
            .collect()
    }

    fn say(&mut self, text: &str) {
        self.transcript.push(text.to_string());
    }

    fn warn(&mut self, text: &str) {
        self.transcript.push(format!("! {text}"));
    }
}

#[cfg(test)]
mod tests {
    //! The questions of 0018 A36, without a database.

    use super::*;

    #[test]
    fn bases_are_completed_and_checked() {
        assert_eq!(
            normalize_base("example.wiki").unwrap(),
            "https://example.wiki"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/").unwrap(),
            "http://localhost:8080"
        );
        assert!(normalize_base("https://example.wiki/w").is_err());
        assert!(normalize_base("ftp://example.wiki").is_err());
        assert!(normalize_base("").is_err());
    }

    #[test]
    fn shell_words_are_quoted_when_needed() {
        assert_eq!(shell_quote("https://a.example"), "https://a.example");
        assert_eq!(shell_quote("My wiki"), "'My wiki'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn a_tenant_is_asked_again_until_each_answer_holds() {
        let taken = Taken {
            slugs: vec!["myfarm".into()],
            hosts: vec![("taken.example".into(), "the host of `other`".into())],
        };
        let mut p = Scripted::new([
            "A new",
            "Bad Slug",
            "wikidata",
            "myfarm",
            "example",
            "https://example.wiki/w",
            "taken.example",
            "example.wiki",
            "Example Wiki",
            "alice",
            "Alice",
            "secret-one",
            "secret-two",
            "secret-one",
            "secret-one",
        ]);
        let a = ask_tenant(&mut p, &taken, true).unwrap();
        assert!(p.finished());
        assert_eq!(a.spec.slug, "example");
        assert_eq!(a.spec.base, "https://example.wiki");
        assert_eq!(a.spec.name.as_deref(), Some("Example Wiki"));
        assert_eq!(a.spec.owner_name, "Alice");
        assert_eq!(a.spec.adopt, None);
        assert_eq!(a.spec.owner, None);
        assert_eq!(a.password.as_deref(), Some("secret-one"));
        let w = p.warnings();
        assert_eq!(w.len(), 7, "{w:?}");
        assert!(w[0].contains("not a slug"));
        assert!(w[1].contains("registered"), "{}", w[1]);
        assert!(w[2].contains("already taken"));
        assert!(w[3].contains("has a path"));
        assert!(w[4].contains("the host of `other`"));
        assert!(w[5].contains("not capitalized"));
        assert!(w[6].contains("differ"));
    }

    #[test]
    fn defaults_leave_the_name_and_password_unset() {
        let mut p = Scripted::new(["", "solo", "solo.example", "", "", ""]);
        let a = ask_tenant(&mut p, &Taken::default(), false).unwrap();
        assert!(p.finished());
        assert_eq!(a.spec.name, None);
        assert_eq!(a.spec.owner_name, "Owner");
        assert_eq!(a.password, None);
    }

    #[test]
    fn an_adoption_asks_for_the_source_and_the_owners_id_there() {
        let mut p = Scripted::new([
            "An existing",
            "not a url",
            "https://mywiki.example/",
            "mywiki",
            "mywiki.example",
            "",
            "0",
            "7",
            "Alice",
            "",
        ]);
        let a = ask_tenant(&mut p, &Taken::default(), false).unwrap();
        assert!(p.finished());
        assert_eq!(a.spec.adopt.as_deref(), Some("https://mywiki.example/"));
        assert_eq!(a.spec.owner, Some(7));
        assert_eq!(p.warnings().len(), 2);
    }

    #[test]
    fn the_instance_then_its_primary_tenant() {
        let mut p = Scripted::new([
            "myfarm",
            "wikidata, internetdomains",
            "",
            "myfarm",
            "example",
            "example.wiki",
            "",
            "",
            "",
            "https://example.wiki/x",
            "",
        ]);
        let a = ask_instance(&mut p).unwrap();
        assert!(p.finished());
        assert_eq!(a.farm_slug, "myfarm");
        assert_eq!(a.providers, ["wikidata", "internetdomains"]);
        assert_eq!(a.farm_base, None);
        assert_eq!(a.tenant.spec.slug, "example");
        // The farm slug cannot be the tenant's; a farm base with a path is refused.
        assert_eq!(p.warnings().len(), 2, "{:?}", p.warnings());
        let cmd = instance_command(&a, Path::new(DEFAULT_KEY_FILE));
        assert_eq!(
            cmd,
            "triplespace instance create \\\n    --farm-slug myfarm \\\n    --provider wikidata \\\n    \
             --provider internetdomains \\\n    --tenant example \\\n    --base https://example.wiki \\\n    \
             --owner-name Owner"
        );
    }

    #[test]
    fn a_farm_base_of_its_own_must_not_share_the_primary_host() {
        let mut p = Scripted::new([
            "myfarm",
            "",
            "",
            "example",
            "example.wiki",
            "",
            "",
            "",
            "http://example.wiki",
            "farm.example",
        ]);
        let a = ask_instance(&mut p).unwrap();
        assert_eq!(a.farm_base.as_deref(), Some("https://farm.example"));
        assert_eq!(p.warnings().len(), 1);
    }
}
