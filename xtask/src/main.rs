//! Repository tasks, run as `cargo xtask <command>` (alias in `.cargo/config.toml`).
//!
//! - `deps`: checks the workspace dependency graph against the crate table of ADR 0005 §2, which
//!   chapter 22 §2.1 of docs/architecture/ holds (0050 §14),
//!   and the rules of 0005 §3 (dependencies point only downward; the core is pure; the
//!   site's crates reach no store).
//! - `wasm`: builds every crate that 0005 §3 rule 7 requires to build for
//!   `wasm32-unknown-unknown`, skipping the ones that do not exist yet.
//! - `manifest`: writes the component manifest of ADR 0077 §15 ([`manifest`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

mod manifest;

const CRATE_TABLE: &str = "docs/architecture/22-crates-and-stack.md";

/// Crates that 0005 §3 rule 7 requires to build for `wasm32-unknown-unknown`, plus the
/// browser bridge crate of 0034 §6.
const WASM_CRATES: &[&str] = &[
    "scatter-wikibase-shape",
    "scatter-wikitext",
    "scatter-pages",
    "scatter-normalize",
    "scatter-merge",
    "scatter-extract",
    "scatter-css",
    "scatter-scope",
    "scatter-tasks",
    "scatter-wasm",
];

/// Crates that 0005 §3 rule 2 says perform no I/O and use no async runtime. Prefix entries
/// end in `*`.
const PURE_CRATES: &[&str] = &[
    "scatter-vocab",
    "scatter-providers",
    "scatter-identity",
    "scatter-normalize",
    "scatter-actors",
    "scatter-pages",
    "scatter-wikitext",
    "scatter-threads",
    "scatter-activitypub",
    "scatter-filter",
    "scatter-mwlog",
    "scatter-wikitext-expand",
    "scatter-css",
    "scatter-scope",
    "scatter-tasks",
    "scatter-shex",
    "scatter-merge",
    "scatter-files",
    "scatter-extract",
    "scatter-wikibase-*",
];

/// Dependencies a pure crate may not have (0005 §3 rule 2, 0033 §3–4).
const IMPURE_DEPS: &[&str] = &[
    "tokio",
    "sqlx",
    "tokio-postgres",
    "reqwest",
    "axum",
    "hyper",
];

/// The site's crates (0005 §3 rule 10): they reach the instance only through its API.
const SITE_CRATES: &[&str] = &["triplespace-ui", "triplespace-client", "triplespace-web"];

/// Clients of the instance's stores; no crate the site's crates reach may depend on one
/// (0005 §3 rule 10). `redis` is not here: the response cache's Valkey client holds
/// public API responses only.
const STORE_CLIENTS: &[&str] = &[
    "tokio-postgres",
    "deadpool-postgres",
    "postgres",
    "sqlx",
    "opensearch",
    "elasticsearch",
];

/// Workspace crates the site's crates may not reach, whatever their dependencies: the
/// API crates, and those that open the instance's stores (0005 §3 rule 10).
const STORE_CRATES: &[&str] = &[
    "triplespace-db",
    "triplespace-projections",
    "triplespace-accounts",
];

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let root = workspace_root()?;
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("deps") => deps(&root),
        Some("wasm") => wasm(&root),
        Some("manifest") => manifest::run(&root, args),
        Some(other) => bail!("unknown task `{other}`; tasks: deps, wasm, manifest"),
        None => bail!("usage: cargo xtask <deps|wasm|manifest>"),
    }
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("xtask is not inside the workspace"))
}

// ---------------------------------------------------------------------------------------
// cargo metadata
// ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

fn metadata(root: &Path) -> Result<Metadata> {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout).context("parsing cargo metadata")
}

// ---------------------------------------------------------------------------------------
// The 0005 §2 table
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    Substrate,
    Wikibase,
    Ingest,
    Triplespace,
}

impl Layer {
    fn parse(cell: &str) -> Option<Self> {
        match cell.trim() {
            "Substrate" => Some(Self::Substrate),
            "Wikibase" => Some(Self::Wikibase),
            "Ingest" => Some(Self::Ingest),
            "Triplespace" => Some(Self::Triplespace),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct TableRow {
    layer: Layer,
    /// Crates named in the "Depends on" cell (internal and external).
    named: BTreeSet<String>,
    /// The cell said "any …": the layer rule applies instead of the named list.
    any: Option<AnyRule>,
}

#[derive(Debug, Clone, Copy)]
enum AnyRule {
    /// "Any lower crate" / "Any crate": anything in the same or a lower layer.
    Lower,
    /// "any layer-1 or layer-2 crate": substrate and Wikibase crates, plus the named ones.
    CoreOnly,
}

fn backticked(cell: &str) -> impl Iterator<Item = String> + '_ {
    cell.split('`').skip(1).step_by(2).map(str::to_string)
}

/// Parses the crate map of 0005 §2 from chapter 22 §2.1: one row per crate, in the order of the table.
fn crate_table(root: &Path) -> Result<BTreeMap<String, TableRow>> {
    let text = std::fs::read_to_string(root.join(CRATE_TABLE))
        .with_context(|| format!("reading {CRATE_TABLE}"))?;
    // The crate map of 0005 §2 lives in chapter 22 §2.1 since 0050 §14 relocated the ADRs' text.
    let section = text
        .split("### 2.1 The table")
        .nth(1)
        .and_then(|s| s.split("### 2.2").next())
        .ok_or_else(|| anyhow!("{CRATE_TABLE}: cannot find the §2.1 crate table"))?;

    let mut rows = BTreeMap::new();
    let mut layer = None;
    for line in section.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').collect();
        if cells.len() < 4 {
            continue;
        }
        if let Some(l) = Layer::parse(cells[0]) {
            layer = Some(l);
        }
        let crates: Vec<String> = backticked(cells[1]).collect();
        if crates.is_empty() || cells[1].contains("Crate") {
            continue; // the header row and the separator
        }
        let deps_cell = cells[cells.len() - 1];
        let lowered = deps_cell.to_ascii_lowercase();
        let any = if lowered.contains("any layer-1") {
            Some(AnyRule::CoreOnly)
        } else if lowered.contains("any ") {
            Some(AnyRule::Lower)
        } else {
            None
        };
        let named: BTreeSet<String> = backticked(deps_cell).collect();
        let layer = layer.ok_or_else(|| anyhow!("row for {crates:?} has no layer"))?;
        for name in crates {
            rows.insert(
                name,
                TableRow {
                    layer,
                    named: named.clone(),
                    any,
                },
            );
        }
    }
    if rows.is_empty() {
        bail!("{CRATE_TABLE}: no crate rows parsed from §2");
    }
    Ok(rows)
}

fn is_pure(name: &str) -> bool {
    PURE_CRATES.iter().any(|p| match p.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => *p == name,
    })
}

// ---------------------------------------------------------------------------------------
// cargo xtask deps
// ---------------------------------------------------------------------------------------

fn deps(root: &Path) -> Result<()> {
    let table = crate_table(root)?;
    let meta = metadata(root)?;
    let members: BTreeSet<&str> = meta
        .packages
        .iter()
        .filter(|p| meta.workspace_members.contains(&p.id))
        .map(|p| p.name.as_str())
        .collect();

    let mut problems = Vec::new();
    for pkg in meta
        .packages
        .iter()
        .filter(|p| members.contains(p.name.as_str()))
    {
        let name = pkg.name.as_str();
        if name == "xtask" {
            continue;
        }
        let Some(row) = table.get(name) else {
            problems.push(format!(
                "`{name}` is in the workspace but not in the 0005 §2 table"
            ));
            continue;
        };
        for dep in pkg
            .dependencies
            .iter()
            .filter(|d| d.kind.as_deref() != Some("dev"))
        {
            let dep_name = dep.name.as_str();
            let internal = members.contains(dep_name) && dep_name != "xtask";

            // Rule 2: the core is pure.
            if is_pure(name) && IMPURE_DEPS.contains(&dep_name) {
                problems.push(format!(
                    "`{name}` is a pure crate (0005 §3 rule 2) but depends on `{dep_name}`"
                ));
            }
            if !internal {
                continue;
            }

            // Rule 1: dependencies point only downward.
            if name.starts_with("scatter-") && dep_name.starts_with("triplespace-") {
                problems.push(format!(
                    "`{name}` depends on `{dep_name}`: no scatter-* crate may depend on a triplespace-* crate (0005 §3 rule 1)"
                ));
                continue;
            }
            let Some(dep_row) = table.get(dep_name) else {
                problems.push(format!(
                    "`{name}` depends on `{dep_name}`, which is not in the 0005 §2 table"
                ));
                continue;
            };
            if dep_row.layer > row.layer {
                problems.push(format!(
                    "`{name}` ({:?}) depends on `{dep_name}` ({:?}): dependencies point only downward (0005 §3 rule 1)",
                    row.layer, dep_row.layer
                ));
                continue;
            }

            // The row's own "Depends on" cell.
            let allowed = row.named.contains(dep_name)
                || match row.any {
                    Some(AnyRule::Lower) => true,
                    Some(AnyRule::CoreOnly) => dep_row.layer <= Layer::Wikibase,
                    None => false,
                };
            if !allowed {
                problems.push(format!(
                    "`{name}` depends on `{dep_name}`, which its 0005 §2 row does not list (listed: {})",
                    if row.named.is_empty() {
                        "—".to_string()
                    } else {
                        row.named.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", ")
                    }
                ));
            }
        }
    }

    problems.extend(site_reaches_no_store(&meta, &members));

    let checked = members.len().saturating_sub(1);
    if problems.is_empty() {
        println!(
            "deps: {checked} workspace crates match the 0005 §2 table ({} rows)",
            table.len()
        );
        Ok(())
    } else {
        for p in &problems {
            eprintln!("error: {p}");
        }
        bail!("{} dependency problem(s)", problems.len())
    }
}

/// Rule 10: walks every workspace crate a site crate reaches through normal and build
/// dependencies, and reports any that is an API or store crate or depends on a store
/// client.
fn site_reaches_no_store(meta: &Metadata, members: &BTreeSet<&str>) -> Vec<String> {
    let by_name: BTreeMap<&str, &Package> = meta
        .packages
        .iter()
        .filter(|p| members.contains(p.name.as_str()))
        .map(|p| (p.name.as_str(), p))
        .collect();
    let mut problems = Vec::new();
    for site in SITE_CRATES {
        let Some(start) = by_name.get(site) else {
            continue;
        };
        // Breadth first, remembering the path to each crate for the message.
        let mut path_to: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        path_to.insert(start.name.as_str(), vec![start.name.as_str()]);
        let mut queue = vec![*start];
        while let Some(pkg) = queue.pop() {
            let here = path_to[pkg.name.as_str()].clone();
            for dep in pkg
                .dependencies
                .iter()
                .filter(|d| d.kind.as_deref() != Some("dev"))
            {
                let name = dep.name.as_str();
                let via = || {
                    let mut p = here.clone();
                    p.push(name);
                    p.join(" → ")
                };
                if STORE_CLIENTS.contains(&name) {
                    problems.push(format!(
                        "`{site}` reaches the store client `{name}` ({}): the site's crates reach no store (0005 §3 rule 10)",
                        via()
                    ));
                    continue;
                }
                let Some(next) = by_name.get(name) else {
                    continue;
                };
                if STORE_CRATES.contains(&name) || name.starts_with("triplespace-api-") {
                    problems.push(format!(
                        "`{site}` reaches `{name}` ({}): the site's crates reach the instance only through its API (0005 §3 rule 10)",
                        via()
                    ));
                    continue;
                }
                if !path_to.contains_key(name) {
                    let mut p = here.clone();
                    p.push(name);
                    path_to.insert(name, p);
                    queue.push(next);
                }
            }
        }
    }
    problems
}

// ---------------------------------------------------------------------------------------
// cargo xtask wasm
// ---------------------------------------------------------------------------------------

fn wasm(root: &Path) -> Result<()> {
    let meta = metadata(root)?;
    let members: BTreeSet<&str> = meta
        .packages
        .iter()
        .filter(|p| meta.workspace_members.contains(&p.id))
        .map(|p| p.name.as_str())
        .collect();
    let present: Vec<&str> = WASM_CRATES
        .iter()
        .copied()
        .filter(|c| members.contains(c))
        .collect();
    for absent in WASM_CRATES.iter().filter(|c| !members.contains(*c)) {
        println!("wasm: `{absent}` not in the workspace yet, skipped");
    }
    if present.is_empty() {
        println!("wasm: nothing to build");
        return Ok(());
    }
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["build", "--target", "wasm32-unknown-unknown"])
        .current_dir(root);
    for c in &present {
        cmd.args(["-p", c]);
    }
    let status = cmd.status().context("running cargo build for wasm32")?;
    if !status.success() {
        bail!("wasm32 build failed for {}", present.join(", "));
    }
    println!("wasm: built {}", present.join(", "));
    Ok(())
}
