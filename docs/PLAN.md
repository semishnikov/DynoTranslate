# Plan

## Mission

Build a desktop application that translates any text on screen, in real time, into the user's own
language and draws the result where the original stands, so a foreign game or application reads as
though it shipped localized.

The measure of success is a single experience: someone starts a game written in another language
and, within about a second of a menu appearing, sees it in their own language, in a matching
style, with no clicks and no configuration. The same holds for any window on the desktop. A
non-technical person of any age goes from installer to first translated screen in under 90 seconds
without reading anything.

Everything else in this plan exists to serve that. It is the reason the project exists, and it
does not change between sessions.

## Scope and constraints

Windows 10 (21H2) and 11, x64 first; OS-specific code stays behind thin adapters so other
platforms remain feasible. Russian and English at reference quality, plus at least eighteen more
interface languages.

When requirements conflict, this order decides: **safety and privacy, then stability, then
correctness, then perceived speed, then visual polish, then extra features.**

Non-negotiables:

1. No injection, hooking, memory reading or drivers. Only OS-sanctioned capture (Windows Graphics
   Capture, DXGI Desktop Duplication as fallback), UI Automation and a transparent click-through
   overlay. This is what keeps the product safe to run next to anti-cheat software.
2. Fail open. Any pipeline error removes the overlay and leaves the host application untouched.
   The overlay never takes focus, never intercepts input and is never captured by its own
   pipeline.
3. Offline-first and private. Fully functional offline once language packs are installed, no
   telemetry by default, screenshots and recognised text never persisted or sent anywhere except
   through an online engine the user enabled for that application.
4. Everything real. No stubs left in shipped paths, no `TODO`, no placeholder data. Every control
   works and every state is designed.
5. Honesty. Nothing is claimed to work or to be tested unless it was run. What could not be
   verified in this environment is said so, in the pull request and in `docs/STATUS.md`.

## Working protocol

One milestone per branch and pull request. The default branch always builds. Every session starts
by reading `docs/STATUS.md` and this file, picks the first unfinished milestone, and ends by
updating `docs/STATUS.md` with what was verified and what was not. Branching, commits, the CI
contract and how to read a failing build are in `docs/WORKFLOW.md`.

## Milestones

| ID | Scope | State |
| --- | --- | --- |
| M0 | Repository, plan, architecture, design tokens, product shell UI on a mocked backend | done |
| M1 | Capture adapter, tile change detection, overlay window, headless pipeline CLI | done |
| M2 | OCR engines behind one trait, UI Automation text source, source merge, layout analysis, language ID, corpus generator and CER benchmark | done |
| M3 | Token protection, translation memory and cache, offline engine and pack manager, glossary, optional online engines with fallback | in progress |
| M4 | Inpainting, font matching, text fitting, temporal stability, RTL and vertical text, visual regression suite | not started |
| M5 | Tauri shell wiring, onboarding, tray, hotkeys, region editor, full i18n, accessibility audit | not started |
| M6 | Performance tuning, edge cases, chaos and soak runs, updater, installer | not started |
| M7 | Release: signed installer, winget manifest, QA report, manual test plan | not started |

M2 breaks down into: OCR engines behind `OcrEngine` (trait, scripted double and CER benchmark:
done); the `TextSource` abstraction with the UI Automation adapter and the source merge (done);
layout analysis — runs to lines to blocks, classified, aligned, with foreground and background
measured per block (done); language identification — script from the code points, then orthographic
cues, held steady per window (done); and the synthetic corpus generator with CER thresholds
enforced in CI (done, PR #7 merged).

M3 breaks down into: token protection and normalization (numbers, key hints, placeholders,
tags, URLs, do-not-translate literals); translation memory and cache (LRU, stats, glossary
version isolation); offline engine pack manager and catalog; per-application and global
glossaries with word boundary matching; dialogue/LLM context sliding window; engine trait with
scripted StubTranslationEngine; and online engine fallback with circuit breaker.

## Pipeline

The stages, in order. Each is a crate or a module behind a trait, so it can be tested on Linux
with a scripted double.

1. **Capture** — Windows Graphics Capture per window or monitor, GPU-side crop and downscale,
   HDR to SDR before recognition, per-monitor DPI, tile-based change detection so unchanged tiles
   are never reprocessed, adaptive rate with a hard cap and back-off when the host drops frames.
2. **Text sources** — UI Automation text and bounds where the operating system provides them,
   OCR for everything else, then merge and de-duplicate: the accessibility tree outranks
   recognition, and two runs sharing half their area are one run.
3. **OCR** — one trait, several engines: `Windows.Media.Ocr`, RapidOCR or PaddleOCR on ONNX
   Runtime with DirectML, Tesseract as fallback. Chosen by script and confidence. Vertical text,
   right-to-left, mixed scripts, small text, light-on-dark, outlined and glowing text.
4. **Layout analysis** — runs to lines to blocks to UI elements, using geometry, alignment,
   colour and font metrics; reading order preserved; foreground and background sampled per block.
5. **Language identification** — Unicode script analysis first, then text-level detection, with
   per-application stickiness and confidence hysteresis.
6. **Translation** — normalise, protect tokens (numbers, key hints such as `[E]` or `Ctrl+S`,
   placeholders, URLs, do-not-translate names), look up the translation memory, then call an
   engine. Whole dialogue blocks in context, never fragments. Per-application glossaries.
7. **Rendering** — composite the overlay with a portable software renderer so it is deterministic
   and testable with golden images; present through a layered window, updating dirty rectangles
   only. Erase the original, then draw the translation in the closest matching font, colour,
   weight and outline, fitted by measurement and never ellipsised.
8. **Temporal stability** — track blocks across frames, reuse translations when recognition
   jitters, require agreement over several frames before replacing, never translate numeric-only
   text.

## Quality targets

Measured and reported honestly; where one is unreachable, the best achieved value is recorded with
the reason. Reference machine: a mainstream six-core CPU with integrated graphics at 1080p.

| Target | Value |
| --- | --- |
| Installer to first translated screen, 50 Mbps | ≤ 90 s |
| Cold start to tray | ≤ 1.5 s |
| Main window interactive | ≤ 800 ms |
| Text change to translated text visible | p50 ≤ 250 ms, p95 ≤ 500 ms |
| CPU on a static screen (no recognition calls) | ≤ 2 % |
| CPU while translating / RAM with one pack / VRAM | ≤ 8 % / ≤ 600 MB / ≤ 200 MB |
| Impact on host frame time | ≤ 3 % |
| OCR character error rate, clean UI text / stylised fonts | ≤ 3 % / ≤ 8 % |
| Visible flicker over 10 minutes on a static scene | zero |
| Memory growth and handle count in the soak test | < 5 %, flat |
| Interface animations / layout shift / axe-core | 60 fps, zero, no serious findings |
| Installer size excluding language packs | ≤ 80 MB |

## Design

The interface is a product, not a demo: one bundled variable UI font with full Cyrillic, a 4-px
grid, semantic colour tokens in OKLCH with light, dark and high-contrast themes, WCAG 2.2 AA
contrast, motion limited to transform and opacity with `prefers-reduced-motion` honoured, custom
controls with every state designed, sentence case, and copy that says what happened and what to
do. Tokens live in `app/src/styles/tokens.css`; the shell in `app/` is built against a mocked
backend so it can be developed and verified without the engine.

## Open decisions

Recorded here so they are not silently re-litigated, and closed with an ADR when they are settled.

- **Product name.** The code and README say Lumen, the repository says DynoTranslate. "Lumen"
  combined with translation is widely used by unrelated projects, so it does not satisfy the
  uniqueness the product needs. Three candidates have to be checked against GitHub, winget and the
  Microsoft Store before v1.0.0; until then the crate prefix stays `lumen-` and the mismatch is
  known and accepted.
- **License.** There is no `LICENSE` file in the repository yet. It has to be the most permissive
  choice compatible with every dependency and every model weight, and it is a legal decision, so
  it belongs to the owner. Blocking before the first release, not before.
- **Offline engine.** CTranslate2 or ONNX Runtime with OPUS-MT or M2M100-class int8 weights.
  Model licenses have to be verified individually: some weight sets are non-commercial.

## Documents

| File | Contents |
| --- | --- |
| `docs/PLAN.md` | This file: mission, constraints, milestones, pipeline, quality targets |
| `docs/STATUS.md` | Where the project stands right now, and what the next session picks up |
| `docs/WORKFLOW.md` | Branching, commits, pull requests, the CI contract, git operations |
| `docs/ARCHITECTURE.md` | Crate boundaries and the adapter seams |
| `docs/adr/` | The decisions that were made, with the reasoning and the alternatives |

## Environment constraint

The development environment is Linux and cannot reach `static.rust-lang.org` or `crates.io`, so no
Rust toolchain or crate can be resolved there. Rust is therefore authored locally and compiled,
linted and tested on GitHub Actions: `ubuntu-latest` for the portable crates and `windows-latest`
for the platform adapters. Nothing in the Rust workspace is claimed to build until that workflow
reports it. `docs/WORKFLOW.md` covers what that implies for reading a failing build.
