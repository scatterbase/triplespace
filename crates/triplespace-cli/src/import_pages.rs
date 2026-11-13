//! `triplespace import` (0008 §9; 10 §6): the pages of a MediaWiki XML export, from any
//! wiki, into a running tenant under the tenant's own IDs. Three passes over the file:
//! the template census, which is printed and, with `--census-only`, is all that happens;
//! the import job over the document pages (`triplespace_api_ingest::import`); and the
//! talk pages converted to threads (`triplespace_api_ingest::talk_import`, 0090 §10).
//! The same code runs behind `action=import` and `Special:Import` (21 §4.3).

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_actors::key::ActorKey;
use scatter_adapter_wikibase::xml_dump;
use scatter_ingest::import::Import as ImportJob;
use scatter_ingest::import::census::Census;
use scatter_ingest::write::Attestation;
use scatter_mediawiki_xml::DumpPage;
use triplespace_api_ingest::import::{census, ensure_accounts, import_pages, issuer_for};
use triplespace_api_ingest::talk_import::{self, Authors, TalkContext};

use crate::common::{connect, farm_of, now, store};

/// `import`.
#[derive(Debug, Args)]
pub struct Import {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// The MediaWiki XML export (`.xml`, `.xml.gz`, `.xml.bz2`), with or without history.
    #[arg(long)]
    pub file: PathBuf,
    /// Where the file came from, for the job's record and the imported authors' prefix:
    /// a site alias (`enwiki`) or a name; `imported` by default, as MediaWiki's importer
    /// names an upload's authors.
    #[arg(long, default_value = "imported")]
    pub source: String,
    /// The registered issuer the source wiki is, whose accounts keep their numeric IDs
    /// (0007 §5); without one, every named author becomes an `imported` surrogate.
    #[arg(long)]
    pub issuer: Option<String>,
    /// The subsidiary running the job, `librarybase:2`; the owner's own key by default.
    #[arg(long)]
    pub subsidiary: Option<String>,
    /// Print the census and import nothing.
    #[arg(long)]
    pub census_only: bool,
    /// Leave the file's talk pages unconverted.
    #[arg(long)]
    pub skip_talk: bool,
}

/// Opens the file as an iterator of its pages, errors written to stderr and counted.
fn pages(path: &std::path::Path, errors: &mut u64) -> Result<Vec<DumpPage>> {
    let mut out = Vec::new();
    for page in xml_dump::open(path).context("open file")? {
        match page {
            Ok(p) => out.push(p),
            Err(e) => {
                *errors += 1;
                eprintln!("file error: {e}");
            }
        }
    }
    Ok(out)
}

/// Prints a census as the command reports it.
pub fn print_census(c: &Census) {
    println!(
        "census: {} pages, {} revisions{}",
        c.pages,
        c.revisions,
        if c.current_only {
            " (current revisions only)"
        } else {
            ""
        }
    );
    println!(
        "  to import {} pages, {} talk pages to convert, {} entity pages refused (only adoption brings entities)",
        c.imported, c.talk, c.entities
    );
    for (why, n) in &c.deferred {
        println!("  deferred   {n} × {why}");
    }
    for (why, n) in &c.ignored {
        println!("  ignored    {n} × {why}");
    }
    if !c.existing.is_empty() {
        println!(
            "  {} titles already exist and take the file's revisions into their history:",
            c.existing.len()
        );
        for t in &c.existing {
            println!("    {t}");
        }
    }
    println!(
        "  templates  {} used, {} in the file, {} missing{}",
        c.templates_used.len(),
        c.templates_in_file.len(),
        c.templates_missing.len(),
        if c.templates_missing.is_empty() {
            String::new()
        } else {
            format!(
                ": {}",
                c.templates_missing
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    if !c.modules_used.is_empty() {
        println!(
            "  modules    {} invoked, {} in the file, {} missing",
            c.modules_used.len(),
            c.modules_in_file.len(),
            c.modules_missing.len()
        );
    }
    let by_status = |items: &BTreeMap<String, scatter_ingest::import::census::Status>| {
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, status) in items {
            groups
                .entry(format!("{status:?}").to_lowercase())
                .or_default()
                .push(name.clone());
        }
        groups
    };
    for (status, names) in by_status(&c.functions) {
        println!("  functions  {status}: {}", names.join(", "));
    }
    for (status, names) in by_status(&c.tags) {
        println!("  tags       {status}: {}", names.join(", "));
    }
}

/// Runs the import.
#[allow(clippy::too_many_lines)]
pub async fn run(args: Import) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let owner: Option<String> = client
        .query_opt(
            "SELECT m.actor_key FROM view.membership m WHERE m.tenant = $1 AND m.\"group\" = 'owner' ORDER BY m.actor_key LIMIT 1",
            &[&args.tenant],
        )
        .await?
        .map(|r| r.get(0));
    let actor = match (&args.subsidiary, owner) {
        (Some(s), _) => ActorKey::parse(s)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .to_string(),
        (None, Some(o)) => o,
        (None, None) => bail!("no owner found for `{}`; pass --subsidiary", args.tenant),
    };

    // Pass 1: the census.
    let mut errors = 0u64;
    let all = pages(&args.file, &mut errors)?;
    let c = census(&client, &args.tenant, all.iter()).await?;
    print_census(&c);
    if errors > 0 {
        println!("  unreadable: {errors} file errors (see stderr)");
    }
    if args.census_only {
        if errors > 0 {
            bail!(
                "the file could not be read whole ({errors} errors, see stderr): the census covers the {} pages read before it",
                all.len()
            );
        }
        return Ok(());
    }
    if c.imported == 0 && c.talk == 0 {
        if errors > 0 {
            bail!("the file could not be read ({errors} errors, see stderr): nothing was imported");
        }
        bail!("nothing to import");
    }

    // Pass 2: the job.
    let accounts = talk_import::accounts_of(all.iter());
    // The registered issuer the source is (0008 A50): named outright, or resolved from
    // `--source` as a site alias or provider slug.
    let issuer = match &args.issuer {
        Some(code) => Some(issuer_for(code, None).with_context(|| {
            format!("`{code}` is not a registered issuer with numeric accounts (issuers.toml)")
        })?),
        None => issuer_for(&args.source, None),
    };
    if let Some(i) = &issuer {
        eprintln!("the source is the registered issuer `{i}`: its accounts keep their IDs");
    }
    let authors = Authors::Imported {
        issuer: issuer.as_deref(),
        accounts: &accounts,
        prefix: &args.source,
    };
    let (store, pipeline) = store(&args.database, &farm)?;
    let t = now();
    if let Some(i) = &issuer {
        let n = ensure_accounts(
            &store,
            &pipeline,
            &args.tenant,
            i,
            &accounts,
            &Attestation::by(&actor),
            t,
        )
        .await
        .context("accounts")?;
        if n > 0 {
            println!("  accounts   {n} of `{i}` recorded");
        }
    }
    let job = ImportJob {
        source: args.source.clone(),
        version: args
            .file
            .file_name()
            .map(|f| f.to_string_lossy().into_owned()),
        history: !c.current_only,
        summary: None,
        job_id: None,
    };
    let (outcome, read) = import_pages(
        &store,
        &pipeline,
        &args.tenant,
        Attestation::by(&actor),
        job,
        &authors,
        all.iter().cloned(),
        t,
    )
    .await
    .context("import")?;
    println!("import job {} finished", outcome.job_id);
    println!(
        "  created    {} pages\n  merged     {} pages that existed\n  revisions  {} written, {} already present, {} restored on top",
        outcome.counts.created,
        outcome.counts.merged,
        outcome.counts.revisions,
        outcome.counts.present,
        outcome.counts.restored
    );
    if read.entities > 0 {
        println!("  refused    {} entity pages", read.entities);
    }
    for (why, n) in &read.left_out {
        println!("  left out   {n} × {why}");
    }
    for r in outcome.rejects.iter().take(20) {
        println!("  reject #{}: {}", r.line, r.reason);
    }

    // Pass 3: the talk pages, comment by comment (0090 §10).
    if !args.skip_talk && c.talk > 0 {
        eprintln!("converting {} talk pages", c.talk);
        let site = scatter_talk::Site::english();
        let ctx = TalkContext {
            tenant: &args.tenant,
            source: &args.source,
            authors,
            actor: &actor,
            job: Some(outcome.job_id),
            site: &site,
        };
        let talk = talk_import::convert_all(
            &store,
            &pipeline,
            &ctx,
            all.into_iter().filter(talk_import::is_talk_page),
            now(),
        )
        .await?;
        println!(
            "  talk       {} pages converted into {} threads, {} posts; {} already present",
            talk.pages, talk.threads, talk.posts, talk.present
        );
        let n: u64 = talk.deferred.values().sum();
        if n > 0 {
            println!("  deferred   {n} talk pages:");
            for (why, k) in &talk.deferred {
                println!("    {k} × {why}");
            }
        }
    }
    // A file that could not be read whole was imported in part: say so, and fail, so
    // that no script takes a truncated export for a finished import.
    if errors > 0 {
        bail!(
            "the file could not be read whole ({errors} errors, see stderr): {} pages were imported before it ({} created, {} merged), and the rest of the file was not",
            outcome.counts.created + outcome.counts.merged,
            outcome.counts.created,
            outcome.counts.merged
        );
    }
    Ok(())
}
