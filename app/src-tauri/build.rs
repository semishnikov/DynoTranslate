use std::fs;
use std::path::Path;

fn main() {
    // `tauri.conf.json` promises DirectML.dll as a bundled resource. The release job copies the
    // real library from System32 before bundling, but plain `cargo clippy`/`check` runs (the CI
    // shell job) never do, and tauri-build hard-fails on a resource that is not there. An empty
    // placeholder keeps those checks running; bundling always ships the real DLL instead.
    let resource = Path::new("resources").join("DirectML.dll");
    if !resource.exists() {
        let _ = fs::create_dir_all("resources").and_then(|()| fs::write(&resource, b""));
    }
    tauri_build::build()
}
