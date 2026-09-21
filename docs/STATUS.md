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

## Done (M2, in progress)

- M2. `lumen-ocr`: the `OcrEngine` trait (`recognize` over regions, one entry per line), the scripted `StubEngine`
  double, and the character error rate benchmark runner with per-case and mean scored reports. Verified green on both
  platforms (CI run 35632210937, draft PR #4).

## Next

- M2. The UI Automation text source (Windows), layout analysis (reading order, blocks), and language identification.

## Known limitations

- The harness synthesises one overlay block per changed region. Real recognised text arrives in M2.
- `DesktopCopySource` is the interim capture path; the Windows Graphics Capture session replaces it, as recorded in
  ADR 0003.

## Blocked

- The development environment cannot resolve a Rust toolchain or dependencies locally. All Rust verification happens
  on GitHub Actions.

## Needed from the owner

- A code-signing certificate before M6, as agreed; not a blocker until then.
