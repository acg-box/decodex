use crate::orchestrator::tests::operator::status::{
	self, EvidenceRequest, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn private_evidence_readback_summarizes_payloads_without_connector() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree(TEST_SERVICE_ID, "issue-1", "x/pubfi-pub-101", ".worktrees/PUB-101")
		.expect("worktree should persist");
	state_store.record_run_attempt("run-1", "issue-1", 1, "failed").expect("run should persist");
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
		project_id: None,
		issue: "PUB-101",
		run_id: Some("run-1"),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
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
