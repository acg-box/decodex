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

fn read_json_file(path: &Path) -> Value {
	let body = fs::read_to_string(path).expect("JSON file should exist");

	serde_json::from_str(&body).expect("JSON file should parse")
}
