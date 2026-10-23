# Triplespace: notes for coding agents

Triplespace is Wikibase reimagined as an append-only log, in Rust: high-speed ingest, foreign
entities (`WDQ42`) alongside local ones, Merkle-verified history, full attribution, erasure, and
compatibility with Wikibase clients (WikibaseIntegrator, Pywikibot, WikidataIntegrator,
QuickStatements). James Hare (Scatter LLC) owns it; he decides, and agents draft.

## Where things are

| Path | Holds | Rule |
|---|---|---|
| `docs/architecture/` | **What holds now**, by subject: 24 chapters (`00`–`23`). Start with `00`, then `22` (crates and stack). | A chapter describes; it decides nothing on its own. |
| `docs/decisions/` | **Why and when**: ADRs, all in the 0050 format. Most Decision sections are now one-line pointers (*Current text: [03] §4.5.*) into a chapter. | Never read an ADR's Decision as the current text if it is a pointer; follow it. |
| `docs/registry/` | Names and codes the crates embed (`include_str!`) and parse at build time: providers, graphs, namespaces, content models, special pages, settings, groups… | A change here is a commit. Codes and numbers are never reused. See its `README.md`, including *Pending allocations*. |
| `docs/testing/` | How to test against real dumps (`real-dumps.md`, with its results log) and write-path performance: measurements, the `pg_stat_statements` recipe, Postgres settings, what a Wikidata-sized load needs (`performance.md`). | Add a results row for every real-dump run. |
| `docs/api/` | Compatibility contracts with Wikibase and MediaWiki, golden snapshots, log test vectors, the OpenAPI file. | Contracts are tested where they are implemented. |
| `crates/` | One workspace: `scatter-*` (shared with Scatterbase, knows nothing of Triplespace) and `triplespace-*` (the product). | See the crate rules below. |
| `tools/` | Scripts outside the build: dump slicing, client-compatibility checks, talk-page prototypes. `tools/README.md`. | |
| `xtask/` | `cargo xtask deps` (dependency graph vs. chapter 22 §2.1), `wasm`, `manifest`. | |
| `ui/`, `i18n/` | The site front end (Codex, Vite) and interface messages. | |

**Looking things up:** `tools/adr.py` answers the common questions in one call:
- `show 0013 §7` prints an ADR section, following its pointer to the chapter text.
- `where <phrase>` lists the chapter sections, ADRs and registry files that cover a subject.
- `log 0008 [A6]` prints an Amendment log, or one entry.
- `questions NNNN` prints an ADR's open questions.
- `outline NN|NNNN` prints a document's headings.
- `code 0013 §7` lists the code that cites a section.

**Is it built?** Every ADR is `Proposed` until it is implemented in full, so the status won't tell
you. `tools/coverage.py` reads the `NNNN §N` citations in code comments: run it alone for one line
per ADR, or with `NNNN` for each section and the crates citing it. A citation means the code is
about a section, not that the section is complete, so read the cited lines before relying on one.
Cite sections in comments as `0013 §7`, and a numbered item as `0013 §7 rule 2` (0050 §11).
`coverage.py --stale` lists citations of sections that don't exist.

**Generated files: never edit by hand.** `docs/decisions/INDEX.md` (`check_adrs.py --index`),
`docs/architecture/MAP.md` (`map.py --write`), and the settings tables in chapter 23
(`settings.py --write`, from `registry/settings.toml`).

**Secrets.** `owner.pw` and `triplespace.key` at the root are local, gitignored secrets. Do not
read, print, copy or commit them.

## Crate rules (chapter 22 §1.2, abridged)

1. Dependencies point only downward through four layers: substrate, Wikibase domain, ingest,
   Triplespace surfaces. No `scatter-*` crate depends on a `triplespace-*` crate; only adapters
   name a specific provider.
2. The core crates listed in 22 §1.2 rule 2 are pure: no I/O, no async runtime, no tokio.
3. Policy is data: registries, role maps and rules are values passed in, never enums or globals.
4. Each hashed type has exactly one canonical encoding function.
5. The crates on rule 7's list build for `wasm32-unknown-unknown`, and so does everything they
   depend on.
6. The site's crates (`triplespace-ui`, `-client`, `-web`) reach the instance only through its
   public API.
7. A new crate or dependency edge goes into the crate map (22 §2.1) through an ADR.

## Checks before calling work done

Run what the change touches; CI (`.github/workflows/ci.yml`) runs all of it.

```sh
cargo fmt --all --check
cargo xtask deps
cargo build --workspace --all-targets
cargo nextest run --workspace          # DB tests need TRIPLESPACE_TEST_DATABASE_URL, else vacuous
cargo test --workspace --doc
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask wasm
(cd ui && npm run lint && npm test && npm run build)
(cd ui && npm run e2e)                  # Playwright + axe; needs the three binaries and a test DB
tools/client_compat.sh                  # WBI, Pywikibot, WDI against a fresh server

docs/check.sh                           # every doc checker; exits non-zero on any failure
docs/check.sh --write                   # regenerate INDEX.md, MAP.md, chapter 23's tables, then check
```

In a fresh Linux sandbox, run `eval "$(tools/dev_env.sh)"` first. It starts Postgres, exports
`TRIPLESPACE_TEST_DATABASE_URL` and the client-compatibility `PYTHON`, and warns if it had to
fall back from the pinned Rust toolchain to stable.

Say which checks you ran and which you could not run (no Postgres, no browser, a stable
toolchain instead of the pinned one), rather than calling the work verified. Without
`TRIPLESPACE_TEST_DATABASE_URL` the database tests print "skipping" and pass vacuously.

## Working rules

- **Never commit or push without James's explicit go-ahead.** Leave the work uncommitted for
  his review.
- If a non-`main` branch is checked out, work on it. Otherwise make a named feature branch.
- Keep history linear: rebase onto `main` when other work merges; no stacked or orphaned
  branches; avoid force-pushes.
- Flag assumptions and open choices explicitly for James to confirm. Do not settle them silently.
- Prefer real data and real clients for testing: the Librarybase fixtures, `tools/*_check.py`.
  Watch for Pywikibot's API cache masking whether a fix worked.

## Design principles James applies

- Prefer a general principle over a narrow mechanism. Extend or retire an existing concept
  before adding a parallel one.
- Handle the common case well first; cover edge cases without compromising it.
- Local pages and data always take precedence in any inheritance stack.
- Default to the simpler, most permissive, reversible option (tombstones, read-time
  resolution) unless isolation, privacy or consistency argues otherwise.
- Size limits are sensible defaults against surprises, and waivable.
- "Running out of codes" is a weak reason for complexity; autonomy and privacy are strong ones.
- Keep wikitext extraction to template/parameter mapping and links; free-text NLP is a
  separate, swappable module.

## Skills

- `.claude/skills/triplespace-architecture/`: finding what the design says about a subject, and
  where in the code it lives.
- `.claude/skills/triplespace-adr/`: writing or changing an ADR, a chapter or a registry file.
