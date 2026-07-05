use crate::orchestrator::tests::operator::status::{
	self, StateStore, TEST_SERVICE_ID, orchestrator,
};

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
