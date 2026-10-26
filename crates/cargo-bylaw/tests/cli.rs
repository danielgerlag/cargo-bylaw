use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn fixture(name: &str) -> PathBuf {
    repository_root().join("fixtures").join(name)
}

#[test]
fn valid_fixture_passes_with_versioned_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-bylaw"))
        .current_dir(fixture("model-boundaries-modules-valid"))
        .args(["check", "--format", "json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["report"]["rule_results"].as_array().unwrap().len(), 4);
}

#[test]
fn invalid_fixture_returns_violation_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-bylaw"))
        .current_dir(fixture("model-boundaries-modules-invalid"))
        .arg("check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("domain-is-internal"));
    assert!(stdout.contains("model-modules-are-acyclic"));
}

#[test]
fn cargo_external_subcommand_argument_is_accepted() {
    let config = repository_root().join("fixtures/config-invalid/bylaw.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-bylaw"))
        .current_dir(repository_root())
        .arg("bylaw")
        .arg("check")
        .arg("--config")
        .arg(config)
        .args(["--format", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["error"]["kind"], "configuration");
}

#[test]
fn invalid_configuration_returns_configuration_exit_code() {
    let config = repository_root().join("fixtures/config-invalid/bylaw.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-bylaw"))
        .current_dir(repository_root())
        .arg("check")
        .arg("--config")
        .arg(config)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("configuration:"));
}

#[test]
fn analyzer_failure_returns_analysis_exit_code() {
    let config = repository_root().join("fixtures/analysis-invalid/bylaw.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-bylaw"))
        .current_dir(repository_root())
        .arg("check")
        .arg("--config")
        .arg(config)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(json["error"]["kind"], "analysis");
}

#[test]
fn attribute_expansions_preserve_fail_closed_exit_codes() {
    for (args, expected_code) in [
        (vec![], 0),
        (vec!["--config", "bylaw-forbidden.toml"], 1),
        (vec!["--features", "app/unresolved"], 3),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-bylaw"))
            .current_dir(fixture("analyzer-proc-macro"))
            .args(["check", "--format", "json"])
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "args: {args:?}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        let diagnostics = json["report"]["analysis_diagnostics"].as_array().unwrap();
        if expected_code == 3 {
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic["code"] == "analyzer.unresolved-path"
                    && diagnostic["message"]
                        .as_str()
                        .unwrap()
                        .contains("missing_dependency::VALUE")
                    && diagnostic["severity"] == "error"
            }));
        } else {
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
        }
        if expected_code == 1 {
            assert!(String::from_utf8_lossy(&output.stdout).contains("forbid-expanded-dependency"));
        }
    }
}
