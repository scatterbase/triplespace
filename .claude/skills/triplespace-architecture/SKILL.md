---
name: triplespace-architecture
description: Find what the Triplespace design currently says about a subject, why it was decided, and which crates and registry files implement it. Use before designing, implementing or answering questions about any Triplespace feature.
---

# Navigating the Triplespace architecture

The design lives in three linked places. Know which one answers the question before reading.

| Question | Read | Not |
|---|---|---|
| What holds now? | `docs/architecture/NN-*.md` (chapters) | An ADR's Decision section, which is usually a pointer |
| Why, when, what was rejected, what is still open? | `docs/decisions/NNNN-*.md`: Context, Direction, Alternatives, Consequences, Open questions, Amendment log | The chapter, which carries none of these |
| Which code, number or name? | `docs/registry/*.toml` and its `README.md` | Tables copied into ADRs (they say "as of An") |
| What must the API look like to clients? | `docs/api/` (`wikibase-compat.md`, `mediawiki-compat.md`, `payloads*.md`, snapshots) | |

## Finding the chapter

1. Read the chapter table in `docs/architecture/README.md` (its *Covers* column is the subject
   index). The parts are: I the log (01–03), II the data model (04–06), III actors, tenancy and
   security (07–09), IV pages and content (10–15), V activity and exchange (16–17),
   VI interfaces (18–21), VII implementation (22–23).
2. Cross-cutting tables have one home each, and most are registry files shown through a
   generated chapter table (24 §11): the crate map (`crates.toml` → 22 §2.1, full entries in
   `CRATES.md`), REST routes (`routes.toml` → 18 §3.2, with a *Built* column from the OpenAPI
   contract), Action API modules (`action-api.toml` → 18 §2.3, *Built* from `paraminfo`),
   permissions (`permissions.toml` + `groups.toml` → 09 §2.2), log events (`log-events.toml` →
   16 §2.3), settings (`settings.toml` → 23 §3). Namespaces and content models are in 10 and the
   `view` schema in 03, still as prose. Link to the row; never edit a generated table.
3. ADR trailers land in fixed chapters: Crates → 22, API → 18, UI → 19, Storage and Search → 03,
   Permissions → 09, RDF → 02, Log events → 16, Rate limits → 07, Tenants → 08, Scatterbase → 22.

Chapters run from 190 to 1,600 lines, so read by section rather than whole. Use `tools/adr.py`,
which prints links between docs as plain citations to keep output short:

```sh
tools/adr.py where entity source           # chapter sections, ADRs, registry files; best first
tools/adr.py outline 05                    # a chapter's (or ADR's) headings
tools/adr.py show 03 §4.5                  # one chapter section
tools/adr.py show 0013 §7                  # an ADR section, its pointer followed to the chapter text
tools/adr.py show 0013 §7 --adr            # the ADR section as written
tools/adr.py show 0013                     # an ADR's header and outline
tools/adr.py log 0008 A6                   # one amendment, with the text it replaced
tools/adr.py questions 0090                # open questions
```

Add `--lines N` to `show` to cap a long section; `show 0098 --full` prints a whole ADR with its
chapter text. Chapter 24 is how the documents fit together.

## Following the links

- **Chapter → ADR.** Each chapter section opens with *Sources: [0013] §7; [0032] §2.*: the ADR
  sections it was assembled from. Follow them for the reasoning.
- **ADR → chapter.** A Decision section reading *Current text: [03] §4.5.* has its text in that
  chapter section. Its heading, number and *Changed by An* line stay in the ADR.
- **ADR → ADR.** `docs/decisions/INDEX.md` lists each ADR's status, last update, chapters and
  what changes it; the header's `Changes` says what it changes.
- **ADR section → chapter, in bulk.** `python3 docs/architecture/map.py --sections` prints every
  Decision section with the chapter sections that cite it. Every section is cited by some chapter
  (0050 and 0098, the process ADRs, by chapter 24); the only exceptions are the stubs.
- **History of a passage.** The ADR's Amendment log quotes replaced text verbatim. Cite text as
  it was by the entry that replaced it (`0008 A6`), never by date.

## Citations

`NNNN` an ADR; `NNNN §N` or `§N.M` a section as it stands now; `NNNN An` an amendment and the
text it replaced; `NNNN Qn` an open question; `NN §N.M` a chapter section (`02 §3.1`). Inside an
ADR a bare `§N`, `An` or `Qn` means that ADR. Section numbers are permanent, so citations are
stable.

## From design to code

- The crate map is `docs/registry/crates.toml`: every crate, its layer, what it holds, what it may
  depend on, and its `pure`/`wasm` marks. Chapter 22 §2.1 shows it one line per crate and
  `docs/architecture/CRATES.md` in full; `cargo xtask deps` checks the real graph against it.
- A registry file's `README.md` row names the crate that embeds it and the ADR sections that
  define it.
- **Is it built? The code says (24 §15).** `tools/coverage.py NNNN` gives each section's status:
  *done* (an `implements NNNN §N (An)` claim in src, a test citing it, no later log entry),
  *amended* (changed since the claim), *untested*, *partial* (claimed in part or only cited as
  about), *not* (cited as not built yet), *none*. The ADR's `Status` header and INDEX's *Built*
  column are written from this; `coverage.py --todo` is the backlog; `coverage.py NNNN §N` and
  `tools/adr.py code NNNN §N` list the citing lines. Read them before relying on a status: an
  about-citation (`0090 §5`) means the code is about the section, not that it is finished.
- `docs/decisions/INDEX.md` also lists every *unconfirmed* decision (an entry Claude logged while
  building, 24 §6) and each ADR's open-question count: what waits on James is there, not in notes.
- `docs/architecture/22` §4.9 and `tools/README.md` say how each contract is tested.

## Before proposing a design

1. Read the home chapter and the Open questions of the ADRs it cites. The question may be
   settled, withdrawn or already open.
2. Look for an existing concept to extend or retire before adding a parallel one. Chapter 00's
   glossary names the concepts.
3. Check the registry for reserved codes, numbers and *Pending allocations*.
4. Where the ADRs leave a choice open, put it to James instead of picking one.

To change anything you find, use the `triplespace-adr` skill.
