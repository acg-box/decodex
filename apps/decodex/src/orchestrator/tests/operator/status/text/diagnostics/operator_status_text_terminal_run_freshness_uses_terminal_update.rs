use crate::orchestrator::tests::operator::status::{
	self, OperatorCodexAccountControlStatus, OperatorStatusSnapshot, orchestrator,
};

#[test]
fn operator_status_text_terminal_run_freshness_uses_terminal_update() {
	let mut terminal_run = status::operator_status_text_current_lane();

	terminal_run.status = String::from("succeeded");
	terminal_run.phase = String::from("completed");
	terminal_run.run_phase = String::from("completed");
	terminal_run.run_lease = true;
	terminal_run.updated_at = String::from("2026-03-14 10:05:00");
	terminal_run.last_run_activity_at = Some(String::from("2026-03-14 10:10:00Z"));

	let history_lanes = orchestrator::operator_history_lanes(&[], &[terminal_run.clone()]);
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
		recent_runs: vec![terminal_run],
		history_lanes,
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("run_id: run-1"));
	assert!(rendered.contains("run_phase: completed"));
	assert!(rendered.contains("run_lease: yes"));
	assert!(rendered.contains("freshness_at: 2026-03-14 10:05:00"));
	assert!(rendered.contains("freshness_source: updated_at"));
	assert!(rendered.contains("last_run_activity_at: 2026-03-14 10:10:00Z"));
}
