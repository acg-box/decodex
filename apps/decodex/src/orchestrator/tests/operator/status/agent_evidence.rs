#[test]
fn agent_evidence_snapshot_writes_index_blockers_capsules_and_event_stream() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be utf-8"));
	let mut active_run = operator_status_text_active_run();

	active_run.suspected_stall = true;
	active_run.phase = String::from("stalled");

	let mut blocked_candidate = operator_status_text_queued_candidates()
		.into_iter()
		.find(|candidate| candidate.issue_identifier == "PUB-102")
		.expect("fixture should include queued issue");

	blocked_candidate.classification = String::from("blocked");
	blocked_candidate.reason = String::from("missing_dispatch_briefing");

	let mut missing_handoff_lane = operator_status_text_post_review_lanes()
		.into_iter()
		.next()
		.expect("fixture should include retained review lane");

	missing_handoff_lane.classification = String::from("blocked");
	missing_handoff_lane.reason = String::from("missing_review_handoff_record");

	let snapshot = OperatorStatusSnapshot {
		project_id: String::from(TEST_SERVICE_ID),
		run_limit: 10,
		warnings: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: vec![active_run.clone()],
		recent_runs: vec![active_run],
		history_lanes: Vec::new(),
		queued_candidates: vec![blocked_candidate],
		worktrees: operator_status_text_worktrees(),
		post_review_lanes: vec![missing_handoff_lane],
	};
	let results = orchestrator::write_agent_evidence_snapshot(
		&snapshot,
		AgentEvidenceSource::DiagnoseCommand,
	)
	.expect("agent evidence should write");
	let result = results.first().expect("project evidence should exist");
	let index_path = temp_dir
		.path()
		.join(".codex/decodex/agent-evidence/pubfi/handoff-index.json");
	let index_json = read_json_file(&index_path);

	assert_eq!(result.project_id, TEST_SERVICE_ID);
	assert_eq!(result.handoff_index_path, index_path.display().to_string());
	assert_eq!(index_json["schema"], "decodex.agent_handoff_index/1");
	assert_eq!(index_json["project_id"], TEST_SERVICE_ID);
	assert_eq!(index_json["source"], "diagnose_command");
	assert_eq!(index_json["summary"]["blocker_count"], 3);
	assert_eq!(index_json["summary"]["run_capsule_count"], 1);
	assert_eq!(
		index_json["run_capsules"][0]["private_evidence"]["evidence_ref"],
		"private-evidence:pubfi/issue-1/run-1/1"
	);
	assert_eq!(
		index_json["run_capsules"][0]["private_evidence"]["read_command"],
		"decodex evidence PUB-101 --run-id run-1 --attempt 1 --json"
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
	let capsule_json = read_json_file(Path::new(capsule_path));

	assert_eq!(capsule_json["schema"], "decodex.run_capsule/1");
	assert_eq!(capsule_json["run_id"], "run-1");
	assert_eq!(capsule_json["diagnosis"]["reason_code"], "suspected_stall");
	assert_eq!(
		capsule_json["private_evidence"]["default_view"],
		"summarized_payloads"
	);

	let blocker_json = read_json_file(
		&temp_dir
			.path()
			.join(".codex/decodex/agent-evidence/pubfi/blockers/pub-101.json"),
	);

	assert_eq!(blocker_json["schema"], "decodex.blocker_snapshot/1");
	assert_eq!(blocker_json["issue_identifier"], "PUB-101");
	assert_eq!(blocker_json["related_run_capsules"][0]["run_id"], "run-1");

	let events_path = temp_dir
		.path()
		.join(".codex/decodex/agent-evidence/pubfi/events.jsonl");
	let events_body = fs::read_to_string(events_path).expect("events stream should exist");
	let event_json: Value =
		serde_json::from_str(events_body.lines().next().expect("event line should exist"))
			.expect("event should be JSON");

	assert_eq!(event_json["schema"], "decodex.agent_evidence_event/1");
	assert_eq!(event_json["blocker_count"], 3);
}

#[test]
fn private_evidence_readback_summarizes_payloads_without_connector() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			"issue-1",
			"x/pubfi-pub-101",
			".worktrees/PUB-101",
		)
		.expect("worktree should persist");
	state_store
		.record_run_attempt("run-1", "issue-1", 1, "failed")
		.expect("run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-1",
			"run-1",
			1,
			"command_failed",
			serde_json::json!({
				"summary": "cargo make test failed",
				"next_action": "repair the failing assertion",
				"stdout": "full command output stays hidden by default",
			}),
		)
		.expect("private evidence should append");

	let request = EvidenceRequest {
		config_path: None,
		issue: "PUB-101",
		run_id: Some("run-1"),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(
		&state_store,
		&config,
		&request,
	)
	.expect("private evidence should read from local state");

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.issue_id, "issue-1");
	assert_eq!(readback.issue_identifier.as_deref(), Some("PUB-101"));
	assert_eq!(readback.latest_event_type.as_deref(), Some("command_failed"));
	assert!(readback.warnings.is_empty());
	assert_eq!(readback.events[0].payload, None);
	assert!(
		readback.events[0]
			.payload_summary
			.preview
			.iter()
			.any(|preview| preview.contains("summary=cargo make test failed"))
	);
	assert_eq!(
		readback.events[0].payload_summary.redacted_default_keys,
		vec![String::from("stdout")]
	);

	let rendered = orchestrator::render_private_evidence_readback(&readback);

	assert!(rendered.contains("event_count: 1"));
	assert!(rendered.contains("redacted_default_keys=stdout"));
	assert!(!rendered.contains("full command output stays hidden by default"));
}

#[test]
fn private_evidence_readback_reports_missing_events_for_known_run() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			"issue-2",
			"x/pubfi-pub-102",
			".worktrees/PUB-102",
		)
		.expect("worktree should persist");
	state_store
		.record_run_attempt("run-empty", "issue-2", 1, "running")
		.expect("run should persist");

	let request = EvidenceRequest {
		config_path: None,
		issue: "PUB-102",
		run_id: Some("run-empty"),
		attempt_number: Some(1),
		json: false,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(
		&state_store,
		&config,
		&request,
	)
	.expect("missing private evidence should still produce readback");

	assert_eq!(readback.event_count, 0);
	assert_eq!(
		readback.warnings,
		vec![String::from("private_execution_evidence_missing")]
	);
	assert!(
		orchestrator::render_private_evidence_readback(&readback)
			.contains("- none")
	);
}

#[test]
fn private_evidence_readback_direct_lookup_uses_stored_issue_id() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
		issue: "PUB-101",
		run_id: Some("run-detached"),
		attempt_number: Some(3),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(
		&state_store,
		&config,
		&request,
	)
	.expect("direct private evidence lookup should infer stored issue id");

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.issue_id, "issue-1");
	assert_eq!(readback.issue_identifier.as_deref(), Some("PUB-101"));
	assert_eq!(readback.latest_event_type.as_deref(), Some("progress_checkpoint"));
	assert!(readback.warnings.is_empty());
}

fn read_json_file(path: &Path) -> Value {
	let body = fs::read_to_string(path).expect("JSON file should exist");

	serde_json::from_str(&body).expect("JSON file should parse")
}
