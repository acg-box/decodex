use std::{fs, path::Path, process::Command};

use rusqlite::Connection;

use crate::{
	RUN_CODEX_ANALYSIS_SCRIPT, RadarBackfillReleaseRangeRequest, RadarBundleValidateRequest,
	RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest, RadarLedgerIngestExistingRequest,
	tests::{self, TestEnvVars},
};

#[test]
fn analysis_helper_fails_closed_without_explicit_boundary_opt_in() {
	let _env = TestEnvVars::set(&[("DECODEX_ALLOW_CODEX_ANALYSIS", None)]);
	let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(Path::parent)
		.expect("apps/decodex should live two levels under the repo root");
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundle_path = temp_dir.path().join("missing-bundle.json");
	let output_path = temp_dir.path().join("analysis.json");
	let output = Command::new("python3")
		.current_dir(repo_root)
		.arg(repo_root.join(RUN_CODEX_ANALYSIS_SCRIPT))
		.arg("--bundle")
		.arg(&bundle_path)
		.arg("--out")
		.arg(&output_path)
		.arg("--repo-root")
		.arg(repo_root)
		.output()
		.expect("Python analysis helper smoke command should execute");
	let stderr = String::from_utf8_lossy(&output.stderr);

	assert!(!output.status.success());
	assert!(
		stderr.contains("requires --allow-ai-analysis-boundary"),
		"unexpected stderr: {stderr}"
	);
	assert!(!output_path.exists());
}

#[test]
fn dry_run_backfill_selects_unpublished_release_window_prs() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let release_delta_path = temp_dir.path().join("release-delta.json");
	let signals_dir = temp_dir.path().join("signals");
	let mut release_delta = tests::valid_release_delta();

	release_delta["compare"]["pr_numbers"] = serde_json::json!([22_414, 22_415, 22_416]);
	release_delta["comparisons"][0]["compare"]["pr_numbers"] =
		serde_json::json!([22_414, 22_415, 22_416]);

	fs::create_dir_all(&signals_dir).expect("signals directory should be created");
	fs::write(release_delta_path.as_path(), release_delta.to_string())
		.expect("release delta should be written");
	fs::write(signals_dir.join("published.json"), tests::valid_signal().to_string())
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

#[test]
fn ledger_bootstrap_drops_legacy_publisher_artifact_links_and_status() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let db_path = temp_dir.path().join("radar.sqlite3");
	let connection = Connection::open(&db_path).expect("temporary ledger should open");

	connection
		.execute_batch(
			"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'publisher_handoff',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				CREATE TABLE radar_review (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  status TEXT NOT NULL CHECK (
				    status IN (
				      'seen',
				      'skipped',
				      'watch',
				      'signal',
				      'control_plane',
				      'publisher_handoff',
				      'deprecated',
				      'archived'
				    )
				  ),
				  reason TEXT NOT NULL DEFAULT '',
				  confidence TEXT CHECK (confidence IN ('confirmed', 'likely', 'weak')),
				  reviewed_at TEXT NOT NULL,
				  updated_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id)
				);
				INSERT INTO artifact_link (
				  repo,
				  subject_kind,
				  subject_id,
				  artifact_kind,
				  path,
				  sha256,
				  size_bytes,
				  created_at
				)
				VALUES (
				  'openai/codex',
				  'pr',
				  '22414',
				  'publisher_handoff',
				  '.agent/automations/decodex/cache/publisher/handoffs/example.json',
				  'abc123',
				  10,
				  '2026-06-01T00:00:00Z'
				);
				INSERT INTO radar_review (
				  repo,
				  subject_kind,
				  subject_id,
				  status,
				  reason,
				  confidence,
				  reviewed_at,
				  updated_at
				)
				VALUES (
				  'openai/codex',
				  'pr',
				  '22414',
				  'publisher_handoff',
				  'legacy publisher handoff',
				  'likely',
				  '2026-06-01T00:00:00Z',
				  '2026-06-01T00:00:00Z'
				);
				",
		)
		.expect("legacy artifact link schema should be created");

	drop(connection);

	crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
		.expect("ledger bootstrap should migrate legacy publisher rows");

	let connection = Connection::open(&db_path).expect("migrated ledger should open");
	let artifact_links: i64 = connection
		.query_row("SELECT COUNT(*) FROM artifact_link", [], |row| row.get(0))
		.expect("artifact link count should be readable");
	let review_status: String = connection
		.query_row("SELECT COALESCE(MAX(status), '') FROM radar_review", [], |row| row.get(0))
		.expect("review status count should be readable");
	let schema_version: String = connection
		.query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| row.get(0))
		.expect("schema version should be readable");

	assert_eq!(artifact_links, 0);
	assert_eq!(review_status, "");
	assert_eq!(schema_version, "5");
}

#[test]
fn ledger_ingests_existing_bundle_analysis_and_signal_artifacts() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundles_dir = temp_dir.path().join("bundles");
	let analysis_dir = temp_dir.path().join("analysis");
	let signals_dir = temp_dir.path().join("signals");
	let db_path = temp_dir.path().join("radar.sqlite3");

	fs::create_dir_all(&bundles_dir).expect("bundles directory should be created");
	fs::create_dir_all(&analysis_dir).expect("analysis directory should be created");
	fs::create_dir_all(&signals_dir).expect("signals directory should be created");
	fs::write(bundles_dir.join("openai-codex-pr-22414.json"), tests::valid_bundle().to_string())
		.expect("bundle fixture should be written");
	fs::write(analysis_dir.join("openai-codex-pr-22414.analysis.json"), r#"{"kind":"capability"}"#)
		.expect("analysis fixture should be written");
	fs::write(signals_dir.join("openai-codex-pr-22414.json"), tests::valid_signal().to_string())
		.expect("signal fixture should be written");

	let summary = crate::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
		db_path: db_path.clone(),
		bundles_dir,
		analysis_dir,
		signals_dir,
	})
	.expect("existing artifacts should ingest");

	assert_eq!(summary.get("bundles_ingested"), Some(&1));
	assert_eq!(summary.get("upstream_commits"), Some(&1));
	assert_eq!(summary.get("radar_reviews"), Some(&1));
	assert_eq!(summary.get("artifact_links"), Some(&3));

	let connection = Connection::open(&db_path).expect("ingested ledger should open");
	let review: (String, String) = connection
		.query_row(
			"SELECT status, confidence FROM radar_review WHERE subject_kind = 'pr'",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.expect("review row should be readable");

	assert_eq!(review, ("signal".into(), "confirmed".into()));
}

#[test]
fn ledger_artifact_link_records_control_plane_upgrade_candidate_after_schema_migration() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let db_path = temp_dir.path().join("radar.sqlite3");
	let candidate_path = temp_dir.path().join("upgrade.json");
	let connection = Connection::open(&db_path).expect("temporary ledger should open");

	connection
		.execute_batch(
			"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				",
		)
		.expect("legacy artifact link schema should be created");

	drop(connection);

	fs::write(&candidate_path, r#"{"schema":"control_plane_upgrade_candidate/v1"}"#)
		.expect("upgrade candidate fixture should be written");
	crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
		.expect("ledger bootstrap should add control-plane upgrade artifact kind");

	let summary = crate::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
		db_path: db_path.clone(),
		repo: "openai/codex".into(),
		subject_kind: "pr".into(),
		subject_id: "22414".into(),
		artifact_kind: "control_plane_upgrade_candidate".into(),
		path: candidate_path,
	})
	.expect("control-plane upgrade artifact link should be recorded");

	assert_eq!(summary.get("artifact_links"), Some(&1));
}

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

	tests::assert_errors(&bundle, []);

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

	fs::write(&bundle_path, tests::valid_bundle().to_string()).expect("bundle should be written");

	let report = crate::validate_bundles(&RadarBundleValidateRequest {
		paths: vec![temp_dir.path().to_path_buf()],
	})
	.expect("bundle directory should validate");

	assert_eq!(report.checked_files, 1);

	fs::write(&signal_path, tests::valid_signal().to_string()).expect("signal should be written");

	let error = crate::validate_bundles(&RadarBundleValidateRequest {
		paths: vec![temp_dir.path().to_path_buf()],
	})
	.expect_err("non-bundle schema should be rejected by bundle validation");
	let message = error.to_string();

	assert!(message.contains("schema must be github_change_bundle/v1"));
}
