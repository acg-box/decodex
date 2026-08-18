//! Process-level checks for informational CLI exits.

#![allow(unused_crate_dependencies)]

use std::process::Command;

use tempfile::TempDir;

#[test]
fn version_exits_without_starting_the_daemon() {
	let home = TempDir::new().expect("create isolated home");
	let output = Command::new(env!("CARGO_BIN_EXE_decodexd"))
		.arg("--version")
		.env("HOME", home.path())
		.output()
		.expect("run version");

	assert!(output.status.success());
	assert_eq!(
		String::from_utf8(output.stdout).expect("version output is UTF-8"),
		format!("decodexd {}\n", env!("CARGO_PKG_VERSION"))
	);
	assert!(output.stderr.is_empty());
	assert!(!home.path().join(".decodex").exists());
}

#[test]
fn artifact_cohort_exits_without_starting_the_daemon() {
	let home = TempDir::new().expect("create isolated home");
	let output = Command::new(env!("CARGO_BIN_EXE_decodexd"))
		.arg("artifact-cohort")
		.env("HOME", home.path())
		.output()
		.expect("run artifact cohort");
	let value: serde_json::Value =
		serde_json::from_slice(&output.stdout).expect("artifact cohort output is JSON");

	assert!(output.status.success());
	assert_eq!(
		value,
		serde_json::json!({
			"schema": "decodex/artifact-cohort/1",
			"artifact_cohort": decodex_protocol::CURRENT_ARTIFACT_COHORT,
			"protocol": decodex_protocol::CURRENT_VERSION,
		}),
	);
	assert!(output.stderr.is_empty());
	assert!(!home.path().join(".decodex").exists());
}

#[test]
fn help_exits_without_starting_the_daemon() {
	let home = TempDir::new().expect("create isolated home");
	let output = Command::new(env!("CARGO_BIN_EXE_decodexd"))
		.arg("--help")
		.env("HOME", home.path())
		.output()
		.expect("run help");
	let stdout = String::from_utf8(output.stdout).expect("help output is UTF-8");

	assert!(output.status.success());
	assert!(stdout.contains("serve"));
	assert!(!stdout.contains("supervise-local"));
	assert!(!stdout.contains("secret-run"));
	assert!(output.stderr.is_empty());
	assert!(!home.path().join(".decodex").exists());
}
