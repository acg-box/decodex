use crate::orchestrator::tests::operator::status::{
	self, EvidenceRequest, ReviewLevel, StateStore, TEST_SERVICE_ID, TempDir, fs, orchestrator,
};

#[test]
fn private_evidence_readback_reports_missing_events_for_known_run() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree(TEST_SERVICE_ID, "issue-2", "x/pubfi-pub-102", ".worktrees/PUB-102")
		.expect("worktree should persist");
	state_store
		.record_run_attempt("run-empty", "issue-2", 1, "running")
		.expect("run should persist");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-102",
		run_id: Some("run-empty"),
		attempt_number: Some(1),
		json: false,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("missing private evidence should still produce readback");

	assert_eq!(readback.event_count, 0);
	assert_eq!(readback.warnings, vec![String::from("private_execution_evidence_missing")]);
	assert!(orchestrator::render_private_evidence_readback(&readback).contains("- none"));
}

#[test]
fn private_evidence_readback_direct_lookup_uses_stored_issue_id() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-1",
			"run-detached",
			3,
			"progress_checkpoint",
			serde_json::json!({
				"summary": "private checkpoint stayed local",
			}),
		)
		.expect("private evidence should append without run metadata");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-101",
		run_id: Some("run-detached"),
		attempt_number: Some(3),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("direct private evidence lookup should infer stored issue id");

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.issue_id, "issue-1");
	assert_eq!(readback.issue_identifier.as_deref(), Some("PUB-101"));
	assert_eq!(readback.latest_event_type.as_deref(), Some("progress_checkpoint"));
	assert!(readback.warnings.is_empty());
}

#[test]
fn private_evidence_readback_returns_manual_adopt_events_and_stable_command() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "pub-1579-manual-adopt-2-de5a4c6bf98a";

	state_store
		.record_run_attempt(run_id, "issue-pub-1579", 2, "succeeded")
		.expect("manual adopt run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-pub-1579",
			run_id,
			2,
			"review_handoff_adopt",
			serde_json::json!({
				"schema": "decodex.review_handoff_recovery_private_event/1",
				"event": "review_handoff_adopt",
				"writeback_stage": "active_label_checked",
				"issue_identifier": "PUB-1579",
				"branch": "x/pubfi-mono-pub-1579",
				"worktree_path": ".worktrees/PUB-1579",
				"pr_url": "https://github.com/helixbox/pubfi-mono/pull/340",
				"pr_head_sha": "4080f5e000000000000000000000000000000000",
				"pr_base_ref": "main",
				"pr_state": "OPEN",
				"mergeable": "MERGEABLE",
				"merge_state_status": "CLEAN",
				"status_check_rollup_state": "SUCCESS",
				"active_label_present": false,
				"active_label_restored": true,
				"existing_retained_worktree_mapping": true,
				"existing_review_lifecycle_authority": false,
				"manual_takeover_adopt": true,
				"next_action": "continue retained post-review lifecycle"
			}),
		)
		.expect("manual adopt private evidence should append");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-1579",
		run_id: Some(run_id),
		attempt_number: Some(2),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("manual adopt evidence should read");
	let expected_command = format!(
		"decodex evidence --config {} PUB-1579 --run-id {run_id} --attempt 2 --json",
		config.config_path().display()
	);

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.issue_id, "issue-pub-1579");
	assert_eq!(readback.issue_identifier.as_deref(), Some("PUB-1579"));
	assert_eq!(readback.latest_event_type.as_deref(), Some("review_handoff_adopt"));
	assert_eq!(readback.read_command, expected_command);
	assert!(readback.warnings.is_empty());
}

#[test]
fn private_evidence_readback_shell_quotes_config_paths_with_spaces() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = temp_dir.path().join("parent space/target-repo");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "pub-space-attempt-1";

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	status::write_service_config(
		&repo_root,
		&status::sample_service_config_toml("pubfi", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let config = status::load_service_config(&repo_root);

	state_store.record_run_attempt(run_id, "issue-space", 1, "failed").expect("run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-space",
			run_id,
			1,
			"harness_outcome",
			serde_json::json!({"schema": "decodex.harness_outcome/1"}),
		)
		.expect("private evidence should append");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "issue-space",
		run_id: Some(run_id),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("private evidence should read");

	assert_eq!(
		readback.read_command,
		format!(
			"decodex evidence --config '{}' issue-space --run-id {run_id} --attempt 1 --json",
			config.config_path().display()
		)
	);
}
