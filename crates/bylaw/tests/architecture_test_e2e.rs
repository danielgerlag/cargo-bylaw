use bylaw::analyzer::{AnalysisOptions, analyze_workspace};
use std::error::Error;
use std::process::{Command, Output};
use std::sync::OnceLock;

#[path = "../../../fixtures/architecture_test_rules.rs"]
mod architecture_test_rules;

struct ChildRun {
    success: bool,
    output: String,
}

static CHILD_RUN: OnceLock<ChildRun> = OnceLock::new();

fn fixture_manifest(name: &str) -> String {
    format!(
        "{}/../../fixtures/{name}/Cargo.toml",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn check_fixture(name: &str, crate_name: &str) -> Result<(), Box<dyn Error>> {
    let graph = analyze_workspace(&AnalysisOptions {
        manifest_path: fixture_manifest(name).into(),
        enable_proc_macros: false,
        enable_build_scripts: false,
        ..AnalysisOptions::default()
    })?;

    architecture_test_rules::module_rules(crate_name)
        .check(&graph)?
        .assert();
    Ok(())
}

#[test]
#[ignore = "executed in the end-to-end child process"]
fn passing_architecture_test_fixture() -> Result<(), Box<dyn Error>> {
    check_fixture(
        "model-boundaries-modules-valid",
        "model_boundaries_modules_valid",
    )
}

#[test]
#[ignore = "executed in the end-to-end child process"]
fn failing_architecture_test_fixture() -> Result<(), Box<dyn Error>> {
    check_fixture(
        "model-boundaries-modules-invalid",
        "model_boundaries_modules_invalid",
    )
}

fn child_run() -> &'static ChildRun {
    CHILD_RUN.get_or_init(|| {
        let output = Command::new(std::env::current_exe().unwrap())
            .arg("architecture_test_fixture")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env("RUST_TEST_NOCAPTURE", "1")
            .output()
            .unwrap();

        ChildRun {
            success: output.status.success(),
            output: combined_output(&output),
        }
    })
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn passing_architecture_test_succeeds() {
    let child = child_run();

    assert!(
        child
            .output
            .contains("test passing_architecture_test_fixture ... ok"),
        "{}",
        child.output
    );
}

#[test]
fn failing_architecture_test_reports_violations() {
    let child = child_run();

    assert!(!child.success, "{}", child.output);
    assert!(
        child
            .output
            .contains("test failing_architecture_test_fixture ... FAILED"),
        "{}",
        child.output
    );
    assert!(
        child.output.contains("domain-is-internal"),
        "{}",
        child.output
    );
    assert!(
        child.output.contains("model-modules-are-acyclic"),
        "{}",
        child.output
    );
}
