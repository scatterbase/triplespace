//! `triplespace challenge solve {challenge}` (0087 §9): prints a solution of a
//! registration or login challenge, so that a script registering one account through the
//! API, or a test, can answer one without a browser.

use anyhow::{Result, bail};
use clap::Args;
use scatter_challenge::{Challenge, solve};

/// `challenge solve`.
#[derive(Debug, Args)]
pub struct Solve {
    /// The challenge, as `tschallengeid` carries it.
    pub challenge: String,
}

/// Runs `challenge solve`: the smallest solution, on one line.
pub fn run(args: &Solve) -> Result<()> {
    let c = match Challenge::parse(args.challenge.trim()) {
        Ok(c) => c,
        Err(e) => bail!("{e}"),
    };
    println!("{}", solve(&c.to_string(), c.bits));
    Ok(())
}
