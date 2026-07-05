use crate::orchestrator::tests::operator::status::{
	OperatorCodexAccountControlStatus, OperatorGitHubCliAuthority, OperatorProjectStatus,
	OperatorStatusSnapshot, orchestrator,
};

#[test]
fn operator_status_text_surfaces_github_cli_authority() {
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: vec![OperatorProjectStatus {
			project_id: String::from("pubfi"),
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
		}],
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

	assert!(rendered.contains(
		"GitHub CLI: tier=configured available=true command_path=/opt/homebrew/bin/gh resolved_path=/opt/homebrew/bin/gh configured_path=/opt/homebrew/bin/gh next_action=No action needed; Decodex will use the configured GitHub CLI path."
	));
}
