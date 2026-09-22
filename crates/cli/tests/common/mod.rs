//! Shared helpers for the integration tests: locating the pipeline binary the test binary
//! sits next to. The tests run the binary in its own process on purpose, so the counting
//! allocator's numbers describe the pipeline and nothing else.

use std::path::PathBuf;

/// The path of the lumen-pipeline binary the integration tests are being run against.
pub fn pipeline_binary() -> PathBuf {
    // Cargo sets this for test targets when it can; if it does, it is authoritative.
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_LUMEN_PIPELINE") {
        return PathBuf::from(path);
    }
    // The test binary lives in target/<profile>/deps and the package's binary in
    // target/<profile>, so one level up from the deps directory is always the right place.
    let exe = std::env::current_exe().expect("the test binary has a path");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("the test binary sits in target/<profile>/deps");
    let name = if cfg!(windows) { "lumen-pipeline.exe" } else { "lumen-pipeline" };
    let path = profile_dir.join(name);
    assert!(path.is_file(), "the pipeline binary was not built at {}", path.display());
    path
}
