# Lumen shell (Tauri 2)

Desktop host for the React interface: main window, tray icon and global shortcuts.

## How it is verified

`app/src-tauri/Cargo.toml` is **not** a workspace member: the workspace CI runs on Ubuntu
without WebKit/GTK system packages, and the shell ships on Windows, where WebView2 needs
nothing installed. It gets its own CI instead:

- The `shell` job in `.github/workflows/ci.yml` runs `fmt --check`, `clippy -D warnings`
  and `cargo check` on `windows-latest` for every pull request.
- `.github/workflows/release.yml` builds the Windows installer (MSI and NSIS) on every pull
  request that touches `app/` and on every push to `main`, and turns version tags into draft
  GitHub releases with updater assets.

On a desktop machine, from `app/` with a normal Rust toolchain:

```sh
npm ci
npm run tauri dev      # develop
npm run tauri build    # bundle
```

## Command surface

| Command | Purpose |
| --- | --- |
| `ping` | Connectivity check from the UI |
| `get_shell_settings` | Hotkeys / tray flags / region as the shell sees them |
| `set_shell_settings` | Push the same settings from the UI |

Global shortcuts registered by `tauri-plugin-global-shortcut` follow the same bindings the
in-window hotkeys use (`Alt+T`, `Alt+Q`, `Alt+Shift+O`).

## Icons

`icons/` is generated from `icon-source.png` (a placeholder glyph until the product art
lands) and committed, so builds never depend on a designer workstation:

```sh
# from app/
npx tauri icon src-tauri/icon-source.png -o src-tauri/icons
```

## Signing updates

Releases are signed with a minisign keypair whose public half lives in the
`plugins.updater.pubkey` field of `tauri.conf.json`. The private half is stored as the
`TAURI_SIGNING_PRIVATE_KEY` repository secret and never enters the repository. To rotate
the keypair, run `npx tauri signer generate` from `app/`, replace the config field, and
update the secret. Pull-request builds sign with an ephemeral key instead, which proves
the signing path without touching the real one; a version tag without the secret fails
loudly rather than publishing an unsigned release.

`tauri build` reads that secret from `TAURI_SIGNING_PRIVATE_KEY` only — key contents, or
a path to the key file. `TAURI_SIGNING_PRIVATE_KEY_PATH` is honoured by `tauri signer sign`
and ignored by the bundler, so a workflow that sets only the path variable compiles and
then aborts when updater artifacts are signed.

## Private-repository updater token

This repository is private, so the updater authenticates its release downloads with a
read-only token baked into the binary at build time (`option_env!("UPDATER_PAT")` in
`src/main.rs`, sent as a Bearer header). Create a fine-grained personal access token with
Contents read-only access to this repository, no expiry where the platform allows it, and
store it as the `UPDATER_PAT` repository secret. The token is extractable from the binary
by anyone who can already read the releases, so keep its scope minimal; rotating it takes
effect in the next built release. Builds without the secret (fork pull requests, local
checks) ship an updater that reports its download failure in the Settings UI instead of
crashing.
