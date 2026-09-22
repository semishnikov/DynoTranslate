# Lumen shell (Tauri 2)

Desktop host for the React interface: main window, tray icon and global shortcuts.

## Why it is outside the workspace CI

`.github/workflows/ci.yml` runs `cargo test --workspace` on Linux and Windows without
WebKit/GTK system packages and without permission for this session to change the workflow
(`docs/WORKFLOW.md`). `src-tauri/Cargo.toml` is therefore **not** a workspace member. The
React half of M5 is verified by the Interface job; this half is verified on a desktop
machine:

```sh
# once, with a normal Rust toolchain and Tauri CLI
cargo install tauri-cli --locked
cd src-tauri && cargo tauri dev
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

`bundle.icon` paths expect `icons/32x32.png`, `icons/128x128.png` and `icons/icon.ico`
under `src-tauri/icons/`. Add the product icons before `cargo tauri build`; they are
binary assets the owner provides.
