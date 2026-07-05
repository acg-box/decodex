use crate::orchestrator::tests::operator::status::{
	self, OperatorCodexAccountControlStatus, OperatorStatusSnapshot, orchestrator,
};

#[test]
fn operator_status_text_explains_unleased_live_running_lane() {
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.run_lease = false;
	current_lane.queue_lease_state = String::from("not_held");
	current_lane.attempt_status = String::from("stalled");
	current_lane.status_projection_reason =
		Some(String::from("terminal_attempt_promoted_by_process_alive"));

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
		current_lanes: vec![current_lane.clone()],
		queued_candidates: Vec::new(),
		recent_runs: vec![current_lane],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("run_lease: no"));
	assert!(rendered.contains("queue_lease_state: not_held"));
	assert!(rendered.contains("queue_lease: not_held (process_alive keeps lane visible)"));
	assert!(
		rendered.contains("status_projection_reason: terminal_attempt_promoted_by_process_alive")
	);
	assert!(rendered.contains("execution_liveness: process_alive"));
}
