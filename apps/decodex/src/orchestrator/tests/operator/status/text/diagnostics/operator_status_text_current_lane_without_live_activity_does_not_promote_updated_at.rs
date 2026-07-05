use crate::orchestrator::tests::operator::status::{
	self, OperatorCodexAccountControlStatus, OperatorStatusSnapshot, orchestrator,
};

#[test]
fn operator_status_text_current_lane_without_live_activity_does_not_promote_updated_at() {
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.updated_at = String::from("2026-03-14 09:00:00");
	current_lane.last_run_activity_at = None;
	current_lane.last_protocol_activity_at = None;
	current_lane.last_progress_at = None;

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

	assert!(rendered.contains("freshness_at: none"));
	assert!(rendered.contains("freshness_source: none"));
	assert!(rendered.contains("updated_at: 2026-03-14 09:00:00"));
}
