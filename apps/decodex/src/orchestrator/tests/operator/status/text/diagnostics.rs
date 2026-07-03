use crate::orchestrator::tests::operator::status::{
	self, OperatorCodexAccountControlStatus, OperatorStatusSnapshot, eyre, orchestrator,
};

#[test]
fn queue_explain_renders_candidate_reasons_without_running_dispatch() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let candidates = status::operator_status_text_queued_candidates();
	let rendered = orchestrator::render_queue_explain(&config, &candidates);

	assert!(rendered.contains("Mode: dry-run queue explain"));
	assert!(rendered.contains("Queued candidates: 3"));
	assert!(rendered.contains("Ready: 1"));
	assert!(rendered.contains("Claimed: 1"));
	assert!(rendered.contains("Closed: 1"));
	assert!(rendered.contains("issue: PUB-102"));
	assert!(rendered.contains("classification: ready"));
	assert!(rendered.contains("reason: eligible_for_dispatch"));
}

#[test]
fn runtime_recovery_warning_keeps_safe_error_class() {
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("Linear tracker request failed"),
		),
		"runtime_recovery_unavailable:tracker"
	);
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("worktree scan failed"),
		),
		"runtime_recovery_unavailable:worktree"
	);
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("sqlite runtime store locked"),
		),
		"runtime_recovery_unavailable:runtime_store"
	);
}

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
