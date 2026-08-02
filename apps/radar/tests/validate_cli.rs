//! CLI regressions for Radar validation paths.

#![allow(unused_crate_dependencies)]

use std::{fs, os::unix::fs::PermissionsExt as _, path::Path, process::Command};

use serde_json::Value;

#[path = "../src/tests/fixtures/valid_upstream_impact.rs"] mod valid_upstream_impact_fixture;
#[path = "../src/tests/fixtures/valid_upstream_review.rs"] mod valid_upstream_review_fixture;

const CACHE_ROOT: &str = ".agent/automations/radar/cache";
const PAIR_ROOT: &str =
	".agent/automations/radar/cache/github/content-review-pairs/content-manager--fixture";
const REVIEW_PATH: &str = ".agent/automations/radar/cache/github/content-review-pairs/content-manager--fixture/review.json";
const IMPACT_PATH: &str = ".agent/automations/radar/cache/github/content-review-pairs/content-manager--fixture/impact.json";

#[test]
fn validate_cli_accepts_content_manager_relative_pair_paths_from_an_isolated_cwd() {
	let cwd = tempfile::tempdir().expect("isolated CLI cwd should be created");
	let pair_root = cwd.path().join(PAIR_ROOT);

	fs::create_dir_all(&pair_root).expect("private pair directory should be created");
	for relative in [
		CACHE_ROOT,
		".agent/automations/radar/cache/github",
		".agent/automations/radar/cache/github/content-review-pairs",
		PAIR_ROOT,
	] {
		fs::set_permissions(cwd.path().join(relative), fs::Permissions::from_mode(0o700))
			.expect("private cache directory mode should be set");
	}
	write_private_json(
		&cwd.path().join(REVIEW_PATH),
		&valid_upstream_review_fixture::valid_upstream_review(),
	);
	write_private_json(
		&cwd.path().join(IMPACT_PATH),
		&valid_upstream_impact_fixture::valid_upstream_impact(),
	);

	let output = Command::new(env!("CARGO_BIN_EXE_radar"))
		.current_dir(cwd.path())
		.args(["validate", REVIEW_PATH, IMPACT_PATH])
		.output()
		.expect("Radar CLI should run");

	assert!(
		output.status.success(),
		"relative Content Manager validation failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let report: Value =
		serde_json::from_slice(&output.stdout).expect("Radar CLI report should be JSON");

	assert_eq!(report["checked_files"], 2);
}

fn write_private_json(path: &Path, value: &Value) {
	let mut payload = serde_json::to_vec_pretty(value).expect("fixture should serialize");

	payload.push(b'\n');
	fs::write(path, payload).expect("private fixture should be written");
	fs::set_permissions(path, fs::Permissions::from_mode(0o600))
		.expect("private fixture mode should be set");
}
