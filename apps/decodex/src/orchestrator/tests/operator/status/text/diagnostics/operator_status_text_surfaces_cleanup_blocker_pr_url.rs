use crate::orchestrator::tests::operator::status::{
	OperatorCodexAccountControlStatus, OperatorStatusSnapshot, orchestrator,
};

#[test]
fn operator_status_text_surfaces_cleanup_blocker_pr_url() {
	let pr_url = "https://github.com/hack-ink/decodex/pull/119";
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
		worktrees: vec![orchestrator::OperatorWorktreeStatus {
			project_id: String::from("pubfi"),
			issue_id: String::from("issue-3"),
			issue_identifier: Some(String::from("PUB-103")),
			issue_state: Some(String::from("Done")),
			branch_name: String::from("x/pubfi-pub-103"),
			worktree_path: String::from(".worktrees/PUB-103"),
			ownership: String::from("post_review_lane"),
			ownership_reason: String::from(
				"Review & Landing owns this worktree as `cleanup_blocked`.",
			),
			provenance: orchestrator::OperatorWorktreeProvenanceStatus {
				source: String::from("runtime_recorded"),
				created_at_unix: Some(1),
				updated_at_unix: Some(2),
				audit_required: false,
			},
			recovery_next_action: None,
			hygiene: None,
		}],
		post_review_lanes: vec![orchestrator::OperatorPostReviewLaneStatus {
			project_id: String::from("pubfi"),
			issue_id: String::from("issue-3"),
			issue_identifier: String::from("PUB-103"),
			issue_state: String::from("Done"),
			branch_name: String::from("x/pubfi-pub-103"),
			worktree_path: String::from(".worktrees/PUB-103"),
			classification: String::from("cleanup_blocked"),
			reason: String::from("retry_budget_exhausted"),
			pr_url: Some(String::from(pr_url)),
			pr_head_sha: Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6")),
			pr_state: Some(String::from("MERGED")),
			review_decision: Some(String::from("APPROVED")),
			mergeable: Some(String::from("MERGEABLE")),
			check_state: Some(String::from("SUCCESS")),
			unresolved_review_threads: Some(0),
			shadowed_by_current_lane: false,
			readback_warning: None,
			readback_root_cause: Some(String::from("lineage_validation_failed")),
			loop_status: None,
		}],
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("classification: cleanup_blocked"));
	assert!(rendered.contains("reason: retry_budget_exhausted"));
	assert!(rendered.contains("readback_root_cause: lineage_validation_failed"));
	assert!(rendered.contains(&format!("pr_url: {pr_url}")));
	assert!(!rendered.contains("pr_url: none"));
}
