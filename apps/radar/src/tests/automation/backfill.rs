use std::fs;

use crate::{RadarBackfillReleaseRangeRequest, tests::fixtures};

#[test]
fn dry_run_backfill_selects_unpublished_release_window_prs() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let release_delta_path = temp_dir.path().join("release-delta.json");
	let signals_dir = temp_dir.path().join("signals");
	let mut release_delta = fixtures::valid_release_delta();

	release_delta["compare"]["pr_numbers"] = serde_json::json!([22_414, 22_415, 22_416]);
	release_delta["comparisons"][0]["compare"]["pr_numbers"] =
		serde_json::json!([22_414, 22_415, 22_416]);

	fs::create_dir_all(&signals_dir).expect("signals directory should be created");
	fs::write(release_delta_path.as_path(), release_delta.to_string())
		.expect("release delta should be written");
	fs::write(signals_dir.join("published.json"), fixtures::valid_signal().to_string())
		.expect("signal should be written");

	let report = crate::backfill_release_range(&RadarBackfillReleaseRangeRequest {
		repo: "openai/codex".into(),
		release_delta: release_delta_path,
		stable_tag: None,
		preview_tag: None,
		signals_dir,
		bundles_dir: temp_dir.path().join("bundles"),
		analysis_dir: temp_dir.path().join("analysis"),
		token_env: None,
		codex_bin: "codex".into(),
		model: None,
		max_prs: Some(1),
		dry_run: true,
		refresh_release_delta_first: false,
		refresh_stable_limit: None,
		refresh_preview_limit: None,
		refresh_pair_limit: None,
		python_bin: "python3".into(),
	})
	.expect("dry-run backfill should select unpublished PRs");

	assert_eq!(report.stable_tag, "rust-v0.1.0");
	assert_eq!(report.preview_tag, "rust-v0.2.0-alpha.1");
	assert_eq!(report.target_prs, vec![22_415]);
	assert_eq!(report.created, 0);
	assert!(report.dry_run);
}
