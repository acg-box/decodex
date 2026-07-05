use crate::orchestrator::tests::operator::status::{
	self, OperatorCodexAccountControlStatus, OperatorStatusSnapshot, ProtocolActivitySummary,
	orchestrator, state,
};

#[test]
fn operator_status_text_sanitizes_private_protocol_activity_details() {
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.protocol_activity = Some(ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456",
				)),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("config at /private/worktree using GITHUB_PAT_Y")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker under /srv/decodex/runtime")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker path=/srv/decodex/runtime")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker (/srv/decodex/runtime)")),
			},
		],
	});

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
		current_lanes: vec![current_lane],
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains(
		"protocol_activity: turn=completed; waiting=turn_completed; rate_limit=none; recent=configWarning:redacted_sensitive_detail, configWarning:redacted_sensitive_detail, configWarning:redacted_sensitive_detail, configWarning:redacted_sensitive_detail, error:redacted_sensitive_detail"
	));
	assert!(!rendered.contains("GITHUB_PAT_Y"));
	assert!(!rendered.contains("ghp_"));
	assert!(!rendered.contains("/private/worktree"));
	assert!(!rendered.contains("/srv/decodex/runtime"));
	assert!(!rendered.contains("path=/srv"));
	assert!(!rendered.contains("(/srv"));
}
