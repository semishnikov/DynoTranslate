# Status

Updated: 2026-09-22

The goal does not change between sessions and is stated in full at the top of `docs/PLAN.md`:
translate the text on screen in real time and draw it where it stands, so a foreign game or
application reads as though it shipped localized. `docs/WORKFLOW.md` covers how work lands.

## Done

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
  and mean scored reports. Green on both platforms (CI run 35632210937). Open as draft PR #4.
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
  - 22 unit tests over the portable half. Open as draft PR #6.
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

## Verification

- CI run 35653140418 at `ceff233` passes `Rust (ubuntu-latest)`, `Rust (windows-latest)` and
  `Interface`: formatting, clippy with warnings denied, the 22 tests in `lumen-source` and the 30
  in `lumen-layout`, and the headless pipeline run on both platforms.
- `lumen-layout` is portable, so every line of it is compiled, linted and tested on Linux as well
  as on Windows. The classification thresholds are pinned by tests rather than left to judgement.
- `lumen-source/src/windows.rs` compiles only on `windows-latest` and has no tests of its own: it
  needs a real desktop session. Its correctness is limited to "it type-checks and lints clean on
  Windows".
- Neither crate has been run against a real application, and no real screenshot has been through
  the layout analysis. The classification is verified against synthetic geometry only.

## Next

1. Wire a source and the layout analysis into `lumen-pipeline`, replacing the stand-in that emits
   one block per changed region, and report the blocks in the JSON output.
2. **M2, language identification.** Unicode script analysis first, then text-level detection, with
   per-application stickiness and hysteresis. Not started.
3. **M2, corpus generator.** Synthetic scenes across fonts, scripts and backgrounds with CER
   thresholds enforced in CI. Not started.
4. Font weight and text effects per block, which layout deliberately leaves to the fidelity work
   in M4.

PR #6 stays a draft until the branch conflict below is decided; its build is green and the slices
it carries are complete.

## Known limitations

- The harness still synthesises one overlay block per changed region. The source and layout stages
  exist and are tested but are not wired into it yet.
- `DesktopCopySource` is the interim capture path; the Windows Graphics Capture session replaces
  it, as recorded in ADR 0003.
- There is no `LICENSE` file in the repository. See the open decisions in `docs/PLAN.md`.

## Conflicting branches

PR #5 (branch `arena/01a0c522-dynotranslate`) delivers the same M2 slice with a different
decomposition: its text sources, merge, layout analysis and language identification live inside
`lumen-ocr` (`source.rs`, `layout.rs`, `language.rs`, `uia.rs`) instead of a separate
`lumen-source` crate, and it is open and marked ready for review. PR #4 and PR #5 overlap on the
OCR slice, and PR #5 and PR #6 overlap on the text sources. Only one decomposition can merge; the
other has to be rebased onto it or closed. This needs a decision before more work lands on either
side.

## Needed from the owner

- A decision on the branch conflict above: which decomposition is the one to build on.
- A `LICENSE` choice. It is a legal decision, so it belongs to the owner; it blocks the first
  release, not the next milestone.
- A product name. The code says Lumen, the repository says DynoTranslate, and "Lumen" alongside
  translation is widely used by unrelated projects. Three candidates have to be checked against
  GitHub, winget and the Microsoft Store.
- A code-signing certificate before M6; not a blocker until then.
