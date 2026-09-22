//! The plan's soak targets, asserted against a real run of the pipeline binary: under 5 %
//! memory growth, flat, over a long run with continuous change.
//!
//! The binary runs in its own process, which is why the counting allocator's numbers say
//! something about the pipeline and nothing about the test harness.

mod common;

use std::process::Command;

fn run_pipeline(args: &[&str]) -> serde_json::Value {
    let output = Command::new(common::pipeline_binary())
        .args(args)
        .output()
        .expect("the pipeline binary is built by cargo");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the soak run exited with {}: {stdout}",
        output.status,
    );
    serde_json::from_str(stdout.trim()).expect("the soak report is JSON")
}

#[test]
fn a_soak_run_keeps_memory_flat() {
    let report = run_pipeline(&["--soak", "600", "--seed", "7", "--width", "640", "--height", "480"]);

    assert_eq!(report["frames"], 600);
    assert!(
        report["static_frames"].as_u64().expect("static frames") > 0,
        "the scene must have static stretches for the pass reuse to matter",
    );

    let memory = &report["memory"];
    let growth = memory["growth_percent"].as_f64().expect("growth percent");
    assert!(
        growth < 5.0,
        "the plan allows under 5% growth over the soak, the run grew {growth}%",
    );
    assert!(memory["end_live_bytes"].as_u64().expect("end") <= memory["peak_live_bytes"].as_u64().expect("peak"));
    assert!(
        memory["allocations"].as_u64().expect("allocations") > 0,
        "the run must actually allocate",
    );
}

#[test]
fn the_soak_is_deterministic_for_a_seed() {
    let args: &[&str] = &["--soak", "120", "--seed", "3", "--width", "480", "--height", "360"];
    let first = run_pipeline(args);
    let second = run_pipeline(args);

    assert_eq!(first["static_frames"], second["static_frames"]);
    assert_eq!(first["changed_frames"], second["changed_frames"]);
    assert_eq!(first["memory"]["allocations"], second["memory"]["allocations"]);
    assert_eq!(first["memory"]["deallocations"], second["memory"]["deallocations"]);
}
