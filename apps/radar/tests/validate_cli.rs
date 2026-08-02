//! CLI regression for explicit relative Radar validation paths.

#![allow(unused_crate_dependencies)]

use std::{fs, os::unix::fs::PermissionsExt as _, process::Command};

use serde_json::Value;

#[test]
fn validate_cli_accepts_a_relative_queue_path_from_an_isolated_cwd() {
	let cwd = tempfile::tempdir().expect("isolated CLI cwd");
	let relative = ".agent/automations/radar/cache/github/review-queue/openai-codex-latest.json";
	let path = cwd.path().join(relative);
	fs::create_dir_all(path.parent().expect("queue parent")).expect("private cache directories");
	for directory in [
		".agent",
		".agent/automations",
		".agent/automations/radar",
		".agent/automations/radar/cache",
		".agent/automations/radar/cache/github",
		".agent/automations/radar/cache/github/review-queue",
	] {
		fs::set_permissions(cwd.path().join(directory), fs::Permissions::from_mode(0o700))
			.expect("private directory mode");
	}
	fs::write(&path, serde_json::to_vec_pretty(&valid_review_queue()).expect("queue JSON"))
		.expect("queue write");
	fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private queue mode");

	let output = Command::new(env!("CARGO_BIN_EXE_radar"))
		.current_dir(cwd.path())
		.args(["validate", relative])
		.output()
		.expect("Radar CLI");
	assert!(
		output.status.success(),
		"relative validation failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let report: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
	assert_eq!(report["checked_files"], 1);
}

fn valid_review_queue() -> Value {
	serde_json::json!({
		"schema": "upstream_review_queue/v1",
		"repo": "openai/codex",
		"generated_at": "2026-06-01T00:00:00Z",
		"source": {
			"default_branch": "main",
			"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			"search_limit": 40
		},
		"subjects": [{
			"subject_kind": "pr",
			"subject_id": "22414",
			"title": "Add Unix socket endpoint support",
			"url": "https://github.com/openai/codex/pull/22414",
			"source_state": "merged",
			"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
			"changed_file_count": 1,
			"sample_paths": ["codex-rs/app-server/src/lib.rs"],
			"surface_hints": ["app_server_protocol"],
			"attention_flags": [],
			"review_priority": "high",
			"review_reason": "Transport behavior changed.",
			"next_step": "ai_review_required"
		}],
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
