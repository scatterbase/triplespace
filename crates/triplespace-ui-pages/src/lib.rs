//! The site's page kinds (0113 §1): the pages a title names, over `triplespace-ui-kit`.
//!
//! - [`entity`]: entity pages, their histories and diffs, and their `action=render`
//!   regions (0010 §2; 0057 §8).
//! - [`document`]: document pages, read, edited and their histories (0008; 19 §3).
//! - [`discussion`]: talk pages and threads, read and posted to (0019 §8; 19 §6.1).
//! - [`tablepage`]: table pages.
//! - [`moderation`]: the Protect, Delete and Move forms and the gate a refused viewer
//!   meets.
//! - [`home`]: the main page, `Project:Home`, generated until a page of that title exists.
//!
//! Which of them serves a title is the router's to say (`triplespace-ui`); none calls it.

#![forbid(unsafe_code)]

pub mod discussion;
pub mod document;
pub mod entity;
pub mod home;
pub mod moderation;
pub mod resolver;
pub mod tablepage;
