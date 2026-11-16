# 0109. The documentation site

- **Status:** Accepted
- **Date:** 2026-11-09
- **Author:** James Hare / Claude Opus
- **Uses:** [0099](0099-status-from-the-code.md), [0105](0105-catalogues-checked-against-the-running-code.md)

## Context

By 2026-11-09 the documentation was Markdown in `docs/`, read on GitHub or in a checkout: 25 architecture chapters, over a hundred ADRs, the registry and the testing notes. What the code had built was known to `tools/coverage.py` and shown as counts (`INDEX.md`) and as one paragraph per crate (`CRATES.md`); no document answered, in the shape of the architecture, what each chapter's design is built of and what the code says about it. Readers had no search across the whole set, and no reference for the REST API but the OpenAPI file itself.

Every committed copy of generated text the repository already had (the index, the open list, the crate map's views, the catalogue tables) was kept honest by a check in CI that refuses a stale one. A copy that is never made needs no such check.

### Direction

James's direction, from the design discussion of 2026-11-09:

- **MkDocs with the Material theme, built and hosted by Read the Docs**; the repository is public.
- **"Make sure there aren't opportunities for the sources (for the docs) themselves to sync out of date with the source of truth."**
- The parts that need the Rust workspace compiled (rustdoc, a reference generated from the command line's own help) are deferred; the generated files already committed stay committed.

## Decision

### 1. The site

*Current text: [24](../architecture/24-documentation.md) §16.*

### 2. What the site generates is never committed

*Current text: [24](../architecture/24-documentation.md) §16.*

### 3. As built

*Current text: [24](../architecture/24-documentation.md) §16.*

### 4. The REST reference

*Current text: [24](../architecture/24-documentation.md) §16.*

## Consequences

- **The documentation has a home with search**, navigation built from the files, and a page per chapter of what is built, each linked to the code at the commit the site was built from.
- **No generated page can be stale**: each is made from its source at build time. The price is that the generated pages exist only on the site, not in a checkout.
- **A broken link in `docs/` now fails CI**, through the strict site build: the build found two links to a chapter file that had been renamed (`18-api-surface.md`), which the existing checkers do not test.
- ~~**The site's generator lives in `tools/`**, which `coverage.py` reads as tests, so no `implements` claim can be written for this ADR where its code is; see Q2.~~ *0099 A3: a product script under `tools/` is src, and `docs_site.py` carries this ADR's claims.*

## Open questions

- **Q1. Rustdoc and the command line's reference.** Both need the workspace compiled, which may not fit Read the Docs' build time. Whether to build them there, in CI and publish them beside the site, or not at all.
- **Q2.** ~~**Claims for code outside the crates.** The site's hooks and `coverage.py` itself are Python under `tools/`, which coverage reads as test code. Whether such tools may carry an `implements` claim (a src class for `tools/` scripts that are product, not tests), so that this ADR, and 0099, can become Accepted.~~ *Settled by James in the design discussion of 2026-11-09: yes; a script under `tools/` that says it is "Product code (24 §15)" is src, as the claims rule now says ([24](../architecture/24-documentation.md) §15).*

## References

- [MkDocs](https://www.mkdocs.org/), [Material for MkDocs](https://squidfunk.github.io/mkdocs-material/), [Read the Docs](https://docs.readthedocs.io/)
- [0099](0099-status-from-the-code.md) §2; [0105](0105-catalogues-checked-against-the-running-code.md) §1
