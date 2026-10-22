//! Converting wikitext talk pages into threads, comment by comment (ADR 0090).
//!
//! [`convert`] splits a talk page's wikitext: each level-two section becomes a [`Thread`], each
//! signed comment a [`Post`] with the post it answers, and headings below level two become
//! heading posts. A wrapper template becomes data: around a whole section, a closing post that
//! closes the thread; around anything less, a collapsing post over the posts it enclosed. A
//! section that will not split falls back to one post, and a page that will not divide to one
//! post (0090 §1–§4, A1). Signatures are found with the universal registry `talk-markup.toml`
//! (§5) and what [`Site`] takes from siteinfo. Every dated comment and heading gets its
//! DiscussionTools name and ID (§9).
//!
//! [`Blame`] replays a page's history line by line, and [`attribute`] resolves each post's
//! author from its signature and the revision that added it (§6–§7).
//!
//! Pure: no I/O. The caller supplies the text, the history and the site.

mod blame;
mod dt;
mod registry;
mod sig;
mod site;
mod split;
mod split_body;
mod split_headings;
mod time;

pub use blame::{Attribution, AuthorSource, Blame, Revision, attribute};
pub use dt::{default_anchor, timestamp_string};
pub use registry::{
    Markup, Months, RegistryError, TALK_MARKUP_TOML, Wrapper, normalize_template_name,
};
pub use sig::{
    SIGNATURE_SCAN_LIMIT, Signature, SignatureKind, normalize_user, signatures, visible_len,
};
pub use site::Site;
pub use split::{
    Conversion, DtIds, Fallback, Header, Options, Post, PostKind, Thread, ThreadOrigin, convert,
};
pub use time::{Format, Found, Stamp, Zone, find_timestamps};
