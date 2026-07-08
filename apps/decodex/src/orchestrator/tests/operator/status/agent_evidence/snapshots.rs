use crate::orchestrator::tests::operator::status::{
	self, AgentEvidenceSource, OperatorCodexAccountControlStatus, OperatorGitHubCliAuthority,
	OperatorPostReviewLaneStatus, OperatorProjectStatus, OperatorQueuedIssueStatus,
	OperatorStatusSnapshot, Path, TEST_SERVICE_ID, TempDir, TestEnvVarGuard, Value,
	agent_evidence::shared, fs, orchestrator,
};

#[test]
fn agent_evidence_snapshot_writes_index_blockers_capsules_and_event_stream() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be utf-8"));
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.suspected_stall = true;
	current_lane.phase = String::from("stalled");
	current_lane.run_phase = String::from("stalled");
	current_lane.counts_as_running = false;
	current_lane.needs_attention = true;

	let snapshot = OperatorStatusSnapshot {
		project_id: String::from(TEST_SERVICE_ID),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: vec![agent_evidence_project_status_with_configured_gh()],
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: vec![current_lane.clone()],
		recent_runs: vec![current_lane],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		queued_candidates: vec![agent_evidence_blocked_candidate()],
		worktrees: status::operator_status_text_worktrees(),
		post_review_lanes: vec![agent_evidence_missing_handoff_lane()],
	};
	let results = orchestrator::write_agent_evidence_snapshot(
		&snapshot,
		AgentEvidenceSource::DiagnoseCommand,
	)
	.expect("agent evidence should write");
	let result = results.first().expect("project evidence should exist");
	let index_path = temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/handoff-index.json");
	let index_json = shared::read_json_file(&index_path);

	assert_eq!(result.project_id, TEST_SERVICE_ID);
	assert_eq!(result.handoff_index_path, index_path.display().to_string());
	assert_eq!(index_json["schema"], "decodex.agent_handoff_index/1");
	assert_eq!(index_json["project_id"], TEST_SERVICE_ID);
	assert_eq!(index_json["source"], "diagnose_command");

	assert_agent_evidence_github_cli_authority(&index_json);

	assert_eq!(index_json["summary"]["blocker_count"], 3);
	assert_eq!(index_json["summary"]["run_capsule_count"], 1);
	assert_eq!(
		index_json["run_capsules"][0]["private_evidence"]["evidence_ref"],
		"private-evidence:pubfi/issue-1/run-1/1"
	);
	assert_eq!(
		index_json["run_capsules"][0]["private_evidence"]["read_command"],
		"decodex evidence --config project.toml PUB-101 --run-id run-1 --attempt 1 --json"
	);
	assert_eq!(
		index_json["blockers"][0]["blocker_snapshot_path"],
		temp_dir
			.path()
			.join(".codex/decodex/agent-evidence/pubfi/blockers/pub-101.json")
			.display()
			.to_string()
	);
	assert!(
		index_json["recovery_contracts"]
			.as_array()
			.expect("recovery contracts should be array")
			.iter()
			.any(|contract| contract["reason_code"] == "missing_review_handoff_record")
	);

	let capsule_path = index_json["run_capsules"][0]["path"]
		.as_str()
		.expect("run capsule path should be a string");
	let capsule_json = shared::read_json_file(Path::new(capsule_path));

	assert_eq!(capsule_json["schema"], "decodex.run_capsule/1");
	assert_eq!(capsule_json["run_id"], "run-1");
	assert_eq!(capsule_json["diagnosis"]["reason_code"], "suspected_stall");
	assert_eq!(capsule_json["private_evidence"]["default_view"], "summarized_payloads");

	let blocker_json = shared::read_json_file(
		&temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/blockers/pub-101.json"),
	);

	assert_eq!(blocker_json["schema"], "decodex.blocker_snapshot/1");
	assert_eq!(blocker_json["issue_identifier"], "PUB-101");
	assert_eq!(blocker_json["related_run_capsules"][0]["run_id"], "run-1");

	let events_path = temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/events.jsonl");
	let events_body = fs::read_to_string(events_path).expect("events stream should exist");
	let event_json: Value =
		serde_json::from_str(events_body.lines().next().expect("event line should exist"))
			.expect("event should be JSON");

	assert_eq!(event_json["schema"], "decodex.agent_evidence_event/1");
	assert_eq!(event_json["blocker_count"], 3);
}

#[test]
fn does_not_turn_waiting_lane_into_attention() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be utf-8"));
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.wait_reason = Some(String::from("tool_execution"));
	current_lane.lane_control_next_action = String::from("continue_owned_attempt");
	current_lane.counts_as_running = true;
	current_lane.needs_attention = false;
	current_lane.suspected_stall = false;

	let snapshot = OperatorStatusSnapshot {
		project_id: String::from(TEST_SERVICE_ID),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: vec![agent_evidence_project_status_with_configured_gh()],
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: vec![current_lane.clone()],
		recent_runs: vec![current_lane],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		queued_candidates: Vec::new(),
		worktrees: status::operator_status_text_worktrees(),
		post_review_lanes: Vec::new(),
	};
	let results =
		orchestrator::write_agent_evidence_snapshot(&snapshot, AgentEvidenceSource::ServeTick)
			.expect("agent evidence should write");
	let result = results.first().expect("project evidence should exist");
	let index_path = temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/handoff-index.json");
	let index_json = shared::read_json_file(&index_path);

	assert_eq!(result.handoff_index.summary.blocker_count, 0);
	assert_eq!(index_json["summary"]["blocker_count"], 0);
	assert_eq!(index_json["blockers"].as_array().expect("blockers should be array").len(), 0);

	let capsule_path = index_json["run_capsules"][0]["path"]
		.as_str()
		.expect("run capsule path should be a string");
	let capsule_json = shared::read_json_file(Path::new(capsule_path));

	assert_eq!(capsule_json["wait_reason"], "tool_execution");
	assert_eq!(capsule_json["diagnosis"]["attention_required"], false);
	assert!(capsule_json["diagnosis"]["reason_code"].is_null());
	assert!(
		!temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/blockers/pub-101.json").exists(),
		"ordinary wait_reason must not create a stale attention blocker file"
	);
}

fn assert_agent_evidence_github_cli_authority(index_json: &Value) {
	assert_eq!(index_json["github_cli_authority"]["discovery_tier"], "configured");
	assert_eq!(index_json["github_cli_authority"]["command_path"], "/opt/homebrew/bin/gh");
	assert_eq!(
		index_json["github_cli_authority"]["next_action"],
		"No action needed; Decodex will use the configured GitHub CLI path."
	);
}

fn agent_evidence_blocked_candidate() -> OperatorQueuedIssueStatus {
	let mut blocked_candidate = status::operator_status_text_queued_candidates()
		.into_iter()
		.find(|candidate| candidate.issue_identifier == "PUB-102")
		.expect("fixture should include queued issue");

	blocked_candidate.classification = String::from("blocked");
	blocked_candidate.reason = String::from("missing_dispatch_briefing");

	blocked_candidate
}

fn agent_evidence_missing_handoff_lane() -> OperatorPostReviewLaneStatus {
	let mut missing_handoff_lane = status::operator_status_text_post_review_lanes()
		.into_iter()
		.next()
		.expect("fixture should include retained review lane");

	missing_handoff_lane.classification = String::from("blocked");
	missing_handoff_lane.reason = String::from("missing_review_handoff_record");

	missing_handoff_lane
}

fn agent_evidence_project_status_with_configured_gh() -> OperatorProjectStatus {
	OperatorProjectStatus {
		project_id: String::from(TEST_SERVICE_ID),
		config_path: String::from("project.toml"),
		repo_root: String::from("/repo/pubfi"),
		enabled: true,
		github_cli_authority: OperatorGitHubCliAuthority {
			command_path: String::from("/opt/homebrew/bin/gh"),
			resolved_path: Some(String::from("/opt/homebrew/bin/gh")),
			configured_path: Some(String::from("/opt/homebrew/bin/gh")),
			discovery_tier: String::from("configured"),
			available: true,
			next_action: String::from(
				"No action needed; Decodex will use the configured GitHub CLI path.",
			),
		},
		current_lane_count: 0,
		running_lane_count: 0,
		queued_candidate_count: 0,
		post_review_lane_count: 0,
		retained_worktree_count: 0,
		waiting_lane_count: 0,
		attention_count: 0,
		cleanup_blocked_count: 0,
		cleanup_pending_count: 0,
		connector_state: String::from("ok"),
		last_activity_at: None,
		warning_count: 0,
	}
}
