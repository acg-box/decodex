use std::{
	env,
	ffi::OsString,
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use serde_json::{self, Value};

use crate::{
	self as radar, RadarBackfillReleaseRangeRequest, RadarBundleValidateRequest,
	RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest, RadarLedgerIngestExistingRequest,
	RadarRenderSignalRequest, RadarValidateRequest, RefreshKind,
};

struct TestEnvVars {
	_lock: crate::test_support::TestEnvLockGuard,
	previous: Vec<(String, Option<OsString>)>,
}

impl TestEnvVars {
	fn set(vars: &[(&str, Option<&str>)]) -> Self {
		let lock = crate::test_support::lock_test_env();
		let previous =
			vars.iter().map(|(key, _)| ((*key).to_owned(), env::var_os(key))).collect::<Vec<_>>();

		for (key, value) in vars {
			match value {
				Some(value) => unsafe { env::set_var(key, value) },
				None => unsafe { env::remove_var(key) },
			}
		}

		Self { _lock: lock, previous }
	}
}

impl Drop for TestEnvVars {
	fn drop(&mut self) {
		for (key, previous) in self.previous.drain(..).rev() {
			match previous {
				Some(previous) => unsafe { env::set_var(key, previous) },
				None => unsafe { env::remove_var(key) },
			}
		}
	}
}

mod artifacts;

mod automation;

fn assert_errors<const N: usize>(payload: &Value, expected: [&str; N]) {
	let validation = radar::validate_artifact(payload);

	for expected_error in expected {
		assert!(
			validation.errors.iter().any(|error| error.contains(expected_error)),
			"expected error containing {expected_error:?}, got {:?}",
			validation.errors
		);
	}

	if expected.is_empty() {
		assert_eq!(validation.errors, Vec::<String>::new());
	}
}

fn assert_path_errors<const N: usize>(path: &str, payload: &Value, expected: [&str; N]) {
	let validation = radar::validate_artifact_for_path(Path::new(path), payload);

	for expected_error in expected {
		assert!(
			validation.errors.iter().any(|error| error.contains(expected_error)),
			"expected error containing {expected_error:?}, got {:?}",
			validation.errors
		);
	}

	if expected.is_empty() {
		assert_eq!(validation.errors, Vec::<String>::new());
	}
}

fn valid_bundle() -> Value {
	serde_json::json!({
		"schema": "github_change_bundle/v1",
		"repo": "openai/codex",
		"analysis_mode": "pr_first",
		"default_branch": "main",
		"primary_pr": {
			"number": 22_414,
			"title": "Add Unix socket endpoint support",
			"body": "",
			"state": "merged",
			"merged_at": "2026-06-01T00:00:00Z",
			"labels": [],
			"url": "https://github.com/openai/codex/pull/22414"
		},
		"commits": [
			{
				"sha": "abc123",
				"message": "Add Unix socket endpoint support",
				"url": "https://github.com/openai/codex/commit/abc123"
			}
		],
		"files": [
			{
				"path": "codex-rs/app-server/src/lib.rs",
				"status": "modified",
				"additions": 12,
				"deletions": 1
			}
		]
	})
}

fn valid_signal() -> Value {
	serde_json::json!({
		"schema": "signal_entry/v1",
		"slug": "openai-codex-pr-22414",
		"lane": "github",
		"kind": "capability",
		"title": "Unix sockets for remote Codex",
		"published_at": "2026-06-01T00:00:00Z",
		"summary": "Remote Codex can use Unix socket endpoints.",
		"why_it_matters": "Operators can use local socket transports.",
		"confidence": "confirmed",
		"impact": "medium",
		"proof_points": ["PR #22414 adds endpoint handling."],
		"source_refs": {
			"repo": "openai/codex",
			"pr_url": "https://github.com/openai/codex/pull/22414",
			"items": [
				{
					"kind": "pull_request",
					"title": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/pull/22414"
				}
			]
		}
	})
}

fn valid_config_feature_catalog() -> Value {
	serde_json::json!({
		"schema": "codex_config_feature_catalog/v1",
		"source_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
		"generated_at": "2026-06-02T00:00:00Z",
		"feature_count": 1,
		"features": [
			{
				"name": "multi_agent_v2",
				"config_path": "features.multi_agent_v2",
				"toml_assignment": "multi_agent_v2 = true",
				"toml_snippet": "[features]\nmulti_agent_v2 = true",
				"cli_enable_flag": "--enable multi_agent_v2",
				"schema_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
				"reference_url": "https://developers.openai.com/codex/config-reference",
				"reference_description": "Enable MultiAgentV2 tools including followup_task; legacy assign_task appears only in older rollout traces.",
				"github_search_url": "https://github.com/openai/codex/search?q=%22multi_agent_v2%22&type=code"
			}
		]
	})
}

fn collect_assign_task_reference_violations(
	path: &Path,
	repo_root: &Path,
	offenders: &mut Vec<String>,
) {
	let Ok(metadata) = fs::metadata(path) else {
		return;
	};

	if metadata.is_dir() {
		let entries = fs::read_dir(path).expect("reference audit directory should be readable");

		for entry in entries {
			let entry = entry.expect("reference audit directory entry should be readable");

			collect_assign_task_reference_violations(&entry.path(), repo_root, offenders);
		}

		return;
	}
	if !metadata.is_file() || !should_audit_multi_agent_v2_reference_file(path) {
		return;
	}

	let text = fs::read_to_string(path).expect("reference audit file should be utf-8 text");
	let lower = text.to_ascii_lowercase();

	if !lower.contains("assign_task") {
		return;
	}
	if lower.contains("followup_task") && radar::has_legacy_multi_agent_v2_context(&lower) {
		return;
	}

	let relative = path.strip_prefix(repo_root).unwrap_or(path);

	offenders.push(relative.display().to_string());
}

fn should_audit_multi_agent_v2_reference_file(path: &Path) -> bool {
	let extension = path.extension().and_then(|value| value.to_str());

	matches!(extension, Some("json" | "md" | "py" | "rs" | "ts" | "tsx"))
}

fn valid_release_delta() -> Value {
	serde_json::json!({
		"schema": "release_delta/v1",
		"repo": "openai/codex",
		"tag_prefix": "rust-v",
		"generated_at": "2026-06-01T00:00:00Z",
		"stable_release": release("rust-v0.1.0", false),
		"prerelease": release("rust-v0.2.0-alpha.1", true),
		"compare": compare(),
		"tracked_signal_slugs": ["openai-codex-pr-22414"],
		"release_options": {
			"stable": [release("rust-v0.1.0", false)],
			"preview": [release("rust-v0.2.0-alpha.1", true)]
		},
		"comparisons": [
			{
				"stable_tag_name": "rust-v0.1.0",
				"prerelease_tag_name": "rust-v0.2.0-alpha.1",
				"compare": compare(),
				"tracked_signal_slugs": ["openai-codex-pr-22414"]
			}
		]
	})
}

fn release(tag_name: &str, prerelease: bool) -> Value {
	serde_json::json!({
		"tag_name": tag_name,
		"name": tag_name,
		"published_at": "2026-06-01T00:00:00Z",
		"url": "https://github.com/openai/codex/releases/tag/rust-v0.1.0",
		"prerelease": prerelease
	})
}

fn compare() -> Value {
	serde_json::json!({
		"status": "ahead",
		"ahead_by": 1,
		"total_commits": 1,
		"url": "https://github.com/openai/codex/compare/rust-v0.1.0...rust-v0.2.0-alpha.1",
		"commit_shas": ["abc123"],
		"pr_numbers": [22_414]
	})
}

fn valid_review_queue() -> Value {
	serde_json::json!({
		"schema": "upstream_review_queue/v1",
		"repo": "openai/codex",
		"generated_at": "2026-06-01T00:00:00Z",
		"source": {
			"default_branch": "main",
			"search_limit": 40
		},
		"subjects": [valid_queue_subject()],
		"counts": {
			"subjects_queued": 1,
			"recent_commits_scanned": 1,
			"published_subjects_seen": 0,
			"critical": 0,
			"high": 1,
			"normal": 0,
			"low": 0
		}
	})
}

fn valid_queue_subject() -> Value {
	serde_json::json!({
		"subject_kind": "pr",
		"subject_id": "22414",
		"title": "Add Unix socket endpoint support",
		"url": "https://github.com/openai/codex/pull/22414",
		"source_state": "merged",
		"commit_shas": ["abc123"],
		"changed_file_count": 1,
		"sample_paths": ["codex-rs/app-server/src/lib.rs"],
		"surface_hints": ["app_server_protocol"],
		"attention_flags": [],
		"review_priority": "high",
		"review_reason": "Transport behavior changed.",
		"next_step": "ai_review_required"
	})
}

fn valid_upstream_review() -> Value {
	serde_json::json!({
		"schema": "upstream_review/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"subject": {
			"subject_kind": "pr",
			"subject_id": "22414",
			"commit_shas": ["abc123"]
		},
		"source_refs": {
			"items": [
				{
					"kind": "pull_request",
					"title": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/pull/22414"
				}
			]
		},
		"reviewed_at": "2026-06-01T00:00:00Z",
		"observed_change": "Remote Codex can use Unix socket endpoints.",
		"changed_surfaces": ["app server"],
		"confidence": "confirmed",
		"evidence": ["PR #22414 updates app-server endpoint handling."],
		"next_actions": [
			{
				"type": "upstream_impact",
				"reason": "Transport behavior can affect Decodex."
			}
		]
	})
}

fn valid_upstream_impact() -> Value {
	serde_json::json!({
		"schema": "upstream_impact/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"source_refs": {
			"items": [
				{
					"kind": "pull_request",
					"title": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/pull/22414"
				}
			]
		},
		"observed_change": "Remote Codex can use Unix socket endpoints.",
		"public_signal_decision": "publish",
		"control_plane_impact": "candidate",
		"publisher_angle": "operator_impact",
		"confidence": "confirmed",
		"evidence": ["PR #22414 updates app-server endpoint handling."]
	})
}

fn valid_control_plane_upgrade_candidate() -> Value {
	serde_json::json!({
		"schema": "control_plane_upgrade_candidate/v1",
		"slug": "openai-codex-pr-22414-control-plane",
		"repo": "openai/codex",
		"status": "proposed",
		"source_refs": {
			"upstream_reviews": [
				".agent/automations/radar/cache/github/reviews/openai-codex-pr-22414.review.json"
			],
			"upstream_impacts": [
				".agent/automations/radar/cache/github/impact/openai-codex-pr-22414.json"
			],
			"urls": ["https://github.com/openai/codex/pull/22414"]
		},
		"observed_change": "Remote Codex can use Unix socket endpoints.",
		"control_plane_impact": "compat_risk",
		"upgrade_path": "compat_risk_mitigation",
		"affected_surfaces": ["app-server protocol"],
		"target_codex": {
			"channel": "stable",
			"version": "0.142.2",
			"tag": "rust-v0.142.2",
			"release_url": "https://github.com/openai/codex/releases/tag/rust-v0.142.2",
			"compatibility_status": "needs_review",
			"matrix_ref": "docs/reference/codex-compatibility-matrix.md#codex-01422"
		},
		"authority": {
			"decision_contract_required": true,
			"program_intake_required": true,
			"mutation_allowed": false,
			"objective_id": "decodex-self-iteration"
		},
		"reason": "The upstream app-server transport change may affect Decodex Control Plane compatibility.",
		"validation_gates": ["decodex probe stdio://", "cargo test -p decodex app_server --lib"],
		"stop_conditions": ["Missing accepted Decision Contract", "Probe failure against the target Codex build"],
		"acceptance_criteria": [
			"Compatibility impact is proven or dismissed with source-backed evidence."
		]
	})
}

fn valid_radar_archive_manifest() -> Value {
	serde_json::json!({
		"schema": "radar_archive_manifest/v1",
		"archive_id": "radar-archive-2026-06-02",
		"created_at": "2026-06-02T03:30:00Z",
		"retention_days": 21,
		"source_commit": "0123456789abcdef0123456789abcdef01234567",
		"release_tag": "radar-archive-2026-06-02",
		"release_url": "https://github.com/hack-ink/decodex/releases/tag/radar-archive-2026-06-02",
		"archive_asset": {
			"name": "radar-archive-2026-06-02.tar.zst",
			"size_bytes": 1_024,
			"sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
		},
		"checksum_asset": {
			"name": "SHA256SUMS",
			"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		},
		"files": [
			{
				"path": ".agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json",
				"kind": "bundle",
				"sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
				"size_bytes": 512
			}
		]
	})
}
