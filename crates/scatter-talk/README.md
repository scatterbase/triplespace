# scatter-talk

Split MediaWiki talk pages into threads and comments, in pure Rust: who wrote each comment, when,
and which comment it answers.

A wiki talk page is one block of wikitext in which people reply by indenting and sign with
`~~~~`. This crate reads it as a discussion forum would hold it. `convert` makes each level-two
section a thread and each signed comment a post, with its author and time from the signature, its
depth, and the post it replies to. Closing and collapsing templates (`{{archive top}}` and the
like) become a closing post or a collapsing post. Every dated comment and heading gets the name
and ID that MediaWiki's DiscussionTools gives it, so links into the page keep working. A section
that will not split cleanly falls back to one post, never to wrong ones.

`Blame` replays a page's history line by line, and `attribute` then says who added each comment:
the signer, or for an unsigned comment the editor whose revision added it.

It does no I/O: you supply the text, the history and the wiki's facts. It is part of
[Triplespace](https://triplespace.readthedocs.io/), which converts imported talk pages with it, and
it knows nothing about Triplespace.

## What it knows about wikis

- **Conventions shared by every wiki** come from a registry embedded in the crate,
  `registry/talk-markup.toml`: the timestamp formats MediaWiki signs with in each language, month
  names, what may stand before an undated signature, and the names of the templates that sign
  for someone (`{{unsigned}}`), outdent, close or collapse. `Markup::embedded()` reads it;
  `Markup::parse` reads your own.
- **What is specific to one wiki** (its content language, the timezone it signs in, the names of
  its User, User talk and Special namespaces) is a `Site`, which you build from the wiki's
  siteinfo. `Site::english()` is English Wikipedia's.

On a sample of English Wikipedia talk pages, it finds at least 99.9% of the comments
DiscussionTools finds there, and agrees with DiscussionTools on at least 97.5% of their parents and
97% of their IDs; a test in the crate's repository holds it to that.

## Example

```rust
use scatter_talk::{convert, Markup, Options, Site};

let page = "\
== Moving the article ==
I think we should move it. [[User:Alice|Alice]] ([[User talk:Alice|talk]]) 10:00, 1 March 2024 (UTC)
:Agreed. [[User:Bob|Bob]] ([[User talk:Bob|talk]]) 11:30, 1 March 2024 (UTC)
";

let conversion = convert(page, Markup::embedded(), &Site::english(), &Options::default());
let thread = &conversion.threads[0];
assert_eq!(thread.subject, "Moving the article");
assert_eq!(thread.posts.len(), 2);

let reply = &thread.posts[1];
assert_eq!(reply.signature.as_ref().map(|s| s.user.as_str()), Some("Bob"));
assert_eq!(reply.parent, Some(0)); // it answers Alice
```

## Licence

GPL-3.0-or-later. See `LICENSE`. The registry file is CC0-1.0, like everything in Triplespace's
`docs/`.

The code comments cite Triplespace's design documents, for example `0090 §5` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
