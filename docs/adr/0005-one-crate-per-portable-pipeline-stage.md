# 5. One crate per portable pipeline stage

Status: accepted

## Context

M2 was delivered twice in parallel, with two decompositions of the same slice:

- PR #5 put the text sources, the merge, layout analysis, language identification and the UI
  Automation adapter inside `lumen-ocr` as modules (`source.rs`, `layout.rs`, `language.rs`,
  `uia.rs`).
- PR #6 gave each portable stage its own crate — `lumen-source`, `lumen-layout`,
  `lumen-language` — with the Windows adapter isolated inside `lumen-source`.

Only one decomposition can merge; the other has to be rebased onto it or closed. While both were
open the milestone had three pull requests against one `docs/PLAN.md` item, no session knew which
lineage to build on, and the corpus generator could not choose its dependencies. `docs/PLAN.md`
allows "a crate or a module behind a trait" per stage, so the plan itself did not settle it.

## Decision

The separate-crate decomposition (the lineage of PR #6) is the one the project builds on:

1. **The architecture document already draws these boundaries.** `docs/ARCHITECTURE.md` separates
   portable stages from platform stages; crate boundaries make the separation enforceable instead
   of advisory. `lumen-layout` and `lumen-language` carry no platform dependency at all, and the
   COM/UI Automation adapter is confined to `lumen-source`, the only crate of the three that
   depends on `windows`. Folding them into `lumen-ocr` would make the recognition trait crate drag
   the accessibility dependency tree everywhere the trait goes.
2. **The dependency graph stays explicit and acyclic.** `lumen-corpus` depends on `lumen-ocr` and
   `lumen-language`; the pipeline depends on all four. Between modules of one crate nothing stops
   `layout` from calling `language` from calling `source` until the cycle exists.
3. **CI economics.** The Ubuntu job compiles and tests every portable line; the smaller the
   platform-bearing crate, the less code is verified only by "it type-checks on Windows".
4. **The work already sits on it.** The pipeline wiring, 84 tests across the three crates, and the
   corpus generator with its CER gate were built against the separate crates. Adopting #5 would
   mean re-deriving all of it for a different file layout, with no functional gain.

Per `docs/WORKFLOW.md` ("one unit of work is one branch, one pull request, one milestone"), M2
lands as a single pull request carrying the whole milestone: the OCR slice from PR #4, the crates
from PR #6, and the corpus generator. PR #5 is closed as superseded — its content remains readable
at `refs/pull/5/head`; nothing is deleted. PRs #4 and #6 are closed when the consolidated pull
request merges, and the stale session branches are removed.

## Consequences

Future stages follow the same rule: a portable stage gets its own crate (translation memory,
glossary, rendering), and a platform adapter lives in the crate that owns the seam, the way
`lumen-source` owns UI Automation and `lumen-capture` owns the Windows capture path. The cost is
more `Cargo.toml` files and a longer workspace member list, which is accepted: the boundary is
worth the paperwork, and the milestone-level pull request keeps the history on `main` linear —
one squashed commit per milestone, as with M0, M1 and now M2.
