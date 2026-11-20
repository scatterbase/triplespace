//! Reading a MediaWiki wiki's pages (0053 §1; 22 §2.1): page bundles, dumps and the
//! title index of page repositories, still to be built. The recent-changes poller that
//! was here is RevisionChest's `mwrecent` since 0112 §3, and a RevisionChest store is read
//! through its `mwrev` (0112 §4).
//!
//! Pure: functions over request parameters and parsed responses. Which wiki, which
//! namespaces and how to send a request are the caller's.

#![forbid(unsafe_code)]
