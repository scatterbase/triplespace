//! The `.env` file: deployment configuration for one instance, read into the environment
//! before the command line is parsed (0033 §12; 23 §5.1).
//!
//! Every flag of `triplespace`, `triplespace-server` and `triplespace-web` that names an
//! environment variable (`TRIPLESPACE_DATABASE_URL`, `TRIPLESPACE_LISTEN`, …) can be set in
//! the file, as can the variables read directly, such as `TRIPLESPACE_OWNER_PASSWORD`.
//! What wins, first match: a flag on the command line, then a variable already in the
//! process environment, then the file. The file never overrides the environment.
//!
//! The file read is the one `TRIPLESPACE_ENV_FILE` names, which must exist; otherwise
//! `.env` in the working directory, if there is one. A file that cannot be read or parsed
//! is an error, never skipped.
//!
//! ```text
//! # .env
//! TRIPLESPACE_DATABASE_URL=postgres://triplespace@localhost/triplespace
//! TRIPLESPACE_LISTEN=127.0.0.1:8080
//! TRIPLESPACE_MODE=development
//! ```
//!
//! Call [`load`] at the top of `main`, before the async runtime starts: the environment is
//! written, and writing it is only sound while the process has one thread.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The variable naming the file to read instead of `./.env`.
pub const ENV_FILE_VAR: &str = "TRIPLESPACE_ENV_FILE";

/// The file read when [`ENV_FILE_VAR`] is unset, relative to the working directory.
pub const DEFAULT_ENV_FILE: &str = ".env";

/// Reads the instance's `.env` file into the environment, without overriding variables
/// already set. Returns the file read, or `None` when there was none to read.
pub fn load() -> Result<Option<PathBuf>> {
    let (path, required) = chosen(std::env::var_os(ENV_FILE_VAR));
    load_from(&path, required)
}

/// The file to read and whether it must exist: the one `named` (from [`ENV_FILE_VAR`]),
/// which must, else [`DEFAULT_ENV_FILE`], which need not.
fn chosen(named: Option<OsString>) -> (PathBuf, bool) {
    match named {
        Some(p) if !p.is_empty() => (PathBuf::from(p), true),
        _ => (PathBuf::from(DEFAULT_ENV_FILE), false),
    }
}

/// Reads `path` into the environment. A missing file is `Ok(None)` unless `required`.
fn load_from(path: &Path, required: bool) -> Result<Option<PathBuf>> {
    match dotenvy::from_path(path) {
        Ok(()) => Ok(Some(path.to_path_buf())),
        Err(e) if e.not_found() && !required => Ok(None),
        Err(e) if e.not_found() => Err(e).with_context(|| {
            format!(
                "{ENV_FILE_VAR} names {}, which does not exist",
                path.display()
            )
        }),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "triplespace-env-file-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_is_dot_env_and_optional() {
        assert_eq!(chosen(None), (PathBuf::from(".env"), false));
        assert_eq!(chosen(Some("".into())), (PathBuf::from(".env"), false));
    }

    #[test]
    fn a_named_file_is_required() {
        assert_eq!(
            chosen(Some("/etc/triplespace/wiki.env".into())),
            (PathBuf::from("/etc/triplespace/wiki.env"), true)
        );
    }

    #[test]
    fn missing_default_is_not_an_error() {
        let p = scratch("missing").join(".env");
        assert_eq!(load_from(&p, false).unwrap(), None);
    }

    #[test]
    fn missing_named_file_is_an_error() {
        let p = scratch("missing-named").join("wiki.env");
        let e = load_from(&p, true).unwrap_err();
        assert!(format!("{e:#}").contains("does not exist"), "{e:#}");
    }

    #[test]
    fn malformed_file_is_an_error() {
        // A parse error on the first line, so nothing is written to the environment.
        let p = scratch("malformed").join(".env");
        std::fs::write(&p, "TRIPLESPACE_ENV_FILE_TEST='unterminated\n").unwrap();
        let e = load_from(&p, false).unwrap_err();
        assert!(format!("{e:#}").contains("read "), "{e:#}");
    }
}
