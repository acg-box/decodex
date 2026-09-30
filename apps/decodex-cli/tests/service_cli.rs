//! Process-level checks for explicit service and informational CLI exits.

#![allow(unused_crate_dependencies)]

use std::process::Command;
#[cfg(unix)] use std::{
	fs,
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
};

use serde_json::Value;
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
	let value: Value = serde_json::from_slice(&output.stdout).expect("build-info output is JSON");

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

#[cfg(unix)]
#[test]
fn account_validation_respects_json_and_human_output_before_transport() {
	let temporary = TempDir::new().unwrap();
	let root = temporary.path().canonicalize().unwrap().join("root");

	fs::create_dir(&root).unwrap();
	fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

	let uid = fs::metadata(&root).unwrap().uid();
	let config = root.join("config.toml");

	fs::write(
		&config,
		format!(
			r#"version = 1
active_profile = "local"
cache = {{}}
[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {uid}
expected_server_identity = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162"
"#
		),
	)
	.unwrap();
	fs::set_permissions(config, fs::Permissions::from_mode(0o600)).unwrap();

	let id = "40000000-0000-4000-8000-000000000001";
	let duplicate_order = format!("{id},{id}");
	let oversized_source = "x".repeat(decodex_protocol::MAX_WIRE_TEXT_BYTES + 1);
	let cases = [
		vec!["inspect", "--account-id", "invalid"],
		vec!["profile", "--account-id", "invalid"],
		vec!["route", "--account-id", id, "--idempotency-key", ""],
		vec!["set-balanced-selection", "--expected-revision", "0", "--idempotency-key", "valid"],
		vec![
			"set-account-order",
			"--order",
			&duplicate_order,
			"--expected-revision",
			"1",
			"--idempotency-key",
			"valid",
		],
		vec![
			"enroll",
			"--operation-id",
			"invalid",
			"--account-id",
			id,
			"--idempotency-key",
			"valid",
		],
		vec![
			"import",
			"--operation-id",
			id,
			"--account-id",
			id,
			"--source",
			&oversized_source,
			"--idempotency-key",
			"valid",
		],
	];

	for args in cases {
		for format in ["json", "human"] {
			let output = Command::new(env!("CARGO_BIN_EXE_decodex"))
				.arg("--root")
				.arg(&root)
				.args(["--output", format, "account"])
				.args(&args)
				.output()
				.unwrap();

			assert_eq!(output.status.code(), Some(2), "{args:?}");

			if format == "json" {
				assert!(
					output.stderr.is_empty(),
					"{args:?}: {}",
					String::from_utf8_lossy(&output.stderr)
				);
				assert_eq!(
					serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
					serde_json::json!({"schema":"decodex/cli-account/1","outcome":"failure","failure":"invalid_input"})
				);
			} else {
				assert!(output.stdout.is_empty());
				assert_eq!(
					String::from_utf8(output.stderr).unwrap(),
					"decodex account: invalid bounded account input\n"
				);
			}
		}
	}
}
