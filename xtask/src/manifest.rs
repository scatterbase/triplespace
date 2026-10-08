//! `cargo xtask manifest`: the component manifest of ADR 0077 §15.
//!
//! What `Special:Version` lists and what its licence subpages serve (0077 §3, §6): the
//! build, the workspace crates and third-party crates linked into each binary, the
//! frontend packages that reach the browser, vendored code, and the licence, copyright and
//! notice files each ships, deduplicated by content. The binaries embed it at build time
//! (their `build.rs`); a build without one embeds a stub.
//!
//! - **Crates** are followed from each binary through normal dependencies only, for the
//!   target given (the host's by default). Build and development dependencies are not in
//!   the binary, and neither are proc-macro crates, which run in the compiler; their own
//!   dependencies are not followed.
//! - **Frontend packages** are those whose files the frontend build put into `ui/dist`,
//!   which the build records in `ui/dist/packages.json` (`ui/vite.config.js`), at the
//!   versions `ui/package-lock.json` pins. Build-only packages such as Vue's compiler are
//!   not among them. Their licence files are read from `ui/node_modules`, so `npm ci` and
//!   `npm run build` run first.
//! - **Vendored code** is each tree with a `vendor.toml` (`name`, `upstream`, `revision`,
//!   `license`) under `crates/` or `vendor/`.
//!
//! Every licence expression must be satisfied by `deny.toml`'s allowlist, as `cargo deny`
//! requires (0033 §1): an `OR` needs one allowed side, an `AND` both. A crate with no
//! `license` field needs a `[[licenses.clarify]]` entry there.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// The binaries whose components are recorded.
pub const BINARIES: &[&str] = &["triplespace-server", "triplespace-web"];

/// Where the manifest goes by default, relative to the workspace root.
pub const DEFAULT_OUT: &str = "target/component-manifest.json";

/// Licence files larger than this are not copied (none in the tree comes close).
const MAX_TEXT: u64 = 256 * 1024;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Option<Resolve>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    repository: Option<String>,
    source: Option<String>,
    manifest_path: PathBuf,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    kind: Vec<String>,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
}

#[derive(Deserialize)]
struct NodeDep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(Deserialize)]
struct DepKind {
    kind: Option<String>,
}

/// The licence policy read from `deny.toml`.
#[derive(Debug, Default)]
pub struct Policy {
    allow: BTreeSet<String>,
    clarify: BTreeMap<String, String>,
}

impl Policy {
    /// Reads `[licenses].allow` and `[[licenses.clarify]]` from `deny.toml`.
    pub fn read(root: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(root.join("deny.toml")).context("reading deny.toml")?;
        let doc: toml::Value = toml::from_str(&text).context("parsing deny.toml")?;
        let licenses = doc.get("licenses");
        let allow = licenses
            .and_then(|l| l.get("allow"))
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let clarify = licenses
            .and_then(|l| l.get("clarify"))
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| {
                        let name = c
                            .get("crate")
                            .or_else(|| c.get("name"))
                            .and_then(toml::Value::as_str)?;
                        let expr = c.get("expression").and_then(toml::Value::as_str)?;
                        Some((
                            name.split('@').next().unwrap_or(name).to_string(),
                            expr.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { allow, clarify })
    }

    /// A policy allowing exactly these identifiers: for tests.
    #[cfg(test)]
    pub fn allowing(ids: &[&str]) -> Self {
        Self {
            allow: ids.iter().map(|s| (*s).to_string()).collect(),
            clarify: BTreeMap::new(),
        }
    }

    /// Whether an SPDX expression is satisfied by the allowlist.
    pub fn allows(&self, expression: &str) -> Result<bool> {
        let tokens = tokenize(expression);
        let mut p = Parser {
            tokens: &tokens,
            at: 0,
            allow: &self.allow,
        };
        let v = p.or()?;
        if p.at != tokens.len() {
            bail!(
                "unexpected `{}` in licence expression `{expression}`",
                tokens[p.at]
            );
        }
        Ok(v)
    }
}

/// Splits an SPDX expression into identifiers, operators and parentheses. The legacy `/`
/// separator (`MIT/Apache-2.0`) is read as `OR`.
fn tokenize(expression: &str) -> Vec<String> {
    let spaced = expression
        .replace('(', " ( ")
        .replace(')', " ) ")
        .replace('/', " OR ");
    spaced.split_whitespace().map(str::to_string).collect()
}

struct Parser<'a> {
    tokens: &'a [String],
    at: usize,
    allow: &'a BTreeSet<String>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }

    fn or(&mut self) -> Result<bool> {
        let mut v = self.and()?;
        while self.peek() == Some("OR") {
            self.at += 1;
            let r = self.and()?;
            v = v || r;
        }
        Ok(v)
    }

    fn and(&mut self) -> Result<bool> {
        let mut v = self.leaf()?;
        while self.peek() == Some("AND") {
            self.at += 1;
            let r = self.leaf()?;
            v = v && r;
        }
        Ok(v)
    }

    fn leaf(&mut self) -> Result<bool> {
        match self.peek() {
            Some("(") => {
                self.at += 1;
                let v = self.or()?;
                if self.peek() != Some(")") {
                    bail!("unbalanced parentheses in a licence expression");
                }
                self.at += 1;
                Ok(v)
            }
            Some(id) if !matches!(id, "AND" | "OR" | ")" | "WITH") => {
                let mut name = id.to_string();
                self.at += 1;
                if self.peek() == Some("WITH") {
                    let exception = self
                        .tokens
                        .get(self.at + 1)
                        .ok_or_else(|| anyhow!("`WITH` without an exception"))?;
                    name = format!("{name} WITH {exception}");
                    self.at += 2;
                }
                Ok(self.allow.contains(&canonical(&name)))
            }
            Some(other) => bail!("unexpected `{other}` in a licence expression"),
            None => bail!("a licence expression ends early"),
        }
    }
}

/// The current SPDX identifier for a deprecated one: `GPL-2.0+` is `GPL-2.0-or-later`,
/// `GPL-2.0` is `GPL-2.0-only`, and likewise for LGPL and AGPL. Others are unchanged.
fn canonical(id: &str) -> String {
    let (base, exception) = match id.split_once(" WITH ") {
        Some((b, e)) => (b, Some(e)),
        None => (id, None),
    };
    let family = ["GPL-", "LGPL-", "AGPL-"]
        .iter()
        .any(|f| base.starts_with(f));
    let base = if !family || base.ends_with("-only") || base.ends_with("-or-later") {
        base.to_string()
    } else if let Some(b) = base.strip_suffix('+') {
        format!("{b}-or-later")
    } else {
        format!("{base}-only")
    };
    match exception {
        Some(e) => format!("{base} WITH {e}"),
        None => base,
    }
}

/// Whether a file name is one of the licence, copyright and notice files a component ships.
fn is_notice(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "LICENSE",
        "LICENCE",
        "COPYING",
        "NOTICE",
        "AUTHORS",
        "COPYRIGHT",
        "UNLICENSE",
    ]
    .iter()
    .any(|p| upper.starts_with(p))
}

/// The texts store: content hash → text.
#[derive(Default)]
struct Texts {
    by_hash: BTreeMap<String, String>,
}

impl Texts {
    /// Reads the notice files in `dir` (and in a `LICENSES/` or `licenses/` folder there),
    /// returning `{file, hash}` entries in name order.
    fn collect(&mut self, dir: &Path) -> Result<Vec<Value>> {
        let mut files = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(files);
        };
        let mut paths: Vec<(String, PathBuf)> = Vec::new();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            if path.is_dir() && name.eq_ignore_ascii_case("licenses") {
                for f in std::fs::read_dir(&path).into_iter().flatten().flatten() {
                    if f.path().is_file() {
                        paths.push((
                            format!("{name}/{}", f.file_name().to_string_lossy()),
                            f.path(),
                        ));
                    }
                }
            } else if path.is_file() && is_notice(&name) {
                paths.push((name, path));
            }
        }
        paths.sort();
        for (name, path) in paths {
            if std::fs::metadata(&path).map_or(0, |m| m.len()) > MAX_TEXT {
                continue;
            }
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
            let hash = hex(&Sha256::digest(text.as_bytes())[..12]);
            self.by_hash.entry(hash.clone()).or_insert(text);
            files.push(json!({"file": name, "hash": hash}));
        }
        Ok(files)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} failed:\n{}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The build: version, commit, whether the tree is modified, the commit's date, rustc and
/// the target.
fn build(root: &Path, target: &str) -> Result<Value> {
    let git = |args: &[&str]| output(Command::new("git").args(args).current_dir(root));
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    let modified = !git(&["status", "--porcelain", "--untracked-files=no"])
        .unwrap_or_default()
        .is_empty();
    let commit_date = git(&["show", "-s", "--format=%cI", "HEAD"]).unwrap_or_default();
    let rustc = output(
        Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into())).arg("--version"),
    )?;
    Ok(json!({
        "commit": commit,
        "modified": modified,
        "commit_date": commit_date,
        "rustc": rustc,
        "target": target,
    }))
}

fn host_target() -> Result<String> {
    let vv =
        output(Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into())).arg("-vV"))?;
    vv.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("rustc -vV names no host"))
}

/// The crates of each binary, gathered from the resolve graph.
#[derive(Default)]
struct Crates {
    workspace: BTreeMap<String, Value>,
    third_party: BTreeMap<String, Value>,
    binaries: serde_json::Map<String, Value>,
}

/// Every package reachable from `root` through normal dependencies, not through
/// proc-macro crates.
fn reachable<'a>(
    root: &'a str,
    nodes: &BTreeMap<&'a str, &'a Node>,
    packages: &BTreeMap<&str, &Package>,
) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([root]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(id) else { continue };
        for d in &node.deps {
            let normal = d.dep_kinds.iter().any(|k| k.kind.is_none());
            let proc_macro = packages.get(d.pkg.as_str()).is_some_and(|p| {
                p.targets
                    .iter()
                    .any(|t| t.kind.iter().any(|k| k == "proc-macro"))
            });
            if normal && !proc_macro {
                queue.push_back(d.pkg.as_str());
            }
        }
    }
    seen
}

/// Checks a component's licence expression against the policy.
fn check(policy: &Policy, what: &str, licence: Option<&str>, problems: &mut Vec<String>) {
    match licence {
        None => problems.push(format!(
            "{what} has no licence field and no clarify entry in deny.toml"
        )),
        Some(l) => match policy.allows(l) {
            Ok(true) => {}
            Ok(false) => problems.push(format!(
                "{what}: `{l}` is not satisfied by deny.toml's allowlist"
            )),
            Err(e) => problems.push(format!("{what}: {e}")),
        },
    }
}

/// The workspace and third-party crates linked into each of [`BINARIES`], for `target`.
fn crates(
    root: &Path,
    target: &str,
    policy: &Policy,
    texts: &mut Texts,
    problems: &mut Vec<String>,
) -> Result<Crates> {
    let meta_json = output(
        Command::new(env!("CARGO"))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--filter-platform",
                target,
            ])
            .current_dir(root),
    )?;
    let meta: Metadata = serde_json::from_str(&meta_json).context("parsing cargo metadata")?;
    let packages: BTreeMap<&str, &Package> =
        meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let members: BTreeSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let resolve = meta
        .resolve
        .as_ref()
        .ok_or_else(|| anyhow!("cargo metadata has no resolve graph"))?;
    let nodes: BTreeMap<&str, &Node> = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out = Crates::default();
    for bin in BINARIES {
        let Some(root_pkg) = meta
            .packages
            .iter()
            .find(|p| p.name == *bin && members.contains(p.id.as_str()))
        else {
            bail!("no workspace package `{bin}`");
        };
        let mut ws_ids = Vec::new();
        let mut crate_ids = Vec::new();
        for id in reachable(root_pkg.id.as_str(), &nodes, &packages) {
            let p = packages[id];
            let key = format!("{}@{}", p.name, p.version);
            let licence = p
                .license
                .clone()
                .or_else(|| policy.clarify.get(&p.name).cloned());
            if members.contains(id) || p.source.is_none() {
                ws_ids.push(key.clone());
                out.workspace.entry(key).or_insert_with(|| {
                    json!({
                        "name": p.name,
                        "version": p.version,
                        "license": licence.clone().unwrap_or_default(),
                        "commercial": p.name.starts_with("scatter-"),
                    })
                });
                continue;
            }
            crate_ids.push(key.clone());
            if out.third_party.contains_key(&key) {
                continue;
            }
            check(
                policy,
                &format!("crate {key}"),
                licence.as_deref(),
                problems,
            );
            let dir = p.manifest_path.parent().unwrap_or(Path::new("."));
            let files = texts.collect(dir)?;
            out.third_party.insert(
                key.clone(),
                json!({
                    "id": key,
                    "name": p.name,
                    "version": p.version,
                    "license": licence.unwrap_or_default(),
                    "repository": p.repository,
                    "texts": files,
                }),
            );
        }
        out.binaries.insert(
            (*bin).to_string(),
            json!({"workspace": ws_ids, "crates": crate_ids}),
        );
    }
    Ok(out)
}

/// Writes the manifest. `args`: `--out PATH`, `--target TRIPLE`.
pub fn run(root: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut out = root.join(DEFAULT_OUT);
    let mut target = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => {
                out = PathBuf::from(args.next().ok_or_else(|| anyhow!("--out needs a path"))?);
            }
            "--target" => {
                target = Some(
                    args.next()
                        .ok_or_else(|| anyhow!("--target needs a triple"))?,
                );
            }
            other => bail!("unknown argument `{other}`; manifest takes --out and --target"),
        }
    }
    let target = match target {
        Some(t) => t,
        None => host_target()?,
    };
    let policy = Policy::read(root)?;
    let mut texts = Texts::default();
    let mut problems = Vec::new();
    let crates = crates(root, &target, &policy, &mut texts, &mut problems)?;
    let packages = frontend(root, &policy, &mut texts, &mut problems)?;
    let vendored = vendored(root, &policy, &mut texts, &mut problems)?;
    if !problems.is_empty() {
        bail!("the manifest was not written:\n  {}", problems.join("\n  "));
    }
    let manifest = json!({
        "format": 1,
        "build": build(root, &target)?,
        "binaries": crates.binaries,
        "workspace": crates.workspace.into_values().collect::<Vec<_>>(),
        "crates": crates.third_party.into_values().collect::<Vec<_>>(),
        "packages": packages,
        "vendored": vendored,
        "texts": texts.by_hash,
    });
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, serde_json::to_string(&manifest)?)
        .with_context(|| format!("writing {}", out.display()))?;
    let count = |k: &str| manifest[k].as_array().map_or(0, Vec::len);
    eprintln!(
        "wrote {} ({} workspace crates, {} crates, {} packages, {} vendored, {} texts)",
        out.display(),
        count("workspace"),
        count("crates"),
        count("packages"),
        count("vendored"),
        manifest["texts"]
            .as_object()
            .map_or(0, serde_json::Map::len),
    );
    Ok(())
}

/// The frontend packages that reach the browser: those `ui/dist/packages.json` names, at
/// their `ui/package-lock.json` versions, with their licence files from `ui/node_modules`.
fn frontend(
    root: &Path,
    policy: &Policy,
    texts: &mut Texts,
    problems: &mut Vec<String>,
) -> Result<Vec<Value>> {
    let bundled: BTreeSet<String> = serde_json::from_str(
        &std::fs::read_to_string(root.join("ui/dist/packages.json")).context(
            "reading ui/dist/packages.json; run `npm ci` and `npm run build` in ui/ first",
        )?,
    )
    .context("parsing ui/dist/packages.json")?;
    let lock_path = root.join("ui/package-lock.json");
    let lock: Value = serde_json::from_str(
        &std::fs::read_to_string(&lock_path).context("reading ui/package-lock.json")?,
    )
    .context("parsing ui/package-lock.json")?;
    let mut out = BTreeMap::new();
    for (path, entry) in lock["packages"].as_object().into_iter().flatten() {
        if path.is_empty()
            || entry["dev"].as_bool() == Some(true)
            || entry["link"].as_bool() == Some(true)
        {
            continue;
        }
        let name = path
            .rsplit("node_modules/")
            .next()
            .unwrap_or(path)
            .to_string();
        if !bundled.contains(&name) {
            continue;
        }
        let version = entry["version"].as_str().unwrap_or_default().to_string();
        let licence = entry["license"].as_str().unwrap_or_default().to_string();
        let key = format!("{name}@{version}");
        if licence.is_empty() {
            problems.push(format!("package {key} has no licence in package-lock.json"));
        } else {
            match policy.allows(&licence) {
                Ok(true) => {}
                Ok(false) => problems.push(format!(
                    "package {key}: `{licence}` is not satisfied by deny.toml's allowlist"
                )),
                Err(e) => problems.push(format!("package {key}: {e}")),
            }
        }
        let dir = root.join("ui").join(path);
        if !dir.is_dir() {
            problems.push(format!(
                "package {key}: {} is missing; run `npm ci` in ui/ first",
                dir.display()
            ));
            continue;
        }
        let files = texts.collect(&dir)?;
        out.insert(
            key.clone(),
            json!({
                "id": key,
                "name": name,
                "version": version,
                "license": licence,
                "repository": repository_of(&dir),
                "texts": files,
            }),
        );
    }
    Ok(out.into_values().collect())
}

/// A package's repository URL from its `package.json`, if it names one.
fn repository_of(dir: &Path) -> Option<String> {
    let pj: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).ok()?).ok()?;
    let r = match &pj["repository"] {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("url")?.as_str()?.to_string(),
        _ => return None,
    };
    Some(
        r.trim_start_matches("git+")
            .trim_end_matches(".git")
            .replace("git://", "https://")
            .replace("ssh://git@", "https://"),
    )
}

#[derive(Deserialize)]
struct VendorToml {
    name: String,
    upstream: String,
    revision: String,
    license: String,
}

/// Vendored trees: each directory under `crates/` or `vendor/` with a `vendor.toml`.
fn vendored(
    root: &Path,
    policy: &Policy,
    texts: &mut Texts,
    problems: &mut Vec<String>,
) -> Result<Vec<Value>> {
    let mut found = Vec::new();
    for top in ["crates", "vendor"] {
        walk(&root.join(top), &mut found);
    }
    found.sort();
    let mut out = Vec::new();
    for file in found {
        let dir = file.parent().unwrap_or(root);
        let v: VendorToml = toml::from_str(&std::fs::read_to_string(&file)?)
            .with_context(|| format!("parsing {}", file.display()))?;
        match policy.allows(&v.license) {
            Ok(true) => {}
            Ok(false) => problems.push(format!(
                "vendored {}: `{}` is not satisfied by deny.toml's allowlist",
                v.name, v.license
            )),
            Err(e) => problems.push(format!("vendored {}: {e}", v.name)),
        }
        let rel = dir.strip_prefix(root).unwrap_or(dir).display().to_string();
        out.push(json!({
            "id": format!("{}@{}", v.name, v.revision),
            "name": v.name,
            "version": v.revision,
            "license": v.license,
            "repository": v.upstream,
            "path": rel,
            "texts": texts.collect(dir)?,
        }));
    }
    Ok(out)
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        if p.is_dir() {
            if name != "target" && name != "node_modules" && name != ".git" {
                walk(&p, found);
            }
        } else if name == "vendor.toml" {
            found.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expressions() {
        let p = Policy::allowing(&["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "ISC"]);
        assert!(p.allows("MIT").unwrap());
        assert!(p.allows("MIT OR Apache-2.0").unwrap());
        assert!(p.allows("MIT/Apache-2.0").unwrap());
        assert!(p.allows("GPL-2.0-only OR MIT").unwrap());
        assert!(!p.allows("GPL-2.0-only AND MIT").unwrap());
        assert!(p.allows("(MIT OR Apache-2.0) AND ISC").unwrap());
        assert!(!p.allows("ISC AND (Apache-2.0 OR ISC) AND OpenSSL").unwrap());
        assert!(p.allows("Apache-2.0 WITH LLVM-exception OR MIT").unwrap());
        assert!(!p.allows("BSD-3-Clause WITH LLVM-exception").unwrap());
        let gpl = Policy::allowing(&["GPL-2.0-or-later"]);
        assert!(gpl.allows("GPL-2.0+").unwrap());
        assert!(
            !gpl.allows("GPL-2.0").unwrap(),
            "GPL-2.0 alone is GPL-2.0-only"
        );
        assert!(p.allows("(MIT").is_err());
        assert!(p.allows("MIT OR").is_err());
    }

    #[test]
    fn notice_files() {
        for n in [
            "LICENSE",
            "LICENSE-MIT",
            "license.txt",
            "COPYING",
            "NOTICE",
            "AUTHORS.md",
            "LICENCE",
        ] {
            assert!(is_notice(n), "{n}");
        }
        for n in ["README.md", "Cargo.toml", "src"] {
            assert!(!is_notice(n), "{n}");
        }
    }
}
