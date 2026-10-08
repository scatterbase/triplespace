// What a binary records about its own build, for Special:Version (ADR 0077 §4, §15).
//
// Included by the `build.rs` of `triplespace-server` and `triplespace-web` (`include!`), so
// it uses only `std` and `serde_json`. Each build script calls `write_version_json`, which
// writes `$OUT_DIR/version.json`: the build (version, commit, whether the tree is
// modified, the commit's date, rustc, target, profile and features) and this binary's
// part of the component manifest that `cargo xtask manifest` writes, with only the texts
// that part refers to. Without a manifest the components are left out and `listed` is
// false: an ordinary development build (0077 §15). A release-profile build without one
// is warned; the packaging step for distributed builds requires it.
//
// The commit's date is recorded, not the time of the build, so that two builds of one
// commit are the same bytes.

#[allow(dead_code)]
fn version_git(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The workspace root, from the build script's crate.
#[allow(dead_code)]
fn version_root() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    manifest_dir
        .ancestors()
        .find(|p| p.join("Cargo.lock").is_file())
        .unwrap_or(&manifest_dir)
        .to_path_buf()
}

/// The build: version, commit, `modified`, the commit's date, rustc, target, profile and
/// features. Asks cargo to run the script again when the commit or the index changes.
#[allow(dead_code)]
fn version_build(root: &std::path::Path) -> serde_json::Value {
    let git_dir = root.join(".git");
    for f in ["HEAD", "index"] {
        println!("cargo:rerun-if-changed={}", git_dir.join(f).display());
    }
    if let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD"))
        && let Some(r) = head.trim().strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed={}", git_dir.join(r).display());
    }
    let commit = version_git(root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let modified = version_git(root, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|s| !s.is_empty());
    let commit_date =
        version_git(root, &["show", "-s", "--format=%cI", "HEAD"]).unwrap_or_default();
    let rustc = std::process::Command::new(
        std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()),
    )
    .arg("--version")
    .output()
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_default();
    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(k, _)| {
            k.strip_prefix("CARGO_FEATURE_")
                .map(|f| f.to_ascii_lowercase().replace('_', "-"))
        })
        .collect();
    features.sort();
    serde_json::json!({
        "version": std::env::var("CARGO_PKG_VERSION").unwrap_or_default(),
        "commit": commit,
        "modified": modified,
        "commit_date": commit_date,
        "rustc": rustc,
        "target": std::env::var("TARGET").unwrap_or_default(),
        "profile": std::env::var("PROFILE").unwrap_or_default(),
        "features": features,
    })
}

/// Copies this binary's part of the manifest into `out`: its workspace crates and
/// third-party crates, every frontend package and vendored tree, and the texts they refer
/// to.
#[allow(dead_code)]
fn version_components(manifest: &serde_json::Value, binary: &str, out: &mut serde_json::Value) {
    use serde_json::{Value, json};
    let part = &manifest["binaries"][binary];
    let pick = |list: &str, ids: &Value| -> Vec<Value> {
        let ids: Vec<&str> = ids
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        manifest[list]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| {
                let id = c["id"].as_str().map_or_else(
                    || {
                        format!(
                            "{}@{}",
                            c["name"].as_str().unwrap_or(""),
                            c["version"].as_str().unwrap_or("")
                        )
                    },
                    str::to_string,
                );
                ids.contains(&id.as_str())
            })
            .cloned()
            .collect()
    };
    let workspace = pick("workspace", &part["workspace"]);
    let crates = pick("crates", &part["crates"]);
    let packages: Vec<Value> = manifest["packages"].as_array().cloned().unwrap_or_default();
    let vendored: Vec<Value> = manifest["vendored"].as_array().cloned().unwrap_or_default();
    let mut texts = serde_json::Map::new();
    for c in crates.iter().chain(&packages).chain(&vendored) {
        for t in c["texts"].as_array().into_iter().flatten() {
            if let Some(h) = t["hash"].as_str()
                && let Some(text) = manifest["texts"].get(h)
            {
                texts.insert(h.to_string(), text.clone());
            }
        }
    }
    out["listed"] = json!(true);
    out["workspace"] = json!(workspace);
    out["crates"] = json!(crates);
    out["packages"] = json!(packages);
    out["vendored"] = json!(vendored);
    out["texts"] = Value::Object(texts);
}

/// Writes `$OUT_DIR/version.json` for `binary`.
#[allow(dead_code)]
fn write_version_json(binary: &str) {
    use serde_json::{Value, json};
    let root = version_root();
    let manifest_path = std::env::var("TRIPLESPACE_MANIFEST").map_or_else(
        |_| root.join("target/component-manifest.json"),
        std::path::PathBuf::from,
    );
    println!("cargo:rerun-if-env-changed=TRIPLESPACE_MANIFEST");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let mut out = json!({
        "format": 1,
        "binary": binary,
        "build": version_build(&root),
        "listed": false,
    });
    if let Some(manifest) = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        let manifest_commit = manifest["build"]["commit"].as_str().unwrap_or_default();
        if !manifest_commit.is_empty()
            && manifest_commit != out["build"]["commit"].as_str().unwrap_or_default()
        {
            println!(
                "cargo:warning=the component manifest is of commit {manifest_commit}; run `cargo xtask manifest` again"
            );
        }
        version_components(&manifest, binary, &mut out);
    } else if std::env::var("PROFILE").as_deref() == Ok("release") {
        // Distributed builds must carry the manifest (ADR 0077 §15); the packaging step
        // that makes them enforces it. A local release build, for benchmarks, is warned.
        println!(
            "cargo:warning=no component manifest: Special:Version will not list components; run `cargo xtask manifest` before a build that is distributed"
        );
    }
    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("version.json");
    std::fs::write(out_path, serde_json::to_string(&out).unwrap()).unwrap();
}
