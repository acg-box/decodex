use std::{cell::Cell, fs};

use sha2::{Digest as _, Sha256};

use crate::{
	RadarBundleValidateRequest,
	tests::{assertions, env::TestEnvVars, fixtures},
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

#[test]
fn installed_bundle_receipt_reports_exact_bytes_and_bounded_structure() {
	let temp_dir = crate::test_support::private_tempdir();
	let bundle_path =
		temp_dir.path().join(".agent/automations/radar/cache/github/bundles/test-run.json");
	let mut bundle = fixtures::valid_bundle();

	bundle["files"][0]["patch_excerpt"] = serde_json::json!("+pub fn install_bundle() -> Receipt");
	let files = bundle["files"].as_array_mut().expect("fixture files should be a list");

	for (index, path) in [
		"apps/radar/src/source_bundle/evidence.rs",
		"apps/radar/src/requests/bundle.rs",
		"apps/radar/src/tests/automation/bundles.rs",
	]
	.into_iter()
	.enumerate()
	{
		files.push(serde_json::json!({
			"path": path,
			"status": "modified",
			"additions": 20,
			"deletions": 0,
			"patch_excerpt": format!("+fn receipt_anchor_{index}() {{}}")
		}));
	}
	bundle["docs_refs"] = serde_json::json!(["apps/radar/README.md", "docs/receipt.md"]);
	bundle["examples_refs"] = serde_json::json!(["docs/examples/receipt.md"]);

	let receipt = crate::install_bundle(&bundle_path, &bundle)
		.expect("valid bundle should install with an evidence receipt");
	let installed = fs::read(&bundle_path).expect("installed bundle should be readable");
	let expected_sha256 =
		Sha256::digest(&installed).iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	assert_eq!(receipt.schema, "radar_bundle_build_receipt/v1");
	assert_eq!(receipt.status, "installed");
	assert_eq!(receipt.bundle_sha256, expected_sha256);
	assert_eq!(receipt.bundle_bytes, installed.len() as u64);
	assert_eq!(receipt.analysis_mode, "pr_first");
	assert_eq!(receipt.commit_count, 1);
	assert_eq!(receipt.file_count, 4);
	assert_eq!(receipt.patch_excerpt_count, 4);
	assert_eq!(receipt.docs_ref_count, 2);
	assert_eq!(receipt.examples_ref_count, 1);

	let serialized = serde_json::to_value(&receipt).expect("receipt should serialize");
	let object = serialized.as_object().expect("receipt should be an object");

	assert_eq!(object.len(), 10, "receipt surface must remain fixed and bounded");
	for forbidden in ["repo", "bundle_path", "files", "commits", "patch_excerpt", "body"] {
		assert!(!object.contains_key(forbidden), "receipt must not expose {forbidden}");
	}
}

#[test]
fn bundle_install_rejects_invalid_input_before_writing() {
	let temp_dir = crate::test_support::private_tempdir();
	let bundle_path =
		temp_dir.path().join(".agent/automations/radar/cache/github/bundles/invalid.json");
	let mut bundle = fixtures::valid_bundle();

	bundle["commits"] = serde_json::json!([]);
	bundle["docs_refs"] = serde_json::json!([]);
	bundle["examples_refs"] = serde_json::json!([]);

	let error = crate::install_bundle(&bundle_path, &bundle)
		.expect_err("invalid bundle must not be installed or receipted");

	assert!(error.to_string().contains("Bundle validation failed"));
	assert!(!bundle_path.exists());
}

#[test]
fn bundle_install_rejects_invalid_receipt_structure_before_writing() {
	let temp_dir = crate::test_support::private_tempdir();
	let bundle_path =
		temp_dir.path().join(".agent/automations/radar/cache/github/bundles/invalid-shape.json");
	let mut bundle = fixtures::valid_bundle();

	bundle["files"][0]["patch_excerpt"] = serde_json::json!(12);
	bundle["docs_refs"] = serde_json::json!([]);
	bundle["examples_refs"] = serde_json::json!([]);

	let error = crate::install_bundle(&bundle_path, &bundle)
		.expect_err("invalid receipt structure must not be installed or receipted");

	assert!(error.to_string().contains("patch_excerpt must be a string or null"));
	assert!(!bundle_path.exists());
}

#[test]
fn bundle_install_rejects_impossible_reference_counts_before_writing() {
	let temp_dir = crate::test_support::private_tempdir();
	let bundle_path =
		temp_dir.path().join(".agent/automations/radar/cache/github/bundles/invalid-count.json");
	let mut bundle = fixtures::valid_bundle();

	bundle["docs_refs"] = serde_json::json!(["docs/one.md", "docs/two.md"]);
	bundle["examples_refs"] = serde_json::json!([]);

	let error = crate::install_bundle(&bundle_path, &bundle)
		.expect_err("impossible reference counts must not be installed or receipted");

	assert!(error.to_string().contains("docs_ref count cannot exceed file count"));
	assert!(!bundle_path.exists());
}

#[test]
fn bundle_install_rejects_readback_mismatch_without_a_receipt() {
	let temp_dir = crate::test_support::private_tempdir();
	let bundle_path =
		temp_dir.path().join(".agent/automations/radar/cache/github/bundles/replaced.json");
	let mut bundle = fixtures::valid_bundle();

	bundle["files"][0]["patch_excerpt"] = serde_json::Value::Null;
	bundle["docs_refs"] = serde_json::json!([]);
	bundle["examples_refs"] = serde_json::json!([]);

	let error = crate::install_bundle_after_write(&bundle_path, &bundle, || {
		fs::write(&bundle_path, b"{}\n").expect("test should replace installed bytes");
	})
	.expect_err("changed installed bytes must not produce a receipt");

	assert!(error.to_string().contains("do not match the deterministic build output"));
}

#[test]
fn private_bundle_install_holds_one_cache_lock_through_readback() {
	let temp_dir = crate::test_support::private_tempdir();
	let cache_root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let bundle_path = cache_root.join("github/bundles/locked.json");
	let bundle = receiptable_bundle();
	let observed_locked = Cell::new(false);

	crate::install_bundle_after_write(&bundle_path, &bundle, || {
		let cache = crate::private_fs::PrivateCache::open_existing(&cache_root)
			.expect("the cache should remain open while installing");
		let error = cache.try_lock().expect_err("a second cache lock must remain blocked");

		assert!(
			error
				.chain()
				.find_map(|cause| cause.downcast_ref::<std::io::Error>())
				.is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
		);
		observed_locked.set(true);
	})
	.expect("the bundle should install under one cache lock");

	assert!(observed_locked.get());
}

#[test]
fn bundle_install_and_build_output_are_private_and_bound_to_the_process_run() {
	let temp_dir = crate::test_support::private_tempdir();
	let run_id = "019fa400-0000-7000-8000-000000000001";
	let stale_run_id = "019fa400-0000-7000-8000-000000000002";
	let cache_root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let expected = cache_root.join(format!("github/bundles/{run_id}.json"));
	let stale = cache_root.join(format!("github/bundles/{stale_run_id}.json"));
	let external = temp_dir.path().join("bundle.json");
	let _env = TestEnvVars::set(&[("CODEX_THREAD_ID", Some(run_id))]);

	crate::operations::validate_current_bundle_output_path(&expected)
		.expect("the exact current run bundle path should validate");
	let stale_error = crate::operations::validate_current_bundle_output_path(&stale)
		.expect_err("a stale run bundle path must fail before GitHub access");
	let external_error = crate::install_bundle(&external, &receiptable_bundle())
		.expect_err("bundle installation must have no external writer authority");

	assert!(stale_error.to_string().contains("current CODEX_THREAD_ID"));
	assert!(external_error.to_string().contains("private Radar cache path"));
	assert!(!external.exists());
}

fn receiptable_bundle() -> serde_json::Value {
	let mut bundle = fixtures::valid_bundle();

	bundle["docs_refs"] = serde_json::json!([]);
	bundle["examples_refs"] = serde_json::json!([]);
	bundle
}
