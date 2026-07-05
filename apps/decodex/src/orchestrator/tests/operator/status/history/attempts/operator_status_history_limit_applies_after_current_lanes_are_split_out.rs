use crate::orchestrator::tests::operator::status::{self, StateStore, orchestrator};

#[test]
fn operator_status_history_limit_applies_after_current_lanes_are_split_out() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let failed_issue = status::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	state_store
		.record_run_attempt("run-active", &active_issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease("pubfi", &active_issue.id, "run-active", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&active_issue.id,
			"x/pubfi-pub-101",
			&config.worktree_root().join(&active_issue.identifier).display().to_string(),
		)
		.expect("active worktree should record");
	state_store
		.record_run_attempt("run-failed", &failed_issue.id, 1, "failed")
		.expect("failed run should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&failed_issue.id,
			"x/pubfi-pub-102",
			&config.worktree_root().join(&failed_issue.identifier).display().to_string(),
		)
		.expect("failed worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].attempt_count, 1);
	assert!(rendered.contains(
		"Run ledger shown: 1 issue lanes from 1 history attempts (current lanes inline)"
	));
	assert_eq!(rendered.matches("run_id: run-active").count(), 1);
	assert_eq!(rendered.matches("run_id: run-failed").count(), 1);

	let history_index = rendered.find("Run Ledger").expect("history section should render");
	let failed_index = rendered.find("run_id: run-failed").expect("failed run should render");

	assert!(
		failed_index > history_index,
		"history-only run should remain visible after current lane overlap is hidden"
	);
}
