//! CLI regressions for Radar validation paths.

#![allow(unused_crate_dependencies)]

use std::{fs, os::unix::fs::PermissionsExt as _, path::Path, process::Command};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

#[path = "../src/tests/fixtures/valid_upstream_impact.rs"] mod valid_upstream_impact_fixture;
#[path = "../src/tests/fixtures/valid_upstream_review.rs"] mod valid_upstream_review_fixture;

const CACHE_ROOT: &str = ".agent/automations/radar/cache";
const PAIR_COLLECTION: &str = ".agent/automations/radar/cache/github/content-review-pairs";
const RUN_ID: &str = "019fa400-0000-7000-8000-000000000001";

#[test]
fn validate_cli_accepts_content_manager_relative_pair_paths_from_an_isolated_cwd() {
	let cwd = tempfile::tempdir().expect("isolated CLI cwd should be created");
	let review = valid_upstream_review_fixture::valid_upstream_review();
	let review_bytes = pretty_json_bytes(&review);
	let mut impact = valid_upstream_impact_fixture::valid_upstream_impact();
	impact["review_lineage"]["artifact_sha256"] = json_string(&sha256_hex(&review_bytes));
	let impact_bytes = pretty_json_bytes(&impact);
	let pair_digest = content_pair_digest(&review_bytes, &impact_bytes);
	let pair_relative = format!("{PAIR_COLLECTION}/{RUN_ID}--{}--{pair_digest}", "a".repeat(64));
	let review_relative = format!("{pair_relative}/review.json");
	let impact_relative = format!("{pair_relative}/impact.json");
	let pair_root = cwd.path().join(&pair_relative);

	fs::create_dir_all(&pair_root).expect("private pair directory should be created");
	for relative in
		[CACHE_ROOT, ".agent/automations/radar/cache/github", PAIR_COLLECTION, &pair_relative]
	{
		fs::set_permissions(cwd.path().join(relative), fs::Permissions::from_mode(0o700))
			.expect("private cache directory mode should be set");
	}
	write_private_bytes(&cwd.path().join(&review_relative), &review_bytes);
	write_private_bytes(&cwd.path().join(&impact_relative), &impact_bytes);

	let output = Command::new(env!("CARGO_BIN_EXE_radar"))
		.current_dir(cwd.path())
		.args(["validate", &review_relative, &impact_relative])
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

fn pretty_json_bytes(value: &Value) -> Vec<u8> {
	let mut payload = serde_json::to_vec_pretty(value).expect("fixture should serialize");

	payload.push(b'\n');
	payload
}

fn json_string(value: &str) -> Value {
	Value::String(value.to_owned())
}

fn sha256_hex(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn content_pair_digest(review: &[u8], impact: &[u8]) -> String {
	let mut digest = Sha256::new();

	digest.update(b"radar-content-review-pair-v1");
	for payload in [review, impact] {
		digest.update(u64::try_from(payload.len()).expect("fixture length").to_be_bytes());
		digest.update(payload);
	}

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_private_bytes(path: &Path, payload: &[u8]) {
	fs::write(path, payload).expect("private fixture should be written");
	fs::set_permissions(path, fs::Permissions::from_mode(0o600))
		.expect("private fixture mode should be set");
}
