# Plan

## Working protocol

One milestone per branch and pull request. The default branch always builds. Every session starts by reading
`docs/STATUS.md`, picks the first unfinished milestone below, and ends by updating `docs/STATUS.md` with what was
verified and what was not.

## Product

Lumen reads the text a window is already showing on screen, translates it locally, and draws the result in place so the
application looks natively localized. No injection, no hooks, no memory reads: only OS-sanctioned capture (Windows
Graphics Capture, DXGI Desktop Duplication fallback), UI Automation, and a transparent click-through overlay.

## Milestones

| ID | Scope | State |
| --- | --- | --- |
| M0 | Repository, plan, architecture, design tokens, product shell UI on a mocked backend | done |
| M1 | Capture adapter, tile change detection, overlay window, headless pipeline CLI | not started |
| M2 | OCR engines behind one trait, UI Automation text source, layout analysis, language ID, CER benchmark | not started |
| M3 | Token protection, translation memory, offline engine and pack manager, optional online engines | not started |
| M4 | Inpainting, font matching, text fitting, temporal stability, RTL and vertical text, golden images | not started |
| M5 | Tauri shell wiring, onboarding, tray, hotkeys, region editor, full i18n, accessibility audit | not started |
| M6 | Performance tuning, edge cases, chaos and soak runs, updater, installer | not started |
| M7 | Release: signed installer, winget manifest, QA report, manual test plan | not started |

## Environment constraint

The development sandbox is Linux and has no access to `static.rust-lang.org`, so no Rust toolchain can be installed
here. M1 onward therefore depends on either an unblocked toolchain mirror or GitHub Actions runners (`windows-latest`
for platform code, `ubuntu-latest` for the portable crates). Until then, work that can be verified locally is the
TypeScript shell and its tests.
