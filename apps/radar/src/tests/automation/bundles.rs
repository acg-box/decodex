use std::fs;

use crate::{
	RadarBundleValidateRequest,
	tests::{assertions, fixtures},
};

#[test]
fn builds_pr_bundle_from_fixture_payloads() {
	let patch = format!("{} --config FEATURE_FLAG=1", "a".repeat(910));
	let pr = serde_json::json!({
		"number": 22_414,
		"title": "Add Unix socket endpoint support",
		"body": "Fixes #123 and enables --sandbox.",
		"state": "closed",
		"merged_at": "2026-06-01T00:00:00Z",
		"labels": [{"name": "enhancement"}],
		"html_url": "https://github.com/openai/codex/pull/22414"
	});
	let commits = vec![serde_json::json!({
		"sha": "abc123",
		"html_url": "https://github.com/openai/codex/commit/abc123",
		"author": {"login": "alice"},
		"commit": {
			"message": "Add Unix socket endpoint support\n\nRefs openai/codex#456",
			"author": {
				"name": "Alice",
				"date": "2026-06-01T00:00:00Z"
			}
		}
	})];
	let files = vec![serde_json::json!({
		"filename": "docs/examples/socket.md",
		"status": "modified",
		"additions": 12,
		"deletions": 1,
		"patch": patch
	})];
	let bundle = crate::build_pr_bundle_from_sources(
		"openai/codex",
		&pr,
		&commits,
		&files,
		"main",
		&["fixture note".into()],
	)
	.expect("PR bundle should build from fixture payloads");

	assertions::assert_errors(&bundle, []);

	assert_eq!(bundle["analysis_mode"], "pr_first");
	assert_eq!(bundle["primary_pr"]["state"], "merged");
	assert_eq!(bundle["primary_pr"]["labels"], serde_json::json!(["enhancement"]));
	assert_eq!(bundle["linked_issues"], serde_json::json!(["#123", "openai/codex#456"]));
	assert_eq!(
		bundle["extracted_flags"],
		serde_json::json!(["--sandbox", "--config", "FEATURE_FLAG=1"])
	);
	assert_eq!(bundle["docs_refs"], serde_json::json!(["docs/examples/socket.md"]));
	assert_eq!(bundle["examples_refs"], serde_json::json!(["docs/examples/socket.md"]));
	assert_eq!(bundle["notes"][1], "fixture note");

	let patch_excerpt =
		bundle["files"][0]["patch_excerpt"].as_str().expect("patch excerpt should be present");

	assert!(patch_excerpt.ends_with("..."));
	assert_eq!(patch_excerpt.chars().count(), 903);
}

#[test]
fn validates_bundle_directories_and_rejects_other_schemas() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundle_path = temp_dir.path().join("bundle.json");
	let signal_path = temp_dir.path().join("signal.json");

	fs::write(&bundle_path, fixtures::valid_bundle().to_string())
		.expect("bundle should be written");

	let report = crate::validate_bundles(&RadarBundleValidateRequest {
		paths: vec![temp_dir.path().to_path_buf()],
	})
	.expect("bundle directory should validate");

	assert_eq!(report.checked_files, 1);

	fs::write(&signal_path, fixtures::valid_signal().to_string())
		.expect("signal should be written");

	let error = crate::validate_bundles(&RadarBundleValidateRequest {
		paths: vec![temp_dir.path().to_path_buf()],
	})
	.expect_err("non-bundle schema should be rejected by bundle validation");
	let message = error.to_string();

	assert!(message.contains("schema must be github_change_bundle/v1"));
}
