# Status

Updated: 2026-09-21

## Done

- M0. Plan, architecture and the first two ADRs written.
- M0. Design tokens (OKLCH ramps, spacing, type scale, motion durations and easings) as a single CSS source with dark
  and light themes.
- M0. Product shell in React and TypeScript with four screens (Overview, Apps, Languages, Settings) and custom
  accessible controls: spring toggle, slider with live value, segmented control with sliding indicator, searchable
  language combobox with native-script names, toasts, empty states.
- M0. The shell builds with `tsc` and `vite build` with no errors and runs against mocked pipeline state.

## Next

- M1. Capture adapter, tile change detection, overlay window, headless pipeline CLI.

## Known limitations

- The interface currently reads from mocked state, not from a running pipeline. Every control is wired and changes real
  application state; none of it yet reaches Windows APIs.
- No Rust code exists yet.

## Blocked

- No Rust toolchain can be installed in the development sandbox: `static.rust-lang.org` is unreachable from it, so
  `rustup` fails at the TLS handshake. Rust milestones need either an allowed mirror or GitHub Actions runners.

## Needed from the owner

- A code-signing certificate (SignPath or Azure Trusted Signing) before any installer can ship.
- A decision on whether online engines (DeepL, Google, Microsoft) should be offered at all; if so, the terms under
  which keys are supplied.
