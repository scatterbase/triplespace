//! `triplespace`: the command line (0005 §2; 0033 §13), as a library so the acceptance
//! tests drive the same code the binary runs.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod adopt;
pub mod common;
pub mod instance;
pub mod status;
pub mod sync;
