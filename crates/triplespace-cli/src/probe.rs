//! The probe (0107 §1; 22 §4.9): what the running code uses from the catalogues it acts on,
//! reported to a file so that `tools/probe_report.py` can compare it with them (24 §11).
//!
//! Both binaries take `--probe FILE` (`TRIPLESPACE_PROBE`). Once it is on, each name is
//! appended to the file as one line, `{kind} {name}`, the first time this process uses it:
//!
//! - `permission edit`: a permission an actor's effective permissions were asked about
//!   (0105 §2; `scatter_actors::observe_permission_checks`);
//! - `setting site.name`: a setting read by key (0107 §2;
//!   `triplespace_accounts::settings::observe_setting_reads`);
//! - `log-event block/block`: a log event the activity projection wrote (0107 §3;
//!   `triplespace_projections::observe_log_events`);
//! - `tenancy groups.global`: a tenancy switch read (0108 §2;
//!   `triplespace_accounts::settings::observe_switch_reads`, `scatter_actors::observe_policy_reads`);
//! - `module action=edit` and `route GET /account`: an Action API module or a REST route the
//!   server served (0108 §1). The server installs those two observers itself, with [`module`]
//!   and [`route`], since this library does not depend on the API crates.
//!
//! Several processes may append to one file (a test run starts the CLI and servers), so a
//! line can appear more than once; the report reads the file as a set.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};

/// The file and the lines already written to it by this process.
static PROBE: OnceLock<Mutex<(File, HashSet<String>)>> = OnceLock::new();

fn report(kind: &str, name: &str) {
    let Some(probe) = PROBE.get() else { return };
    let Ok(mut probe) = probe.lock() else { return };
    let (file, seen) = &mut *probe;
    let line = format!("{kind} {name}");
    if seen.insert(line.clone()) {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

fn permission(name: &str) {
    report("permission", name);
}

fn setting(key: &str) {
    report("setting", key);
}

fn log_event(event: &str) {
    report("log-event", event);
}

fn switch(name: &str) {
    report("tenancy", name);
}

/// Reports an Action API module served, as `action-api.toml` spells it (0108 §1).
pub fn module(name: &str) {
    report("module", name);
}

/// Reports a REST route served, as `routes.toml` spells it (0108 §1).
pub fn route(name: &str) {
    report("route", name);
}

/// Starts the probe: opens the file for appending and installs the observers of every kind
/// but the API's (see [`module`] and [`route`]);
/// implements 0107 §1 (A1) and implements 0105 §2 (A2). Calling it again in the same process
/// changes nothing.
///
/// # Errors
///
/// The file cannot be opened.
pub fn start(path: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("--probe {}", path.display()))?;
    let _ = PROBE.set(Mutex::new((file, HashSet::new())));
    scatter_actors::observe_permission_checks(permission);
    triplespace_accounts::settings::observe_setting_reads(setting);
    triplespace_projections::observe_log_events(log_event);
    triplespace_accounts::settings::observe_switch_reads(switch);
    scatter_actors::observe_policy_reads(switch);
    Ok(())
}

#[cfg(test)]
mod tests {
    // 0107 §1, 0108 §2: each kind of use is written once, as `{kind} {name}`.
    #[test]
    fn the_probe_writes_each_use_once() {
        let path = std::env::temp_dir().join(format!("ts-probe-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        super::start(&path).unwrap();
        let e = scatter_actors::Effective::default();
        assert!(!e.holds("edit"));
        assert!(!e.holds("edit"));
        let snap = triplespace_accounts::settings::Snapshot::default();
        let _ = snap.value("site.language");
        super::log_event("block/block");
        let lines: Vec<String> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            lines,
            [
                "permission edit",
                "setting site.language",
                // Resolving a tenant setting reads the `config.template` switch (0108 §2).
                "tenancy config.template",
                "log-event block/block"
            ]
        );
    }
}
