//! CLI regressions for refresh-bound Radar review selection.

#![allow(unused_crate_dependencies)]

use std::{
	fs,
	os::unix::fs::PermissionsExt as _,
	path::{Path, PathBuf},
	process::{Command, Output},
};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

const CACHE_ROOT: &str = ".agent/automations/radar/cache";
const QUEUE_PATH: &str =
	".agent/automations/radar/cache/github/review-queue/openai-codex-latest.json";

#[test]
fn review_next_cli_accepts_the_refresh_queue_digest() {
	let cwd = isolated_cwd();
	let queue_sha256 = write_queue(cwd.path());
	let output = run_review_next(cwd.path(), &queue_sha256);

	assert!(
		output.status.success(),
		"matching refresh receipt failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let report: Value =
		serde_json::from_slice(&output.stdout).expect("review-next report should be JSON");

	assert_eq!(report["status"], "needs_source_review");
	assert_eq!(report["queue_generation"]["sha256"], queue_sha256);
}

#[test]
fn review_next_cli_rejects_a_mismatched_refresh_queue_digest() {
	let cwd = isolated_cwd();

	write_queue(cwd.path());
	let output = run_review_next(cwd.path(), &"0".repeat(64));

	assert!(!output.status.success());
	assert!(String::from_utf8_lossy(&output.stderr).contains("does not match the refresh receipt"));
}

fn run_review_next(cwd: &Path, expected_queue_sha256: &str) -> Output {
	Command::new(env!("CARGO_BIN_EXE_radar"))
		.current_dir(cwd)
		.env("NO_COLOR", "1")
		.arg("review-next")
		.arg("--cache-root")
		.arg(CACHE_ROOT)
		.arg("--expected-queue-sha256")
		.arg(expected_queue_sha256)
		.arg("--max-age-hours")
		.arg("876000")
		.output()
		.expect("Radar CLI should run")
}

fn isolated_cwd() -> tempfile::TempDir {
	tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))
		.expect("isolated CLI cwd should be created under a non-symlink root")
}

fn write_queue(cwd: &Path) -> String {
	let queue_path = cwd.join(QUEUE_PATH);

	fs::create_dir_all(queue_path.parent().expect("queue should have a parent"))
		.expect("private queue directory should be created");
	for path in private_directories(cwd) {
		fs::set_permissions(path, fs::Permissions::from_mode(0o700))
			.expect("private cache directory mode should be set");
	}
	let mut payload = serde_json::to_vec_pretty(&valid_queue()).expect("queue should serialize");

	payload.push(b'\n');
	fs::write(&queue_path, &payload).expect("queue should be written");
	fs::set_permissions(&queue_path, fs::Permissions::from_mode(0o600))
		.expect("queue mode should be set");

	Sha256::digest(&payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn private_directories(cwd: &Path) -> Vec<PathBuf> {
	[
		".agent",
		".agent/automations",
		".agent/automations/radar",
		CACHE_ROOT,
		".agent/automations/radar/cache/github",
		".agent/automations/radar/cache/github/review-queue",
	]
	.into_iter()
	.map(|relative| cwd.join(relative))
	.collect()
}

fn valid_queue() -> Value {
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
			"attention_flags": ["new_feature"],
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
