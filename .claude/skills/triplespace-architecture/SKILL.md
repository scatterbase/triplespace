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
2. Cross-cutting tables have one home each: namespaces and content models in 10, the REST
   route table in 18, the permission catalogue in 09, log events in 16, the `view` schema in 03,
   the crate map in 22 §2.1, the settings catalogue in 23. Link to the row; don't copy it.
3. ADR trailers land in fixed chapters: Crates → 22, API → 18, UI → 19, Storage and Search → 03,
   Permissions → 09, RDF → 02, Log events → 16, Rate limits → 07, Tenants → 08, Scatterbase → 22.

Chapters run from 190 to 1,600 lines, so read by section rather than whole:

```sh
cd docs
grep -n '^## \|^### ' architecture/05-providers-and-ingest.md      # outline
grep -rn 'entity source' architecture/ | cut -c1-160                # find a subject
sed -n '/^### 4.5 /,/^### 4.6 /p' architecture/03-storage-caches-and-search.md   # one section
```

## Following the links

- **Chapter → ADR.** Each chapter section opens with *Sources: [0013] §7; [0032] §2.*: the ADR
  sections it was assembled from. Follow them for the reasoning.
- **ADR → chapter.** A Decision section reading *Current text: [03] §4.5.* has its text in that
  chapter section. Its heading, number and *Changed by An* line stay in the ADR.
- **ADR → ADR.** `docs/decisions/INDEX.md` lists each ADR's status, last update, what it
  changes and what changes it. Within an ADR, the header's `Changes`/`Uses` fields and the
  *Changes to other ADRs* table carry the same graph.
- **ADR section → chapter, in bulk.** `docs/architecture/MAP.md` assigns every numbered
  Decision section to its chapter(s); `—` means process, not architecture (0050, the changelog
  stubs of 0005 §8 and 0013 §12).
- **History of a passage.** The ADR's Amendment log quotes replaced text verbatim. Cite text as
  it was by the entry that replaced it (`0008 A6`), never by date.

## Citations

`NNNN` an ADR; `NNNN §N` or `§N.M` a section as it stands now; `NNNN An` an amendment and the
text it replaced; `NNNN Qn` an open question; `NN §N.M` a chapter section (`02 §3.1`). Inside an
ADR a bare `§N`, `An` or `Qn` means that ADR. Section numbers are permanent, so citations are
stable.

## From design to code

- The crate map is chapter 22 §2.1: every crate, its layer, what it owns and whether it builds
  for wasm. `cargo xtask deps` checks the real graph against it.
- A registry file's `README.md` row names the crate that embeds it and the ADR sections that
  define it.
- `rg` the crate for the ADR number: code comments cite sections (`0090 §5`).
- `docs/architecture/22` §4.9 and `tools/README.md` say how each contract is tested.

## Before proposing a design

1. Read the home chapter and the Open questions of the ADRs it cites. The question may be
   settled, withdrawn or already open.
2. Look for an existing concept to extend or retire before adding a parallel one. Chapter 00's
   glossary names the concepts.
3. Check the registry for reserved codes, numbers and *Pending allocations*.
4. Where the ADRs leave a choice open, put it to James instead of picking one.

To change anything you find, use the `triplespace-adr` skill.
