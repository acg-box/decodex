use std::{
	fs::{self},
	time::{Duration, SystemTime},
};

use tempfile::TempDir;

use crate::{
	maintenance::{
		self, MaintenanceMode, MaintenancePolicy, MaintenancePruneRequest, MaintenanceScope, tests,
	},
	test_support::TestEnvVarGuard,
};

#[test]
fn prune_rotates_oversized_logs_and_agent_evidence_events() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let log_dir = temp_dir.path().join(".codex/decodex/logs");
	let evidence_dir = temp_dir.path().join(".codex/decodex/agent-evidence/decodex");
	let log_path = log_dir.join("decodex.log");
	let events_path = evidence_dir.join("events.jsonl");

	fs::create_dir_all(&log_dir).expect("log dir should create");
	fs::create_dir_all(&evidence_dir).expect("evidence dir should create");
	fs::write(&log_path, b"0123456789abcdef").expect("log should write");
	fs::write(&events_path, b"0123456789abcdef").expect("events should write");

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy {
			log_rotate_bytes: 8,
			evidence_rotate_bytes: 8,
			..MaintenancePolicy::default()
		},
	)
	.expect("maintenance should run");

	assert_eq!(report.logs.rotated_files, 1);
	assert_eq!(report.agent_evidence.rotated_files, 1);
	assert_eq!(fs::metadata(&log_path).expect("log should remain").len(), 0);
	assert_eq!(fs::metadata(&events_path).expect("events should remain").len(), 0);
	assert_eq!(
		fs::read_dir(&log_dir)
			.expect("log dir should list")
			.filter_map(std::result::Result::ok)
			.filter(|entry| entry.path() != log_path)
			.count(),
		1
	);
	assert_eq!(
		fs::read_dir(&evidence_dir)
			.expect("evidence dir should list")
			.filter_map(std::result::Result::ok)
			.filter(|entry| entry.path() != events_path)
			.count(),
		1
	);
}

#[test]
fn prune_deletes_only_rotated_logs_and_agent_evidence_after_fourteen_days() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let log_dir = temp_dir.path().join(".codex/decodex/logs");
	let evidence_dir = temp_dir.path().join(".codex/decodex/agent-evidence/decodex");
	let current_log_path = log_dir.join("decodex.log");
	let old_log_path = log_dir.join("decodex.1.log");
	let fresh_log_path = log_dir.join("decodex.2.log");
	let current_events_path = evidence_dir.join("events.jsonl");
	let old_events_path = evidence_dir.join("events.1.jsonl");
	let fresh_events_path = evidence_dir.join("events.2.jsonl");
	let old_time = SystemTime::now() - Duration::from_secs(15 * 24 * 60 * 60);
	let fresh_time = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);

	fs::create_dir_all(&log_dir).expect("log dir should create");
	fs::create_dir_all(&evidence_dir).expect("evidence dir should create");

	for path in [
		&current_log_path,
		&old_log_path,
		&fresh_log_path,
		&current_events_path,
		&old_events_path,
		&fresh_events_path,
	] {
		fs::write(path, b"event\n").expect("maintenance fixture should write");
	}

	tests::set_file_modified(&current_log_path, old_time);
	tests::set_file_modified(&old_log_path, old_time);
	tests::set_file_modified(&fresh_log_path, fresh_time);
	tests::set_file_modified(&current_events_path, old_time);
	tests::set_file_modified(&old_events_path, old_time);
	tests::set_file_modified(&fresh_events_path, fresh_time);

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy::default(),
	)
	.expect("maintenance should run");

	assert_eq!(report.logs.deleted_files, 1);
	assert_eq!(report.agent_evidence.deleted_files, 1);
	assert!(current_log_path.exists());
	assert!(!old_log_path.exists());
	assert!(fresh_log_path.exists());
	assert!(current_events_path.exists());
	assert!(!old_events_path.exists());
	assert!(fresh_events_path.exists());
}
