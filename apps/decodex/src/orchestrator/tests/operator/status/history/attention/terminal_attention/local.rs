use crate::orchestrator::tests::operator::status::{
	self, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn local_status_summary_counts_terminal_history_needs_attention_without_queue_candidate() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-xy-922",
		"XY-922",
		"Todo",
		&[],
		Some(3),
		"2026-06-11T09:08:00Z",
	);
	let local_comments =
		status::retained_partial_progress_linear_execution_history_comments(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"xy/profit-pilot-xy-922",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("retained worktree should be recorded");
	state_store
		.record_run_attempt("xy-922-attempt-1-1781168400", &issue.id, 1, "failed")
		.expect("failed attempt should record");

	status::seed_local_linear_execution_events(&state_store, &local_comments);

	let snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let worktree = snapshot.worktrees.first().expect("retained worktree should render");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(
		snapshot.queued_candidates.is_empty(),
		"terminal ledger attention should not require a queued candidate"
	);
	assert_eq!(snapshot.projects[0].attention_count, 1);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
	assert_eq!(lane.ledger_outcome.ledger_status, "present");
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert_eq!(
		lane.ledger_outcome.needs_attention_reason.as_deref(),
		Some("Decodex retained validation-ready partial progress for manual review.")
	);
	assert_eq!(lane.latest_run.status, "needs_attention");
	assert_eq!(lane.latest_run.phase, "needs_attention");
	assert_eq!(worktree.ownership, "retained_attention");
	assert_eq!(snapshot_json["projects"][0]["attention_count"], 1);
	assert_eq!(snapshot_json["queued_candidates"].as_array().map(Vec::len), Some(0));
	assert_eq!(snapshot_json["history_lanes"][0]["latest_run"]["status"], "needs_attention");
	assert_eq!(snapshot_json["worktrees"][0]["ownership"], "retained_attention");
	assert!(rendered.contains("outcome: needs_attention"));
	assert!(rendered.contains(
		"needs_attention_reason: Decodex retained validation-ready partial progress for manual review."
	));
	assert!(rendered.contains("role: retained_attention"));
	assert!(rendered.contains("Current attention: 1"));
	assert!(rendered.contains("History-only terminal attention: 0"));
}

#[test]
fn local_status_summary_ignores_history_only_terminal_attention_without_current_owner() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-pub-1549",
		"PUB-1549",
		"Todo",
		&[],
		Some(3),
		"2026-06-12T01:56:00Z",
	);
	let local_comments =
		status::retained_partial_progress_linear_execution_history_comments(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-mono-pub-1549",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember previous lane ownership");
	state_store
		.record_run_attempt("pub-1549-attempt-1-1781240781", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("historical lane should not have current retained ownership");

	status::seed_local_linear_execution_events(&state_store, &local_comments);

	let snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.current_lanes.len(), 0);
	assert_eq!(snapshot.queued_candidates.len(), 0);
	assert_eq!(snapshot.post_review_lanes.len(), 0);
	assert_eq!(snapshot.worktrees.len(), 0);
	assert_eq!(snapshot.projects[0].current_lane_count, 0);
	assert_eq!(snapshot.projects[0].running_lane_count, 0);
	assert_eq!(snapshot.projects[0].queued_candidate_count, 0);
	assert_eq!(snapshot.projects[0].post_review_lane_count, 0);
	assert_eq!(snapshot.projects[0].attention_count, 0);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert_eq!(snapshot.projects[0].cleanup_blocked_count, 0);
	assert_eq!(snapshot.projects[0].cleanup_pending_count, 0);
	assert_eq!(lane.ledger_outcome.ledger_status, "present");
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert_eq!(lane.latest_run.status, "needs_attention");
	assert_eq!(lane.latest_run.phase, "needs_attention");
	assert!(!lane.latest_run.run_lease);
	assert_eq!(snapshot_json["projects"][0]["attention_count"], 0);
	assert_eq!(snapshot_json["worktrees"].as_array().map(Vec::len), Some(0));
	assert_eq!(snapshot_json["history_lanes"][0]["latest_run"]["status"], "needs_attention");
	assert!(rendered.contains("Current attention: 0"));
	assert!(rendered.contains("History-only terminal attention: 1"));
	assert!(rendered.contains(
		"Current attention action: none; terminal attention rows below are Run Ledger history only."
	));
	assert!(rendered.contains("outcome: needs_attention"));
}

#[test]
fn local_status_projects_review_handoff_writeback_failure_recovery_action() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-xy-1113",
		"XY-1113",
		"Todo",
		&["decodex:needs-attention"],
		Some(3),
		"2026-06-27T00:17:07Z",
	);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/profit-pilot-xy-1113",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("retained worktree should be recorded");
	state_store
		.record_run_attempt("xy-1113-attempt-1-1782480586", &issue.id, 1, "failed")
		.expect("failed attempt should record");

	status::seed_local_linear_execution_events(
		&state_store,
		&[status::linear_execution_history_comment(
			&issue,
			"terminal_failure",
			"2026-06-27T00:17:07Z",
			"manual_attention",
			|record| {
				record.branch = Some(String::from("y/profit-pilot-xy-1113"));
				record.worktree_path = Some(String::from(".worktrees/XY-1113"));
				record.pr_url =
					Some(String::from("https://github.com/hack-ink/profit-pilot/pull/398"));
				record.summary = Some(String::from("Decodex run failed and needs attention."));
				record.error_class = Some(String::from("review_handoff_writeback_failed"));
				record.next_action = Some(String::from("Decodex run failed and needs attention."));
				record.blockers = Some(vec![String::from("review handoff writeback failed")]);
				record.evidence = Some(vec![String::from("retained PR lane evidence present")]);
			},
		)],
	);

	let snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let worktree = snapshot.worktrees.first().expect("retained worktree should render");

	assert_eq!(worktree.ownership, "retained_attention");
	assert_eq!(
		worktree.recovery_next_action.as_deref(),
		Some(
			"Run `decodex recover review-handoff diagnose XY-1113 --json` to verify retained PR lineage, then follow the reported rebind recovery command."
		)
	);
}
