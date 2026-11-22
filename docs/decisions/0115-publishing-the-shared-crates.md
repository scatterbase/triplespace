# 0115. Publishing the shared crates

- **Status:** Proposed
- **Date:** 2026-11-22
- **Updated:** 2026-11-22 (A1)
- **Author:** James Hare / Claude Opus
- **Uses:** [0042](0042-template-expansion-and-parsoid.md), [0097](0097-versioning-and-skins.md)

## Context

[0005](0005-crate-organization.md) §6 kept every crate in this workspace and planned for Scatterbase to take the `scatter-*` crates as Git dependencies pinned to a commit, the crates moving to a repository of their own when Scatterbase first depended on them. [0097](0097-versioning-and-skins.md) §3 gave each `scatter-*` crate its own version and a public-API contract checked by `cargo-semver-checks` "against its last published version once it has one". By November 2026 no crate had been published, and every one was `publish = false`.

A survey of the 26 `scatter-*` crates that day found that a few were of use beyond Triplespace and Scatterbase, and that four things stood in the way of publishing any of them:

- **The registry files.** Eight crates embed `docs/registry/*.toml` with `include_str!("../../../docs/registry/…")`. `cargo package` packages only what is under a crate's directory, so a published crate would not build.
- **Path dependencies.** Workspace crates name each other by `path` alone, which crates.io refuses; every crate a published crate depends on has to be published too.
- **Triplespace in the public API.** Provider-prefixed IDs (`WDQ42`), tenant-relative IDs (`QQQ5`), keyed types, Triplespace's CSS classes, and public functions that consult a built-in default registry, in crates whose names promise something general.
- **The vendored `parse-wiki-text-2`** in `scatter-wikitext`, a path dependency with Triplespace's patches.

`scatter-wikitext-expand` had none of the last three: its dependencies were `serde` and `toml`, it embedded one registry file, and its public API was MediaWiki's preprocessor with an `ExpandHost` trait. The names it and the other candidates would take were free on crates.io.

### Direction

James's direction, from the design discussion of 2026-11-22:

- **"Let's work on publishing scatter-wikitext-expand and a plan for untangling the rest in your 'strong candidates' table so they can be more publishable."** The candidates were `scatter-wikitext-expand`, the Wikibase model stack (`scatter-providers`, `scatter-normalize`, `scatter-wikibase-model`, `scatter-wikibase-rdf`) and `scatter-wikitext` with `scatter-talk`.
- Asked how a published crate should carry its registry files, he chose **symlinks** into `docs/registry/` over a checked copy or moving the files into the crates.
- Asked what replaces the plan of a separate repository, he chose **publishing to crates.io from this workspace**.
- Asked for the first version, he chose **0.1.0 now**, publishing the slice as it is.
- Asked who publishes, he chose **himself, by hand**, with a workflow on tags left for later.

On Q1, the vendored `parse-wiki-text-2`, later the same day:

- **"I think it would be more straightforward to just fork outright and publish our fork as 0.1.0"**, rather than offering the patches upstream first or asking to co-maintain upstream.
- Asked what becomes of chapter 22's "changes are offered upstream where they are general", he chose **making offering upstream optional**.
- **"I pick the name "scatter-wikitext-parse". Let's keep the crate MIT-0."**

Then, offered the four steps that finish this work (the pull request for the fork, Q10, Q11 and Q2, each as recommended):

- **"Do all four steps to finish the publishing work"**.

Offered the changes proposed for the fork itself (a step budget instead of a timeout, the positions `scatter-wikitext` searched the source for, a fuzz target for the parser alone, and the stale fuzz lock file), confirming 0115 A1 in the same message:

- **"Before merging, I want you to make your additional proposed changes and then include them in the pull request"**.

## Decision

### 1. Shared crates are published to crates.io from this workspace (amends 0005 §6)

*Current text: [22](../architecture/22-crates-and-stack.md) §3.4, §3.6.*

### 2. A published crate carries its registry files as symlinks (extends 0033 §12)

*Current text: [22](../architecture/22-crates-and-stack.md) §3.6; [23](../architecture/23-configuration-and-registry.md) §4.1.*

### 3. What a crate needs before its first publication

*Current text: [22](../architecture/22-crates-and-stack.md) §3.6.*

### 4. `scatter-wikitext-expand` is the first, at 0.1.0

*Current text: [22](../architecture/22-crates-and-stack.md) §3.6.*

### 5. `scatter-wikitext-parse`: the parser, forked and published at 0.1.0 (amends [0033](0033-backend-stack.md) §9.1, §16; settles 0033 Q3)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.2; [22](../architecture/22-crates-and-stack.md) §3.3, §3.4, §3.6, §4.6.*

### 6. A renderer's own classes take a configured prefix

*Current text: [10](../architecture/10-pages-and-content-models.md) §5.2; [11](../architecture/11-rendering-templates-and-modules.md) §1.2.*

### 7. CI checks every publishable crate

*Current text: [22](../architecture/22-crates-and-stack.md) §3.6.*

### 8. `scatter-wikitext-expand` is a wasm crate

*Current text: [22](../architecture/22-crates-and-stack.md) §3.6.*

## Alternatives considered

- **A checked copy of each registry file inside the crate**, with a checker that fails when it differs from `docs/registry/`. It works on every platform, but it is a second copy to regenerate after every registry edit.
- **Moving the registry of record into the crates**, with `docs/registry/` linking to or generated from them. It would reverse the rule that `docs/registry/` is the source, which the documentation site, the catalogues and independent implementations read.
- **A build script** that copies from `docs/registry/` into `OUT_DIR` when it is there. It still needs a packaged copy for the build from crates.io, so it is the checked copy with more machinery.
- **A repository of their own for the `scatter-*` crates**, as 0005 §6 planned. Publishing does not need it, and it would split every change that crosses the line between the shared crates and the product into two pull requests.
- **Publishing from CI on a tag.** Deferred, not rejected: the first publications are few and James wants to make them himself.

## Consequences

- **A published crate's public API is a contract** under Cargo's SemVer rules (0097 §3), and `cargo-semver-checks` applies to it from its first publication.
- **A registry edit can be a crate release.** A file a published crate embeds is the default it ships; changing it changes what the next version of the crate does, and is versioned with it (22 §3.5).
- **The repository has symlinks**, its first. A checkout on Windows needs `core.symlinks`; without it the crate's test fails rather than the crate building with the wrong text.
- **The rustdoc of a published crate cites ADRs and chapters** that a reader on docs.rs has not seen; the README says where they are published.
- **The untangling of the other candidates is not decided here.** It is the open questions below, each with a recommendation, for James to decide one by one.

## Open questions

The plan for the other strong candidates. Each depends on the questions before it in the order of Q9.

- **Q1. The vendored `parse-wiki-text-2`.** *Settled by §5.*
- **Q2. `scatter-wikitext`'s CSS classes.** *Settled by §6.*
- **Q3. `scatter-talk`.** It is nearly general already: every public function takes the `Markup` registry explicitly, and English Wikipedia's conventions (`Site::english()`, the English months, `{{Unsigned}}`) are data in `talk-markup.toml` or in a `Site` built from siteinfo. It needs `scatter-wikitext` only for `decode_entities` and `escape_id`. *Recommendation:* publish it after `scatter-wikitext`, with `talk-markup.toml` as a symlink (§2) and its Cargo description rewritten without the ADR number.
- **Q4. `scatter-vocab`.** Most of it is the IRIs of public vocabularies (PROV-O, PAV, SIOC, FOAF, Activity Streams, Dublin Core, schema.org, SKOS, the Wikibase ontology…). The `scatter` module is `https://scatter.red/terms/v0/`, which is itself a published vocabulary that Triplespace shares with Scatterbase (0001 A2), and `scatter-wikibase-rdf` does not use it. *Recommendation:* publish it as it is, when `scatter-wikibase-rdf` needs it published. It is of little use on its own.
- **Q5. `scatter-providers`.** Its whole API is Triplespace's provider model: two-letter codes, revision IDs packed as `number << 40 | n`, tenant entity sources, adapters, trust and chip colours. Only `IdGrammar` is general. *Recommendation:* never publish it. Instead, take it out of the dependency tree of the published Wikibase crates (Q7, Q8), and leave it a shared crate for Scatterbase as it is.
- **Q6. `scatter-normalize`.** It holds two things. One is general: the URL normalizer, the UTS 46 host normalizer, keywords, value keys by Wikibase data type, and the resolver normalizers. The other is Triplespace's: keyed types, notation schemes, surrogates, and `keyed-types.toml` and `notation-schemes.toml`. The keyed types are a closed `match` on `domain`, `keyword` and `notation` (`keyed.rs`), not the open registry they look like, and `notation::normalize` and keyed-type normalization consult the default scheme registry, which a caller cannot replace. *Recommendation:* bring `scatter-keyed` back, retiring the "formerly `scatter-keyed`" merge (22 §2.1): keyed types, notation, surrogates and their two registries go there; `scatter-normalize` keeps the general part and `resolvers.toml`, and is published. The hidden default-registry calls go in the same change. This changes the crate map through 0005 §2.
- **Q7. `scatter-wikibase-model`.** The general core is entities, statements, values, canonical JSON, Wikibase's own hashes over its emulation of PHP `serialize()`, and the site list. Triplespace's part is the three ID forms and tenant-relative IDs in `EntityId::parse`, the `wikibase-domain`, `-keyword` and `-notation` data types, `value_iri`, the search document, fusion keys, badges and `Hasher::for_provider`; `EntityId::joins_clusters` consults the keyed registry's default. Options: (a) one crate whose `EntityId` parses plain Wikibase IDs and takes any further forms through a grammar passed in, with Triplespace's forms, search, keys and badges in a crate above it; (b) make the model generic over its ID type; (c) leave it unpublished. *Recommendation: (a), done last.* Seventeen crates depend on it, so a crate above it that re-exports the core keeps most of them unchanged. The Triplespace data types can become `DataType::Other`. `sites.toml`, which is generated from Wikimedia's site matrix and is general, goes behind a `sites` feature. The crate above is named in the change that makes it, through 0005 §2.
- **Q8. `scatter-wikibase-rdf`.** The renderer, both flavours, the dump header, N-Triples and Turtle are general Wikibase RDF 1.0.0. `IriScheme` resolves foreign, source and keyed IDs through the provider and keyed registries, and `with_default_registries` and `Namespaces::triplespace` are Triplespace's. *Recommendation:* concept and document IRIs come through a trait. The registry-backed implementation moves to `triplespace-rdf`, which drops the dependencies on `scatter-providers` and `scatter-normalize`. Publish after Q7.
- **Q9. The order.** *Recommendation:* the wikitext line first, since it is smaller and nearly ready (Q1, Q2, Q3). Then the general part of `scatter-normalize` (Q6) and `scatter-vocab` (Q4). Then the Wikibase model (Q7) and RDF (Q8), whose split touches most of the workspace.
- **Q10. Checks in CI for published crates.** *Settled by §7.*
- **Q11. `scatter-wikitext-expand` on the wasm list.** *Settled by §8.*

## References

- [The Cargo book: `cargo package`](https://doc.rust-lang.org/cargo/commands/cargo-package.html) and [publishing on crates.io](https://doc.rust-lang.org/cargo/reference/publishing.html).
- [`cargo-semver-checks`](https://github.com/obi1kenobi/cargo-semver-checks).

## Amendment log

### A1. The class prefix is `wt-` by default

- **Date:** 2026-11-22
- **Source:** Direct: Claude, while building the class prefix, 2026-11-22; confirmed by James, 2026-11-22
- **Change:** extends §6
- **Summary:** `ConfigSource::default()` gives `class_prefix` the value `wt-`, so a caller outside Triplespace gets `wt-chip` and `wt-repeat`, and Triplespace's configuration sets `ts-`. Q2's recommendation named only Triplespace's prefix. An empty default was not taken because `chip` and `repeat` alone would collide with a host page's own classes, and `ts-` because it names Triplespace in a crate meant to know nothing of it. Chapter text: 11 §1.2.
