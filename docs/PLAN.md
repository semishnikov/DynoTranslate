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
| M1 | Capture adapter, tile change detection, overlay window, headless pipeline CLI | done |
| M2 | OCR engines behind one trait, UI Automation text source, layout analysis, language ID, CER benchmark | done |
| M3 | Token protection, translation memory, offline engine and pack manager, optional online engines | not started |
| M4 | Inpainting, font matching, text fitting, temporal stability, RTL and vertical text, golden images | not started |
| M5 | Tauri shell wiring, onboarding, tray, hotkeys, region editor, full i18n, accessibility audit | not started |
| M6 | Performance tuning, edge cases, chaos and soak runs, updater, installer | not started |
| M7 | Release: signed installer, winget manifest, QA report, manual test plan | not started |

## Translation engines

The offline engine is the default and the only one required for the product to work: it runs on the user's machine, it
costs nothing and no text leaves the computer. Online engines (DeepL, Google, Microsoft, a custom LLM endpoint) are
optional modules behind the same `TranslationEngine` trait, enabled per application with a user-supplied key, and they
fall back to the offline engine on timeout or error. Landing in M3.

## Environment constraint

The development environment is Linux and cannot reach `static.rust-lang.org` or `crates.io`, so no Rust toolchain or
crate can be resolved there. Rust is therefore authored locally and compiled, linted and tested on GitHub Actions:
`ubuntu-latest` for the portable crates and `windows-latest` for the platform adapters. Nothing in the Rust workspace
is claimed to build until that workflow reports it.
