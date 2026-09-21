# Status

Updated: 2026-09-21

## Done

- M0. Plan, architecture, ADRs 0001 and 0002, design tokens, and the product shell with four screens.
- M1. Rust workspace with four crates:
  - `lumen-core`: frame model with stride-correct addressing, rectangle and damage math, tile-based change detection,
    and the adaptive capture scheduler that idles at 2 Hz and backs off when the foreground application drops frames.
  - `lumen-capture`: the `CaptureSource` trait, a deterministic scripted scene source for tests and benchmarks, and a
    Windows adapter that enumerates real windows and copies their pixels.
  - `lumen-overlay`: the deterministic compositor with damage tracking, the `OverlaySurface` contract with the safety
    properties expressed as data, an in-memory surface, and the Windows layered click-through window.
  - `lumen-pipeline`: the headless harness. A scene or a PNG goes in; overlay PNGs and a JSON report with per-frame
    timings, change regions, damage and presentation cost come out.
- M1. ADR 0003 (capture path and the adapter boundary) and ADR 0004 (overlay properties are asserted, not assumed).
- M1. CI workflow at `.github/workflows/ci.yml` covering fmt, clippy with warnings denied, tests and a pipeline run on
  `ubuntu-latest` and `windows-latest`, plus lint and build for the interface.
- M1. Verified green: CI run 35629894251 passes on all three jobs (Rust ubuntu, Rust windows, interface). The Windows
  adapters compile against `windows` 0.58 with its `Param<T>` calling convention, and the full test suite passes on
  both platforms. Merged to `main` as `dea9a45` (PR #3); the disconnected PR #2 was closed as superseded.

## Done (M2)

- M2. `lumen-ocr`: the `OcrEngine` trait (`recognize` over regions, one entry per line), the scripted `StubEngine`
  double, and the character error rate benchmark runner with per-case and mean scored reports. Verified green on both
  platforms (CI run 35632210937, draft PR #4).
- M2. `lumen-ocr` text layers. `source`: `TextSpan` (`TextOrigin::{Ocr, Automation}`, bounds, confidence), the
  `TextSource` snapshot contract, `OcrSource` over any engine, and `merge()` folding recognition and UI Automation
  views — empty spans dropped, pairs overlapping with IoU ≥ 0.5 take the automation text over the recognition bounds,
  reading order preserved. `layout`: `analyze()` groups lines into blocks (same plate and language, horizontal
  overlap, at most half a line of leading) classified as button, tooltip, dialogue, menu, subtitle or body from text
  size and aspect ratio, with background (most frequent opaque pixel) and text colour (second) sampled per block.
  `language`: `identify()` names en/ru/ja/zh/ko/ar/he/el/und from Unicode blocks alone (kana and Hangul are decisive
  over Han). Verified green on both platforms (CI run 35638528319, PR #5): 34/34 tests, clippy `-D warnings` and
  rustfmt clean.
- M2. The UI Automation text source and the harness wiring. `lumen-ocr::uia` turns `UiaSource` into a `TextSource`
  over the live element tree: the control view walk (bounded by element and depth budgets) reports every named,
  on-screen element as a span with full confidence, translated from desktop to frame coordinates; the span policy is
  portable and tested everywhere, only the COM walk is platform code (`windows` 0.58). `lumen-pipeline` now runs the
  product's text stages — source snapshot, `merge()`, `layout::analyze()` — in place of its per-region placeholder
  blocks, with the scripted engine standing in for recognition; the menu scene paints ink bars across its plates so
  colour sampling sees plate and glyph pixels, and the report records the text each frame presents. Verified green on
  all three jobs (CI run 35646741096, PR #5): 37/37 tests including the three `uia` span-policy cases.

## Next

- M3. Token protection, translation memory, the offline engine and pack manager.
- `DesktopCopySource` is the interim capture path; the Windows Graphics Capture session replaces it, as recorded in
  ADR 0003.

## Blocked

- The sandbox cannot reach `static.rust-lang.org` or `crates.io`, so the full workspace (Windows adapters, the
  `png`/`serde_json`/`tracing` tree) still builds only on GitHub Actions. The `lumen-ocr` slice was additionally
  verified before the push with a locally assembled toolchain (rustc 1.88 from npm `@rustbin`, dependencies vendored
  from GitHub): all 37 tests, clippy with warnings denied, and rustfmt clean — including a `x86_64-pc-windows-msvc`
  cross-check of the UIA code with the same `RUSTFLAGS` CI uses.

## Needed from the owner

- A code-signing certificate before M6, as agreed; not a blocker until then.
