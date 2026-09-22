//! Fail-open under scripted faults: the overlay clears exactly when the window is lost,
//! survives every other failure, and the run never panics.

mod common;

use std::process::Command;

fn run_chaos(args: &[&str]) -> serde_json::Value {
    let output = Command::new(common::pipeline_binary())
        .args(args)
        .output()
        .expect("the pipeline binary is built by cargo");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the chaos run exited with {}: {stdout}",
        output.status,
    );
    serde_json::from_str(stdout.trim()).expect("the chaos report is JSON")
}

#[test]
fn the_pipeline_fails_open_under_injected_faults() {
    common::install_annotations();
    let report = run_chaos(&["--chaos", "400", "--seed", "42", "--width", "640", "--height", "480"]);

    assert_eq!(report["frames"], 400);
    let faults = &report["faults"];
    assert!(
        faults["read_errors"].as_u64().expect("read errors") > 0,
        "the fault plan must hit a transient read error",
    );
    assert!(
        faults["target_lost"].as_u64().expect("target lost") > 0,
        "the fault plan must lose the window",
    );
    assert!(
        faults["engine_errors"].as_u64().expect("engine errors") > 0,
        "the fault plan must hit the translation engine",
    );
    // Every lost window clears the overlay, and only a lost window does.
    assert_eq!(
        report["cleared_frames"].as_u64().expect("cleared"),
        faults["target_lost"].as_u64().expect("target lost"),
        "the overlay clears exactly when the window is lost",
    );
    assert_eq!(report["panics"], 0);
}

#[test]
fn the_chaos_run_is_deterministic_for_a_seed() {
    common::install_annotations();
    let args: &[&str] = &["--chaos", "200", "--seed", "5", "--width", "480", "--height", "360"];
    let first = run_chaos(args);
    let second = run_chaos(args);

    assert_eq!(first["faults"], second["faults"]);
    assert_eq!(first["cleared_frames"], second["cleared_frames"]);
}
