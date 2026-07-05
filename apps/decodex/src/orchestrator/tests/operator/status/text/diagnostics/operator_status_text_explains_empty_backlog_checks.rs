use crate::orchestrator::tests::operator::status::{
	OperatorCodexAccountControlStatus, OperatorStatusSnapshot, orchestrator,
};

#[test]
fn operator_status_text_explains_empty_backlog_checks() {
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("Backlog: 0"));
	assert!(rendered.contains("Hint: check `Todo`"));
	assert!(rendered.contains("`decodex:queued:<service-id>`"));
	assert!(rendered.contains("`decodex:queued:pubfi`"));
	assert!(rendered.contains("opt-out/manual-only"));
	assert!(rendered.contains("needs-attention"));
	assert!(rendered.contains("non-terminal state"));
	assert!(rendered.contains("dependency blockers"));
	assert!(rendered.contains("no active issue claim"));
}
