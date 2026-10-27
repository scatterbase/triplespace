//! The `.env` file through the `triplespace` binary (23 §5.1): flag, then environment, then
//! file. No database is needed: `status` fails parsing a URL that is not one, which shows
//! where the value came from, and fails in clap when there is no value at all.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NOT_A_URL: &str = "from-the-env-file";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "triplespace-env-file-it-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `triplespace status` in `dir`, with none of the variables under test inherited.
fn status(dir: &Path, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_triplespace"));
    cmd.current_dir(dir)
        .arg("status")
        .args(args)
        .env_remove("TRIPLESPACE_DATABASE_URL")
        .env_remove("TRIPLESPACE_ENV_FILE");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Whether `status` got as far as parsing a database URL (the value reached it).
fn parsed_url(o: &Output) -> bool {
    !o.status.success() && stderr(o).contains("database URL")
}

#[test]
fn without_a_file_the_url_is_required() {
    let dir = scratch("none");
    let o = status(&dir, &[], &[]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("--database"), "{}", stderr(&o));
}

#[test]
fn dot_env_in_the_working_directory_is_read() {
    let dir = scratch("dot-env");
    std::fs::write(
        dir.join(".env"),
        format!("# instance settings\nTRIPLESPACE_DATABASE_URL={NOT_A_URL}\n"),
    )
    .unwrap();
    let o = status(&dir, &[], &[]);
    assert!(parsed_url(&o), "{}", stderr(&o));
}

#[test]
fn the_environment_wins_over_the_file() {
    let dir = scratch("env-wins");
    // A valid-looking URL in the file; the environment's unparseable one must be used.
    std::fs::write(
        dir.join(".env"),
        "TRIPLESPACE_DATABASE_URL=postgres://nobody@127.0.0.1:1/none\n",
    )
    .unwrap();
    let o = status(&dir, &[("TRIPLESPACE_DATABASE_URL", NOT_A_URL)], &[]);
    assert!(parsed_url(&o), "{}", stderr(&o));
}

#[test]
fn a_flag_wins_over_the_file() {
    let dir = scratch("flag-wins");
    std::fs::write(
        dir.join(".env"),
        "TRIPLESPACE_DATABASE_URL=postgres://nobody@127.0.0.1:1/none\n",
    )
    .unwrap();
    let o = status(&dir, &[], &["--database", NOT_A_URL]);
    assert!(parsed_url(&o), "{}", stderr(&o));
}

#[test]
fn triplespace_env_file_names_another_file() {
    let dir = scratch("named");
    let file = dir.join("wiki.env");
    std::fs::write(&file, format!("TRIPLESPACE_DATABASE_URL={NOT_A_URL}\n")).unwrap();
    let elsewhere = scratch("named-cwd");
    let o = status(
        &elsewhere,
        &[("TRIPLESPACE_ENV_FILE", file.to_str().unwrap())],
        &[],
    );
    assert!(parsed_url(&o), "{}", stderr(&o));
}

#[test]
fn a_named_file_that_is_missing_is_an_error() {
    let dir = scratch("named-missing");
    let o = status(&dir, &[("TRIPLESPACE_ENV_FILE", "nope.env")], &[]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("does not exist"), "{}", stderr(&o));
}

#[test]
fn a_malformed_file_is_an_error() {
    let dir = scratch("malformed");
    std::fs::write(dir.join(".env"), "TRIPLESPACE_DATABASE_URL='unterminated\n").unwrap();
    let o = status(&dir, &[], &[]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("read .env"), "{}", stderr(&o));
}
