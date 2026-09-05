//! Process-level checks for explicit service and informational CLI exits.

#![allow(unused_crate_dependencies)]

use std::process::Command;

use tempfile::TempDir;

#[test]
fn version_exits_without_starting_the_service() {
	let home = TempDir::new().expect("create isolated home");
	let output = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.arg("--version")
		.env("HOME", home.path())
		.output()
		.expect("run version");

	assert!(output.status.success());
	assert_eq!(
		String::from_utf8(output.stdout).expect("version output is UTF-8"),
		format!("decodex {}\n", env!("CARGO_PKG_VERSION"))
	);
	assert!(output.stderr.is_empty());
	assert!(!home.path().join(".decodex").exists());
}

#[test]
fn no_subcommand_displays_help_without_starting_the_service() {
	let home = TempDir::new().expect("create isolated home");
	let output = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.env("HOME", home.path())
		.output()
		.expect("run without a subcommand");
	let stderr = String::from_utf8(output.stderr).expect("help error output is UTF-8");

	assert_eq!(output.status.code(), Some(2));
	assert!(output.stdout.is_empty());
	assert!(stderr.contains("Usage: decodex"));
	assert!(stderr.contains("serve"));
	assert!(!home.path().join(".decodex").exists());
}

#[test]
fn build_info_exits_without_starting_the_service() {
	let home = TempDir::new().expect("create isolated home");
	let output = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.args(["--output", "json", "build-info"])
		.env("HOME", home.path())
		.output()
		.expect("run build-info");
	let value: serde_json::Value =
		serde_json::from_slice(&output.stdout).expect("build-info output is JSON");

	assert!(output.status.success());
	assert_eq!(value["schema"], "decodex/build-info/1");
	assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
	assert!(value["commit"].as_str().is_some_and(|commit| commit.len() == 40));
	assert!(value["dirty"].is_boolean());
	assert!(output.stderr.is_empty());
	assert!(!home.path().join(".decodex").exists());
}

#[test]
fn help_exits_without_starting_the_service() {
	let home = TempDir::new().expect("create isolated home");
	let output = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.arg("--help")
		.env("HOME", home.path())
		.output()
		.expect("run help");
	let stdout = String::from_utf8(output.stdout).expect("help output is UTF-8");

	assert!(output.status.success());
	assert!(stdout.contains("serve"));
	assert!(!stdout.contains("build-info"));
	assert!(!stdout.contains("supervise-local"));
	assert!(!stdout.contains("secret-run"));
	assert!(output.stderr.is_empty());
	assert!(!home.path().join(".decodex").exists());
}

#[test]
fn database_initialize_and_validate_are_owned_by_the_unified_binary() {
	let temporary = TempDir::new().expect("create isolated database root parent");
	let root =
		temporary.path().canonicalize().expect("canonicalize database root parent").join("root");

	let initialize = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.args(["initialize-local-database", "--root"])
		.arg(&root)
		.output()
		.expect("initialize local database");
	assert!(
		initialize.status.success(),
		"initialization failed: {}",
		String::from_utf8_lossy(&initialize.stderr)
	);
	assert!(initialize.stdout.is_empty());
	assert!(root.join("server/decodex.sqlite3").is_file());

	let validate = Command::new(env!("CARGO_BIN_EXE_decodex"))
		.args(["validate-local-database", "--root"])
		.arg(&root)
		.output()
		.expect("validate local database");
	assert!(
		validate.status.success(),
		"validation failed: {}",
		String::from_utf8_lossy(&validate.stderr)
	);
	assert!(validate.stdout.is_empty());
}
