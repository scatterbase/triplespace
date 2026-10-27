//! Reading a MediaWiki wiki (0053 §1; 22 §2.1). What is here so far is what a follower
//! needs: [`recentchanges`], the `list=recentchanges` query and the reading of its
//! batches (0092 §1, §9), shared by the entity follower (`scatter-adapter-wikibase`) and
//! the page follower (`triplespace-repos`). The page bundles, dumps and title index of 0053
//! arrive with page repositories.
//!
//! Pure: functions over request parameters and parsed responses. Which wiki, which
//! namespaces and how to send a request are the caller's.

#![forbid(unsafe_code)]

pub mod recentchanges;

pub use recentchanges::{Batch, Change, Poller, Query, Row, Windows};
