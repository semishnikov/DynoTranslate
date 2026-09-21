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
- M1. CI workflow covering fmt, clippy with warnings denied, tests and a pipeline run on `ubuntu-latest` and
  `windows-latest`, plus lint and build for the interface.

## Next

- M2. OCR engines behind one trait, the UI Automation text source, layout analysis, language identification, and the
  character error rate benchmark.

## Known limitations

- The Rust workspace has not been compiled anywhere yet. It is written against the documented APIs of `windows` 0.58,
  `png` and `serde`, but no build has confirmed it. The first CI run is the verification, and any compile errors it
  reports are fixed at the start of the next session.
- The harness synthesises one overlay block per changed region. Real recognised text arrives in M2.
- `DesktopCopySource` is the interim capture path; the Windows Graphics Capture session replaces it, as recorded in
  ADR 0003.

## Blocked

- The development environment cannot reach `static.rust-lang.org` or `crates.io`, so no Rust toolchain or dependency
  can be resolved locally. All Rust verification happens on GitHub Actions.
- The CI workflow could not be pushed: the GitHub App backing this session lacks the `workflows` permission, and the
  push is rejected. The file is committed at `docs/ci/ci.yml` and must be copied to `.github/workflows/ci.yml` by the
  owner, or the permission granted.

## Needed from the owner

- Copy `docs/ci/ci.yml` to `.github/workflows/ci.yml` (or grant the `workflows` permission) so the Rust workspace is
  actually built and tested.
- A code-signing certificate before M6, as agreed; not a blocker until then.
