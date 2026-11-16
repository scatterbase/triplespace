//! `triplespace backup-key generate|register|show|unseal|reseal` (0103 A3, A5): the backup
//! key set from the command line.
//!
//! - `generate` makes a holder's key off the instance: it writes the secret to a file only
//!   its owner may read and prints the public half to register.
//! - `register` records the set as the operator. The instance's `owner` registers it with
//!   `PUT /backup-keys`; the command line does so only on an instance with no owner
//!   account, as 0103 A3 has it.
//! - `show` prints the set in force.
//! - `unseal` turns a holder's sealed share of a backup into a share file, offline, with
//!   their secret key; a restore combines `k` of them.
//! - `reseal` seals an older backup's data key to another set: given `k` share files of the
//!   set the backup was made under, it writes the backup's sealed key anew. The instance
//!   never does this by itself (0103 A3).
//!
//! A backup's sealed key is the CBOR [`SealedKey`] a backup carries.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use scatter_integrity::backup_key::{KeySet, PublicKey, SealedKey, SecretKey, Share, seal_key};
use scatter_projection::Backend as _;
use triplespace_api_ingest::backup_keys;

use crate::common::{connect, farm_of, now, store};

/// `backup-key` subcommands.
#[derive(Debug, Subcommand)]
pub enum BackupKey {
    /// Make a holder's key: the secret to a file, the public half printed.
    Generate(Generate),
    /// Register the set as the operator, on an instance with no owner account.
    Register(Register),
    /// Print the set in force.
    Show(Target),
    /// Unseal a holder's share of a backup into a share file.
    Unseal(Unseal),
    /// Seal a backup's data key to another set, from `k` shares of its own.
    Reseal(Reseal),
}

/// The database.
#[derive(Debug, Args)]
pub struct Target {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// `backup-key generate`.
#[derive(Debug, Args)]
pub struct Generate {
    /// Where to write the secret key; it must not exist.
    #[arg(long)]
    pub out: PathBuf,
}

/// `backup-key register`.
#[derive(Debug, Args)]
pub struct Register {
    /// The database.
    #[command(flatten)]
    pub target: Target,
    /// A holder's public key, `backup-key:…`; once per key.
    #[arg(long = "key", required = true)]
    pub keys: Vec<String>,
    /// How many keys restore a backup: 1, any one of them, unless raised.
    #[arg(long, default_value_t = 1)]
    pub threshold: usize,
}

/// `backup-key unseal`.
#[derive(Debug, Args)]
pub struct Unseal {
    /// The backup's sealed key.
    #[arg(long)]
    pub sealed: PathBuf,
    /// The holder's secret key file.
    #[arg(long)]
    pub key: PathBuf,
    /// Where to write the share; it must not exist.
    #[arg(long)]
    pub out: PathBuf,
}

/// `backup-key reseal`.
#[derive(Debug, Args)]
pub struct Reseal {
    /// The backup's sealed key, rewritten in place.
    #[arg(long)]
    pub sealed: PathBuf,
    /// A share file of the set the backup was made under; as many as it needs.
    #[arg(long = "share", required = true)]
    pub shares: Vec<PathBuf>,
    /// A public key of the new set; once per key.
    #[arg(long = "key", required = true)]
    pub keys: Vec<String>,
    /// How many keys of the new set restore it.
    #[arg(long, default_value_t = 1)]
    pub threshold: usize,
}

/// Runs a `backup-key` command.
///
/// # Errors
///
/// As each command's.
pub async fn run(cmd: BackupKey) -> Result<()> {
    match cmd {
        BackupKey::Generate(args) => generate(&args),
        BackupKey::Register(args) => register(args).await,
        BackupKey::Show(args) => show(args).await,
        BackupKey::Unseal(args) => unseal(&args),
        BackupKey::Reseal(args) => reseal(&args),
    }
}

/// Writes `text` to a new file that only its owner may read.
fn write_secret(path: &Path, text: &str) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut f = options
        .open(path)
        .with_context(|| format!("create {} (it must not exist)", path.display()))?;
    writeln!(f, "{text}").with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

fn key_set(keys: &[String], threshold: usize) -> Result<KeySet> {
    let keys = keys
        .iter()
        .map(|k| PublicKey::parse(k).with_context(|| format!("`{k}`")))
        .collect::<Result<Vec<_>>>()?;
    Ok(KeySet::new(keys, threshold)?)
}

fn generate(args: &Generate) -> Result<()> {
    let secret = SecretKey::generate();
    write_secret(&args.out, &secret.to_text())?;
    let public = secret.public();
    println!("{}", public.to_text());
    println!(
        "  key {} — the secret is in {}; keep it offline, never on the instance",
        public.key_id(),
        args.out.display()
    );
    Ok(())
}

async fn register(args: Register) -> Result<()> {
    let set = key_set(&args.keys, args.threshold)?;
    let client = connect(&args.target.database).await?;
    if backup_keys::has_owner(&client)
        .await
        .map_err(anyhow::Error::msg)?
    {
        bail!(
            "this instance has an owner account: the owner registers the backup key set \
             (PUT /backup-keys at the farm base), not the command line (0103 A3)"
        );
    }
    let farm = farm_of(&client).await?;
    let actor = farm.operator().to_string();
    let (store, pipeline) = store(&args.target.database, &farm)?;
    let mut cx = store.begin().await?;
    let offset = backup_keys::register(
        &store,
        &pipeline,
        &mut cx,
        &set,
        &actor,
        Some("backup-key register"),
        now(),
    )
    .await
    .context("record the set in the instance config")?;
    store.commit(cx).await?;
    println!(
        "backup key set registered at offset {offset}: {} keys, any {} restore",
        set.keys().len(),
        set.threshold()
    );
    Ok(())
}

async fn show(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    match backup_keys::current(&client)
        .await
        .map_err(anyhow::Error::msg)?
    {
        None => println!("no backup key set is registered"),
        Some(r) => {
            println!(
                "backup key set at offset {}, by {}: any {} of {} keys restore",
                r.offset,
                r.actor,
                r.set.threshold(),
                r.set.keys().len()
            );
            for k in r.set.keys() {
                println!("  {}  {}", k.key_id(), k.to_text());
            }
        }
    }
    Ok(())
}

fn sealed_key(path: &Path) -> Result<SealedKey> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    SealedKey::decode(&bytes).with_context(|| format!("{} is not a sealed key", path.display()))
}

fn unseal(args: &Unseal) -> Result<()> {
    let sealed = sealed_key(&args.sealed)?;
    let secret = SecretKey::parse(&read(&args.key)?)?;
    let share = sealed.unseal(&secret)?;
    write_secret(&args.out, &share.to_text())?;
    println!(
        "share of key {} written to {}; the backup needs {} share(s) to restore",
        share.key_id(),
        args.out.display(),
        sealed.threshold
    );
    Ok(())
}

fn reseal(args: &Reseal) -> Result<()> {
    let sealed = sealed_key(&args.sealed)?;
    let shares = args
        .shares
        .iter()
        .map(|p| Share::parse(&read(p)?).with_context(|| format!("{} is not a share", p.display())))
        .collect::<Result<Vec<_>>>()?;
    let key = sealed.combine(&shares)?;
    let set = key_set(&args.keys, args.threshold)?;
    let resealed = seal_key(&set, &sealed.context, &key)?;
    let bytes = resealed.encode()?;
    let tmp = args.sealed.with_extension("resealing");
    std::fs::write(&tmp, &bytes).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &args.sealed)
        .with_context(|| format!("replace {}", args.sealed.display()))?;
    println!(
        "{} resealed to {} keys, any {} restore",
        args.sealed.display(),
        set.keys().len(),
        set.threshold()
    );
    Ok(())
}
