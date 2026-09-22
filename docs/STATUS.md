# Status

Updated: 2026-09-22

The goal does not change between sessions and is stated in full at the top of `docs/PLAN.md`:
translate the text on screen in real time and draw it where it stands, so a foreign game or
application reads as though it shipped localized. `docs/WORKFLOW.md` covers how work lands.

## Done

- **M5.** Tauri shell wiring, onboarding, tray, hotkeys, region editor, full i18n and the
  accessibility audit. Merged as PR #10; `main` is at `6b5705b`.
- **M4.** Portable renderer (fitting, inpainting, bundled faces), temporal stability, stroke-weight
  estimation, RTL/vertical writing modes, visual regression suite, ADR 0006, and the pipeline
  wired through stability + stub translation. Green on all three jobs (CI run 35728755276) and
  merged as PR #9; `main` is at `b4573a9`.
- **M3.** Token protection, translation memory and cache, offline pack manager, glossary and the
  fallback engine stack. Green on all three jobs (CI run 35718597892) and merged as PR #8;
  `main` is at `8fd881a`.
- **M0.** Plan, architecture, ADRs 0001 and 0002, design tokens, and the product shell with four
  screens. Merged as PR #1.
- **M1.** Rust workspace with four crates:
  - `lumen-core`: frame model with stride-correct addressing, rectangle and damage math,
    tile-based change detection, and the adaptive capture scheduler that idles at 2 Hz and backs
    off when the foreground application drops frames.
  - `lumen-capture`: the `CaptureSource` trait, a deterministic scripted scene source for tests
    and benchmarks, and a Windows adapter that enumerates real windows and copies their pixels.
  - `lumen-overlay`: the deterministic compositor with damage tracking, the `OverlaySurface`
    contract with the safety properties expressed as data, an in-memory surface, and the Windows
    layered click-through window.
  - `lumen-pipeline`: the headless harness. A scene or a PNG goes in; overlay PNGs and a JSON
    report with per-frame timings, change regions, damage and presentation cost come out.
- **M1.** ADR 0003 (capture path and the adapter boundary) and ADR 0004 (overlay properties are
  asserted, not assumed). Verified green on all three jobs (CI run 35629894251) and merged as PR
  #3; `main` is at `dea9a45`.
- **M2, OCR slice.** `lumen-ocr`: the `OcrEngine` trait (`recognize` over regions, one entry per
  line), the scripted `StubEngine` double, and the character error rate benchmark with per-case
  and mean scored reports. Green on both platforms (CI run 35632210937). Carried by the
  consolidated M2 pull request; the slice PR #4 closes at the merge (ADR 0005).
- **M2, text sources.** `lumen-source`:
  - `TextSource`, the trait both recognition and operating-system sources implement, with
    `ReadRequest` (frame, target window, regions of interest) and `TextRun` (text, bounds, source,
    confidence).
  - `merge`, which resolves two sources into one list: the operating system outranks recognition,
    then confidence, then text length; a run sharing at least `MergePolicy::min_iou` (default 0.5)
    of its area with an accepted run is dropped; survivors come back in reading order.
  - `OcrTextSource`, the bridge that turns any `OcrEngine` into a source, and `StubSource` for
    tests.
  - `UiAutomationSource`, the Windows adapter: initialises COM, walks the target window's
    accessibility tree, reads each element's text pattern line by line and its value pattern as a
    fallback, converts desktop coordinates into frame pixels and discards off-screen elements.
  - 22 unit tests over the portable half. Carried by the consolidated M2 pull request; the slice
    PR #6 closes at the merge (ADR 0005).
- **M2, layout analysis.** `lumen-layout`, the portable stage between the text sources and
  translation:
  - `group_lines` joins runs that share a baseline into rows, by vertical overlap and by a
    horizontal gap measured in line heights.
  - `group_blocks` joins rows that sit close vertically and share a column into one paragraph,
    list or dialogue box.
  - `analyse` classifies every block as a button, menu entry, tooltip, dialogue, subtitle or label
    from geometry and measured colour, works out whether it is left, centred or right in the
    window, and reports an estimated glyph height and a confidence.
  - Colour sampling measures rather than assumes: the background is the modal colour of a ring
    just outside the text, the foreground the commonest colour inside it that is not that
    background, with a legibility fallback when a block has no ink of its own to measure.
  - 30 unit tests, 935 lines, no platform code.
- **M2, pipeline wiring.** The harness now runs the real path instead of a stand-in:
  - a scripted scene carries the text it stands for, and `SceneTextSource` reports it the way a
    recognition engine would, frame by frame;
  - `lumen-pipeline` reads, merges, analyses and composes, so the code the benchmarks measure is
    the code the product will run;
  - the report lists the text each frame produced, with its box, alongside the timings.
  - A plain PNG still gets one invented block per changed region, because no engine reads an image
    yet.
- **M2, language identification.** `lumen-language`, 1112 lines, no model and no training data:
  - `script_of` and `dominant_script` settle the writing system from the code points, which needs
    no evidence at all, and narrow a passage to a handful of candidates.
  - `identify` scores the candidates on three things: letters only one of them uses, the function
    words grammar forces into a sentence, and the menu words that appear on nearly every screen of
    an application. It returns a language and a confidence, and `Unknown` when there is nothing to
    go on.
  - `Tracker` holds the answer per window and only gives it up to a rival that turns up
    `switch_after` times running and with a real margin, so a loading screen of numbers or one
    short button cannot flip a session. A window with no language yet takes the first real answer
    at once — hysteresis protects an answer that exists.
  - The pipeline feeds every pass through it and reports the language per frame and for the run.
  - 32 tests. The limits are stated where they belong: two languages that share an alphabet and a
    vocabulary get a low confidence rather than a confident guess, and Han on its own cannot be
    told from kanji-only Japanese.
- **M2, corpus generator.** `lumen-corpus`: the synthetic recognition corpus, its gate and its
  harness — 3658 lines, 66 tests, no platform code:
  - `font` and `render`: every glyph is a stroke skeleton on an integer design grid — 139
    skeletons plus 35 aliases covering the Latin and Cyrillic alphabets, digits and interface
    punctuation — drawn as antialiased capsules in regular, bold and italic. The ground-truth box
    of a line is measured from the pixels the draw actually changed rather than promised from
    metrics, and a character without a skeleton is an error, never blank ink; Greek, Han, kana,
    hangul, Arabic, Hebrew, Thai and Devanagari stay out of the corpus until they have skeletons.
  - `phrases`: 87 interface phrases across 11 language packs, written in the menu vocabulary
    identification scores against, every one checked against the font at test time.
  - `scene` and `plan`: deterministic composition — seed in, pixels and truth out — over the full
    product of language × style × background, with shrink-to-fit sizing, six contrast palettes and
    solid, gradient and noise backgrounds; one plan seed fans out to distinct per-scene seeds.
  - `gate`: every scene becomes a `lumen-ocr` benchmark case and the mean character error rate of
    each category is held under the plan's ceilings (3 % clean UI text, 8 % stylised) by workspace
    tests CI runs; every transcript is scored through `lumen-language::identify`, so the
    identification scoring now measures against corpus material instead of hand-written phrases.
  - `report` and the `lumen-corpus` binary: PNGs plus a `corpus.json` ground-truth manifest and a
    `report.json` of both scores, exit status = gate verdict — the hook a CI step can use.
  - The honest limit, stated in the crate docs as well: the engine under test is the corpus's own
    scripted double — ground truth replayed through `StubEngine` — because no engine reads pixels
    yet. What is pinned is the corpus, the scoring path and the thresholds; no real engine's
    accuracy is claimed.

## Verification

- CI run 35714597253 at `9fb1872` passes all three jobs: `Rust (ubuntu-latest)`, `Rust (windows-latest)` and
  `Interface`. All 222 workspace tests (including the 66 `lumen-corpus` tests, the CER threshold assertions,
  and the language identification evaluation against corpus material) pass cleanly, Clippy reports zero
  warnings, formatting is verified, and the headless pipeline runs green on both Linux and Windows.
- `lumen-layout`, `lumen-language` and `lumen-corpus` are portable, so every line of all three is compiled,
  linted and tested on Linux as well as on Windows. Their thresholds are pinned by tests rather than left to
  judgement.
- `lumen-source/src/windows.rs` compiles only on `windows-latest` and has no tests of its own: it
  needs a real desktop session. Its correctness is limited to "it type-checks and lints clean on
  Windows".
- Neither crate has been run against a real application, and no real screenshot has been through
  the layout analysis. The classification is verified against synthetic geometry only.
- The pipeline artifact cannot be downloaded from this environment (`gh run download` fails with
  `EOF` on `productionresultssa2.blob.core.windows.net`), so the report numbers were not read back;
  the assertions in `a_scene_run_reports_the_text_the_pipeline_read`, which ran on both platforms,
  are what confirms the wiring.

## Done: M6 branch (performance, soak and chaos, edge cases)

On branch `arena/01a0c968-dynotranslate` (PR #12), continued after the previous session hit its
limit. Green on all three jobs (CI run 35746341895) and marked ready for review.

What the continuation session did, oldest first:

- Borrow-check fix (`merged.clone()`), then the formatting backlog the annotation cap hid
  behind earlier diffs: all multi-argument calls over the 72-character cap broken one per line
  with trailing commas, overlong chains and struct literals restructured.
- Lint round: the static-skip check binds the previous pass with `if let` (tuple form, MSRV is
  still 1.77, so no let-chains), the driven-scene tuple has an alias, test-only runner
  instrumentation is `cfg(test)`, and the never-read source counters are gone.
- Two compositor damage tests asserted the block rect exactly; paint covers the block plus its
  ink overflow by design, so they now assert one region covering the block corners, like the
  neighbouring damage tests.
- CI failure reporting: every cli test binary installs the `test-panic` hook, the Test step tees
  its output, and the failure reporter restates failed binaries/tests plus the release-run tail.
- The budgets test compared a skipped frame against exact detection parity (missed by 7 µs on
  a shared agent) and capped a changed pass at 500 ms (cold font work alone costs two seconds
  in an unoptimized build). The relative ceiling is now twice the worst detection, the absolute
  one five seconds, with the rationale in the comment.
- The counting allocator delegates to `System` explicitly.
- Formatting rules proven against green code this session: call/macro arguments stay whole up
  to exactly 72 characters (inclusive) and break above it; a single argument uses the full
  width; `fn` signatures join while the whole line fits 120; match-arm tuples stay vertical.

The updater and installer joined the same branch. `src-tauri/` moved under `app/` (standard
Tauri layout, so `frontendDist` resolves); the icon set is generated from a placeholder
`icon-source.png` and committed; `release.yml` builds the MSI/NSIS installers on pull requests
and `main` and turns `v*` tags into signed draft releases; the `shell` job checks the Tauri
crate on `windows-latest`. The updater reads releases from this repository with a read-only
token baked into the binary (owner choice over a public mirror or a public repo); the signing
keypair was generated in-session, the public half is committed in `tauri.conf.json`, the private
half travels to the owner in chat only. The Settings About card checks, downloads, installs and
relaunches through a new updater store, verified locally with `tsc`, `vite build` and `oxlint`.

## In progress: installer fix and the edge-case pass

Continued on `arena/01a0ca15-dynotranslate`, branched from `19219f9`, because that session's
branch cannot be pushed to from here. This supersedes PR #12 the way #12 superseded #11.

What was verified on `19219f9` (CI run 35757463931): Interface, Shell, Rust ubuntu and Rust
windows are green. The bundle job (run 35757463866) failed after 16 minutes. Its annotations
were the tail of `tauri info`, not the build error: GitHub keeps about ten error annotations
per step, and the reporter emitted `tauri info` first, so the error never left the runner.
`gh run view --log` still dies with `EOF` on the log host.

The ephemeral key was exported as `TAURI_SIGNING_PRIVATE_KEY_PATH`. `tauri build` does not
read that variable ([tauri#15028](https://github.com/tauri-apps/tauri/issues/15028)); it reads
`TAURI_SIGNING_PRIVATE_KEY` as key contents or as a path. The workflow now sets that name.
Run 35760746298 confirmed the Windows process can open the file (the preflight step passed)
and then failed in `Build the installer`. The annotations from that run are `thiserror`
download lines: the reporter grepped for `error`, and that crate name filled the budget.
The reporter now emits the last eight lines of the build log, which is where the bundler
prints the failure. The bundle job is not green.

Pushed in the same commits, not yet confirmed by CI: merge drops empty and zero-area runs;
stability does not queue blank text; edge tests cover a 1×1 frame, padded strides, a sub-tile
frame, a zero-height line, a one-pixel block, an empty fit, token overlap and a dropped
sentinel, translation-memory normalisation and the capacity floor, and the circuit breaker
reopening from half-open. Nothing here was executed locally. There is still no Rust toolchain
in this environment.

## Next

1. **Land M6** from `arena/01a0ca15-dynotranslate` (it supersedes PR #12; do not merge #12, its
   installer job is red for the reason above). Merge after `bundle-windows` is green on the new
   pull request. Then the owner stores the two secrets (`TAURI_SIGNING_PRIVATE_KEY` from the
   chat handoff, plus a fresh read-only `UPDATER_PAT`) and rehearses one `v*` tag into a draft
   release on a desktop machine. If the bundle job fails again, the build step's annotations
   are the error; read those before changing the workflow.
2. **Golden images.** The suite compares `crates/render/tests/golden/label.png` when it exists
   and otherwise falls back to invariants. Generating the first golden needs a machine that can
   run `cargo test` (the development environment cannot), so it is an owner step: render the
   label scene, save it as that path, re-run.
3. **The gate step in the workflow.** `lumen-corpus` exits on the gate verdict, but adding a step
   that runs it touches `.github/workflows/**`, which is owner-only under `docs/WORKFLOW.md`. Until
   it lands, the workspace tests CI already runs carry the thresholds.
4. **An engine that reads pixels.** The gate is measured with the scripted double; the first real
   engine is passed to `gate::score_recognition` in the double's place and nothing else changes.
   The corpus font's remaining scripts (Greek, Han, kana, hangul, Arabic, Hebrew, Thai,
   Devanagari) join as skeleton additions when the languages that need them do.
5. Text effects beyond weight (outline detection from the source pixels), which layout still
   leaves alone.
6. M7: Release: signed installer, winget manifest, QA report, manual test plan. The shell
   still keeps settings in memory only; the architecture's versioned settings file is not
   wired, and the interface does not call the shell commands yet. That is the first gap after
   M6 lands, not a reason to hold the installer.

## Known limitations

- The harness reads the whole frame on every pass, so text that stopped moving is not forgotten.
  Skipping work on unchanged tiles needs the previous pass to be reusable, which is still open.
- Recognition of a plain PNG is still the region stand-in; no engine reads an image yet.
- Language identification is orthography, not a trained classifier. It is exact for the script and
  for languages with letters of their own, and a leaning for the rest. Replacing the scoring with a
  small model stays open; translation now gives something to compare against.
- Font matching runs against the six bundled DejaVu faces only (ADR 0006). There is no family name
  to match against from recognition, and system fonts are deliberately not consulted, so a game set
  in a display face is rendered in DejaVu.
- `DesktopCopySource` is the interim capture path; the Windows Graphics Capture session replaces
  it, as recorded in ADR 0003.
- There is no `LICENSE` file in the repository. See the open decisions in `docs/PLAN.md`.

## Resolved branch conflict

The M2 decomposition conflict between PR #5 (monolithic `lumen-ocr`) and PR #6 (separate crates
for portable stages) was decided in ADR 0005 in favour of separate crates. PR #5 has been closed
as superseded. M2 lands as a single pull request carrying the whole milestone from OCR through the
corpus gate; PRs #4 and #6 will be closed when it merges.

## Needed from the owner

- A `LICENSE` choice. It is a legal decision, so it belongs to the owner; it blocks the first
  release, not the next milestone.
- A product name. The code says Lumen, the repository says DynoTranslate, and "Lumen" alongside
  translation is widely used by unrelated projects. Three candidates have to be checked against
  GitHub, winget and the Microsoft Store.
- A Windows Authenticode certificate before the signed installer in M7. The updater minisign
  key is a different key; its private half is the `TAURI_SIGNING_PRIVATE_KEY` secret, handed
  over in chat, and is not this certificate.

